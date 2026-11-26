//! Servicio de Windows `NT AUTHORITY\SYSTEM`, migración de datos en `ProgramData` y supervisor Session 0 -> consola activa.

use crate::service::HostRuntimeInfo;
use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::fs;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{error, info};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_SUCCESS, FILETIME, HANDLE, HWND, STILL_ACTIVE, WAIT_OBJECT_0,
};
use windows::Win32::Security::{
    CheckTokenMembership, CreateWellKnownSid, DuplicateTokenEx, SecurityImpersonation,
    SetTokenInformation, TokenPrimary, TokenSessionId, WinLocalSystemSid, PSID, TOKEN_ALL_ACCESS,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE,
    REG_DWORD, REG_OPTION_NON_VOLATILE,
};
use windows::Win32::System::RemoteDesktop::{ProcessIdToSessionId, WTSGetActiveConsoleSessionId};
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, CreateServiceW, DeleteService,
    OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, RegisterServiceCtrlHandlerExW,
    SetServiceStatus, StartServiceCtrlDispatcherW, StartServiceW, LPHANDLER_FUNCTION_EX,
    SC_HANDLE, SC_MANAGER_CONNECT, SC_MANAGER_CREATE_SERVICE, SC_STATUS_PROCESS_INFO,
    SERVICE_ACCEPT_SESSIONCHANGE, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP, SERVICE_ALL_ACCESS,
    SERVICE_AUTO_START, SERVICE_CONTROL_INTERROGATE, SERVICE_CONTROL_SESSIONCHANGE,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_ERROR_NORMAL, SERVICE_PAUSE_PENDING,
    SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS,
    SERVICE_STATUS_HANDLE, SERVICE_STATUS_PROCESS, SERVICE_STOPPED, SERVICE_STOP_PENDING,
    SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
};
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenProcessToken,
    TerminateProcess, WaitForSingleObject, DETACHED_PROCESS, PROCESS_ALL_ACCESS,
    PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

pub const SERVICE_NAME: &str = "RandyDeskService";
pub const SERVICE_DISPLAY_NAME: &str = "RandyDesk Remote Desktop Service";

const PROGRAM_FILES_DIR: &str = r"C:\Program Files\RandyDesk";
const PROGRAM_DATA_DIR: &str = r"C:\ProgramData\RandyDesk";
const REG_POLICY_SYSTEM_SUBKEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
const STATE_STALE_SECS: u64 = 20;

static SERVICE_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static SERVICE_STATUS_HANDLE_RAW: AtomicIsize = AtomicIsize::new(0);

/// Estado instalado del servicio de Windows en el equipo local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatusKind {
    NotInstalled,
    Stopped,
    Running,
    Pending,
}

/// Configuración persistida en `C:\ProgramData\RandyDesk\service_config.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceConfig {
    pub port: u16,
    pub password: String,
    pub relay_url: Option<String>,
    pub monitor: usize,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            port: randydesk_proto::DEFAULT_PORT,
            password: String::new(),
            relay_url: Some(randydesk_proto::DEFAULT_RELAY_URL.to_string()),
            monitor: 0,
        }
    }
}

/// Estado en vivo escrito periódicamente por el worker del servicio (`service_state.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceRuntimeState {
    pub host_id: String,
    pub port: u16,
    pub lan_ip: Option<String>,
    pub tailscale_ip: Option<String>,
    pub relay_url: Option<String>,
    pub relay_connected: bool,
    pub fingerprint_sha256: String,
    pub active_clients: usize,
    pub version: String,
    pub running_as_system: bool,
    pub updated_unix_secs: u64,
}

struct ScHandleGuard(SC_HANDLE);

impl Drop for ScHandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseServiceHandle(self.0);
            }
        }
    }
}

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Directorio estable de instalación del binario del servicio.
pub fn installed_exe_path() -> PathBuf {
    PathBuf::from(PROGRAM_FILES_DIR).join("RandyDesk.exe")
}

/// Directorio compartido del servicio (`C:\ProgramData\RandyDesk`).
pub fn program_data_dir() -> PathBuf {
    PathBuf::from(PROGRAM_DATA_DIR)
}

/// Directorio donde se guardan `host_id.txt` y certificados TLS: usa `ProgramData` si existe o si corre como `SYSTEM`.
pub fn shared_host_data_dir() -> PathBuf {
    let pd = program_data_dir();
    if is_running_as_system() || pd.join("host_id.txt").exists() {
        return pd;
    }
    if let Some(proj) = ProjectDirs::from("com", "RandyDesk", "RandyDesk") {
        return proj.config_dir().to_path_buf();
    }
    pd
}

/// Indica si el proceso actual corre con la cuenta `NT AUTHORITY\SYSTEM`.
pub fn is_running_as_system() -> bool {
    let mut sid_buf = [0u8; 64];
    let mut cb_sid = sid_buf.len() as u32;
    let psid = PSID(sid_buf.as_mut_ptr() as *mut c_void);
    let mut is_member = windows::Win32::Foundation::BOOL(0);
    unsafe {
        if CreateWellKnownSid(WinLocalSystemSid, PSID::default(), psid, &mut cb_sid).is_err() {
            return false;
        }
        if CheckTokenMembership(None, psid, &mut is_member).is_err() {
            return false;
        }
    }
    is_member.as_bool()
}

/// Consulta el estado del servicio `RandyDeskService` sin requerir privilegios de Administrador.
pub fn query_service_status() -> ServiceStatusKind {
    let Ok(scm) = (unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) })
    else {
        return ServiceStatusKind::NotInstalled;
    };
    let _scm_guard = ScHandleGuard(scm);
    let svc_name: Vec<u16> = SERVICE_NAME.encode_utf16().chain([0]).collect();
    let Ok(svc) =
        (unsafe { OpenServiceW(scm, PCWSTR::from_raw(svc_name.as_ptr()), SERVICE_QUERY_STATUS) })
    else {
        return ServiceStatusKind::NotInstalled;
    };
    let _svc_guard = ScHandleGuard(svc);

    let mut ssp = SERVICE_STATUS_PROCESS::default();
    let mut bytes_needed = 0u32;
    let buf = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut ssp as *mut SERVICE_STATUS_PROCESS).cast::<u8>(),
            size_of::<SERVICE_STATUS_PROCESS>(),
        )
    };
    if unsafe { QueryServiceStatusEx(svc, SC_STATUS_PROCESS_INFO, Some(buf), &mut bytes_needed) }
        .is_err()
    {
        return ServiceStatusKind::Stopped;
    }

    match ssp.dwCurrentState {
        SERVICE_RUNNING => ServiceStatusKind::Running,
        SERVICE_STOPPED => ServiceStatusKind::Stopped,
        _ => ServiceStatusKind::Pending,
    }
}

/// Guarda `service_config.json` en `ProgramData` (o `%TEMP%` si todavía no existe permiso en `ProgramData`).
pub fn save_service_config(cfg: &ServiceConfig) -> Result<()> {
    let pd = program_data_dir();
    if fs::create_dir_all(&pd).is_ok() {
        let target = pd.join("service_config.json");
        if let Ok(json) = serde_json::to_string_pretty(cfg) {
            if fs::write(&target, json).is_ok() {
                return Ok(());
            }
        }
    }
    let temp = std::env::temp_dir().join("randydesk_service_config.json");
    let json = serde_json::to_string_pretty(cfg)?;
    fs::write(temp, json)?;
    Ok(())
}

/// Carga la configuración del servicio desde `ProgramData`, con fallback a valores por defecto.
pub fn load_service_config() -> ServiceConfig {
    let path = program_data_dir().join("service_config.json");
    if let Ok(raw) = fs::read_to_string(path) {
        if let Ok(cfg) = serde_json::from_str::<ServiceConfig>(&raw) {
            return cfg;
        }
    }
    ServiceConfig::default()
}

/// Escribe `service_state.json` de forma atómica para que la GUI lea el estado del servicio `SYSTEM`.
pub fn write_service_runtime_state(info: &HostRuntimeInfo, version: &str) {
    let dir = program_data_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let state = ServiceRuntimeState {
        host_id: info.host_id.clone(),
        port: info.port,
        lan_ip: info.lan_ip.clone(),
        tailscale_ip: info.tailscale_ip.clone(),
        relay_url: info.relay_url.clone(),
        relay_connected: info.relay_connected,
        fingerprint_sha256: info.fingerprint_sha256.clone(),
        active_clients: info.active_clients,
        version: version.to_string(),
        running_as_system: info.is_system || is_running_as_system(),
        updated_unix_secs: now_unix_secs(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = dir.join("service_state.json.tmp");
        let dst = dir.join("service_state.json");
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, &dst);
        }
    }
}

/// Lee el estado en vivo del servicio `SYSTEM` solo si el servicio está activo y su timestamp es reciente.
pub fn read_service_runtime_state() -> Option<ServiceRuntimeState> {
    if query_service_status() != ServiceStatusKind::Running {
        return None;
    }
    let raw = fs::read_to_string(program_data_dir().join("service_state.json")).ok()?;
    let state = serde_json::from_str::<ServiceRuntimeState>(&raw).ok()?;
    if !is_state_fresh(state.updated_unix_secs, now_unix_secs(), STATE_STALE_SECS) {
        return None;
    }
    Some(state)
}

pub(crate) fn is_state_fresh(updated_unix_secs: u64, now_unix_secs: u64, max_age_secs: u64) -> bool {
    now_unix_secs.saturating_sub(updated_unix_secs) <= max_age_secs
}

/// Solicita elevación UAC para instalar o actualizar e iniciar el servicio Windows `SYSTEM`.
pub fn request_install_service_elevated(
    host_id: &str,
    port: u16,
    password: &str,
    relay_url: Option<&str>,
) -> Result<()> {
    let cfg = ServiceConfig {
        port,
        password: password.to_string(),
        relay_url: relay_url.map(ToString::to_string),
        monitor: 0,
    };
    let _ = save_service_config(&cfg);

    let exe = std::env::current_exe()?;
    let mut args = format!("--install-service --host-id {}", quote_arg(host_id));
    if let Some(proj) = ProjectDirs::from("com", "RandyDesk", "RandyDesk") {
        let dir = proj.config_dir().to_string_lossy().to_string();
        args.push_str(&format!(" --source-dir {}", quote_arg(&dir)));
    }
    launch_elevated_action(&exe, &args)
}

/// Solicita elevación UAC para detener y desinstalar el servicio Windows `SYSTEM`.
pub fn request_uninstall_service_elevated() -> Result<()> {
    let exe = std::env::current_exe()?;
    launch_elevated_action(&exe, "--uninstall-service")
}

fn launch_elevated_action(exe: &Path, args: &str) -> Result<()> {
    let exe_wide: Vec<u16> = exe
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain([0])
        .collect();
    let args_wide: Vec<u16> = args.encode_utf16().chain([0]).collect();

    let ret = unsafe {
        ShellExecuteW(
            HWND::default(),
            w!("runas"),
            PCWSTR::from_raw(exe_wide.as_ptr()),
            PCWSTR::from_raw(args_wide.as_ptr()),
            PCWSTR::null(),
            SW_HIDE,
        )
    };
    if (ret.0 as isize) <= 32 {
        return Err(anyhow!(
            "No se completó la elevación UAC para gestionar el servicio (código {})",
            ret.0 as isize
        ));
    }
    Ok(())
}

/// Instala o actualiza `RandyDeskService` como `LocalSystem` (debe invocarse con privilegios elevados).
pub fn install_service_now(host_id: Option<&str>, source_dir: Option<&Path>) -> Result<()> {
    stop_service_if_running(Duration::from_secs(6));

    let install_dir = Path::new(PROGRAM_FILES_DIR);
    fs::create_dir_all(install_dir)
        .with_context(|| format!("no se pudo crear {}", install_dir.display()))?;
    let target_exe = installed_exe_path();
    let current_exe = std::env::current_exe()?;
    if current_exe != target_exe {
        copy_with_retry(&current_exe, &target_exe)?;
    }

    let pd = program_data_dir();
    fs::create_dir_all(&pd).with_context(|| format!("no se pudo crear {}", pd.display()))?;

    // Migra ID numérico y certificados TLS del usuario si aún no existen en ProgramData
    if let Some(src) = source_dir {
        for fname in ["host_id.txt", "host_cert.der", "host_key.der"] {
            let dst_file = pd.join(fname);
            let src_file = src.join(fname);
            if !dst_file.exists() && src_file.exists() {
                let _ = fs::copy(&src_file, &dst_file);
            }
        }
    }
    if let Some(id) = host_id {
        if let Some(clean_id) = randydesk_proto::parse_numeric_id(id) {
            let id_file = pd.join("host_id.txt");
            if !id_file.exists() {
                let _ = fs::write(&id_file, clean_id);
            }
        }
    }

    // Mueve el JSON temporal si existiera en %TEMP%
    let temp_cfg = std::env::temp_dir().join("randydesk_service_config.json");
    let pd_cfg = pd.join("service_config.json");
    if temp_cfg.exists() {
        if fs::copy(&temp_cfg, &pd_cfg).is_ok() {
            let _ = fs::remove_file(&temp_cfg);
        }
    } else if !pd_cfg.exists() {
        let _ = save_service_config(&ServiceConfig::default());
    }

    let _ = fs::remove_file(stop_signal_path());
    let _ = enable_software_sas_policy();

    let cmd_str = format!("{} --service", quote_arg(&target_exe.to_string_lossy()));
    let cmd_wide: Vec<u16> = cmd_str.encode_utf16().chain([0]).collect();
    let svc_name_w: Vec<u16> = SERVICE_NAME.encode_utf16().chain([0]).collect();
    let display_w: Vec<u16> = SERVICE_DISPLAY_NAME.encode_utf16().chain([0]).collect();

    let scm = unsafe {
        OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE,
        )
    }
    .context("no se pudo abrir Service Control Manager (se requiere Administrador)")?;
    let _scm_guard = ScHandleGuard(scm);

    let svc = match unsafe { OpenServiceW(scm, PCWSTR::from_raw(svc_name_w.as_ptr()), SERVICE_ALL_ACCESS) } {
        Ok(existing) => {
            unsafe {
                ChangeServiceConfigW(
                    existing,
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_AUTO_START,
                    SERVICE_ERROR_NORMAL,
                    PCWSTR::from_raw(cmd_wide.as_ptr()),
                    PCWSTR::null(),
                    None,
                    PCWSTR::null(),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    PCWSTR::from_raw(display_w.as_ptr()),
                )
            }
            .context("no se pudo actualizar configuración del servicio")?;
            existing
        }
        Err(_) => unsafe {
            CreateServiceW(
                scm,
                PCWSTR::from_raw(svc_name_w.as_ptr()),
                PCWSTR::from_raw(display_w.as_ptr()),
                SERVICE_ALL_ACCESS,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_AUTO_START,
                SERVICE_ERROR_NORMAL,
                PCWSTR::from_raw(cmd_wide.as_ptr()),
                PCWSTR::null(),
                None,
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
            )
        }
        .context("no se pudo crear RandyDeskService")?,
    };
    let _svc_guard = ScHandleGuard(svc);

    unsafe { StartServiceW(svc, None) }.context("no se pudo iniciar RandyDeskService")?;
    info!("RandyDeskService instalado e iniciado como NT AUTHORITY\\SYSTEM");
    Ok(())
}

/// Detiene y elimina `RandyDeskService` del SCM (debe invocarse elevado).
pub fn uninstall_service_now() -> Result<()> {
    stop_service_if_running(Duration::from_secs(6));
    let scm = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .context("no se pudo abrir Service Control Manager")?;
    let _scm_guard = ScHandleGuard(scm);

    let svc_name_w: Vec<u16> = SERVICE_NAME.encode_utf16().chain([0]).collect();
    if let Ok(svc) =
        unsafe { OpenServiceW(scm, PCWSTR::from_raw(svc_name_w.as_ptr()), SERVICE_ALL_ACCESS) }
    {
        let _svc_guard = ScHandleGuard(svc);
        unsafe { DeleteService(svc) }.context("no se pudo eliminar RandyDeskService")?;
    }
    let _ = fs::remove_file(program_data_dir().join("service_state.json"));
    Ok(())
}

fn stop_service_if_running(timeout: Duration) {
    let Ok(scm) = (unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) })
    else {
        return;
    };
    let _scm_guard = ScHandleGuard(scm);
    let svc_name_w: Vec<u16> = SERVICE_NAME.encode_utf16().chain([0]).collect();
    let Ok(svc) =
        (unsafe { OpenServiceW(scm, PCWSTR::from_raw(svc_name_w.as_ptr()), SERVICE_ALL_ACCESS) })
    else {
        return;
    };
    let _svc_guard = ScHandleGuard(svc);

    let mut status = SERVICE_STATUS::default();
    let _ = unsafe { ControlService(svc, SERVICE_CONTROL_STOP, &mut status) };

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let mut ssp = SERVICE_STATUS_PROCESS::default();
        let mut needed = 0u32;
        let buf = unsafe {
            std::slice::from_raw_parts_mut(
                (&mut ssp as *mut SERVICE_STATUS_PROCESS).cast::<u8>(),
                size_of::<SERVICE_STATUS_PROCESS>(),
            )
        };
        if unsafe { QueryServiceStatusEx(svc, SC_STATUS_PROCESS_INFO, Some(buf), &mut needed) }
            .is_err()
            || ssp.dwCurrentState == SERVICE_STOPPED
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn copy_with_retry(src: &Path, dst: &Path) -> Result<()> {
    let mut last_err = None;
    for _ in 0..10 {
        match fs::copy(src, dst) {
            Ok(_) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    Err(anyhow!(
        "no se pudo copiar {} hacia {}: {:?}",
        src.display(),
        dst.display(),
        last_err
    ))
}

fn enable_software_sas_policy() -> Result<()> {
    let subkey: Vec<u16> = REG_POLICY_SYSTEM_SUBKEY.encode_utf16().chain([0]).collect();
    let mut hkey = HKEY::default();
    let err = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR::from_raw(subkey.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        )
    };
    if err != ERROR_SUCCESS {
        return Err(anyhow!("RegCreateKeyExW devolvió {err:?}"));
    }
    let val: u32 = 3;
    let set_err = unsafe {
        RegSetValueExW(
            hkey,
            w!("SoftwareSASGeneration"),
            0,
            REG_DWORD,
            Some(&val.to_le_bytes()),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if set_err != ERROR_SUCCESS {
        return Err(anyhow!("RegSetValueExW devolvió {set_err:?}"));
    }
    Ok(())
}

/// Punto de entrada cuando `RandyDesk.exe --service` es lanzado por Windows SCM en Session 0.
pub fn run_windows_service() -> Result<()> {
    SERVICE_STOP_REQUESTED.store(false, Ordering::SeqCst);
    let mut svc_name_w: Vec<u16> = SERVICE_NAME.encode_utf16().chain([0]).collect();
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(svc_name_w.as_mut_ptr()),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR::null(),
            lpServiceProc: None,
        },
    ];
    unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) }
        .context("StartServiceCtrlDispatcherW falló (debe ejecutarse desde Windows SCM)")
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut PWSTR) {
    let svc_name_w: Vec<u16> = SERVICE_NAME.encode_utf16().chain([0]).collect();
    let handler: LPHANDLER_FUNCTION_EX = Some(service_ctrl_handler);
    let Ok(handle) =
        RegisterServiceCtrlHandlerExW(PCWSTR::from_raw(svc_name_w.as_ptr()), handler, None)
    else {
        return;
    };
    SERVICE_STATUS_HANDLE_RAW.store(handle.0 as isize, Ordering::SeqCst);

    report_service_status(handle, SERVICE_START_PENDING, 0, 3000);
    let _ = fs::create_dir_all(program_data_dir());
    let _ = fs::remove_file(stop_signal_path());
    report_service_status(handle, SERVICE_RUNNING, 0, 0);

    service_supervisor_loop();

    let _ = fs::write(stop_signal_path(), b"stop");
    let _ = fs::remove_file(program_data_dir().join("service_state.json"));
    report_service_status(handle, SERVICE_STOPPED, 0, 0);
}

unsafe extern "system" fn service_ctrl_handler(
    dw_control: u32,
    _dw_event_type: u32,
    _lp_event_data: *mut c_void,
    _lp_context: *mut c_void,
) -> u32 {
    let handle = SERVICE_STATUS_HANDLE(SERVICE_STATUS_HANDLE_RAW.load(Ordering::Acquire) as *mut c_void);
    match dw_control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            SERVICE_STOP_REQUESTED.store(true, Ordering::SeqCst);
            let _ = fs::write(stop_signal_path(), b"stop");
            if !handle.is_invalid() {
                report_service_status(handle, SERVICE_STOP_PENDING, 0, 3000);
            }
            ERROR_SUCCESS.0
        }
        SERVICE_CONTROL_INTERROGATE => ERROR_SUCCESS.0,
        SERVICE_CONTROL_SESSIONCHANGE => ERROR_SUCCESS.0,
        _ if dw_control == SERVICE_PAUSE_PENDING.0 => ERROR_SUCCESS.0,
        _ => ERROR_SUCCESS.0,
    }
}

unsafe fn report_service_status(
    handle: SERVICE_STATUS_HANDLE,
    state: windows::Win32::System::Services::SERVICE_STATUS_CURRENT_STATE,
    exit_code: u32,
    wait_hint: u32,
) {
    let controls = if state == SERVICE_START_PENDING {
        0
    } else {
        SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN | SERVICE_ACCEPT_SESSIONCHANGE
    };
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: controls,
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: wait_hint,
    };
    let _ = SetServiceStatus(handle, &status);
}

struct ChildWorker {
    session_id: u32,
    process: HandleGuard,
}

fn service_supervisor_loop() {
    let mut worker: Option<ChildWorker> = None;
    let exe_path = installed_exe_path();

    while !SERVICE_STOP_REQUESTED.load(Ordering::Acquire) {
        let active_sid = unsafe { WTSGetActiveConsoleSessionId() };
        if active_sid != u32::MAX {
            let need_restart = match &worker {
                None => true,
                Some(w) => w.session_id != active_sid || !is_process_alive(w.process.0),
            };
            if need_restart {
                if let Some(old) = worker.take() {
                    terminate_and_wait(&old.process.0);
                }
                match spawn_worker_in_session(&exe_path, active_sid) {
                    Ok(proc_handle) => {
                        info!("Host worker SYSTEM iniciado en sesión consola {active_sid}");
                        worker = Some(ChildWorker {
                            session_id: active_sid,
                            process: HandleGuard(proc_handle),
                        });
                    }
                    Err(err) => {
                        error!("Error lanzando host worker en sesión {active_sid}: {err:#}");
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(600));
    }

    if let Some(old) = worker.take() {
        let _ = fs::write(stop_signal_path(), b"stop");
        let exited = unsafe { WaitForSingleObject(old.process.0, 1500) } == WAIT_OBJECT_0;
        if !exited {
            terminate_and_wait(&old.process.0);
        }
    }
    let _ = fs::remove_file(stop_signal_path());
}

fn is_process_alive(handle: HANDLE) -> bool {
    let mut code = 0u32;
    if unsafe { GetExitCodeProcess(handle, &mut code) }.is_err() {
        return false;
    }
    code == STILL_ACTIVE.0 as u32
}

fn terminate_and_wait(handle: &HANDLE) {
    unsafe {
        let _ = TerminateProcess(*handle, 0);
        let _ = WaitForSingleObject(*handle, 1000);
    }
}

pub(crate) fn is_winlogon_exe_name(wide: &[u16]) -> bool {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len]).eq_ignore_ascii_case("winlogon.exe")
}

/// Busca el PID de `winlogon.exe` dentro de la sesión interactiva indicada (estilo RustDesk `GetLogonPid`).
fn find_winlogon_pid_in_session(session_id: u32) -> Option<u32> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let _snap_guard = HandleGuard(snap);

    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snap, &mut entry) }.is_err() {
        return None;
    }

    loop {
        if is_winlogon_exe_name(&entry.szExeFile) {
            let mut proc_sid = 0u32;
            if unsafe { ProcessIdToSessionId(entry.th32ProcessID, &mut proc_sid) }.is_ok()
                && proc_sid == session_id
            {
                return Some(entry.th32ProcessID);
            }
        }
        if unsafe { Process32NextW(snap, &mut entry) }.is_err() {
            break;
        }
    }
    None
}

/// Obtiene un token `SYSTEM` interactivo desde `winlogon.exe` de la sesión activa (con fallback duplicando el token del servicio).
fn open_session_system_token(session_id: u32) -> Result<HandleGuard> {
    if let Some(winlogon_pid) = find_winlogon_pid_in_session(session_id) {
        if let Ok(winlogon_proc) = unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, winlogon_pid) } {
            let _proc_guard = HandleGuard(winlogon_proc);
            let mut raw_token = HANDLE::default();
            if unsafe { OpenProcessToken(winlogon_proc, TOKEN_ALL_ACCESS, &mut raw_token) }.is_ok()
            {
                let _raw_guard = HandleGuard(raw_token);
                let mut primary_token = HANDLE::default();
                if unsafe {
                    DuplicateTokenEx(
                        raw_token,
                        TOKEN_ALL_ACCESS,
                        None,
                        SecurityImpersonation,
                        TokenPrimary,
                        &mut primary_token,
                    )
                }
                .is_ok()
                {
                    return Ok(HandleGuard(primary_token));
                }
            }
        }
    }

    let mut proc_token = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_QUERY,
            &mut proc_token,
        )
    }
    .context("OpenProcessToken SYSTEM falló")?;
    let _proc_tok_guard = HandleGuard(proc_token);

    let mut dup_token = HANDLE::default();
    unsafe {
        DuplicateTokenEx(
            proc_token,
            TOKEN_ALL_ACCESS,
            None,
            SecurityImpersonation,
            TokenPrimary,
            &mut dup_token,
        )
    }
    .context("DuplicateTokenEx falló")?;
    let dup_guard = HandleGuard(dup_token);

    unsafe {
        SetTokenInformation(
            dup_guard.0,
            TokenSessionId,
            (&session_id as *const u32).cast(),
            size_of::<u32>() as u32,
        )
    }
    .context("SetTokenInformation(TokenSessionId) falló")?;

    Ok(dup_guard)
}

fn spawn_worker_in_session(exe_path: &Path, session_id: u32) -> Result<HANDLE> {
    let token_guard = open_session_system_token(session_id)?;

    let mut desktop_w: Vec<u16> = r"winsta0\default".encode_utf16().chain([0]).collect();
    let mut cmd_w: Vec<u16> = build_worker_command_line(exe_path)
        .encode_utf16()
        .chain([0])
        .collect();
    let work_dir = exe_path.parent().unwrap_or_else(|| Path::new(PROGRAM_FILES_DIR));
    let work_dir_w: Vec<u16> = work_dir
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain([0])
        .collect();

    let si = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        lpDesktop: PWSTR(desktop_w.as_mut_ptr()),
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_HIDE.0 as u16,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();

    unsafe {
        CreateProcessAsUserW(
            token_guard.0,
            PCWSTR::null(),
            PWSTR(cmd_w.as_mut_ptr()),
            None,
            None,
            false,
            DETACHED_PROCESS,
            None,
            PCWSTR::from_raw(work_dir_w.as_ptr()),
            &si,
            &mut pi,
        )
    }
    .context("CreateProcessAsUserW --host-worker falló")?;

    if !pi.hThread.is_invalid() {
        unsafe {
            let _ = CloseHandle(pi.hThread);
        }
    }
    Ok(pi.hProcess)
}

/// Ejecuta el agente host con privilegios `SYSTEM` dentro de la sesión de escritorio activa.
pub fn run_host_worker(rt_handle: &tokio::runtime::Handle, app_version: &str) -> Result<()> {
    let cfg = load_service_config();
    let mut svc = crate::service::HostService::start(
        rt_handle,
        cfg.port,
        cfg.password,
        cfg.monitor,
        randydesk_proto::StreamSettings::default(),
        cfg.relay_url,
    )?;

    info!(
        "Host worker activo (ID={}, is_system={})",
        svc.info().host_id,
        is_running_as_system()
    );

    let stop_file = stop_signal_path();
    while !stop_file.exists() {
        let info = svc.info();
        write_service_runtime_state(&info, app_version);
        std::thread::sleep(Duration::from_secs(2));
    }

    svc.stop();
    let _ = fs::remove_file(program_data_dir().join("service_state.json"));
    let _ = FILETIME::default();
    Ok(())
}

pub(crate) fn build_worker_command_line(exe_path: &Path) -> String {
    format!("{} --host-worker", quote_arg(&exe_path.to_string_lossy()))
}

fn stop_signal_path() -> PathBuf {
    program_data_dir().join("worker.stop")
}

fn quote_arg(val: &str) -> String {
    format!("\"{}\"", val.replace('"', "\\\""))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_config_and_state_roundtrip_json() {
        let cfg = ServiceConfig {
            port: 21118,
            password: "secret".into(),
            relay_url: Some("wss://randydesk-relay.onrender.com".into()),
            monitor: 1,
        };
        let s = serde_json::to_string(&cfg).expect("serialize cfg");
        let back: ServiceConfig = serde_json::from_str(&s).expect("deserialize cfg");
        assert_eq!(cfg, back);

        assert!(is_state_fresh(1000, 1015, 20));
        assert!(!is_state_fresh(1000, 1025, 20));
    }

    #[test]
    fn formats_quoted_worker_command_line() {
        let cmd = build_worker_command_line(Path::new(r"C:\Program Files\RandyDesk\RandyDesk.exe"));
        assert_eq!(cmd, r#""C:\Program Files\RandyDesk\RandyDesk.exe" --host-worker"#);
    }

    #[test]
    fn matches_winlogon_name_case_insensitive() {
        let winlogon: Vec<u16> = "WinLogon.EXE\0other".encode_utf16().collect();
        let explorer: Vec<u16> = "explorer.exe\0".encode_utf16().collect();
        assert!(is_winlogon_exe_name(&winlogon));
        assert!(!is_winlogon_exe_name(&explorer));
    }
}
