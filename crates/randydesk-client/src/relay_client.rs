//! Sesión remota de cliente a través del servidor Relay WebSocket con cifrado E2E.

use crate::decoder::FrameBuffer;
use crate::net::{ConnectRequest, ConnectionState, NetCommand, SessionExit, SharedSessionState};
use anyhow::{anyhow, Context, Result};
use eframe::egui;
use futures_util::{SinkExt, StreamExt};
use randydesk_proto::{
    compute_proof, ClientMsg, E2eCipher, E2eKeypair, HostMsg, PROTOCOL_VERSION,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::info;
use url::Url;

/// Conecta con un host remoto identificado por `target_id` vía `relay_url` usando X25519 + ChaCha20.
pub async fn run_relay_session(
    relay_url: &str,
    target_id: &str,
    req: &mut ConnectRequest,
    cmd_rx: &mut mpsc::Receiver<NetCommand>,
    shared: &Arc<Mutex<SharedSessionState>>,
    ctx: &egui::Context,
    session_start: Instant,
) -> Result<SessionExit> {
    let base = relay_url.trim_end_matches('/');
    let ws_endpoint = format!("{base}/ws/client/{target_id}");
    Url::parse(&ws_endpoint).with_context(|| format!("URL de Relay inválida: {ws_endpoint}"))?;

    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_endpoint)
        .await
        .with_context(|| format!("no se pudo conectar al servidor Relay ({base})"))?;

    // 1. Esperar señal READY o error de disponibilidad del Relay
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(txt))) if txt == "READY" => break,
            Some(Ok(Message::Text(txt))) if txt.starts_with("ERR:") => {
                let reason = txt.trim_start_matches("ERR:").trim().to_string();
                return Ok(SessionExit::AuthFailed(reason));
            }
            Some(Ok(Message::Ping(p))) => {
                ws.send(Message::Pong(p)).await?;
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err(anyhow!("el servidor Relay cerró la conexión antes de emparejar"));
            }
            Some(Err(err)) => return Err(anyhow!("error esperando puente Relay: {err}")),
            _ => {}
        }
    }

    // 2. Handshake E2E X25519 directo con el equipo destino
    let (secret, my_pub) = E2eKeypair::generate();
    let peer_pub = read_binary_32(&mut ws).await?;
    ws.send(Message::Binary(my_pub.to_vec())).await?;
    let mut cipher = E2eCipher::from_handshake(secret, peer_pub, true);

    // 3. Autenticación sobre el túnel cifrado
    send_e2e(
        &mut ws,
        &mut cipher,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION,
        },
    )
    .await?;

    let (host_name, width, height, nonce) = match recv_e2e::<HostMsg>(&mut ws, &mut cipher).await? {
        HostMsg::AuthChallenge {
            version,
            nonce,
            host_name,
            width,
            height,
        } => {
            if version != PROTOCOL_VERSION {
                return Ok(SessionExit::AuthFailed(format!(
                    "Versión incompatible del equipo remoto: {version}"
                )));
            }
            (host_name, width, height, nonce)
        }
        other => return Err(anyhow!("respuesta inesperada al Hello E2E: {other:?}")),
    };

    let proof = compute_proof(&nonce, &req.password);
    send_e2e(&mut ws, &mut cipher, &ClientMsg::AuthResponse { proof }).await?;

    match recv_e2e::<HostMsg>(&mut ws, &mut cipher).await? {
        HostMsg::AuthResult { ok: true, .. } => {}
        HostMsg::AuthResult {
            ok: false, reason, ..
        } => {
            return Ok(SessionExit::AuthFailed(
                reason.unwrap_or_else(|| "Contraseña o PIN incorrecto".to_string()),
            ));
        }
        other => return Err(anyhow!("respuesta inesperada a AuthResponse E2E: {other:?}")),
    }

    send_e2e(&mut ws, &mut cipher, &ClientMsg::UpdateSettings(req.settings)).await?;

    if let Ok(mut guard) = shared.lock() {
        guard.status = ConnectionState::Connected {
            host_name,
            width,
            height,
        };
    }
    ctx.request_repaint();
    info!("Sesión Relay E2E autenticada con ID {target_id}");

    let mut fb = FrameBuffer::new();
    let mut ping_ticker = tokio::time::interval(Duration::from_secs(2));
    let mut ws_ping_ticker = tokio::time::interval(Duration::from_secs(20));
    let mut stat_window_start = Instant::now();
    let mut frames_in_window: u32 = 0;
    let mut bytes_in_window: usize = 0;

    loop {
        tokio::select! {
            biased;
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(NetCommand::Input(ev)) => {
                        send_e2e(&mut ws, &mut cipher, &ClientMsg::Input(ev)).await?;
                    }
                    Some(NetCommand::UpdateSettings(settings)) => {
                        req.settings = settings;
                        send_e2e(&mut ws, &mut cipher, &ClientMsg::UpdateSettings(settings)).await?;
                    }
                    Some(NetCommand::RequestKeyframe) => {
                        send_e2e(&mut ws, &mut cipher, &ClientMsg::RequestKeyframe).await?;
                    }
                    Some(NetCommand::ClipboardSet(text)) => {
                        send_e2e(&mut ws, &mut cipher, &ClientMsg::ClipboardSet(text)).await?;
                    }
                    Some(NetCommand::ClipboardGet) => {
                        send_e2e(&mut ws, &mut cipher, &ClientMsg::ClipboardGet).await?;
                    }
                    Some(NetCommand::Disconnect) | None => {
                        let _ = send_e2e(&mut ws, &mut cipher, &ClientMsg::Disconnect).await;
                        let _ = ws.close(None).await;
                        return Ok(SessionExit::UserRequested);
                    }
                }
            }
            incoming = ws.next() => {
                match incoming {
                    Some(Ok(Message::Binary(data))) => {
                        let msg: HostMsg = cipher.decrypt_msg(&data)?;
                        match msg {
                            HostMsg::Pong(sent_ms) => {
                                let now_ms = session_start.elapsed().as_millis() as u64;
                                let rtt = now_ms.saturating_sub(sent_ms);
                                if let Ok(mut guard) = shared.lock() {
                                    guard.stats.rtt_ms = Some(rtt);
                                    guard.stats.elapsed_secs = session_start.elapsed().as_secs();
                                }
                                ctx.request_repaint();
                            }
                            HostMsg::ClipboardData(text) => {
                                if let Ok(mut guard) = shared.lock() {
                                    guard.incoming_clipboard = Some(text);
                                }
                                ctx.request_repaint();
                            }
                            video @ HostMsg::VideoFrame { .. } => {
                                if let HostMsg::VideoFrame { tiles, .. } = &video {
                                    bytes_in_window += tiles.iter().map(|t| t.data.len()).sum::<usize>();
                                }
                                let decoded = fb.apply_frame(&video)?;
                                frames_in_window += 1;

                                let elapsed_win = stat_window_start.elapsed().as_secs_f32();
                                if let Ok(mut guard) = shared.lock() {
                                    guard.latest_frame = Some(Arc::new(decoded));
                                    guard.frame_seq += 1;
                                    guard.stats.elapsed_secs = session_start.elapsed().as_secs();
                                    if elapsed_win >= 1.0 {
                                        guard.stats.fps = frames_in_window as f32 / elapsed_win;
                                        guard.stats.kbps = (bytes_in_window as f32 * 8.0 / 1000.0) / elapsed_win;
                                        frames_in_window = 0;
                                        bytes_in_window = 0;
                                        stat_window_start = Instant::now();
                                    }
                                }
                                ctx.request_repaint();
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        ws.send(Message::Pong(p)).await?;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        return Err(anyhow!("sesión cerrada por el equipo remoto o Relay"));
                    }
                    Some(Err(err)) => return Err(anyhow!("error WS Relay cliente: {err}")),
                    _ => {}
                }
            }
            _ = ping_ticker.tick() => {
                let now_ms = session_start.elapsed().as_millis() as u64;
                send_e2e(&mut ws, &mut cipher, &ClientMsg::Ping(now_ms)).await?;
            }
            _ = ws_ping_ticker.tick() => {
                ws.send(Message::Ping(vec![])).await?;
            }
        }
    }
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

async fn send_e2e<S, T>(ws: &mut S, cipher: &mut E2eCipher, msg: &T) -> Result<()>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    T: serde::Serialize,
{
    let encrypted = cipher.encrypt_msg(msg)?;
    ws.send(Message::Binary(encrypted)).await?;
    Ok(())
}

async fn recv_e2e<T>(
    ws: &mut (impl StreamExt<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
              + Unpin),
    cipher: &mut E2eCipher,
) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    while let Some(item) = ws.next().await {
        match item? {
            Message::Binary(bytes) => return Ok(cipher.decrypt_msg(&bytes)?),
            Message::Close(_) => return Err(anyhow!("sesión Relay cerrada por el par")),
            _ => {}
        }
    }
    Err(anyhow!("fin de stream Relay"))
}
