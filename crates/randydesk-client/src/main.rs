//! Punto de entrada unificado de RandyDesk (anfitrión + visor gráfico en un solo binario).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod decoder;
mod host_panel;
mod keymap;
mod net;

use anyhow::Result;
use app::RandyDeskApp;
use eframe::egui;

fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("no se pudo instalar el proveedor criptográfico ring"))?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let rt_handle = runtime.handle().clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 760.0])
            .with_min_inner_size([900.0, 580.0]),
        ..Default::default()
    };

    eframe::run_native(
        "RandyDesk — Escritorio Remoto",
        options,
        Box::new(move |cc| Ok(Box::new(RandyDeskApp::new(cc, rt_handle)))),
    )
    .map_err(|err| anyhow::anyhow!("error ejecutando interfaz gráfica: {err}"))?;

    Ok(())
}
