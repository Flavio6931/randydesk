//! Punto de entrada unificado de RandyDesk (GUI, servicio Windows SYSTEM, worker e instalador CLI).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod cards;
mod config;
mod decoder;
mod file_manager;
mod host_panel;
mod keymap;
mod net;
mod relay_client;
mod session_bar;
mod theme;
mod tray;
mod updater;
mod win_hook;

use anyhow::Result;
use app::RandyDeskApp;
use eframe::egui;
use std::path::PathBuf;

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--service") {
        init_logging();
        return randydesk_host::win_service::run_windows_service();
    }

    if args.iter().any(|a| a == "--install-service") {
        init_logging();
        let host_id = arg_value(&args, "--host-id");
        let source_dir = arg_value(&args, "--source-dir").map(PathBuf::from);
        return randydesk_host::win_service::install_service_now(
            host_id.as_deref(),
            source_dir.as_deref(),
        );
    }

    if args.iter().any(|a| a == "--uninstall-service") {
        init_logging();
        return randydesk_host::win_service::uninstall_service_now();
    }

    if args.iter().any(|a| a == "--apply-update") {
        init_logging();
        let target = arg_value(&args, "--target")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("falta --target en --apply-update"))?;
        let wait_pid = arg_value(&args, "--wait-pid")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let reinstall_service = args.iter().any(|a| a == "--reinstall-service");
        return updater::run_apply_update(&target, wait_pid, reinstall_service);
    }

    if let Some(cleanup_path) = arg_value(&args, "--cleanup-file") {
        updater::run_cleanup_temp_file(PathBuf::from(cleanup_path));
    }

    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("no se pudo instalar el proveedor criptográfico ring"))?;

    init_logging();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    if args.iter().any(|a| a == "--host-worker") {
        return randydesk_host::win_service::run_host_worker(
            runtime.handle(),
            env!("CARGO_PKG_VERSION"),
        );
    }

    let rt_handle = runtime.handle().clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 740.0])
            .with_min_inner_size([920.0, 580.0])
            .with_icon(tray::build_egui_icon()),
        ..Default::default()
    };

    let title = format!("RandyDesk v{}", env!("CARGO_PKG_VERSION"));
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| Ok(Box::new(RandyDeskApp::new(cc, rt_handle)))),
    )
    .map_err(|err| anyhow::anyhow!("error ejecutando interfaz gráfica: {err}"))?;

    Ok(())
}
