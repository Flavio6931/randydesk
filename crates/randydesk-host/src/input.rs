//! Inyección de eventos de ratón y teclado en Windows mediante `SendInput` (estilo `enigo` de RustDesk + desktop activo).

use crate::capture::MonitorGeometry;
use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use randydesk_proto::{InputEvent, MouseButton};
use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, OnceLock};
use tracing::warn;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, HANDLE, HWND};
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, GetThreadDesktop, GetUserObjectInformationW, OpenInputDesktop,
    SetThreadDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, HDESK, UOI_NAME,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, MapVirtualKeyExW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE,
    KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
    KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC_EX, MOUSEINPUT, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
    MOUSEEVENTF_WHEEL, VIRTUAL_KEY,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetSystemMetrics, GetWindowThreadProcessId, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOWNORMAL,
};

/// Identificador `dwExtraInfo` usado en `SendInput` para distinguir eventos propios inyectados por RandyDesk.
pub const RANDYDESK_INPUT_EXTRA_INFO: usize = 0x5244_4B01;

const VK_PACKET_U16: u16 = 0x00E7;
const DESKTOP_SWITCH_ACCESS: u32 = 0x01FF;

static UIPI_BLOCK_DETECTED: AtomicBool = AtomicBool::new(false);
static INPUT_WORKER_TX: OnceLock<SyncSender<(InputEvent, MonitorGeometry)>> = OnceLock::new();
static DEBUG_LOG_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

/// Registra eventos de teclado en `%APPDATA%\RandyDesk\input_debug.log` para diagnóstico local.
pub fn append_input_debug_log(line: &str) {
    let mutex = DEBUG_LOG_FILE.get_or_init(|| {
        let opened = ProjectDirs::from("com", "RandyDesk", "RandyDesk").and_then(|proj| {
            let dir = proj.config_dir();
            fs::create_dir_all(dir).ok()?;
            let path = dir.join("input_debug.log");
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
                .ok()
        });
        Mutex::new(opened)
    });
    if let Ok(mut guard) = mutex.lock() {
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

fn input_worker_sender() -> &'static SyncSender<(InputEvent, MonitorGeometry)> {
    INPUT_WORKER_TX.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<(InputEvent, MonitorGeometry)>(512);
        std::thread::Builder::new()
            .name("randydesk-input-injector".into())
            .spawn(move || {
                while let Ok((ev, monitor)) = rx.recv() {
                    ensure_input_desktop();
                    inject_input_on_current_thread(&ev, monitor);
                }
            })
            .expect("spawn input injector thread");
        tx
    })
}

/// Encola un evento remoto para inyectarlo en orden FIFO desde el hilo dedicado ligado al Input Desktop activo.
pub fn inject_input(event: &InputEvent, monitor: MonitorGeometry) {
    let _ = input_worker_sender().try_send((event.clone(), monitor));
}

/// Ejecuta la inyección `SendInput` directamente sobre el hilo actual.
fn inject_input_on_current_thread(event: &InputEvent, monitor: MonitorGeometry) {
    match *event {
        InputEvent::MouseMove { x_norm, y_norm } => {
            let (abs_x, abs_y) = map_normalized_to_virtual_desk(x_norm, y_norm, monitor);
            send_mouse(MOUSEINPUT {
                dx: abs_x,
                dy: abs_y,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: RANDYDESK_INPUT_EXTRA_INFO,
            });
        }
        InputEvent::MouseButton { button, down } => {
            let flag = match (button, down) {
                (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
                (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
                (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
                (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
            };
            send_mouse(MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flag,
                time: 0,
                dwExtraInfo: RANDYDESK_INPUT_EXTRA_INFO,
            });
        }
        InputEvent::MouseWheel { delta_x, delta_y } => {
            if delta_y != 0 {
                send_mouse(MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: (delta_y as i32) as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: RANDYDESK_INPUT_EXTRA_INFO,
                });
            }
            if delta_x != 0 {
                send_mouse(MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: (delta_x as i32) as u32,
                    dwFlags: MOUSEEVENTF_HWHEEL,
                    time: 0,
                    dwExtraInfo: RANDYDESK_INPUT_EXTRA_INFO,
                });
            }
        }
        InputEvent::Key {
            vk,
            scan,
            extended,
            down,
        } => {
            let resolved_scan_ex = if scan == 0 && vk != VK_PACKET_U16 {
                resolve_vk_to_vsc_ex(vk)
            } else {
                0
            };
            let ki = build_keybd_input(vk, scan, extended, down, resolved_scan_ex);
            dispatch_keyboard_with_log(ki, vk, scan, extended, down);
        }
        InputEvent::UnicodeChar { ch, down } => {
            let mut buf = [0u16; 2];
            for code_unit in ch.encode_utf16(&mut buf).iter().copied() {
                let mut flags = KEYEVENTF_UNICODE;
                if !down {
                    flags |= KEYEVENTF_KEYUP;
                }
                send_keyboard(KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: code_unit,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: RANDYDESK_INPUT_EXTRA_INFO,
                });
            }
        }
    }
}

/// Resuelve un Virtual-Key a scan code usando el `HKL` real de la ventana activa en primer plano.
fn resolve_vk_to_vsc_ex(vk: u16) -> u32 {
    unsafe {
        let fg_tid = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let layout = GetKeyboardLayout(fg_tid);
        MapVirtualKeyExW(vk as u32, MAPVK_VK_TO_VSC_EX, layout)
    }
}

/// Indica si un Virtual-Key corresponde a tecla extendida Win32 (`Win`, navegación, `Insert`/`Delete`, `RControl`, `RAlt`, `Numpad/`).
fn is_extended_vk(vk: u16) -> bool {
    matches!(
        vk,
        0x21..=0x28 | 0x2D | 0x2E | 0x5B..=0x5D | 0xA3 | 0xA5 | 0x6F
    )
}

/// Construye `KEYBDINPUT` al estilo `enigo` de RustDesk (`wVk + wScan` sin `KEYEVENTF_SCANCODE`) con fallback `VK_PACKET`/scancode.
pub(crate) fn build_keybd_input(
    vk: u16,
    scan: u16,
    extended: bool,
    down: bool,
    resolved_scan_ex: u32,
) -> KEYBDINPUT {
    if vk == VK_PACKET_U16 {
        let mut flags = KEYEVENTF_UNICODE;
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        return KEYBDINPUT {
            wVk: VIRTUAL_KEY(0),
            wScan: scan,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: RANDYDESK_INPUT_EXTRA_INFO,
        };
    }

    let (raw_scan, is_extended) = if scan != 0 {
        let raw = scan & 0x00FF;
        let prefix = (scan >> 8) & 0x00FF;
        let ext = if raw == 0x36 {
            false
        } else {
            extended || prefix == 0xE0 || prefix == 0xE1 || is_extended_vk(vk)
        };
        (raw, ext)
    } else {
        let raw = (resolved_scan_ex & 0x00FF) as u16;
        let prefix = (resolved_scan_ex >> 8) & 0x00FF;
        let ext = if raw == 0x36 {
            false
        } else {
            extended || prefix == 0xE0 || prefix == 0xE1 || is_extended_vk(vk)
        };
        (raw, ext)
    };

    if vk != 0 {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if is_extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        KEYBDINPUT {
            wVk: VIRTUAL_KEY(vk),
            wScan: raw_scan,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: RANDYDESK_INPUT_EXTRA_INFO,
        }
    } else if raw_scan != 0 {
        let mut flags = KEYEVENTF_SCANCODE;
        if is_extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        KEYBDINPUT {
            wVk: VIRTUAL_KEY(0),
            wScan: raw_scan,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: RANDYDESK_INPUT_EXTRA_INFO,
        }
    } else {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if is_extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        KEYBDINPUT {
            wVk: VIRTUAL_KEY(0),
            wScan: 0,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: RANDYDESK_INPUT_EXTRA_INFO,
        }
    }
}

/// Adjunta el hilo dedicado al escritorio de entrada activo de Windows (`Default`, `Winlogon`, etc.) como hace RustDesk.
fn ensure_input_desktop() {
    unsafe {
        let Ok(input_desk) = OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_ACCESS_FLAGS(DESKTOP_SWITCH_ACCESS),
        ) else {
            return;
        };
        let Ok(current_desk) = GetThreadDesktop(GetCurrentThreadId()) else {
            let _ = CloseDesktop(input_desk);
            return;
        };

        let same = desktop_name(current_desk) == desktop_name(input_desk);
        if !same && SetThreadDesktop(input_desk).is_ok() {
            return;
        }
        let _ = CloseDesktop(input_desk);
    }
}

unsafe fn desktop_name(desk: HDESK) -> Option<Vec<u16>> {
    let mut buf = [0u16; 128];
    let mut needed = 0u32;
    GetUserObjectInformationW(
        HANDLE(desk.0),
        UOI_NAME,
        Some(buf.as_mut_ptr() as *mut c_void),
        (buf.len() * size_of::<u16>()) as u32,
        Some(&mut needed),
    )
    .ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(buf[..len].to_vec())
}

/// Indica si el proceso actual de RandyDesk está ejecutándose con privilegios de Administrador (elevado).
pub fn is_process_elevated() -> bool {
    unsafe { is_handle_elevated(GetCurrentProcess()).unwrap_or(false) }
}

/// Indica si la ventana activa en primer plano pertenece a un proceso elevado.
pub fn is_foreground_window_elevated() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(proc_handle) => {
                let res = is_handle_elevated(proc_handle).unwrap_or(false);
                let _ = CloseHandle(proc_handle);
                res
            }
            Err(err) => err.code() == ERROR_ACCESS_DENIED.to_hresult() && !is_process_elevated(),
        }
    }
}

unsafe fn is_handle_elevated(proc_handle: HANDLE) -> Option<bool> {
    let mut token = HANDLE::default();
    OpenProcessToken(proc_handle, TOKEN_QUERY, &mut token).ok()?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut ret_len = 0u32;
    let res = GetTokenInformation(
        token,
        TokenElevation,
        Some(&mut elevation as *mut _ as *mut c_void),
        size_of::<TOKEN_ELEVATION>() as u32,
        &mut ret_len,
    );
    let _ = CloseHandle(token);
    res.ok()?;
    Some(elevation.TokenIsElevated != 0)
}

/// Indica si se detectó un bloqueo UIPI reciente (ventana remota elevada con RandyDesk sin elevar).
pub fn uipi_block_detected() -> bool {
    if is_process_elevated() {
        return false;
    }
    UIPI_BLOCK_DETECTED.load(Ordering::Relaxed)
}

/// Relanza el ejecutable actual solicitando elevación UAC ("Ejecutar como Administrador").
pub fn relaunch_as_admin() -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe_wide = HSTRING::from(exe.as_os_str());
    let ret = unsafe {
        ShellExecuteW(
            HWND::default(),
            w!("runas"),
            PCWSTR::from_raw(exe_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if (ret.0 as isize) <= 32 {
        return Err(anyhow!(
            "No se completó la elevación a Administrador (código ShellExecute {})",
            ret.0 as isize
        ));
    }
    Ok(())
}

fn map_normalized_to_virtual_desk(
    x_norm: f32,
    y_norm: f32,
    monitor: MonitorGeometry,
) -> (i32, i32) {
    let x_clamped = x_norm.clamp(0.0, 1.0);
    let y_clamped = y_norm.clamp(0.0, 1.0);

    let target_x =
        monitor.origin_x as f64 + (x_clamped as f64) * (monitor.width.saturating_sub(1) as f64);
    let target_y =
        monitor.origin_y as f64 + (y_clamped as f64) * (monitor.height.saturating_sub(1) as f64);

    let virt_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) } as f64;
    let virt_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) } as f64;
    let virt_w = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1) as f64;
    let virt_h = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1) as f64;

    let abs_x = (((target_x - virt_x) * 65535.0) / (virt_w - 1.0).max(1.0)).round() as i32;
    let abs_y = (((target_y - virt_y) * 65535.0) / (virt_h - 1.0).max(1.0)).round() as i32;
    (abs_x.clamp(0, 65535), abs_y.clamp(0, 65535))
}

fn send_mouse(mi: MOUSEINPUT) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi },
    };
    dispatch_input(input);
}

fn send_keyboard(ki: KEYBDINPUT) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki },
    };
    let _ = dispatch_input(input);
}

fn dispatch_keyboard_with_log(ki: KEYBDINPUT, vk: u16, scan: u16, extended: bool, down: bool) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki },
    };
    let (sent, err_code) = dispatch_input(input);
    let elev = is_process_elevated();
    let fg_elev = is_foreground_window_elevated();
    append_input_debug_log(&format!(
        "[HOST-KEY] vk=0x{vk:04X} scan=0x{scan:04X} ext={extended} down={down} -> wVk=0x{:04X} wScan=0x{:04X} flags=0x{:04X} sent={sent} err={err_code} elev={elev} fg_elev={fg_elev}",
        ki.wVk.0, ki.wScan, ki.dwFlags.0
    ));
}

fn dispatch_input(input: INPUT) -> (u32, u32) {
    let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
    let err = if sent == 0 {
        unsafe { GetLastError().0 }
    } else {
        0
    };
    if sent == 0 {
        if !is_process_elevated() && is_foreground_window_elevated() {
            UIPI_BLOCK_DETECTED.store(true, Ordering::Relaxed);
        }
        warn!("SendInput devolvió 0 (posible bloqueo UIPI, err={err})");
    } else if !is_process_elevated() && is_foreground_window_elevated() {
        UIPI_BLOCK_DETECTED.store(true, Ordering::Relaxed);
    }
    (sent, err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win_keys_use_enigo_vk_and_extended_flag() {
        let lwin_down = build_keybd_input(0x5B, 0x5B, false, true, 0);
        assert_eq!(lwin_down.wVk, VIRTUAL_KEY(0x5B));
        assert_eq!(lwin_down.wScan, 0x5B);
        assert_eq!(lwin_down.dwFlags, KEYEVENTF_EXTENDEDKEY);

        let rwin_up = build_keybd_input(0x5C, 0, false, false, 0xE05C);
        assert_eq!(rwin_up.wVk, VIRTUAL_KEY(0x5C));
        assert_eq!(rwin_up.wScan, 0x5C);
        assert_eq!(rwin_up.dwFlags, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP);
    }

    #[test]
    fn normal_key_and_rshift_and_unicode_follow_rustdesk_rules() {
        let a_down = build_keybd_input(0x41, 0x1E, false, true, 0);
        assert_eq!(a_down.wVk, VIRTUAL_KEY(0x41));
        assert_eq!(a_down.wScan, 0x1E);
        assert_eq!(a_down.dwFlags, KEYBD_EVENT_FLAGS(0));

        let rshift_down = build_keybd_input(0xA1, 0x36, true, true, 0);
        assert_eq!(rshift_down.wVk, VIRTUAL_KEY(0xA1));
        assert_eq!(rshift_down.wScan, 0x36);
        assert_eq!(rshift_down.dwFlags, KEYBD_EVENT_FLAGS(0));

        let packet = build_keybd_input(VK_PACKET_U16, 'á' as u16, false, true, 0);
        assert_eq!(packet.wVk, VIRTUAL_KEY(0));
        assert_eq!(packet.wScan, 'á' as u16);
        assert_eq!(packet.dwFlags, KEYEVENTF_UNICODE);

        let scan_only = build_keybd_input(0, 0x1E, false, true, 0);
        assert_eq!(scan_only.wVk, VIRTUAL_KEY(0));
        assert_eq!(scan_only.wScan, 0x1E);
        assert_eq!(scan_only.dwFlags, KEYEVENTF_SCANCODE);
    }
}
