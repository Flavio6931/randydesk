//! Prevención no intrusiva de suspensión de Windows durante recepción o sesiones remotas activas.

use windows::Win32::System::Power::{
    SetThreadExecutionState, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};

/// Reinicia el temporizador de suspensión del sistema operativo sin forzar pantalla encendida.
pub fn keep_system_awake() {
    unsafe {
        let _ = SetThreadExecutionState(ES_SYSTEM_REQUIRED);
    }
}

/// Reinicia los temporizadores de sistema y pantalla durante una sesión remota interactiva.
pub fn keep_session_awake() {
    unsafe {
        let _ = SetThreadExecutionState(ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED);
    }
}

/// Bloquea la estación de trabajo de Windows en el equipo anfitrión.
pub fn lock_workstation() -> bool {
    unsafe { windows::Win32::System::Shutdown::LockWorkStation().is_ok() }
}

/// Envía la secuencia de atención segura (`Ctrl+Alt+Del`) mediante `sas.dll!SendSAS` (requiere servicio `SYSTEM`).
pub fn send_sas() -> bool {
    use windows::core::{s, w};
    use windows::Win32::Foundation::{BOOL, FreeLibrary};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    unsafe {
        let Ok(hmod) = LoadLibraryW(w!("sas.dll")) else {
            return false;
        };
        let res = if let Some(proc) = GetProcAddress(hmod, s!("SendSAS")) {
            let func: unsafe extern "system" fn(BOOL) = std::mem::transmute(proc);
            func(BOOL(0));
            true
        } else {
            false
        };
        let _ = FreeLibrary(hmod);
        res
    }
}
