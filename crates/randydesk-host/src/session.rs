//! Gestión de sesiones QUIC entrantes: autenticación, stream de control y streaming de video.

use crate::capture::{MonitorGeometry, ScreenCapturer};
use crate::clipboard;
use crate::encoder::TileEncoder;
use crate::input;
use anyhow::{anyhow, Context, Result};
use quinn::{Connection, Endpoint};
use randydesk_proto::{
    generate_nonce, read_msg, verify_proof, write_msg, ClientMsg, HostMsg, StreamSettings,
    PROTOCOL_VERSION,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Fábrica de capturadores de pantalla inyectable para producción o pruebas.
pub type CapturerFactory = Arc<dyn Fn() -> Result<Box<dyn ScreenCapturer>> + Send + Sync>;

/// Configuración compartida para ejecutar el servidor de sesiones del host.
#[derive(Clone)]
pub struct HostServerConfig {
    pub password: String,
    pub host_name: String,
    pub default_settings: StreamSettings,
    pub capturer_factory: CapturerFactory,
    pub active_clients: Arc<AtomicUsize>,
}

/// Guardia RAII para llevar el conteo exacto de sesiones activas en curso.
struct ActiveClientGuard {
    counter: Arc<AtomicUsize>,
}

impl ActiveClientGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self { counter }
    }
}

impl Drop for ActiveClientGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Inicia el bucle de aceptación de conexiones QUIC hasta que se cierre el endpoint.
pub async fn accept_loop(endpoint: Endpoint, cfg: HostServerConfig) {
    while let Some(incoming) = endpoint.accept().await {
        let cfg_clone = cfg.clone();
        tokio::spawn(async move {
            match incoming.await {
                Ok(conn) => {
                    let remote = conn.remote_address();
                    let _client_guard = ActiveClientGuard::new(Arc::clone(&cfg_clone.active_clients));
                    info!("Nueva conexión entrante desde {remote}");
                    if let Err(err) = handle_connection(conn, cfg_clone).await {
                        warn!("Sesión con {remote} finalizada con error: {err:#}");
                    } else {
                        info!("Sesión con {remote} cerrada limpiamente");
                    }
                }
                Err(err) => warn!("Error completando handshake QUIC: {err}"),
            }
        });
    }
}

/// Atiende una conexión QUIC completa: handshake de autenticación + control y video en paralelo.
pub async fn handle_connection(conn: Connection, cfg: HostServerConfig) -> Result<()> {
    let (mut ctrl_send, mut ctrl_recv) = conn
        .accept_bi()
        .await
        .context("cliente cerró antes de abrir stream de control")?;

    // 1. Saludo inicial del cliente
    match read_msg::<_, ClientMsg>(&mut ctrl_recv).await? {
        ClientMsg::Hello { version } if version == PROTOCOL_VERSION => {}
        ClientMsg::Hello { version } => {
            let _ = write_msg(
                &mut ctrl_send,
                &HostMsg::AuthResult {
                    ok: false,
                    reason: Some(format!("Versión incompatible: {version}")),
                },
            )
            .await;
            let _ = ctrl_send.finish();
            let _ = tokio::time::timeout(Duration::from_millis(200), conn.closed()).await;
            return Err(anyhow!("versión de protocolo incompatible: {version}"));
        }
        other => {
            return Err(anyhow!("se esperaba ClientMsg::Hello, llegó {other:?}"));
        }
    }

    // 2. Inicializar capturador para conocer dimensiones del monitor
    let capturer = (cfg.capturer_factory)()?;
    let monitor = capturer.dimensions();
    let nonce = generate_nonce();

    write_msg(
        &mut ctrl_send,
        &HostMsg::AuthChallenge {
            version: PROTOCOL_VERSION,
            nonce,
            host_name: cfg.host_name.clone(),
            width: monitor.width,
            height: monitor.height,
        },
    )
    .await?;

    // 3. Verificar respuesta criptográfica
    match read_msg::<_, ClientMsg>(&mut ctrl_recv).await? {
        ClientMsg::AuthResponse { proof } => {
            let auth_ok = cfg.password.is_empty() || verify_proof(&nonce, &cfg.password, &proof);
            if !auth_ok {
                write_msg(
                    &mut ctrl_send,
                    &HostMsg::AuthResult {
                        ok: false,
                        reason: Some("Contraseña o PIN incorrecto".to_string()),
                    },
                )
                .await?;
                let _ = ctrl_send.finish();
                let _ = tokio::time::timeout(Duration::from_millis(200), conn.closed()).await;
                return Err(anyhow!("autenticación rechazada (contraseña incorrecta)"));
            }
        }
        other => {
            return Err(anyhow!(
                "se esperaba ClientMsg::AuthResponse, llegó {other:?}"
            ));
        }
    }

    write_msg(
        &mut ctrl_send,
        &HostMsg::AuthResult {
            ok: true,
            reason: None,
        },
    )
    .await?;

    // 4. Abrir stream dedicado unidireccional para video (sin Head-of-Line con control)
    let mut video_send = conn
        .open_uni()
        .await
        .context("no se pudo abrir stream unidireccional de video")?;

    let shared_settings = Arc::new(Mutex::new(cfg.default_settings));
    let force_keyframe = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let (frame_tx, mut frame_rx) = mpsc::channel::<HostMsg>(2);

    let capture_thread = spawn_capture_worker(
        capturer,
        Arc::clone(&shared_settings),
        Arc::clone(&force_keyframe),
        Arc::clone(&stop_flag),
        frame_tx,
    );

    let session_res = run_session_loops(
        &conn,
        &mut ctrl_send,
        &mut ctrl_recv,
        &mut video_send,
        &mut frame_rx,
        monitor,
        &shared_settings,
        &force_keyframe,
    )
    .await;

    stop_flag.store(true, Ordering::Relaxed);
    let _ = capture_thread.join();
    session_res
}

#[allow(clippy::too_many_arguments)]
async fn run_session_loops(
    conn: &Connection,
    ctrl_send: &mut quinn::SendStream,
    ctrl_recv: &mut quinn::RecvStream,
    video_send: &mut quinn::SendStream,
    frame_rx: &mut mpsc::Receiver<HostMsg>,
    monitor: MonitorGeometry,
    shared_settings: &Arc<Mutex<StreamSettings>>,
    force_keyframe: &Arc<AtomicBool>,
) -> Result<()> {
    loop {
        tokio::select! {
            biased;
            msg_res = read_msg::<_, ClientMsg>(ctrl_recv) => {
                match msg_res {
                    Ok(ClientMsg::Input(ev)) => {
                        input::inject_input(&ev, monitor);
                    }
                    Ok(ClientMsg::UpdateSettings(new_settings)) => {
                        if let Ok(mut guard) = shared_settings.lock() {
                            if guard.codec != new_settings.codec {
                                force_keyframe.store(true, Ordering::Relaxed);
                            }
                            *guard = new_settings;
                        }
                    }
                    Ok(ClientMsg::RequestKeyframe) => {
                        force_keyframe.store(true, Ordering::Relaxed);
                    }
                    Ok(ClientMsg::ClipboardSet(text)) => {
                        clipboard::set_text(&text);
                    }
                    Ok(ClientMsg::ClipboardGet) => {
                        let text = clipboard::get_text();
                        write_msg(ctrl_send, &HostMsg::ClipboardData(text)).await?;
                    }
                    Ok(ClientMsg::Ping(ts)) => {
                        write_msg(ctrl_send, &HostMsg::Pong(ts)).await?;
                    }
                    Ok(ClientMsg::Disconnect) => {
                        conn.close(0u32.into(), b"cliente desconectado");
                        return Ok(());
                    }
                    Ok(other) => {
                        warn!("Mensaje inesperado en sesión autenticada: {other:?}");
                    }
                    Err(err) => {
                        return Err(anyhow!("stream de control cerrado: {err}"));
                    }
                }
            }
            maybe_frame = frame_rx.recv() => {
                match maybe_frame {
                    Some(frame_msg) => {
                        write_msg(video_send, &frame_msg).await?;
                    }
                    None => {
                        return Err(anyhow!("hilo de captura finalizado inesperadamente"));
                    }
                }
            }
        }
    }
}

pub(crate) fn spawn_capture_worker(
    mut capturer: Box<dyn ScreenCapturer>,
    shared_settings: Arc<Mutex<StreamSettings>>,
    force_keyframe: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    frame_tx: mpsc::Sender<HostMsg>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut encoder = TileEncoder::new();
        let mut last_awake = Instant::now() - Duration::from_secs(60);
        while !stop_flag.load(Ordering::Relaxed) {
            if last_awake.elapsed() >= Duration::from_secs(30) {
                crate::power::keep_session_awake();
                last_awake = Instant::now();
            }
            let settings = shared_settings
                .lock()
                .map(|s| *s)
                .unwrap_or_default();
            let fps = settings.target_fps.clamp(5, 60) as u64;
            let frame_budget = Duration::from_millis(1000 / fps);
            let started = Instant::now();

            let timeout_ms = frame_budget.as_millis().min(100) as u32;
            match capturer.capture(timeout_ms) {
                Ok(Some(captured)) => {
                    let keyframe = force_keyframe.swap(false, Ordering::Relaxed);
                    match encoder.encode(&captured, settings, keyframe) {
                        Ok(Some(msg)) => {
                            if let Err(err) = frame_tx.try_send(msg) {
                                match err {
                                    mpsc::error::TrySendError::Full(_) => {
                                        // Backpressure: si se descartó un keyframe, reintentar en el próximo cuadro.
                                        if keyframe {
                                            force_keyframe.store(true, Ordering::Relaxed);
                                        }
                                    }
                                    mpsc::error::TrySendError::Closed(_) => break,
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(err) => error!("Error codificando frame: {err:#}"),
                    }
                }
                Ok(None) => {
                    // Si el cliente pidió keyframe con pantalla estática, forzamos esperar o invalidar.
                    if force_keyframe.load(Ordering::Relaxed) {
                        encoder.invalidate();
                    }
                }
                Err(err) => {
                    error!("Error en captura de pantalla: {err:#}");
                    std::thread::sleep(Duration::from_millis(200));
                }
            }

            let elapsed = started.elapsed();
            if elapsed < frame_budget {
                std::thread::sleep(frame_budget - elapsed);
            }
        }
    })
}
