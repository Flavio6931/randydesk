//! Icono de aplicación y controlador de bandeja del sistema (System Tray) en Windows.

use eframe::egui;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE, WPARAM};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindow, IsWindowVisible, PostMessageW, SetForegroundWindow, ShowWindow, GW_OWNER,
    GWL_EXSTYLE, SW_HIDE, SW_RESTORE, WM_CLOSE, WS_EX_TOOLWINDOW,
};

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
    hidden: Arc<AtomicBool>,
    main_hwnd: Arc<AtomicIsize>,
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
        let hidden = Arc::new(AtomicBool::new(false));
        let main_hwnd = Arc::new(AtomicIsize::new(0));
        let stop_flag = Arc::new(AtomicBool::new(false));

        let restore_clone = Arc::clone(&restore_requested);
        let hidden_clone = Arc::clone(&hidden);
        let hwnd_clone = Arc::clone(&main_hwnd);
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
                        restore_window(&ctx, &restore_clone, &hidden_clone, &hwnd_clone);
                    }
                }

                while let Ok(ev) = menu_rx.try_recv() {
                    if ev.id == open_id {
                        restore_window(&ctx, &restore_clone, &hidden_clone, &hwnd_clone);
                    } else if ev.id == exit_id {
                        exit_window(&ctx, &hidden_clone, &hwnd_clone);
                    }
                }

                std::thread::sleep(Duration::from_millis(60));
            }
        });

        Some(Self {
            _tray_icon: tray_icon,
            restore_requested,
            hidden,
            main_hwnd,
            stop_flag,
            worker: Some(worker),
        })
    }

    /// Localiza y cachea el `HWND` real de la ventana principal de RandyDesk en este proceso.
    pub fn cache_main_hwnd(&self) {
        let _ = resolve_or_cache_hwnd(&self.main_hwnd);
    }

    /// Oculta la ventana principal en la bandeja mediante Win32 (`SW_HIDE`) sin congelar el bucle gráfico.
    pub fn hide_to_tray(&self) -> bool {
        let Some(hwnd) = resolve_or_cache_hwnd(&self.main_hwnd) else {
            return false;
        };
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        self.hidden.store(true, Ordering::SeqCst);
        true
    }

    /// Consulta directamente a Win32 (`IsIconic`) si la ventana principal está minimizada.
    pub fn is_window_minimized(&self) -> bool {
        let Some(hwnd) = resolve_or_cache_hwnd(&self.main_hwnd) else {
            return false;
        };
        unsafe { IsIconic(hwnd).as_bool() }
    }

    /// Indica si la ventana principal se encuentra oculta en la bandeja del sistema.
    #[allow(dead_code)]
    pub fn is_hidden(&self) -> bool {
        self.hidden.load(Ordering::Acquire)
    }

    /// Devuelve `true` una vez si el usuario solicitó restaurar desde la bandeja.
    pub fn take_restore_requested(&self) -> bool {
        self.restore_requested.swap(false, Ordering::SeqCst)
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

fn hwnd_to_raw(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

fn raw_to_valid_hwnd(raw: isize) -> Option<HWND> {
    if raw == 0 {
        return None;
    }
    let hwnd = HWND(raw as *mut c_void);
    if unsafe { IsWindow(hwnd).as_bool() } {
        Some(hwnd)
    } else {
        None
    }
}

fn resolve_or_cache_hwnd(cached: &AtomicIsize) -> Option<HWND> {
    if let Some(hwnd) = raw_to_valid_hwnd(cached.load(Ordering::Acquire)) {
        return Some(hwnd);
    }
    let hwnd = find_main_hwnd()?;
    cached.store(hwnd_to_raw(hwnd), Ordering::Release);
    Some(hwnd)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowMatch {
    Preferred,
    Fallback,
    Ignore,
}

fn classify_window_candidate(title: &str, visible: bool) -> WindowMatch {
    if title.starts_with("RandyDesk") {
        WindowMatch::Preferred
    } else if visible && !title.is_empty() {
        WindowMatch::Fallback
    } else {
        WindowMatch::Ignore
    }
}

struct HwndSearch {
    own_pid: u32,
    preferred: Option<HWND>,
    fallback: Option<HWND>,
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam.0 as *mut HwndSearch);

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid != search.own_pid {
        return TRUE;
    }

    if matches!(GetWindow(hwnd, GW_OWNER), Ok(owner) if !owner.is_invalid()) {
        return TRUE;
    }

    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
    if (ex_style & (WS_EX_TOOLWINDOW.0 as i32)) != 0 {
        return TRUE;
    }

    let mut buf = [0u16; 256];
    let len = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
    let title = String::from_utf16_lossy(&buf[..len]);
    let visible = IsWindowVisible(hwnd).as_bool();

    match classify_window_candidate(&title, visible) {
        WindowMatch::Preferred => {
            if search.preferred.is_none() || visible {
                search.preferred = Some(hwnd);
            }
            if visible {
                return BOOL(0);
            }
        }
        WindowMatch::Fallback => {
            if search.fallback.is_none() {
                search.fallback = Some(hwnd);
            }
        }
        WindowMatch::Ignore => {}
    }

    TRUE
}

fn find_main_hwnd() -> Option<HWND> {
    let mut search = HwndSearch {
        own_pid: unsafe { GetCurrentProcessId() },
        preferred: None,
        fallback: None,
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            LPARAM(&mut search as *mut HwndSearch as isize),
        );
    }
    search.preferred.or(search.fallback)
}

fn restore_window(
    ctx: &egui::Context,
    restore_flag: &AtomicBool,
    hidden_flag: &AtomicBool,
    main_hwnd: &AtomicIsize,
) {
    hidden_flag.store(false, Ordering::SeqCst);
    restore_flag.store(true, Ordering::SeqCst);
    if let Some(hwnd) = resolve_or_cache_hwnd(main_hwnd) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

fn exit_window(ctx: &egui::Context, hidden_flag: &AtomicBool, main_hwnd: &AtomicIsize) {
    hidden_flag.store(false, Ordering::SeqCst);
    if let Some(hwnd) = resolve_or_cache_hwnd(main_hwnd) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_32x32_app_icon_with_expected_pixels() {
        let (rgba, w, h) = build_app_icon_rgba();
        assert_eq!(w, 32);
        assert_eq!(h, 32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        // Esquina transparente
        assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
        // Centro de fondo azul #0078FF
        let center_idx = ((12 * 32 + 12) * 4) as usize;
        assert_eq!(&rgba[center_idx..center_idx + 4], &[0x00, 0x78, 0xFF, 0xFF]);
    }

    #[test]
    fn classifies_main_window_candidates_even_when_hidden() {
        assert_eq!(
            classify_window_candidate("RandyDesk v0.4.4", true),
            WindowMatch::Preferred
        );
        assert_eq!(
            classify_window_candidate("RandyDesk v0.4.4", false),
            WindowMatch::Preferred
        );
        assert_eq!(
            classify_window_candidate("Otra Ventana", true),
            WindowMatch::Fallback
        );
        assert_eq!(
            classify_window_candidate("Helper Oculto", false),
            WindowMatch::Ignore
        );
        assert_eq!(classify_window_candidate("", true), WindowMatch::Ignore);
    }

    #[test]
    fn rejects_zero_raw_hwnd() {
        assert!(raw_to_valid_hwnd(0).is_none());
    }
}
