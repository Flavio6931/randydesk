//! Cliente QUIC con verificación TLS TOFU, keep-alive para sesiones largas y reconexión automática.

use crate::decoder::{DecodedFrame, FrameBuffer};
use anyhow::{anyhow, Context, Result};
use eframe::egui;
use quinn::{ClientConfig, Endpoint, TransportConfig, VarInt};
use randydesk_proto::{
    compute_proof, parse_numeric_id, read_msg, write_msg, ClientMsg, HostMsg, InputEvent,
    StreamSettings, PROTOCOL_VERSION,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{info, warn};

const ALPN_PROTOCOL: &[u8] = b"randydesk-v1";

/// Solicitud de conexión iniciada desde la interfaz gráfica.
#[derive(Debug, Clone)]
pub struct ConnectRequest {
    pub address: String,
    pub password: String,
    pub expected_fingerprint: Option<String>,
    pub relay_url: Option<String>,
    pub settings: StreamSettings,
}

/// Comandos que la UI envía a la sesión activa en curso.
#[derive(Debug, Clone)]
pub enum NetCommand {
    Input(InputEvent),
    UpdateSettings(StreamSettings),
    RequestKeyframe,
    ClipboardSet(String),
    ClipboardGet,
    Disconnect,
}

/// Estado de la conexión mostrado en la UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting { address: String },
    Connected { host_name: String, width: u32, height: u32 },
    Reconnecting { attempt: u32, reason: String },
    Error(String),
}

/// Métricas vivas de la sesión remota.
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub fps: f32,
    pub kbps: f32,
    pub rtt_ms: Option<u64>,
    pub elapsed_secs: u64,
    pub fingerprint_sha256: Option<String>,
}

/// Estado compartido entre el runtime Tokio y el hilo de renderizado `eframe`.
pub struct SharedSessionState {
    pub status: ConnectionState,
    pub stats: SessionStats,
    pub latest_frame: Option<Arc<DecodedFrame>>,
    pub frame_seq: u64,
    pub incoming_clipboard: Option<String>,
}

impl Default for SharedSessionState {
    fn default() -> Self {
        Self {
            status: ConnectionState::Disconnected,
            stats: SessionStats::default(),
            latest_frame: None,
            frame_seq: 0,
            incoming_clipboard: None,
        }
    }
}

/// Verificador TLS 1.3 basado en TOFU (Trust On First Use) por huella SHA-256 del certificado.
#[derive(Debug)]
struct TofuCertVerifier {
    expected_fingerprint: Option<String>,
    observed_fingerprint: Arc<Mutex<Option<String>>>,
}

impl ServerCertVerifier for TofuCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let fp = format_sha256(end_entity.as_ref());
        if let Some(expected) = &self.expected_fingerprint {
            if !expected.eq_ignore_ascii_case(&fp) {
                return Err(rustls::Error::General(format!(
                    "Huella TLS cambió (posible MITM). Esperada: {expected}, recibida: {fp}"
                )));
            }
        }
        if let Ok(mut guard) = self.observed_fingerprint.lock() {
            *guard = Some(fp);
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Controlador usado por la UI para iniciar o cerrar sesiones sin bloquear el hilo gráfico.
pub struct NetworkHandle {
    pub shared: Arc<Mutex<SharedSessionState>>,
    rt_handle: tokio::runtime::Handle,
    active_task: Option<tokio::task::JoinHandle<()>>,
    cmd_tx: Option<mpsc::Sender<NetCommand>>,
    last_mouse_move: Instant,
}

impl NetworkHandle {
    /// Inicializa el controlador asociado al runtime Tokio de fondo.
    pub fn new(rt_handle: tokio::runtime::Handle) -> Self {
        Self {
            shared: Arc::new(Mutex::new(SharedSessionState::default())),
            rt_handle,
            active_task: None,
            cmd_tx: None,
            last_mouse_move: Instant::now() - Duration::from_secs(1),
        }
    }

    /// Inicia una nueva conexión (cancelando cualquier sesión previa).
    pub fn connect(&mut self, req: ConnectRequest, ctx: egui::Context) {
        self.disconnect();
        let (cmd_tx, cmd_rx) = mpsc::channel(256);
        self.cmd_tx = Some(cmd_tx);
        let shared = Arc::clone(&self.shared);
        if let Ok(mut guard) = shared.lock() {
            *guard = SharedSessionState {
                status: ConnectionState::Connecting {
                    address: req.address.clone(),
                },
                ..Default::default()
            };
        }
        ctx.request_repaint();

        let handle = self
            .rt_handle
            .spawn(session_supervisor(req, cmd_rx, shared, ctx));
        self.active_task = Some(handle);
    }

    /// Envía un evento de entrada aplicando throttle a ~90 Hz para `MouseMove`.
    pub fn send_input(&mut self, ev: InputEvent) {
        if matches!(ev, InputEvent::MouseMove { .. }) {
            let now = Instant::now();
            if now.duration_since(self.last_mouse_move) < Duration::from_millis(11) {
                return;
            }
            self.last_mouse_move = now;
        }
        self.send_cmd(NetCommand::Input(ev));
    }

    /// Envía un comando general hacia la sesión activa.
    pub fn send_cmd(&self, cmd: NetCommand) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.try_send(cmd);
        }
    }

    /// Cierra la sesión actual de forma limpia y detiene la reconexión automática.
    pub fn disconnect(&mut self) {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.try_send(NetCommand::Disconnect);
        }
        if let Some(task) = self.active_task.take() {
            task.abort();
        }
        if let Ok(mut guard) = self.shared.lock() {
            guard.status = ConnectionState::Disconnected;
            guard.latest_frame = None;
        }
    }
}

async fn session_supervisor(
    mut req: ConnectRequest,
    mut cmd_rx: mpsc::Receiver<NetCommand>,
    shared: Arc<Mutex<SharedSessionState>>,
    ctx: egui::Context,
) {
    let mut attempt: u32 = 0;
    let session_start = Instant::now();

    loop {
        let res = run_single_session(
            &mut req,
            &mut cmd_rx,
            &shared,
            &ctx,
            session_start,
        )
        .await;

        match res {
            Ok(SessionExit::UserRequested) => {
                if let Ok(mut guard) = shared.lock() {
                    guard.status = ConnectionState::Disconnected;
                }
                ctx.request_repaint();
                break;
            }
            Ok(SessionExit::AuthFailed(reason)) => {
                if let Ok(mut guard) = shared.lock() {
                    guard.status = ConnectionState::Error(reason);
                }
                ctx.request_repaint();
                break;
            }
            Err(err) => {
                attempt += 1;
                let backoff_secs = match attempt {
                    1 => 1,
                    2 => 2,
                    _ => 5,
                };
                warn!("Conexión interrumpida ({err:#}); reintentando en {backoff_secs}s");
                if let Ok(mut guard) = shared.lock() {
                    guard.status = ConnectionState::Reconnecting {
                        attempt,
                        reason: format!("{err}"),
                    };
                }
                ctx.request_repaint();

                // Permite cancelar inmediatamente mientras espera el backoff
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
                    cmd = cmd_rx.recv() => {
                        match cmd {
                            Some(NetCommand::Disconnect) | None => {
                                if let Ok(mut guard) = shared.lock() {
                                    guard.status = ConnectionState::Disconnected;
                                }
                                ctx.request_repaint();
                                break;
                            }
                            Some(NetCommand::UpdateSettings(s)) => req.settings = s,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

pub(crate) enum SessionExit {
    UserRequested,
    AuthFailed(String),
}

async fn run_single_session(
    req: &mut ConnectRequest,
    cmd_rx: &mut mpsc::Receiver<NetCommand>,
    shared: &Arc<Mutex<SharedSessionState>>,
    ctx: &egui::Context,
    session_start: Instant,
) -> Result<SessionExit> {
    if let Some(target_id) = parse_numeric_id(&req.address) {
        let Some(relay_url) = req.relay_url.clone() else {
            return Ok(SessionExit::AuthFailed(
                "Para conectar con un ID de 9 dígitos, configura primero la URL del servidor Relay abajo en 'Configurar servidor'.".to_string(),
            ));
        };
        return crate::relay_client::run_relay_session(
            &relay_url,
            &target_id,
            req,
            cmd_rx,
            shared,
            ctx,
            session_start,
        )
        .await;
    }

    let addr: SocketAddr = tokio::net::lookup_host(&req.address)
        .await
        .with_context(|| format!("no se pudo resolver {}", req.address))?
        .next()
        .ok_or_else(|| anyhow!("dirección sin resultados DNS: {}", req.address))?;

    let observed_fp = Arc::new(Mutex::new(None));
    let verifier = Arc::new(TofuCertVerifier {
        expected_fingerprint: req.expected_fingerprint.clone(),
        observed_fingerprint: Arc::clone(&observed_fp),
    });

    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];

    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)?;
    let mut client_cfg = ClientConfig::new(Arc::new(quic_crypto));
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(5)));
    transport.max_idle_timeout(Some(VarInt::from_u32(60_000).into()));
    transport.receive_window(VarInt::from_u32(8 * 1024 * 1024));
    transport.stream_receive_window(VarInt::from_u32(4 * 1024 * 1024));
    client_cfg.transport_config(Arc::new(transport));

    let bind_addr = match addr {
        SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), 0),
    };
    let mut endpoint = Endpoint::client(bind_addr)?;
    endpoint.set_default_client_config(client_cfg);

    let conn = endpoint
        .connect(addr, "randydesk.local")?
        .await
        .with_context(|| {
            format!(
                "sin respuesta en {addr} (si está en otra red usa su ID numérico vía Relay o revisa Firewall UDP)"
            )
        })?;

    let fingerprint = observed_fp
        .lock()
        .ok()
        .and_then(|g| g.clone());
    if req.expected_fingerprint.is_none() {
        req.expected_fingerprint = fingerprint.clone();
    }

    let (mut ctrl_send, mut ctrl_recv) = conn.open_bi().await?;
    write_msg(
        &mut ctrl_send,
        &ClientMsg::Hello {
            version: PROTOCOL_VERSION,
        },
    )
    .await?;

    let (host_name, width, height, nonce) = match read_msg::<_, HostMsg>(&mut ctrl_recv).await? {
        HostMsg::AuthChallenge {
            version,
            nonce,
            host_name,
            width,
            height,
        } => {
            if version != PROTOCOL_VERSION {
                return Ok(SessionExit::AuthFailed(format!(
                    "Versión incompatible del host: {version}"
                )));
            }
            (host_name, width, height, nonce)
        }
        other => return Err(anyhow!("respuesta inesperada al Hello: {other:?}")),
    };

    let proof = compute_proof(&nonce, &req.password);
    write_msg(&mut ctrl_send, &ClientMsg::AuthResponse { proof }).await?;

    match read_msg::<_, HostMsg>(&mut ctrl_recv).await? {
        HostMsg::AuthResult { ok: true, .. } => {}
        HostMsg::AuthResult {
            ok: false, reason, ..
        } => {
            return Ok(SessionExit::AuthFailed(
                reason.unwrap_or_else(|| "Autenticación rechazada".to_string()),
            ));
        }
        other => return Err(anyhow!("respuesta inesperada a AuthResponse: {other:?}")),
    }

    // Enviar preferencias iniciales de códec/calidad/FPS
    write_msg(&mut ctrl_send, &ClientMsg::UpdateSettings(req.settings)).await?;

    let mut video_recv = conn
        .accept_uni()
        .await
        .context("el host no abrió stream de video")?;

    if let Ok(mut guard) = shared.lock() {
        guard.status = ConnectionState::Connected {
            host_name,
            width,
            height,
        };
        guard.stats.fingerprint_sha256 = fingerprint;
    }
    ctx.request_repaint();
    info!("Sesión QUIC autenticada con {}", req.address);

    let mut fb = FrameBuffer::new();
    let mut ping_ticker = tokio::time::interval(Duration::from_secs(2));
    let mut stat_window_start = Instant::now();
    let mut frames_in_window: u32 = 0;
    let mut bytes_in_window: usize = 0;

    loop {
        tokio::select! {
            biased;
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(NetCommand::Input(ev)) => {
                        write_msg(&mut ctrl_send, &ClientMsg::Input(ev)).await?;
                    }
                    Some(NetCommand::UpdateSettings(settings)) => {
                        req.settings = settings;
                        write_msg(&mut ctrl_send, &ClientMsg::UpdateSettings(settings)).await?;
                    }
                    Some(NetCommand::RequestKeyframe) => {
                        write_msg(&mut ctrl_send, &ClientMsg::RequestKeyframe).await?;
                    }
                    Some(NetCommand::ClipboardSet(text)) => {
                        write_msg(&mut ctrl_send, &ClientMsg::ClipboardSet(text)).await?;
                    }
                    Some(NetCommand::ClipboardGet) => {
                        write_msg(&mut ctrl_send, &ClientMsg::ClipboardGet).await?;
                    }
                    Some(NetCommand::Disconnect) | None => {
                        let _ = write_msg(&mut ctrl_send, &ClientMsg::Disconnect).await;
                        conn.close(0u32.into(), b"cierre de usuario");
                        return Ok(SessionExit::UserRequested);
                    }
                }
            }
            ctrl_msg = read_msg::<_, HostMsg>(&mut ctrl_recv) => {
                match ctrl_msg? {
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
                    _ => {}
                }
            }
            video_msg = read_msg::<_, HostMsg>(&mut video_recv) => {
                let msg = video_msg?;
                if let HostMsg::VideoFrame { tiles, .. } = &msg {
                    bytes_in_window += tiles.iter().map(|t| t.data.len()).sum::<usize>();
                }
                let decoded = fb.apply_frame(&msg)?;
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
            _ = ping_ticker.tick() => {
                let now_ms = session_start.elapsed().as_millis() as u64;
                write_msg(&mut ctrl_send, &ClientMsg::Ping(now_ms)).await?;
            }
        }
    }
}

fn format_sha256(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}
