//! Servidor Rendezvous + Relay WebSocket zero-knowledge para sesiones E2E de RandyDesk.

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use rand::{rngs::OsRng, RngCore};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use tracing::{info, warn};

struct RegisteredHost {
    conn_id: u64,
    pair_tx: mpsc::UnboundedSender<String>,
}

/// Estado compartido en memoria del servidor Relay.
#[derive(Default)]
pub struct RelayState {
    hosts: RwLock<HashMap<String, RegisteredHost>>,
    pending: RwLock<HashMap<String, oneshot::Sender<WebSocketStream<TcpStream>>>>,
    next_conn_id: AtomicU64,
}

/// Ejecuta el bucle principal del servidor Relay aceptando conexiones WebSocket y healthchecks HTTP.
pub async fn run_relay_listener(listener: TcpListener) -> Result<()> {
    let state = Arc::new(RelayState::default());
    loop {
        let (stream, remote) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(err) = handle_tcp_connection(stream, state_clone).await {
                warn!("Conexión Relay desde {remote} cerrada con error: {err:#}");
            }
        });
    }
}

async fn handle_tcp_connection(mut stream: TcpStream, state: Arc<RelayState>) -> Result<()> {
    let mut peek_buf = [0u8; 1024];
    let n = stream.peek(&mut peek_buf).await?;
    let head = String::from_utf8_lossy(&peek_buf[..n]).to_ascii_lowercase();

    // Responde healthchecks HTTP planos (Render / Cloudflare) sin romper handshakes WebSocket.
    if (head.starts_with("get ") || head.starts_with("head "))
        && head.contains("\r\n\r\n")
        && !head.contains("upgrade: websocket")
    {
        let body = "RandyDesk Relay OK\n";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(resp.as_bytes()).await?;
        stream.shutdown().await?;
        return Ok(());
    }

    let mut path = String::new();
    #[allow(clippy::result_large_err)]
    let ws_stream = tokio_tungstenite::accept_hdr_async(
        stream,
        |req: &Request, resp: Response| -> std::result::Result<Response, ErrorResponse> {
            path = req.uri().path().to_string();
            Ok(resp)
        },
    )
    .await?;

    if let Some(host_id) = path.strip_prefix("/ws/host/") {
        let host_id = host_id.trim_matches('/').to_string();
        if host_id.is_empty() {
            return Ok(());
        }
        handle_host_channel(ws_stream, host_id, state).await?;
    } else if let Some(target_id) = path.strip_prefix("/ws/client/") {
        let target_id = target_id.trim_matches('/').to_string();
        handle_client_channel(ws_stream, target_id, state).await?;
    } else if let Some(session_id) = path.strip_prefix("/ws/bridge/") {
        let session_id = session_id.trim_matches('/').to_string();
        if let Some(tx) = state.pending.write().await.remove(&session_id) {
            let _ = tx.send(ws_stream);
        }
    }

    Ok(())
}

async fn handle_host_channel(
    mut ws: WebSocketStream<TcpStream>,
    host_id: String,
    state: Arc<RelayState>,
) -> Result<()> {
    let conn_id = state.next_conn_id.fetch_add(1, Ordering::Relaxed);
    let (pair_tx, mut pair_rx) = mpsc::unbounded_channel::<String>();

    state.hosts.write().await.insert(
        host_id.clone(),
        RegisteredHost {
            conn_id,
            pair_tx,
        },
    );
    info!("Host registrado en Relay: ID {host_id}");

    loop {
        tokio::select! {
            maybe_session = pair_rx.recv() => {
                match maybe_session {
                    Some(session_id) => {
                        if ws.send(Message::Text(format!("PAIR:{session_id}"))).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            incoming = ws.next() => {
                match incoming {
                    Some(Ok(Message::Ping(payload))) => {
                        if ws.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
        }
    }

    let mut hosts = state.hosts.write().await;
    if let Some(current) = hosts.get(&host_id) {
        if current.conn_id == conn_id {
            hosts.remove(&host_id);
            info!("Host desconectado del Relay: ID {host_id}");
        }
    }
    Ok(())
}

async fn handle_client_channel(
    mut client_ws: WebSocketStream<TcpStream>,
    target_id: String,
    state: Arc<RelayState>,
) -> Result<()> {
    let pair_tx = {
        let hosts = state.hosts.read().await;
        hosts.get(&target_id).map(|h| h.pair_tx.clone())
    };

    let Some(pair_tx) = pair_tx else {
        let _ = client_ws
            .send(Message::Text(
                "ERR:El equipo destino no está conectado al servidor Relay".to_string(),
            ))
            .await;
        let _ = client_ws.close(None).await;
        return Ok(());
    };

    let session_id = random_hex_id();
    let (bridge_tx, bridge_rx) = oneshot::channel();
    state.pending.write().await.insert(session_id.clone(), bridge_tx);

    if pair_tx.send(session_id.clone()).is_err() {
        state.pending.write().await.remove(&session_id);
        let _ = client_ws
            .send(Message::Text(
                "ERR:El equipo destino se desconectó".to_string(),
            ))
            .await;
        let _ = client_ws.close(None).await;
        return Ok(());
    }

    let bridge_ws = match tokio::time::timeout(Duration::from_secs(10), bridge_rx).await {
        Ok(Ok(ws)) => ws,
        _ => {
            state.pending.write().await.remove(&session_id);
            let _ = client_ws
                .send(Message::Text(
                    "ERR:El equipo destino no respondió al puente a tiempo".to_string(),
                ))
                .await;
            let _ = client_ws.close(None).await;
            return Ok(());
        }
    };

    client_ws.send(Message::Text("READY".to_string())).await?;
    info!("Puente E2E establecido para ID {target_id} (sesión {session_id})");

    forward_ws_pair(client_ws, bridge_ws).await;
    Ok(())
}

async fn forward_ws_pair(a: WebSocketStream<TcpStream>, b: WebSocketStream<TcpStream>) {
    let (mut a_tx, mut a_rx) = a.split();
    let (mut b_tx, mut b_rx) = b.split();

    let a_to_b = async {
        while let Some(Ok(msg)) = a_rx.next().await {
            if msg.is_close() {
                break;
            }
            if b_tx.send(msg).await.is_err() {
                break;
            }
        }
        let _ = b_tx.close().await;
    };

    let b_to_a = async {
        while let Some(Ok(msg)) = b_rx.next().await {
            if msg.is_close() {
                break;
            }
            if a_tx.send(msg).await.is_err() {
                break;
            }
        }
        let _ = a_tx.close().await;
    };

    tokio::select! {
        _ = a_to_b => {}
        _ = b_to_a => {}
    }
}

fn random_hex_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
