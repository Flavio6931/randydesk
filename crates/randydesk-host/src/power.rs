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
