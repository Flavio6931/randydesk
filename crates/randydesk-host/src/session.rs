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
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tracing::{error, info, warn};

/// Slot de último cuadro sin cola FIFO para evitar lag acumulado y preservar keyframes pendientes.
pub(crate) struct LatestFrameSlot {
    pending: Mutex<Option<HostMsg>>,
    closed: AtomicBool,
    notify: Notify,
}

impl LatestFrameSlot {
    /// Crea un nuevo slot vacío abierto para entrega de cuadros.
    pub(crate) fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            closed: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }

    /// Inserta el cuadro más reciente; devuelve `false` si el slot ya fue cerrado.
    ///
    /// Si había un cuadro pendiente sin enviar, combina los tiles (prevaleciendo los nuevos)
    /// y preserva `full_refresh = true` cuando el cuadro reemplazado era un keyframe.
    pub(crate) fn push(&self, mut msg: HostMsg) -> bool {
        if self.closed.load(Ordering::Acquire) {
            return false;
        }
        {
            let mut guard = match self.pending.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            if self.closed.load(Ordering::Acquire) {
                return false;
            }
            if let Some(prev) = guard.take() {
                merge_video_frames(prev, &mut msg);
            }
            *guard = Some(msg);
        }
        self.notify.notify_one();
        true
    }

    /// Espera y extrae el cuadro más reciente, o devuelve `None` si el slot cerró sin pendientes.
    pub(crate) async fn recv(&self) -> Option<HostMsg> {
        loop {
            let notified = self.notify.notified();
            if let Some(msg) = self.take_pending() {
                return Some(msg);
            }
            if self.closed.load(Ordering::Acquire) {
                return None;
            }
            notified.await;
        }
    }

    /// Marca el slot como cerrado, limpia cualquier cuadro pendiente y despierta al receptor.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut guard) = self.pending.lock() {
            *guard = None;
        }
        self.notify.notify_one();
        self.notify.notify_waiters();
    }

    fn take_pending(&self) -> Option<HostMsg> {
        let mut guard = match self.pending.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.take()
    }
}

fn merge_video_frames(prev: HostMsg, next: &mut HostMsg) {
    let HostMsg::VideoFrame {
        width: prev_w,
        height: prev_h,
        full_refresh: prev_full,
        tiles: prev_tiles,
        ..
    } = prev
    else {
        return;
    };

    let HostMsg::VideoFrame {
        width: next_w,
        height: next_h,
        full_refresh: next_full,
        tiles: next_tiles,
        ..
    } = next
    else {
        return;
    };

    let next_was_full = *next_full;
    if prev_full {
        *next_full = true;
    }

    if prev_w != *next_w || prev_h != *next_h || next_was_full {
        return;
    }

    let mut merged = HashMap::with_capacity(prev_tiles.len() + next_tiles.len());
    for tile in prev_tiles {
        merged.insert((tile.x, tile.y), tile);
    }
    for tile in std::mem::take(next_tiles) {
        merged.insert((tile.x, tile.y), tile);
    }
    let mut out: Vec<_> = merged.into_values().collect();
    out.sort_unstable_by_key(|t| (t.y, t.x));
    *next_tiles = out;
}

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
    let scale_percent = Arc::new(AtomicU8::new(100));
    let force_keyframe = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let frame_slot = Arc::new(LatestFrameSlot::new());

    let capture_thread = spawn_capture_worker(
        capturer,
        Arc::clone(&shared_settings),
        Arc::clone(&scale_percent),
        Arc::clone(&force_keyframe),
        Arc::clone(&stop_flag),
        Arc::clone(&frame_slot),
    );

    let session_res = run_session_loops(
        &conn,
        &mut ctrl_send,
        &mut ctrl_recv,
        &mut video_send,
        &frame_slot,
        monitor,
        &shared_settings,
        &scale_percent,
        &force_keyframe,
    )
    .await;

    stop_flag.store(true, Ordering::Relaxed);
    frame_slot.close();
    let _ = capture_thread.join();
    session_res
}

#[allow(clippy::too_many_arguments)]
async fn run_session_loops(
    conn: &Connection,
    ctrl_send: &mut quinn::SendStream,
    ctrl_recv: &mut quinn::RecvStream,
    video_send: &mut quinn::SendStream,
    frame_slot: &LatestFrameSlot,
    monitor: MonitorGeometry,
    shared_settings: &Arc<Mutex<StreamSettings>>,
    scale_percent: &Arc<AtomicU8>,
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
                    Ok(ClientMsg::SetStreamScale(pct)) => {
                        let clamped = pct.clamp(50, 100);
                        if scale_percent.swap(clamped, Ordering::Relaxed) != clamped {
                            force_keyframe.store(true, Ordering::Relaxed);
                        }
                    }
                    Ok(ClientMsg::LockWorkStation) => {
                        let _ = crate::power::lock_workstation();
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
            maybe_frame = frame_slot.recv() => {
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

struct SlotCloseGuard(Arc<LatestFrameSlot>);

impl Drop for SlotCloseGuard {
    fn drop(&mut self) {
        self.0.close();
    }
}

pub(crate) fn spawn_capture_worker(
    mut capturer: Box<dyn ScreenCapturer>,
    shared_settings: Arc<Mutex<StreamSettings>>,
    scale_percent: Arc<AtomicU8>,
    force_keyframe: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    frame_slot: Arc<LatestFrameSlot>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let _slot_guard = SlotCloseGuard(Arc::clone(&frame_slot));
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
            let scale = scale_percent.load(Ordering::Relaxed);
            let fps = settings.target_fps.clamp(5, 60) as u64;
            let frame_budget = Duration::from_millis(1000 / fps);
            let started = Instant::now();

            let timeout_ms = frame_budget.as_millis().min(100) as u32;
            match capturer.capture(timeout_ms) {
                Ok(Some(captured)) => {
                    let keyframe = force_keyframe.swap(false, Ordering::Relaxed);
                    match encoder.encode_scaled(&captured, settings, scale, keyframe) {
                        Ok(Some(msg)) => {
                            if !frame_slot.push(msg) {
                                break;
                            }
                        }
                        Ok(None) => {}
                        Err(err) => error!("Error codificando frame: {err:#}"),
                    }
                    capturer.recycle_buffer(captured.bgra);
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

#[cfg(test)]
mod tests {
    use super::*;
    use randydesk_proto::{CodecKind, EncodedTile};

    fn make_frame(frame_id: u64, full_refresh: bool, coords: &[(u32, u32, u8)]) -> HostMsg {
        HostMsg::VideoFrame {
            frame_id,
            width: 256,
            height: 256,
            full_refresh,
            tiles: coords
                .iter()
                .map(|&(x, y, val)| EncodedTile {
                    x,
                    y,
                    width: 128,
                    height: 128,
                    codec: CodecKind::Jpeg,
                    data: vec![val],
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn latest_frame_slot_drops_lag_preserves_keyframe_and_merges_tiles() {
        let slot = LatestFrameSlot::new();

        // Keyframe inicial con 2 tiles pendiente, sobrescrito por delta antes de ser consumido
        assert!(slot.push(make_frame(1, true, &[(0, 0, 1), (128, 0, 2)])));
        assert!(slot.push(make_frame(2, false, &[(128, 0, 20), (0, 128, 3)])));

        let msg = slot.recv().await.expect("debe entregar frame fusionado");
        match msg {
            HostMsg::VideoFrame {
                frame_id,
                full_refresh,
                tiles,
                ..
            } => {
                assert_eq!(frame_id, 2);
                assert!(full_refresh, "debe preservar full_refresh del keyframe sobrescrito");
                assert_eq!(tiles.len(), 3);
                assert_eq!(tiles[0].data, vec![1]);
                assert_eq!(tiles[1].data, vec![20]);
                assert_eq!(tiles[2].data, vec![3]);
            }
            other => panic!("mensaje inesperado: {other:?}"),
        }

        // Un nuevo full_refresh reemplaza por completo un delta previo
        assert!(slot.push(make_frame(3, false, &[(0, 0, 5)])));
        assert!(slot.push(make_frame(4, true, &[(128, 128, 9)])));
        let msg2 = slot.recv().await.expect("debe entregar frame 4");
        match msg2 {
            HostMsg::VideoFrame {
                frame_id,
                full_refresh,
                tiles,
                ..
            } => {
                assert_eq!(frame_id, 4);
                assert!(full_refresh);
                assert_eq!(tiles.len(), 1);
                assert_eq!(tiles[0].data, vec![9]);
            }
            other => panic!("mensaje inesperado: {other:?}"),
        }

        slot.close();
        assert!(!slot.push(make_frame(5, false, &[(0, 0, 1)])));
        assert!(slot.recv().await.is_none());
    }
}
