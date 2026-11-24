//! Prueba de integración end-to-end del servidor Relay con cifrado E2E X25519 + ChaCha20.

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use randydesk_host::capture::{CapturedFrame, MonitorGeometry, ScreenCapturer};
use randydesk_host::relay_agent::run_relay_loop;
use randydesk_host::session::HostServerConfig;
use randydesk_proto::{
    compute_proof, ClientMsg, CodecKind, E2eCipher, E2eKeypair, HostMsg, StreamSettings,
    PROTOCOL_VERSION,
};
use randydesk_relay::run_relay_listener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

struct MockCapturer {
    emitted: bool,
}

impl ScreenCapturer for MockCapturer {
    fn capture(&mut self, _timeout_ms: u32) -> Result<Option<CapturedFrame>> {
        if self.emitted {
            std::thread::sleep(Duration::from_millis(20));
            return Ok(None);
        }
        self.emitted = true;
        Ok(Some(CapturedFrame {
            width: 128,
            height: 128,
            bgra: vec![99u8; 128 * 128 * 4],
        }))
    }

    fn dimensions(&self) -> MonitorGeometry {
        MonitorGeometry {
            origin_x: 0,
            origin_y: 0,
            width: 128,
            height: 128,
        }
    }
}

#[tokio::test]
async fn relay_healthcheck_and_e2e_session_work_end_to_end() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let relay_task = tokio::spawn(async move {
        let _ = run_relay_listener(listener).await;
    });

    // 1. Verificar healthcheck HTTP plano
    {
        let mut sock = TcpStream::connect(addr).await?;
        sock.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await?;
        let mut resp = String::new();
        sock.read_to_string(&mut resp).await?;
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("RandyDesk Relay OK"));
    }

    // 2. Registrar host con MockCapturer en el Relay
    let host_id = "482910331".to_string();
    let relay_ws = format!("ws://{addr}");
    let relay_connected = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let cfg = HostServerConfig {
        password: "clave-relay-e2e".to_string(),
        host_name: "PC-COMPAÑERO".to_string(),
        default_settings: StreamSettings {
            codec: CodecKind::Lz4Rgba,
            jpeg_quality: 80,
            target_fps: 30,
        },
        capturer_factory: Arc::new(|| Ok(Box::new(MockCapturer { emitted: false }))),
        active_clients: Arc::new(AtomicUsize::new(0)),
    };

    let host_task = tokio::spawn(run_relay_loop(
        relay_ws.clone(),
        host_id.clone(),
        cfg,
        Arc::clone(&relay_connected),
        Arc::clone(&stop_flag),
    ));

    // Esperar registro del host
    for _ in 0..40 {
        if relay_connected.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(relay_connected.load(Ordering::Relaxed));

    // 3. Conectar cliente con clave incorrecta -> rechazo
    {
        let (mut ws, _) =
            tokio_tungstenite::connect_async(format!("{relay_ws}/ws/client/{host_id}")).await?;
        let ready = ws.next().await.expect("ready msg")?;
        assert_eq!(ready, Message::Text("READY".to_string()));

        let (c_sec, c_pub) = E2eKeypair::generate();
        let h_pub = match ws.next().await.expect("h_pub")? {
            Message::Binary(b) => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&b);
                arr
            }
            other => panic!("inesperado: {other:?}"),
        };
        ws.send(Message::Binary(c_pub.to_vec())).await?;
        let mut cipher = E2eCipher::from_handshake(c_sec, h_pub, true);

        ws.send(Message::Binary(
            cipher.encrypt_msg(&ClientMsg::Hello {
                version: PROTOCOL_VERSION,
            })?,
        ))
        .await?;

        let nonce = match next_e2e(&mut ws, &mut cipher).await? {
            HostMsg::AuthChallenge { nonce, .. } => nonce,
            other => panic!("se esperaba AuthChallenge, llegó {other:?}"),
        };
        let bad_proof = compute_proof(&nonce, "clave-mala");
        ws.send(Message::Binary(
            cipher.encrypt_msg(&ClientMsg::AuthResponse { proof: bad_proof })?,
        ))
        .await?;
        let res: HostMsg = next_e2e(&mut ws, &mut cipher).await?;
        assert!(matches!(res, HostMsg::AuthResult { ok: false, .. }));
    }

    // 4. Conectar cliente con clave válida -> Ping/Pong + VideoFrame cifrado E2E
    {
        let (mut ws, _) =
            tokio_tungstenite::connect_async(format!("{relay_ws}/ws/client/{host_id}")).await?;
        let ready = ws.next().await.expect("ready msg")?;
        assert_eq!(ready, Message::Text("READY".to_string()));

        let (c_sec, c_pub) = E2eKeypair::generate();
        let h_pub = match ws.next().await.expect("h_pub")? {
            Message::Binary(b) => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&b);
                arr
            }
            other => panic!("inesperado: {other:?}"),
        };
        ws.send(Message::Binary(c_pub.to_vec())).await?;
        let mut cipher = E2eCipher::from_handshake(c_sec, h_pub, true);

        ws.send(Message::Binary(
            cipher.encrypt_msg(&ClientMsg::Hello {
                version: PROTOCOL_VERSION,
            })?,
        ))
        .await?;

        let nonce = match next_e2e(&mut ws, &mut cipher).await? {
            HostMsg::AuthChallenge {
                host_name,
                width,
                height,
                nonce,
                ..
            } => {
                assert_eq!(host_name, "PC-COMPAÑERO");
                assert_eq!((width, height), (128, 128));
                nonce
            }
            other => panic!("se esperaba AuthChallenge, llegó {other:?}"),
        };

        let good_proof = compute_proof(&nonce, "clave-relay-e2e");
        ws.send(Message::Binary(
            cipher.encrypt_msg(&ClientMsg::AuthResponse { proof: good_proof })?,
        ))
        .await?;
        let auth_res: HostMsg = next_e2e(&mut ws, &mut cipher).await?;
        assert!(matches!(auth_res, HostMsg::AuthResult { ok: true, .. }));

        ws.send(Message::Binary(cipher.encrypt_msg(&ClientMsg::Ping(777))?))
            .await?;

        let mut got_pong = false;
        let mut got_frame = false;
        while !(got_pong && got_frame) {
            match next_e2e(&mut ws, &mut cipher).await? {
                HostMsg::Pong(777) => got_pong = true,
                HostMsg::VideoFrame {
                    width,
                    height,
                    tiles,
                    ..
                } => {
                    assert_eq!((width, height), (128, 128));
                    assert_eq!(tiles.len(), 1);
                    got_frame = true;
                }
                _ => {}
            }
        }

        ws.send(Message::Binary(
            cipher.encrypt_msg(&ClientMsg::Disconnect)?,
        ))
        .await?;
    }

    stop_flag.store(true, Ordering::Relaxed);
    host_task.abort();

    // 5. Acceso directo sin contraseña cuando el host tiene password vacío ("")
    let open_id = "111222333".to_string();
    let open_connected = Arc::new(AtomicBool::new(false));
    let open_stop = Arc::new(AtomicBool::new(false));
    let open_cfg = HostServerConfig {
        password: String::new(),
        host_name: "PC-ABIERTA".to_string(),
        default_settings: StreamSettings::default(),
        capturer_factory: Arc::new(|| Ok(Box::new(MockCapturer { emitted: false }))),
        active_clients: Arc::new(AtomicUsize::new(0)),
    };
    let open_task = tokio::spawn(run_relay_loop(
        relay_ws.clone(),
        open_id.clone(),
        open_cfg,
        Arc::clone(&open_connected),
        Arc::clone(&open_stop),
    ));
    for _ in 0..40 {
        if open_connected.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(open_connected.load(Ordering::Relaxed));

    {
        let (mut ws, _) =
            tokio_tungstenite::connect_async(format!("{relay_ws}/ws/client/{open_id}")).await?;
        assert_eq!(ws.next().await.expect("ready")?, Message::Text("READY".to_string()));
        let (c_sec, c_pub) = E2eKeypair::generate();
        let h_pub: [u8; 32] = match ws.next().await.expect("h_pub")? {
            Message::Binary(b) => b.try_into().expect("32 bytes"),
            other => panic!("inesperado: {other:?}"),
        };
        ws.send(Message::Binary(c_pub.to_vec())).await?;
        let mut cipher = E2eCipher::from_handshake(c_sec, h_pub, true);
        ws.send(Message::Binary(
            cipher.encrypt_msg(&ClientMsg::Hello {
                version: PROTOCOL_VERSION,
            })?,
        ))
        .await?;
        let nonce = match next_e2e(&mut ws, &mut cipher).await? {
            HostMsg::AuthChallenge { nonce, .. } => nonce,
            other => panic!("inesperado: {other:?}"),
        };
        let empty_proof = compute_proof(&nonce, "");
        ws.send(Message::Binary(
            cipher.encrypt_msg(&ClientMsg::AuthResponse { proof: empty_proof })?,
        ))
        .await?;
        let auth_res = next_e2e(&mut ws, &mut cipher).await?;
        assert!(matches!(auth_res, HostMsg::AuthResult { ok: true, .. }));
        ws.send(Message::Binary(cipher.encrypt_msg(&ClientMsg::Disconnect)?))
            .await?;
    }

    open_stop.store(true, Ordering::Relaxed);
    open_task.abort();
    relay_task.abort();
    Ok(())
}

async fn next_e2e(
    ws: &mut (impl StreamExt<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
              + Unpin),
    cipher: &mut E2eCipher,
) -> Result<HostMsg> {
    while let Some(item) = ws.next().await {
        if let Message::Binary(bytes) = item? {
            return Ok(cipher.decrypt_msg(&bytes)?);
        }
    }
    anyhow::bail!("stream cerrado inesperadamente")
}
