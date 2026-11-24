//! Hook de teclado de bajo nivel (`WH_KEYBOARD_LL`) para replicar `Win` y atajos en la PC remota.

use crate::net::NetCommand;
use randydesk_proto::InputEvent;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc;
use tracing::warn;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK,
    KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Códigos Virtual-Key de modificadores que se liberan al perder foco para evitar teclas pegadas.
const MODIFIER_VKS: &[u16] = &[0x5B, 0x5C, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5];
const LLKHF_INJECTED_BIT: u32 = 0x10;

struct HookSharedState {
    installed: AtomicBool,
    active: AtomicBool,
    own_pid: u32,
    tx: Mutex<Option<mpsc::Sender<NetCommand>>>,
}

static HOOK_STATE: OnceLock<Arc<HookSharedState>> = OnceLock::new();

fn shared_state() -> &'static Arc<HookSharedState> {
    HOOK_STATE.get_or_init(|| {
        Arc::new(HookSharedState {
            installed: AtomicBool::new(false),
            active: AtomicBool::new(false),
            own_pid: unsafe { GetCurrentProcessId() },
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
    /// Instala el hook de bajo nivel en un hilo Win32 dedicado con bucle de mensajes.
    pub fn new() -> Self {
        let _ = shared_state();
        let thread_id = Arc::new(AtomicU32::new(0));
        let tid_clone = Arc::clone(&thread_id);

        let worker = std::thread::spawn(move || unsafe {
            tid_clone.store(GetCurrentThreadId(), Ordering::SeqCst);
            let hmod: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
            let hook: HHOOK = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_kb_proc), hmod, 0) {
                Ok(h) => h,
                Err(err) => {
                    warn!("No se pudo instalar hook WH_KEYBOARD_LL: {err}");
                    shared_state().installed.store(false, Ordering::SeqCst);
                    return;
                }
            };
            shared_state().installed.store(true, Ordering::SeqCst);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            shared_state().installed.store(false, Ordering::SeqCst);
            let _ = UnhookWindowsHookEx(hook);
        });

        Self {
            thread_id,
            worker: Some(worker),
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

    /// Activa o desactiva la captura exclusiva de teclado hacia `tx`; libera modificadores al soltar.
    pub fn set_capture(&mut self, active: bool, tx: Option<mpsc::Sender<NetCommand>>) {
        if self.was_active == active {
            if active {
                if let Ok(mut guard) = shared_state().tx.lock() {
                    if guard.is_none() && tx.is_some() {
                        *guard = tx;
                    }
                }
            }
            return;
        }

        let state = shared_state();
        if self.was_active && !active {
            if let Ok(guard) = state.tx.lock() {
                if let Some(prev_tx) = guard.as_ref() {
                    release_modifiers(prev_tx);
                }
            }
        }
        state.active.store(active, Ordering::SeqCst);
        if let Ok(mut guard) = state.tx.lock() {
            *guard = if active { tx } else { None };
        }
        self.was_active = active;
    }

    /// Indica si el hook está activo e interceptando realmente las teclas hacia la PC remota.
    pub fn is_capturing(&self) -> bool {
        self.was_active && self.is_installed()
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

unsafe extern "system" fn low_level_kb_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let state = shared_state();
        if state.active.load(Ordering::Relaxed) {
            let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
            let is_injected = (kb.flags.0 & LLKHF_INJECTED_BIT) != 0;

            if !is_injected && is_own_window_foreground(state.own_pid) {
                let msg_id = w_param.0 as u32;
                let down = match msg_id {
                    WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
                    WM_KEYUP | WM_SYSKEYUP => Some(false),
                    _ => None,
                };
                if let Some(down) = down {
                    if let Ok(guard) = state.tx.lock() {
                        if let Some(tx) = guard.as_ref() {
                            let vk = kb.vkCode as u16;
                            let extended =
                                (kb.flags.0 & LLKHF_EXTENDED.0) != 0 || matches!(vk, 0x5B | 0x5C);
                            let _ = tx.try_send(NetCommand::Input(InputEvent::Key {
                                vk,
                                scan: kb.scanCode as u16,
                                extended,
                                down,
                            }));
                            return LRESULT(1);
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
