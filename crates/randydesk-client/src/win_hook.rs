//! Hook de teclado de bajo nivel (`WH_KEYBOARD_LL`) estilo RustDesk para capturar solo `Win` y atajos de sistema.

use crate::net::NetCommand;
use randydesk_host::input::{append_input_debug_log, RANDYDESK_INPUT_EXTRA_INFO};
use randydesk_proto::InputEvent;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc;
use tracing::warn;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG, PM_NOREMOVE, PM_NOYIELD, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Códigos Virtual-Key de modificadores que se liberan al perder foco para evitar teclas pegadas.
const MODIFIER_VKS: &[u16] = &[0x5B, 0x5C, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5];
const VK_PACKET_U16: u16 = 0x00E7;

struct HookSharedState {
    installed: AtomicBool,
    active: AtomicBool,
    win_down: AtomicBool,
    own_pid: u32,
    sent_keys: AtomicU64,
    tx: Mutex<Option<mpsc::Sender<NetCommand>>>,
}

static HOOK_STATE: OnceLock<Arc<HookSharedState>> = OnceLock::new();

fn shared_state() -> &'static Arc<HookSharedState> {
    HOOK_STATE.get_or_init(|| {
        Arc::new(HookSharedState {
            installed: AtomicBool::new(false),
            active: AtomicBool::new(false),
            win_down: AtomicBool::new(false),
            own_pid: unsafe { GetCurrentProcessId() },
            sent_keys: AtomicU64::new(0),
            tx: Mutex::new(None),
        })
    })
}

/// Controlador RAII del hilo que mantiene instalado `WH_KEYBOARD_LL` mientras corre la app.
pub struct KeyboardHookGuard {
    thread_id: Arc<AtomicU32>,
    worker: Option<std::thread::JoinHandle<()>>,
    was_active: bool,
}

impl KeyboardHookGuard {
    /// Instala el hook de bajo nivel en un hilo Win32 dedicado con cola de mensajes inicializada.
    pub fn new() -> Self {
        let _ = shared_state();
        let thread_id = Arc::new(AtomicU32::new(0));
        let tid_clone = Arc::clone(&thread_id);

        let worker = std::thread::Builder::new()
            .name("randydesk-kb-hook".into())
            .spawn(move || unsafe {
                tid_clone.store(GetCurrentThreadId(), Ordering::SeqCst);
                let mut msg = MSG::default();
                let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE | PM_NOYIELD);

                let hmod: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
                let hook: HHOOK =
                    match SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_kb_proc), hmod, 0) {
                        Ok(h) => h,
                        Err(err) => {
                            warn!("No se pudo instalar hook WH_KEYBOARD_LL: {err}");
                            shared_state().installed.store(false, Ordering::SeqCst);
                            append_input_debug_log(&format!(
                                "[CLIENT-HOOK] WH_KEYBOARD_LL installed=false err={err}"
                            ));
                            return;
                        }
                    };
                shared_state().installed.store(true, Ordering::SeqCst);
                append_input_debug_log("[CLIENT-HOOK] WH_KEYBOARD_LL installed=true");

                while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                shared_state().installed.store(false, Ordering::SeqCst);
                let _ = UnhookWindowsHookEx(hook);
            })
            .ok();

        Self {
            thread_id,
            worker,
            was_active: false,
        }
    }

    /// Indica si la ventana de RandyDesk está actualmente en primer plano en Windows.
    pub fn is_window_foreground() -> bool {
        let own_pid = shared_state().own_pid;
        unsafe { is_own_window_foreground(own_pid) }
    }

    /// Indica si el hook Win32 quedó instalado correctamente.
    pub fn is_installed(&self) -> bool {
        shared_state().installed.load(Ordering::Acquire)
    }

    /// Cantidad acumulada de teclas de sistema capturadas por el hook.
    pub fn sent_key_count(&self) -> u64 {
        shared_state().sent_keys.load(Ordering::Relaxed)
    }

    /// Activa o desactiva la captura de atajos de sistema hacia `tx`; libera modificadores al soltar.
    pub fn set_capture(&mut self, active: bool, tx: Option<mpsc::Sender<NetCommand>>) {
        let state = shared_state();
        if self.was_active == active {
            if active {
                if let Ok(mut guard) = state.tx.lock() {
                    if guard.as_ref().is_none_or(|old| old.is_closed()) && tx.is_some() {
                        *guard = tx;
                    }
                }
            }
            return;
        }

        if self.was_active && !active {
            self.release_modifiers_now();
        }
        if !active {
            state.win_down.store(false, Ordering::Relaxed);
        }
        state.active.store(active, Ordering::SeqCst);
        if let Ok(mut guard) = state.tx.lock() {
            *guard = if active { tx } else { None };
        }
        self.was_active = active;
    }

    /// Libera los modificadores en la PC remota sin desconectar el canal ni apagar el hook.
    pub fn release_modifiers_now(&self) {
        shared_state().win_down.store(false, Ordering::Relaxed);
        if let Ok(guard) = shared_state().tx.try_lock() {
            if let Some(prev_tx) = guard.as_ref() {
                release_modifiers(prev_tx);
            }
        }
    }
}

impl Drop for KeyboardHookGuard {
    fn drop(&mut self) {
        self.set_capture(false, None);
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

fn release_modifiers(tx: &mpsc::Sender<NetCommand>) {
    for &vk in MODIFIER_VKS {
        let extended = matches!(vk, 0x5B | 0x5C | 0xA3 | 0xA5);
        let _ = tx.try_send(NetCommand::Input(InputEvent::Key {
            vk,
            scan: 0,
            extended,
            down: false,
        }));
    }
}

/// Decide al estilo RustDesk (`windows.cc`) si una pulsación debe interceptarse como hotkey de sistema.
/// Devuelve `(interceptar, nuevo_estado_win_down)`. Las teclas normales regresan `false` hacia `egui`.
pub(crate) fn should_intercept_system_key(
    vk: u16,
    down: bool,
    win_was_down: bool,
    alt_down: bool,
    ctrl_down: bool,
) -> (bool, bool) {
    match vk {
        0x5B | 0x5C => (true, down),
        0x2C => (true, win_was_down),
        0x09 if alt_down => (true, win_was_down),
        0x1B if alt_down || ctrl_down => (true, win_was_down),
        _ if win_was_down => (true, win_was_down),
        _ => (false, win_was_down),
    }
}

/// Convierte los campos del hook Win32 `KBDLLHOOKSTRUCT` en `InputEvent::Key` manejando `VK_PACKET` y extendidas.
pub(crate) fn build_hook_key_event(vk_code: u32, scan_code: u32, flags: u32, down: bool) -> InputEvent {
    let vk = vk_code as u16;
    if vk == VK_PACKET_U16 {
        return InputEvent::Key {
            vk,
            scan: scan_code as u16,
            extended: false,
            down,
        };
    }

    let raw_scan = (scan_code & 0x00FF) as u16;
    let extended = if raw_scan == 0x36 {
        false
    } else {
        (flags & LLKHF_EXTENDED.0) != 0
            || matches!(
                vk,
                0x21..=0x28 | 0x2D | 0x2E | 0x5B..=0x5D | 0xA3 | 0xA5 | 0x6F
            )
    };

    InputEvent::Key {
        vk,
        scan: raw_scan,
        extended,
        down,
    }
}

unsafe extern "system" fn low_level_kb_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let state = shared_state();
        if state.active.load(Ordering::Relaxed) {
            let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
            let is_self_injected = kb.dwExtraInfo == RANDYDESK_INPUT_EXTRA_INFO;

            if !is_self_injected && is_own_window_foreground(state.own_pid) {
                let msg_id = w_param.0 as u32;
                let down = match msg_id {
                    WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
                    WM_KEYUP | WM_SYSKEYUP => Some(false),
                    _ => None,
                };
                if let Some(down) = down {
                    let vk = kb.vkCode as u16;
                    let win_was_down = state.win_down.load(Ordering::Relaxed);
                    let alt_down = (GetAsyncKeyState(0x12) as u16 & 0x8000) != 0;
                    let ctrl_down = (GetAsyncKeyState(0x11) as u16 & 0x8000) != 0;
                    let (intercept, new_win_down) =
                        should_intercept_system_key(vk, down, win_was_down, alt_down, ctrl_down);
                    state.win_down.store(new_win_down, Ordering::Relaxed);

                    if intercept {
                        if let Ok(guard) = state.tx.try_lock() {
                            if let Some(tx) = guard.as_ref() {
                                let ev =
                                    build_hook_key_event(kb.vkCode, kb.scanCode, kb.flags.0, down);
                                match tx.try_send(NetCommand::Input(ev)) {
                                    Ok(()) => {
                                        state.sent_keys.fetch_add(1, Ordering::Relaxed);
                                        append_input_debug_log(&format!(
                                            "[CLIENT-HOOK] syskey vk=0x{vk:04X} scan=0x{:02X} down={down} -> sent=ok",
                                            kb.scanCode & 0xFF
                                        ));
                                        return LRESULT(1);
                                    }
                                    Err(err) => {
                                        append_input_debug_log(&format!(
                                            "[CLIENT-HOOK] syskey vk=0x{vk:04X} down={down} -> send_err={err}"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    CallNextHookEx(None, n_code, w_param, l_param)
}

unsafe fn is_own_window_foreground(own_pid: u32) -> bool {
    let hwnd = GetForegroundWindow();
    if hwnd.is_invalid() {
        return false;
    }
    let mut fg_pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut fg_pid));
    fg_pid == own_pid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_win_rshift_and_vk_packet_correctly() {
        let lwin = build_hook_key_event(0x5B, 0x5B, LLKHF_EXTENDED.0, true);
        assert_eq!(
            lwin,
            InputEvent::Key {
                vk: 0x5B,
                scan: 0x5B,
                extended: true,
                down: true,
            }
        );

        let rshift = build_hook_key_event(0xA1, 0x36, LLKHF_EXTENDED.0, true);
        assert_eq!(
            rshift,
            InputEvent::Key {
                vk: 0xA1,
                scan: 0x36,
                extended: false,
                down: true,
            }
        );

        let packet = build_hook_key_event(VK_PACKET_U16 as u32, 'ñ' as u32, 0, true);
        assert_eq!(
            packet,
            InputEvent::Key {
                vk: VK_PACKET_U16,
                scan: 'ñ' as u16,
                extended: false,
                down: true,
            }
        );
    }

    #[test]
    fn intercepts_only_system_hotkeys_like_rustdesk() {
        // Teclas normales pasan a egui
        assert_eq!(should_intercept_system_key(0x41, true, false, false, false), (false, false));
        assert_eq!(should_intercept_system_key(0x0D, true, false, false, false), (false, false));
        assert_eq!(should_intercept_system_key(0x09, true, false, false, false), (false, false));

        // Tecla Win y combinaciones Win+R se interceptan
        assert_eq!(should_intercept_system_key(0x5B, true, false, false, false), (true, true));
        assert_eq!(should_intercept_system_key(0x52, true, true, false, false), (true, true));
        assert_eq!(should_intercept_system_key(0x5B, false, true, false, false), (true, false));

        // Alt+Tab, Alt+Esc, Ctrl+Esc y PrintScreen se interceptan
        assert_eq!(should_intercept_system_key(0x09, true, false, true, false), (true, false));
        assert_eq!(should_intercept_system_key(0x1B, true, false, true, false), (true, false));
        assert_eq!(should_intercept_system_key(0x1B, true, false, false, true), (true, false));
        assert_eq!(should_intercept_system_key(0x2C, true, false, false, false), (true, false));
    }
}
