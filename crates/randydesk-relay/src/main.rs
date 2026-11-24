//! Punto de entrada CLI del servidor Relay de RandyDesk (para Render Free o cloudflared).

use anyhow::{Context, Result};
use clap::Parser;
use randydesk_proto::DEFAULT_RELAY_PORT;
use randydesk_relay::run_relay_listener;
use std::net::{IpAddr, SocketAddr};
use tokio::net::TcpListener;
use tracing::info;

#[derive(Debug, Parser)]
#[command(
    name = "randydesk-relay",
    about = "Servidor Rendezvous + Relay WebSocket zero-knowledge para RandyDesk"
)]
struct Cli {
    /// Dirección de escucha local.
    #[arg(long, default_value = "0.0.0.0")]
    bind: IpAddr,

    /// Puerto TCP de escucha (también lee variable PORT para Render/Railway/Fly).
    #[arg(long)]
    port: Option<u16>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let port = cli
        .port
        .or_else(|| std::env::var("PORT").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(DEFAULT_RELAY_PORT);

    let addr = SocketAddr::new(cli.bind, port);
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("no se pudo abrir puerto TCP {addr}"))?;

    println!("============================================================");
    println!("  RandyDesk Relay v{} - Puente E2E Zero-Knowledge", env!("CARGO_PKG_VERSION"));
    println!("============================================================");
    println!("  Escuchando en : {addr}");
    println!("  Healthcheck   : http://{addr}/health");
    println!("  Prueba rápida : cloudflared tunnel --url http://127.0.0.1:{port}");
    println!("============================================================");

    info!("Servidor Relay listo en {addr}");
    run_relay_listener(listener).await
}
