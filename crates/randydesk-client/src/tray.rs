//! Icono de aplicación y controlador de bandeja del sistema (System Tray) en Windows.

use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Genera el icono RGBA8 32x32 de RandyDesk (fondo azul `#0078FF` con pantalla blanca).
pub fn build_app_icon_rgba() -> (Vec<u8>, u32, u32) {
    let w = 32u32;
    let h = 32u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];

    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            // Esquinas suavemente redondeadas
            let corner = (x < 3 && y < 3)
                || (x >= w - 3 && y < 3)
                || (x < 3 && y >= h - 3)
                || (x >= w - 3 && y >= h - 3);
            if corner {
                continue;
            }

            let is_screen_border = (6..=25).contains(&x)
                && (7..=20).contains(&y)
                && (x == 6 || x == 25 || y == 7 || y == 20);
            let is_stand = (14..=17).contains(&x) && (21..=23).contains(&y)
                || (10..=21).contains(&x) && y == 24;

            if is_screen_border || is_stand {
                rgba[idx] = 245;
                rgba[idx + 1] = 248;
                rgba[idx + 2] = 255;
                rgba[idx + 3] = 255;
            } else {
                rgba[idx] = 0x00;
                rgba[idx + 1] = 0x78;
                rgba[idx + 2] = 0xFF;
                rgba[idx + 3] = 255;
            }
        }
    }
    (rgba, w, h)
}

/// Construye `egui::IconData` usando el mismo icono vectorial de la bandeja.
pub fn build_egui_icon() -> egui::IconData {
    let (rgba, width, height) = build_app_icon_rgba();
    egui::IconData {
        rgba,
        width,
        height,
    }
}

/// Mantiene el icono activo en la bandeja del sistema y gestiona restaurar o salir.
pub struct TrayController {
    _tray_icon: TrayIcon,
    restore_requested: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl TrayController {
    /// Crea el icono en la bandeja del sistema con menú ("Abrir RandyDesk" / "Salir").
    pub fn new(ctx: egui::Context) -> Option<Self> {
        let (rgba, w, h) = build_app_icon_rgba();
        let icon = match Icon::from_rgba(rgba, w, h) {
            Ok(i) => i,
            Err(err) => {
                warn!("No se pudo construir icono de bandeja: {err}");
                return None;
            }
        };

        let menu = Menu::new();
        let open_item = MenuItem::new("Abrir RandyDesk", true, None);
        let exit_item = MenuItem::new("Salir", true, None);
        let open_id = open_item.id().clone();
        let exit_id = exit_item.id().clone();

        if menu.append(&open_item).is_err() || menu.append(&exit_item).is_err() {
            warn!("No se pudo construir menú de bandeja");
            return None;
        }

        let tray_icon = match TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("RandyDesk — Listo en segundo plano")
            .with_icon(icon)
            .build()
        {
            Ok(t) => t,
            Err(err) => {
                warn!("No se pudo registrar icono en bandeja del sistema: {err}");
                return None;
            }
        };

        let restore_requested = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let restore_clone = Arc::clone(&restore_requested);
        let stop_clone = Arc::clone(&stop_flag);

        let worker = std::thread::spawn(move || {
            let tray_rx = TrayIconEvent::receiver();
            let menu_rx = MenuEvent::receiver();

            while !stop_clone.load(Ordering::Relaxed) {
                while let Ok(ev) = tray_rx.try_recv() {
                    let should_open = matches!(
                        ev,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } | TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        }
                    );
                    if should_open {
                        restore_window(&ctx, &restore_clone);
                    }
                }

                while let Ok(ev) = menu_rx.try_recv() {
                    if ev.id == open_id {
                        restore_window(&ctx, &restore_clone);
                    } else if ev.id == exit_id {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        ctx.request_repaint();
                    }
                }

                std::thread::sleep(Duration::from_millis(60));
            }
        });

        Some(Self {
            _tray_icon: tray_icon,
            restore_requested,
            stop_flag,
            worker: Some(worker),
        })
    }

    /// Devuelve `true` una vez si el usuario solicitó restaurar desde la bandeja.
    pub fn take_restore_requested(&self) -> bool {
        self.restore_requested.swap(false, Ordering::Relaxed)
    }
}

impl Drop for TrayController {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

fn restore_window(ctx: &egui::Context, restore_flag: &Arc<AtomicBool>) {
    restore_flag.store(true, Ordering::Relaxed);
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}
