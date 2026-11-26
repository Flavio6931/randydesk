//! Prueba de integración end-to-end del servidor QUIC del host en loopback `127.0.0.1:0`.

use anyhow::Result;
use quinn::Endpoint;
use randydesk_host::capture::{CapturedFrame, MonitorGeometry, ScreenCapturer};
use randydesk_host::session::{accept_loop, HostServerConfig};
use randydesk_host::tls::{create_ephemeral_server_identity, ALPN_PROTOCOL};
use randydesk_proto::{
    compute_proof, read_msg, write_msg, ClientMsg, CodecKind, HostMsg, StreamSettings,
    PROTOCOL_VERSION,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
struct AcceptAnyCertForTest;

impl ServerCertVerifier for AcceptAnyCertForTest {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

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
            bgra: vec![120u8; 128 * 128 * 4],
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

fn build_test_client_endpoint() -> Result<Endpoint> {
    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCertForTest))
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
    let quic_cfg = quinn::crypto::rustls::QuicClientConfig::try_from(tls)?;
    let mut endpoint = Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(quic_cfg)));
    Ok(endpoint)
}

#[tokio::test]
async fn quic_loopback_rejects_bad_password_and_streams_frame_on_valid_auth() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let identity = create_ephemeral_server_identity()?;
    let server_ep = Endpoint::server(
        identity.server_config,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
    )?;
    let server_addr = server_ep.local_addr()?;

    let cfg = HostServerConfig {
        password: "secreto-test".to_string(),
        host_name: "PC-PRUEBA".to_string(),
        default_settings: StreamSettings {
            codec: CodecKind::Lz4Rgba,
            jpeg_quality: 80,
            target_fps: 30,
        },
        default_monitor: 0,
        capturer_factory: Arc::new(|_| Ok(Box::new(MockCapturer { emitted: false }))),
        active_clients: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        direct_endpoints: Vec::new(),
    };

    let server_task = tokio::spawn(accept_loop(server_ep.clone(), cfg));
    let client_ep = build_test_client_endpoint()?;

    // 1. Intento con contraseña incorrecta debe ser rechazado
    {
        let conn = client_ep.connect(server_addr, "localhost")?.await?;
        let (mut tx, mut rx) = conn.open_bi().await?;
        write_msg(
            &mut tx,
            &ClientMsg::Hello {
                version: PROTOCOL_VERSION,
            },
        )
        .await?;
        let challenge: HostMsg = read_msg(&mut rx).await?;
        let nonce = match challenge {
            HostMsg::AuthChallenge { nonce, .. } => nonce,
            other => panic!("se esperaba AuthChallenge, llegó {other:?}"),
        };
        let wrong_proof = compute_proof(&nonce, "clave-incorrecta");
        write_msg(&mut tx, &ClientMsg::AuthResponse { proof: wrong_proof }).await?;
        let res: HostMsg = read_msg(&mut rx).await?;
        assert!(matches!(res, HostMsg::AuthResult { ok: false, .. }));
    }

    // 2. Intento con contraseña válida debe autenticar, responder Ping->Pong y entregar VideoFrame
    {
        let conn = client_ep.connect(server_addr, "localhost")?.await?;
        let (mut tx, mut rx) = conn.open_bi().await?;
        write_msg(
            &mut tx,
            &ClientMsg::Hello {
                version: PROTOCOL_VERSION,
            },
        )
        .await?;
        let challenge: HostMsg = read_msg(&mut rx).await?;
        let nonce = match challenge {
            HostMsg::AuthChallenge {
                host_name,
                width,
                height,
                nonce,
                ..
            } => {
                assert_eq!(host_name, "PC-PRUEBA");
                assert_eq!((width, height), (128, 128));
                nonce
            }
            other => panic!("se esperaba AuthChallenge, llegó {other:?}"),
        };
        let valid_proof = compute_proof(&nonce, "secreto-test");
        write_msg(&mut tx, &ClientMsg::AuthResponse { proof: valid_proof }).await?;
        let res: HostMsg = read_msg(&mut rx).await?;
        assert!(matches!(res, HostMsg::AuthResult { ok: true, .. }));

        // Ping -> Pong (ignorando mensajes auxiliares como MonitorList/CursorShape)
        write_msg(&mut tx, &ClientMsg::Ping(12345)).await?;
        loop {
            match read_msg::<_, HostMsg>(&mut rx).await? {
                HostMsg::Pong(12345) => break,
                HostMsg::MonitorList { .. } | HostMsg::CursorShape(_) | HostMsg::AudioData(_) => {
                    continue
                }
                other => panic!("se esperaba Pong(12345), llegó {other:?}"),
            }
        }

        // Recibir VideoFrame por el stream unidireccional de video
        let mut video_rx = conn.accept_uni().await?;
        let frame_msg: HostMsg = read_msg(&mut video_rx).await?;
        match frame_msg {
            HostMsg::VideoFrame {
                width,
                height,
                full_refresh,
                tiles,
                ..
            } => {
                assert_eq!((width, height), (128, 128));
                assert!(full_refresh);
                assert_eq!(tiles.len(), 1);
            }
            other => panic!("se esperaba VideoFrame, llegó {other:?}"),
        }

        write_msg(&mut tx, &ClientMsg::Disconnect).await?;
    }

    server_ep.close(0u32.into(), b"fin test");
    server_task.abort();
    Ok(())
}
