//! Agente saliente hacia el servidor Relay con cifrado E2E (X25519 + ChaCha20) y WebSocket `TCP_NODELAY`.

use crate::clipboard;
use crate::input;
use crate::session::{spawn_capture_worker, HostServerConfig, LatestFrameSlot};
use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;
use futures_util::{SinkExt, StreamExt};
use randydesk_proto::{
    generate_host_id, generate_nonce, parse_numeric_id, verify_proof, ClientMsg, E2eCipher,
    E2eKeypair, HostMsg, PROTOCOL_VERSION,
};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};
use url::Url;

/// Carga o genera un ID numérico persistente de 9 dígitos para este equipo en `%APPDATA%\RandyDesk\host_id.txt`.
pub fn load_or_create_host_id() -> String {
    if let Some(proj) = ProjectDirs::from("com", "RandyDesk", "RandyDesk") {
        let dir = proj.config_dir();
        let path = dir.join("host_id.txt");
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Some(id) = parse_numeric_id(&raw) {
                return id;
            }
        }
        let id = generate_host_id();
        if fs::create_dir_all(dir).is_ok() {
            let _ = fs::write(&path, &id);
        }
        return id;
    }
    generate_host_id()
}

/// Mantiene registrado el host en el servidor Relay y atiende solicitudes `PAIR:<session_id>`.
pub async fn run_relay_loop(
    relay_url: String,
    host_id: String,
    cfg: HostServerConfig,
    relay_connected: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
) {
    let base = relay_url.trim_end_matches('/').to_string();
    while !stop_flag.load(Ordering::Relaxed) {
        let ws_endpoint = format!("{base}/ws/host/{host_id}");
        if let Err(err) = Url::parse(&ws_endpoint) {
            warn!("URL de Relay inválida ({ws_endpoint}): {err}");
            relay_connected.store(false, Ordering::Relaxed);
            break;
        }

        match tokio_tungstenite::connect_async_with_config(&ws_endpoint, None, true).await {
            Ok((mut ws, _)) => {
                relay_connected.store(true, Ordering::Relaxed);
                info!("Host registrado en Relay {base} con ID {host_id}");
                let mut ping_ticker = tokio::time::interval(Duration::from_secs(20));

                loop {
                    if stop_flag.load(Ordering::Relaxed) {
                        let _ = ws.close(None).await;
                        relay_connected.store(false, Ordering::Relaxed);
                        return;
                    }

                    tokio::select! {
                        incoming = ws.next() => {
                            match incoming {
                                Some(Ok(Message::Text(txt))) => {
                                    if let Some(session_id) = txt.strip_prefix("PAIR:") {
                                        let bridge_url = format!("{base}/ws/bridge/{session_id}");
                                        let cfg_clone = cfg.clone();
                                        tokio::spawn(async move {
                                            if let Err(err) = handle_relay_bridge_session(bridge_url, cfg_clone).await {
                                                warn!("Sesión Relay finalizada con error: {err:#}");
                                            }
                                        });
                                    }
                                }
                                Some(Ok(Message::Ping(p))) => {
                                    if ws.send(Message::Pong(p)).await.is_err() {
                                        break;
                                    }
                                }
                                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                                _ => {}
                            }
                        }
                        _ = ping_ticker.tick() => {
                            crate::power::keep_system_awake();
                            if ws.send(Message::Ping(vec![])).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                relay_connected.store(false, Ordering::Relaxed);
            }
            Err(err) => {
                relay_connected.store(false, Ordering::Relaxed);
                warn!("No se pudo conectar al Relay {base}: {err}");
            }
        }

        for _ in 0..30 {
            if stop_flag.load(Ordering::Relaxed) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    relay_connected.store(false, Ordering::Relaxed);
}

struct ActiveRelayGuard {
    counter: Arc<AtomicUsize>,
}

impl ActiveRelayGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self { counter }
    }
}

impl Drop for ActiveRelayGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

enum OutboundItem {
    Host(HostMsg),
    Pong(Vec<u8>),
    Close,
}

async fn handle_relay_bridge_session(bridge_url: String, cfg: HostServerConfig) -> Result<()> {
    let _active_guard = ActiveRelayGuard::new(Arc::clone(&cfg.active_clients));
    let (mut ws, _) = tokio_tungstenite::connect_async_with_config(&bridge_url, None, true)
        .await
        .with_context(|| format!("error abriendo puente Relay {bridge_url}"))?;

    // 1. Handshake E2E X25519
    let (secret, my_pub) = E2eKeypair::generate();
    ws.send(Message::Binary(my_pub.to_vec())).await?;
    let peer_pub = read_binary_32(&mut ws).await?;
    let mut cipher = E2eCipher::from_handshake(secret, peer_pub, false);

    // 2. Hello cifrado
    match recv_e2e(&mut ws, &mut cipher).await? {
        ClientMsg::Hello { version } if version == PROTOCOL_VERSION => {}
        ClientMsg::Hello { version } => {
            let _ = send_e2e(
                &mut ws,
                &mut cipher,
                &HostMsg::AuthResult {
                    ok: false,
                    reason: Some(format!("Versión incompatible: {version}")),
                },
            )
            .await;
            let _ = ws.close(None).await;
            return Err(anyhow!("versión incompatible: {version}"));
        }
        other => return Err(anyhow!("se esperaba Hello, llegó {other:?}")),
    }

    // 3. Capturador y autenticación (acceso directo si password está vacío)
    let capturer = (cfg.capturer_factory)()?;
    let monitor = capturer.dimensions();
    let nonce = generate_nonce();
    send_e2e(
        &mut ws,
        &mut cipher,
        &HostMsg::AuthChallenge {
            version: PROTOCOL_VERSION,
            nonce,
            host_name: cfg.host_name.clone(),
            width: monitor.width,
            height: monitor.height,
        },
    )
    .await?;

    match recv_e2e(&mut ws, &mut cipher).await? {
        ClientMsg::AuthResponse { proof } => {
            let auth_ok = cfg.password.is_empty() || verify_proof(&nonce, &cfg.password, &proof);
            if !auth_ok {
                let _ = send_e2e(
                    &mut ws,
                    &mut cipher,
                    &HostMsg::AuthResult {
                        ok: false,
                        reason: Some("Contraseña o PIN incorrecto".to_string()),
                    },
                )
                .await;
                let _ = ws.close(None).await;
                return Err(anyhow!("autenticación Relay rechazada"));
            }
        }
        other => return Err(anyhow!("se esperaba AuthResponse, llegó {other:?}")),
    }

    send_e2e(
        &mut ws,
        &mut cipher,
        &HostMsg::AuthResult {
            ok: true,
            reason: None,
        },
    )
    .await?;

    let shared_settings = Arc::new(Mutex::new(cfg.default_settings));
    let force_keyframe = Arc::new(AtomicBool::new(true));
    let stop_capture = Arc::new(AtomicBool::new(false));
    let frame_slot = Arc::new(LatestFrameSlot::new());
    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<OutboundItem>(64);

    let capture_thread = spawn_capture_worker(
        capturer,
        Arc::clone(&shared_settings),
        Arc::clone(&force_keyframe),
        Arc::clone(&stop_capture),
        Arc::clone(&frame_slot),
    );

    let (mut ws_tx, mut ws_rx) = ws.split();
    let (mut enc, mut dec) = cipher.split();
    let writer_slot = Arc::clone(&frame_slot);

    let writer_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                maybe_ctrl = ctrl_rx.recv() => {
                    match maybe_ctrl {
                        Some(OutboundItem::Host(msg)) => {
                            let Ok(bytes) = enc.encrypt_msg(&msg) else { break; };
                            if ws_tx.send(Message::Binary(bytes)).await.is_err() {
                                break;
                            }
                        }
                        Some(OutboundItem::Pong(p)) => {
                            if ws_tx.send(Message::Pong(p)).await.is_err() {
                                break;
                            }
                        }
                        Some(OutboundItem::Close) | None => {
                            let _ = ws_tx.close().await;
                            break;
                        }
                    }
                }
                maybe_frame = writer_slot.recv() => {
                    match maybe_frame {
                        Some(frame_msg) => {
                            let Ok(bytes) = enc.encrypt_msg(&frame_msg) else { break; };
                            if ws_tx.send(Message::Binary(bytes)).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    });

    let session_res: Result<()> = loop {
        match ws_rx.next().await {
            Some(Ok(Message::Binary(data))) => match dec.decrypt_msg::<ClientMsg>(&data) {
                Ok(ClientMsg::Input(ev)) => input::inject_input(&ev, monitor),
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
                Ok(ClientMsg::ClipboardSet(text)) => clipboard::set_text(&text),
                Ok(ClientMsg::ClipboardGet) => {
                    let text = clipboard::get_text();
                    let _ = ctrl_tx.try_send(OutboundItem::Host(HostMsg::ClipboardData(text)));
                }
                Ok(ClientMsg::Ping(ts)) => {
                    let _ = ctrl_tx.try_send(OutboundItem::Host(HostMsg::Pong(ts)));
                }
                Ok(ClientMsg::Disconnect) => {
                    let _ = ctrl_tx.send(OutboundItem::Close).await;
                    break Ok(());
                }
                Ok(other) => warn!("Mensaje inesperado en sesión Relay: {other:?}"),
                Err(err) => break Err(anyhow!("error descifrando mensaje cliente: {err}")),
            },
            Some(Ok(Message::Ping(p))) => {
                let _ = ctrl_tx.try_send(OutboundItem::Pong(p));
            }
            Some(Ok(Message::Close(_))) | None => break Ok(()),
            Some(Err(err)) => break Err(anyhow!("error WS Relay host: {err}")),
            _ => {}
        }
    };

    stop_capture.store(true, Ordering::Relaxed);
    frame_slot.close();
    writer_task.abort();
    let _ = capture_thread.join();
    session_res
}

async fn read_binary_32<S>(ws: &mut S) -> Result<[u8; 32]>
where
    S: StreamExt<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(item) = ws.next().await {
        match item? {
            Message::Binary(bytes) if bytes.len() == 32 => {
                let mut out = [0u8; 32];
                out.copy_from_slice(&bytes);
                return Ok(out);
            }
            Message::Binary(bytes) => {
                return Err(anyhow!("clave pública X25519 inválida ({} bytes)", bytes.len()));
            }
            Message::Close(_) => return Err(anyhow!("puente cerrado durante handshake E2E")),
            _ => {}
        }
    }
    Err(anyhow!("fin de stream durante handshake E2E"))
}

async fn send_e2e<S>(ws: &mut S, cipher: &mut E2eCipher, msg: &HostMsg) -> Result<()>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let encrypted = cipher.encrypt_msg(msg)?;
    ws.send(Message::Binary(encrypted)).await?;
    Ok(())
}

async fn recv_e2e(
    ws: &mut (impl StreamExt<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
              + Unpin),
    cipher: &mut E2eCipher,
) -> Result<ClientMsg> {
    while let Some(item) = ws.next().await {
        match item? {
            Message::Binary(bytes) => return Ok(cipher.decrypt_msg::<ClientMsg>(&bytes)?),
            Message::Close(_) => return Err(anyhow!("sesión Relay cerrada por el par")),
            _ => {}
        }
    }
    Err(anyhow!("fin de stream Relay"))
}
