//! Hook de teclado de bajo nivel (`WH_KEYBOARD_LL`) estilo RustDesk para capturar solo `Win` y atajos de sistema.

use crate::net::NetCommand;
use randydesk_host::input::{append_input_debug_log, RANDYDESK_INPUT_EXTRA_INFO};
use randydesk_proto::InputEvent;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex, OnceLock};
use tokio::sync::mpsc;
use tracing::warn;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetAncestor, GetCursorPos, GetForegroundWindow, GetMessageW,
    GetWindowThreadProcessId, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, WindowFromPoint, GA_ROOT, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, MSG, PM_NOREMOVE, PM_NOYIELD, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Códigos Virtual-Key de modificadores que se liberan al perder foco para evitar teclas pegadas.
const MODIFIER_VKS: &[u16] = &[0x5B, 0x5C, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5];
const VK_PACKET_U16: u16 = 0x00E7;
const VK_LWIN_U16: u16 = 0x005B;
const VK_RWIN_U16: u16 = 0x005C;

/// Modo de captura/traducción de teclado hacia la sesión remota.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardMode {
    /// Texto normal vía ventana y captura de `Win` / `Alt+Tab` por hook de bajo nivel.
    #[default]
    Translate,
    /// Captura directa de todas las teclas físicas por hook mientras está en sesión.
    Map,
}

struct HookSharedState {
    installed: AtomicBool,
    active: AtomicBool,
    map_mode: AtomicBool,
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
            map_mode: AtomicBool::new(false),
            win_down: AtomicBool::new(false),
            own_pid: unsafe { GetCurrentProcessId() },
            sent_keys: AtomicU64::new(0),
            tx: Mutex::new(None),
        })
    })
}

/// Controlador RAII con ciclo de vida dinámico estilo RustDesk para `WH_KEYBOARD_LL`.
pub struct KeyboardHookGuard {
    thread_id: Arc<AtomicU32>,
    worker: Option<std::thread::JoinHandle<()>>,
    was_active: bool,
}

impl Default for KeyboardHookGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardHookGuard {
    /// Crea el controlador sin instalar `WH_KEYBOARD_LL` hasta que `set_capture(true, ...)` sea invocado.
    pub fn new() -> Self {
        let _ = shared_state();
        Self {
            thread_id: Arc::new(AtomicU32::new(0)),
            worker: None,
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

    /// Cambia el modo de teclado (`Translate`: solo atajos sistema, `Map`: teclado físico completo).
    pub fn set_mode(&self, mode: KeyboardMode) {
        shared_state()
            .map_mode
            .store(matches!(mode, KeyboardMode::Map), Ordering::Relaxed);
    }

    /// Indica si el hook está activo capturando todas las teclas físicas (`Modo mapa`).
    pub fn is_capturing_all_keys(&self) -> bool {
        self.was_active
            && self.is_installed()
            && shared_state().map_mode.load(Ordering::Relaxed)
    }

    /// Activa o desactiva la captura de atajos de sistema hacia `tx`; instala y desinstala el hook dinámicamente.
    pub fn set_capture(&mut self, active: bool, tx: Option<mpsc::Sender<NetCommand>>) {
        let state = shared_state();

        if active {
            if self.worker.as_ref().is_some_and(|h| h.is_finished()) {
                if let Some(handle) = self.worker.take() {
                    let _ = handle.join();
                }
                self.thread_id.store(0, Ordering::SeqCst);
            }

            if let Ok(mut guard) = state.tx.lock() {
                if !self.was_active || (guard.as_ref().is_none_or(|old| old.is_closed()) && tx.is_some()) {
                    *guard = tx;
                }
            }
            state.active.store(true, Ordering::SeqCst);

            if self.worker.is_none() {
                self.spawn_hook_thread();
            }
            self.was_active = true;
            return;
        }

        if self.was_active {
            self.release_modifiers_now();
        }
        state.win_down.store(false, Ordering::Relaxed);
        state.active.store(false, Ordering::SeqCst);

        if self.was_active || self.worker.is_some() {
            if let Ok(mut guard) = state.tx.lock() {
                *guard = None;
            }
            self.stop_hook_thread();
        }
        self.was_active = false;
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

    fn spawn_hook_thread(&mut self) {
        let tid_clone = Arc::clone(&self.thread_id);
        let (ready_tx, ready_rx) = std_mpsc::sync_channel::<()>(1);

        match std::thread::Builder::new()
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
                            let _ = ready_tx.send(());
                            return;
                        }
                    };
                shared_state().installed.store(true, Ordering::SeqCst);
                append_input_debug_log("[CLIENT-HOOK] WH_KEYBOARD_LL installed=true");
                let _ = ready_tx.send(());

                while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                shared_state().installed.store(false, Ordering::SeqCst);
                let _ = UnhookWindowsHookEx(hook);
                append_input_debug_log("[CLIENT-HOOK] WH_KEYBOARD_LL uninstalled=true");
            }) {
            Ok(handle) => {
                let _ = ready_rx.recv();
                self.worker = Some(handle);
            }
            Err(err) => {
                warn!("No se pudo crear hilo del hook WH_KEYBOARD_LL: {err}");
                shared_state().installed.store(false, Ordering::SeqCst);
            }
        }
    }

    fn stop_hook_thread(&mut self) {
        let tid = self.thread_id.swap(0, Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        shared_state().installed.store(false, Ordering::SeqCst);
    }
}

impl Drop for KeyboardHookGuard {
    fn drop(&mut self) {
        self.set_capture(false, None);
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

/// Decide si el hook debe capturar según si nuestra ventana está en primer plano o bajo el cursor.
pub(crate) fn should_capture_for_pids(own_pid: u32, fg_pid: u32, cursor_pid: u32) -> bool {
    own_pid != 0 && (fg_pid == own_pid || cursor_pid == own_pid)
}

unsafe extern "system" fn low_level_kb_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let state = shared_state();
        let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode as u16;
        let msg_id = w_param.0 as u32;
        let down = match msg_id {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
            WM_KEYUP | WM_SYSKEYUP => Some(false),
            _ => None,
        };
        let active = state.active.load(Ordering::Relaxed);
        let is_self_injected = kb.dwExtraInfo == RANDYDESK_INPUT_EXTRA_INFO;
        let fg_pid = foreground_window_pid();
        let cursor_pid = cursor_window_pid();

        if vk == VK_LWIN_U16 || vk == VK_RWIN_U16 {
            let has_tx = state.tx.try_lock().map(|g| g.is_some()).unwrap_or(false);
            append_input_debug_log(&format!(
                "[CLIENT-HOOK] win_key vk=0x{vk:04X} active={active} fg_pid={fg_pid} cursor_pid={cursor_pid} own_pid={} has_tx={has_tx} down={:?} self_injected={is_self_injected}",
                state.own_pid, down
            ));
        }

        if active
            && !is_self_injected
            && should_capture_for_pids(state.own_pid, fg_pid, cursor_pid)
        {
            if let Some(down) = down {
                let win_was_down = state.win_down.load(Ordering::Relaxed);
                let map_mode = state.map_mode.load(Ordering::Relaxed);
                let intercept = if map_mode {
                    if matches!(vk, VK_LWIN_U16 | VK_RWIN_U16) {
                        state.win_down.store(down, Ordering::Relaxed);
                    }
                    true
                } else {
                    let alt_down = (GetAsyncKeyState(0x12) as u16 & 0x8000) != 0;
                    let ctrl_down = (GetAsyncKeyState(0x11) as u16 & 0x8000) != 0;
                    let (hit, new_win_down) =
                        should_intercept_system_key(vk, down, win_was_down, alt_down, ctrl_down);
                    state.win_down.store(new_win_down, Ordering::Relaxed);
                    hit
                };

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
    CallNextHookEx(None, n_code, w_param, l_param)
}

unsafe fn foreground_window_pid() -> u32 {
    let hwnd = GetForegroundWindow();
    if hwnd.is_invalid() {
        return 0;
    }
    let mut fg_pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut fg_pid));
    fg_pid
}

unsafe fn cursor_window_pid() -> u32 {
    let mut pt = POINT::default();
    if GetCursorPos(&mut pt).is_err() {
        return 0;
    }
    let hwnd = WindowFromPoint(pt);
    if hwnd.is_invalid() {
        return 0;
    }
    let root = GetAncestor(hwnd, GA_ROOT);
    let target = if root.is_invalid() { hwnd } else { root };
    let mut pid = 0u32;
    GetWindowThreadProcessId(target, Some(&mut pid));
    pid
}

unsafe fn is_own_window_foreground(own_pid: u32) -> bool {
    let fg_pid = foreground_window_pid();
    fg_pid != 0 && fg_pid == own_pid
}

#[cfg(test)]
mod tests {
    use super::*;

    static LIFECYCLE_LOCK: Mutex<()> = Mutex::new(());

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

        // Tecla Win izquierda/derecha y combinaciones Win+R / Win+E se interceptan
        assert_eq!(should_intercept_system_key(0x5B, true, false, false, false), (true, true));
        assert_eq!(should_intercept_system_key(0x5C, true, false, false, false), (true, true));
        assert_eq!(should_intercept_system_key(0x52, true, true, false, false), (true, true));
        assert_eq!(should_intercept_system_key(0x45, true, true, false, false), (true, true));
        assert_eq!(should_intercept_system_key(0x5B, false, true, false, false), (true, false));
        assert_eq!(should_intercept_system_key(0x5C, false, true, false, false), (true, false));

        // Alt+Tab, Alt+Esc, Ctrl+Esc y PrintScreen se interceptan
        assert_eq!(should_intercept_system_key(0x09, true, false, true, false), (true, false));
        assert_eq!(should_intercept_system_key(0x1B, true, false, true, false), (true, false));
        assert_eq!(should_intercept_system_key(0x1B, true, false, false, true), (true, false));
        assert_eq!(should_intercept_system_key(0x2C, true, false, false, false), (true, false));
    }

    #[test]
    fn captures_when_foreground_or_cursor_is_over_own_window() {
        // Caso real Windows 11: Menú Inicio (SearchHost PID 8496) en foreground mientras cursor sigue sobre RandyDesk
        assert!(should_capture_for_pids(12736, 12736, 12736));
        assert!(should_capture_for_pids(12736, 8496, 12736));
        assert!(should_capture_for_pids(12736, 12736, 9999));
        assert!(!should_capture_for_pids(12736, 8496, 9999));
        assert!(!should_capture_for_pids(0, 8496, 0));
    }

    #[test]
    fn dynamic_hook_lifecycle_installs_and_uninstalls_with_set_capture() {
        let _lock = LIFECYCLE_LOCK.lock().expect("lifecycle lock");
        let mut guard = KeyboardHookGuard::new();

        // Al arrancar no debe haber hook instalado ni hilo corriendo
        assert!(!guard.is_installed());
        assert!(guard.worker.is_none());
        assert_eq!(guard.thread_id.load(Ordering::SeqCst), 0);

        let (tx, mut rx) = mpsc::channel(32);
        guard.set_capture(true, Some(tx));
        assert!(guard.worker.is_some());
        assert_ne!(guard.thread_id.load(Ordering::SeqCst), 0);
        assert!(guard.is_installed());

        // Llamadas repetidas por frame no deben cerrar ni reinstalar el hilo
        let initial_tid = guard.thread_id.load(Ordering::SeqCst);
        guard.set_capture(true, None);
        assert_eq!(guard.thread_id.load(Ordering::SeqCst), initial_tid);
        assert!(guard.is_installed());

        // Al desactivar: libera modificadores, limpia tx, envía WM_QUIT y hace join()
        guard.set_capture(false, None);
        assert!(!guard.is_installed());
        assert!(guard.worker.is_none());
        assert_eq!(guard.thread_id.load(Ordering::SeqCst), 0);
        assert!(shared_state().tx.lock().expect("tx lock").is_none());

        let mut released_vks = Vec::new();
        while let Ok(NetCommand::Input(InputEvent::Key { vk, down, .. })) = rx.try_recv() {
            assert!(!down);
            released_vks.push(vk);
        }
        assert_eq!(released_vks, MODIFIER_VKS);

        // Puede reactivarse dinámicamente para una nueva sesión y limpiarse en Drop
        let (tx2, _rx2) = mpsc::channel(16);
        guard.set_capture(true, Some(tx2));
        assert!(guard.is_installed());
        drop(guard);
        assert!(!shared_state().installed.load(Ordering::Acquire));
    }

    #[test]
    fn release_modifiers_now_emits_keyups_without_uninstalling_hook() {
        let _lock = LIFECYCLE_LOCK.lock().expect("lifecycle lock");
        let mut guard = KeyboardHookGuard::new();
        let (tx, mut rx) = mpsc::channel(32);
        guard.set_capture(true, Some(tx));
        assert!(guard.is_installed());

        guard.release_modifiers_now();
        assert!(guard.is_installed());

        let mut released = Vec::new();
        while let Ok(NetCommand::Input(InputEvent::Key { vk, extended, down, .. })) = rx.try_recv()
        {
            assert!(!down);
            released.push((vk, extended));
        }
        assert_eq!(released.len(), MODIFIER_VKS.len());
        assert!(released.contains(&(0x5B, true)));
        assert!(released.contains(&(0x5C, true)));
        assert!(released.contains(&(0xA0, false)));
        assert!(released.contains(&(0xA3, true)));
        assert!(released.contains(&(0xA5, true)));

        guard.set_capture(false, None);
        assert!(!guard.is_installed());
    }
}
