//! Punto de entrada CLI del agente host RandyDesk.

use anyhow::{Context, Result};
use clap::Parser;
use quinn::Endpoint;
use randydesk_host::capture;
use randydesk_host::session::{accept_loop, HostServerConfig};
use randydesk_host::tls;
use randydesk_proto::{
    discover_public_addr_sync, generate_session_pin, CodecKind, StreamSettings, DEFAULT_PORT,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

#[derive(Debug, Parser)]
#[command(
    name = "randydesk-host",
    about = "Agente remoto RandyDesk (captura DXGI/GDI, QUIC TLS 1.3 y control de entrada)"
)]
struct Cli {
    /// Dirección IP local de escucha.
    #[arg(long, default_value = "0.0.0.0")]
    bind: IpAddr,

    /// Puerto UDP/QUIC de escucha.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// Contraseña fija para acceso remoto (también lee RANDYDESK_PASSWORD). Si se omite, genera PIN temporal.
    #[arg(long)]
    password: Option<String>,

    /// Índice del monitor a compartir (0 = principal).
    #[arg(long, default_value_t = 0)]
    monitor: usize,

    /// Cuadros por segundo objetivo por defecto (5..60).
    #[arg(long, default_value_t = 30)]
    fps: u8,

    /// Calidad JPEG inicial por defecto (35..95).
    #[arg(long, default_value_t = 75)]
    quality: u8,

    /// URL del servidor Relay WebSocket (ej. wss://tu-relay.onrender.com; también lee RANDYDESK_RELAY_URL).
    #[arg(long)]
    relay: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("no se pudo instalar el proveedor criptográfico ring"))?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let raw_password = cli
        .password
        .or_else(|| std::env::var("RANDYDESK_PASSWORD").ok());
    let (password, is_ephemeral_pin) = match raw_password {
        Some(p) if !p.trim().is_empty() => (p, false),
        _ => (generate_session_pin(), true),
    };

    let identity = tls::load_or_create_server_identity()?;
    let addr = SocketAddr::new(cli.bind, cli.port);
    let relay_url = cli
        .relay
        .or_else(|| std::env::var("RANDYDESK_RELAY_URL").ok())
        .and_then(|u| randydesk_proto::normalize_relay_ws_url(&u));

    let std_udp =
        UdpSocket::bind(addr).with_context(|| format!("no se pudo abrir socket UDP en {addr}"))?;
    let actual_port = std_udp.local_addr()?.port();
    let punch_socket = std_udp.try_clone().ok().map(Arc::new);
    let public_endpoint = if relay_url.is_some() && !cli.bind.is_loopback() {
        discover_public_addr_sync(&std_udp, Duration::from_millis(350))
    } else {
        None
    };
    let endpoint = Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(identity.server_config),
        std_udp,
        Arc::new(quinn::TokioRuntime),
    )
    .with_context(|| format!("no se pudo abrir socket QUIC en {addr}"))?;

    let host_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows-Host".to_string());
    let local_ip = detect_local_ip().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));

    let host_id = randydesk_host::relay_agent::load_or_create_host_id();

    println!("============================================================");
    println!("  RandyDesk Host v{} - Listo para sesiones ilimitadas", env!("CARGO_PKG_VERSION"));
    println!("============================================================");
    println!("  Equipo      : {host_name}");
    println!("  ID numérico : {}", randydesk_proto::format_host_id(&host_id));
    println!("  Escucha     : {addr}");
    println!("  IP LAN      : {local_ip}:{actual_port}");
    if let Some(pub_addr) = public_endpoint {
        println!("  P2P STUN    : {pub_addr}");
    }
    if let Some(ref r) = relay_url {
        println!("  Relay       : {r}");
    }
    println!("  Monitor     : {}", cli.monitor);
    println!("  Huella TLS  : {}", identity.fingerprint_sha256);
    if is_ephemeral_pin {
        println!("  PIN sesión  : {password}  (temporal)");
    } else {
        println!("  Contraseña  : [configurada por parámetro/entorno]");
    }
    println!("============================================================");
    println!("  Consejo: con Relay configurado conecta solo con el ID numérico\n");

    let mut direct_endpoints = vec![format!("{local_ip}:{actual_port}")];
    if let Some(pub_addr) = public_endpoint {
        let pub_str = pub_addr.to_string();
        if !direct_endpoints.contains(&pub_str) {
            direct_endpoints.push(pub_str);
        }
    }

    let cfg = HostServerConfig {
        password,
        host_name,
        default_settings: StreamSettings {
            codec: CodecKind::Jpeg,
            jpeg_quality: cli.quality.clamp(35, 95),
            target_fps: cli.fps.clamp(5, 60),
        },
        default_monitor: cli.monitor,
        capturer_factory: Arc::new(capture::create_capturer),
        active_clients: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        direct_endpoints,
        punch_socket,
    };

    let relay_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let relay_connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(r_url) = relay_url {
        tokio::spawn(randydesk_host::relay_agent::run_relay_loop(
            r_url,
            host_id,
            cfg.clone(),
            relay_connected,
            Arc::clone(&relay_stop),
        ));
    }

    info!("Servidor QUIC iniciado en {addr}. Pulsa Ctrl+C para detener.");
    tokio::select! {
        _ = accept_loop(endpoint.clone(), cfg) => {}
        _ = tokio::signal::ctrl_c() => {
            info!("Señal Ctrl+C recibida; cerrando servidor...");
            relay_stop.store(true, std::sync::atomic::Ordering::Relaxed);
            endpoint.close(0u32.into(), b"apagado del host");
        }
    }

    Ok(())
}

fn detect_local_ip() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip())
}
