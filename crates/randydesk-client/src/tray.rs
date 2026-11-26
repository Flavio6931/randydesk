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
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindow,
    GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, PostMessageW, SetForegroundWindow, ShowWindow, ASFW_ANY, GW_OWNER,
    GWL_EXSTYLE, SW_HIDE, SW_RESTORE, SW_SHOW, WM_CLOSE, WS_EX_TOOLWINDOW,
};

/// Renderiza el logotipo vectorial `Portal R` a cualquier tamaño con supersampling 4x4.
pub fn render_portal_r_rgba(size: u32) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let mut acc_r = 0.0f32;
            let mut acc_g = 0.0f32;
            let mut acc_b = 0.0f32;
            let mut acc_a = 0.0f32;

            for sy in 0..4 {
                for sx in 0..4 {
                    let x = ((px as f32) + (sx as f32 + 0.5) * 0.25) * 100.0 / (size as f32);
                    let y = ((py as f32) + (sy as f32 + 0.5) * 0.25) * 100.0 / (size as f32);
                    let (r, g, b, a) = sample_portal_r(x, y);
                    acc_r += r * a;
                    acc_g += g * a;
                    acc_b += b * a;
                    acc_a += a;
                }
            }

            let idx = ((py * size + px) * 4) as usize;
            if acc_a > 0.0 {
                out[idx] = (acc_r / acc_a).round().clamp(0.0, 255.0) as u8;
                out[idx + 1] = (acc_g / acc_a).round().clamp(0.0, 255.0) as u8;
                out[idx + 2] = (acc_b / acc_a).round().clamp(0.0, 255.0) as u8;
                out[idx + 3] = ((acc_a / 16.0) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

fn lerp_chan(a: u8, b: u8, t: f32) -> f32 {
    let tc = t.clamp(0.0, 1.0);
    (a as f32) + ((b as f32) - (a as f32)) * tc
}

fn sample_portal_r(x: f32, y: f32) -> (f32, f32, f32, f32) {
    let rad = 24.0f32;
    let b = 50.0 - rad;
    let qx = (x - 50.0).abs() - b;
    let qy = (y - 50.0).abs() - b;
    let sd = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rad;
    if sd > 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }

    if (x - 78.5).hypot(y - 22.5) <= 5.5 {
        return (0x10 as f32, 0xB9 as f32, 0x81 as f32, 1.0);
    }

    let in_spine = (26.0..=39.0).contains(&x) && (22.0..=78.0).contains(&y);
    let in_bowl_outer = ((26.0..=56.0).contains(&x) && (22.0..=60.0).contains(&y))
        || ((x - 56.0).hypot(y - 41.0) <= 19.0);
    let in_bowl_inner = ((39.0..=55.0).contains(&x) && (33.0..=49.0).contains(&y))
        || ((x - 55.0).hypot(y - 41.0) <= 8.0);
    if (in_spine || in_bowl_outer) && !in_bowl_inner {
        let t = (y - 22.0) / 56.0;
        return (
            lerp_chan(255, 0xD8, t),
            lerp_chan(255, 0xE8, t),
            255.0,
            1.0,
        );
    }

    if (55.0..=78.0).contains(&y) {
        let u = (y - 55.0) / 23.0;
        let xl = 47.0 + 16.0 * u;
        let xr = 61.0 + 17.0 * u;
        if x >= xl && x <= xr {
            return (
                lerp_chan(0x3B, 0x06, u),
                lerp_chan(0x82, 0xB6, u),
                lerp_chan(0xF6, 0xD4, u),
                1.0,
            );
        }
    }

    if sd > -2.0 {
        return (0x2D as f32, 0x46 as f32, 0x78 as f32, 1.0);
    }

    let bg_t = (x + y) / 200.0;
    let mut r = lerp_chan(0x10, 0x07, bg_t);
    let mut g = lerp_chan(0x1A, 0x0B, bg_t);
    let mut b_col = lerp_chan(0x33, 0x14, bg_t);
    let glow_d = (x - 50.0).hypot(y - 48.0);
    if glow_d < 44.0 {
        let k = (1.0 - glow_d / 44.0).powi(2) * 0.24;
        r = r * (1.0 - k) + (0x2F as f32) * k;
        g = g * (1.0 - k) + (0x81 as f32) * k;
        b_col = b_col * (1.0 - k) + (0xF7 as f32) * k;
    }
    (r, g, b_col, 1.0)
}

/// Genera el icono RGBA8 32x32 (`Portal R`) para la bandeja del sistema.
pub fn build_app_icon_rgba() -> (Vec<u8>, u32, u32) {
    (render_portal_r_rgba(32), 32, 32)
}

/// Construye `egui::IconData` 64x64 nítido para ventana y barra de tareas.
pub fn build_egui_icon() -> egui::IconData {
    egui::IconData {
        rgba: render_portal_r_rgba(64),
        width: 64,
        height: 64,
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

    /// Fuerza el foco Win32 hacia la ventana principal de RandyDesk cuando el usuario interactúa con ella.
    pub fn ensure_foreground(&self) {
        if let Some(hwnd) = resolve_or_cache_hwnd(&self.main_hwnd) {
            unsafe {
                force_foreground(hwnd);
            }
        }
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

unsafe fn force_foreground(hwnd: HWND) {
    let _ = AllowSetForegroundWindow(ASFW_ANY);
    let _ = ShowWindow(
        hwnd,
        if IsIconic(hwnd).as_bool() {
            SW_RESTORE
        } else {
            SW_SHOW
        },
    );
    if SetForegroundWindow(hwnd).as_bool() {
        let _ = BringWindowToTop(hwnd);
        let _ = SetActiveWindow(hwnd);
        let _ = SetFocus(hwnd);
        return;
    }

    let fg = GetForegroundWindow();
    if !fg.is_invalid() && fg != hwnd {
        let fg_tid = GetWindowThreadProcessId(fg, None);
        let cur_tid = GetCurrentThreadId();
        if fg_tid != 0 && fg_tid != cur_tid {
            let attached = AttachThreadInput(cur_tid, fg_tid, true).as_bool();
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetActiveWindow(hwnd);
            let _ = SetFocus(hwnd);
            if attached {
                let _ = AttachThreadInput(cur_tid, fg_tid, false);
            }
            return;
        }
    }
    let _ = BringWindowToTop(hwnd);
    let _ = SetForegroundWindow(hwnd);
    let _ = SetActiveWindow(hwnd);
    let _ = SetFocus(hwnd);
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
            force_foreground(hwnd);
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
        // Trazo vertical blanco de la 'R' en (10, 14)
        let r_idx = ((14 * 32 + 10) * 4) as usize;
        assert!(rgba[r_idx] > 200 && rgba[r_idx + 1] > 200 && rgba[r_idx + 3] == 255);
        // Haz diagonal cobalto-cyan en (20, 22)
        let beam_idx = ((22 * 32 + 20) * 4) as usize;
        assert!(rgba[beam_idx + 2] > 180 && rgba[beam_idx + 3] == 255);
        // Nodo vivo esmeralda en (25, 7)
        let dot_idx = ((7 * 32 + 25) * 4) as usize;
        assert!(rgba[dot_idx + 1] > 150 && rgba[dot_idx + 3] == 255);
        // Fondo obsidiana en (5, 24)
        let bg_idx = ((24 * 32 + 5) * 4) as usize;
        assert!(rgba[bg_idx] < 40 && rgba[bg_idx + 3] == 255);
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
