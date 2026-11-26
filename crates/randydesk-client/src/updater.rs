//! Auto-actualizador en 1 clic desde GitHub Releases (descarga verificada + reemplazo seguro en Windows).

use anyhow::{anyhow, Context, Result};
use eframe::egui;
use futures_util::StreamExt;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

pub const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/Flavio6931/randydesk/releases/latest";
pub const RELEASE_ASSET_NAME: &str = "RandyDesk.exe";
pub const MIN_VALID_EXE_BYTES: u64 = 1_000_000;
const TEMP_UPDATE_PREFIX: &str = "RandyDesk_update_";

/// Metadatos de una nueva versión disponible en GitHub Releases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag: String,
    pub download_url: String,
    pub asset_size: u64,
}

/// Estado observable por la UI del ciclo de actualización.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(ReleaseInfo),
    Downloading { version: String, percent: u8 },
    Applying { version: String },
    Error(String),
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Parsea una etiqueta semver (`v0.5.3`, `0.5.3-rc1`) a `(major, minor, patch)`.
pub fn parse_semver(raw: &str) -> Option<(u32, u32, u32)> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let core = trimmed.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Devuelve `true` solo si `latest` es estrictamente mayor que `current`.
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_semver(current), parse_semver(latest)) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    }
}

/// Valida que el archivo descargado comience con la cabecera PE de Windows (`MZ`).
pub fn is_valid_pe_header(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == b'M' && bytes[1] == b'Z'
}

/// Verifica que `path` apunte exclusivamente al binario temporal propio en `%TEMP%` antes de borrarlo.
pub fn is_safe_temp_cleanup_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !name.starts_with(TEMP_UPDATE_PREFIX) || !name.ends_with(".exe") {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let temp_dir = std::env::temp_dir();
    let p_canon = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let t_canon = fs::canonicalize(&temp_dir).unwrap_or(temp_dir);
    p_canon == t_canon
}

/// Controlador asíncrono para consultar, descargar y aplicar actualizaciones desde GitHub.
pub struct UpdaterHandle {
    state: Arc<Mutex<UpdateState>>,
    exit_requested: Arc<AtomicBool>,
    rt_handle: tokio::runtime::Handle,
    ctx: egui::Context,
}

impl UpdaterHandle {
    /// Crea el controlador y lanza una consulta silenciosa inicial en segundo plano.
    pub fn new(rt_handle: tokio::runtime::Handle, ctx: egui::Context) -> Self {
        let handle = Self {
            state: Arc::new(Mutex::new(UpdateState::Idle)),
            exit_requested: Arc::new(AtomicBool::new(false)),
            rt_handle,
            ctx,
        };
        handle.check_now(true);
        handle
    }

    /// Devuelve una copia instantánea del estado actual del actualizador.
    pub fn state(&self) -> UpdateState {
        self.state
            .lock()
            .map(|g| g.clone())
            .unwrap_or(UpdateState::Idle)
    }

    /// Devuelve `true` una vez cuando el actualizador lanzó el instalador y requiere cerrar la app actual.
    pub fn take_exit_requested(&self) -> bool {
        self.exit_requested.swap(false, Ordering::SeqCst)
    }

    /// Consulta la última Release publicada en GitHub (sin mostrar errores si `silent_errors = true`).
    pub fn check_now(&self, silent_errors: bool) {
        {
            let Ok(mut guard) = self.state.lock() else {
                return;
            };
            if matches!(
                *guard,
                UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Applying { .. }
            ) {
                return;
            }
            *guard = UpdateState::Checking;
        }
        self.ctx.request_repaint();

        let state = Arc::clone(&self.state);
        let ctx = self.ctx.clone();
        self.rt_handle.spawn(async move {
            let next_state = match fetch_latest_release().await {
                Ok(Some(info)) => UpdateState::Available(info),
                Ok(None) => UpdateState::UpToDate,
                Err(err) => {
                    warn!("No se pudo consultar actualizaciones: {err:#}");
                    if silent_errors {
                        UpdateState::Idle
                    } else {
                        UpdateState::Error(format!("{err}"))
                    }
                }
            };
            if let Ok(mut guard) = state.lock() {
                *guard = next_state;
            }
            ctx.request_repaint();
        });
    }

    /// Descarga la Release indicada a `%TEMP%`, valida su PE y lanza `--apply-update` antes de cerrar.
    pub fn download_and_apply(&self, info: ReleaseInfo, reinstall_service: bool) {
        {
            let Ok(mut guard) = self.state.lock() else {
                return;
            };
            if matches!(
                *guard,
                UpdateState::Downloading { .. } | UpdateState::Applying { .. }
            ) {
                return;
            }
            *guard = UpdateState::Downloading {
                version: info.version.clone(),
                percent: 0,
            };
        }
        self.ctx.request_repaint();

        let state = Arc::clone(&self.state);
        let exit_requested = Arc::clone(&self.exit_requested);
        let ctx = self.ctx.clone();
        self.rt_handle.spawn(async move {
            match download_and_spawn_updater(&info, reinstall_service, &state, &ctx).await {
                Ok(()) => {
                    exit_requested.store(true, Ordering::SeqCst);
                    ctx.request_repaint();
                }
                Err(err) => {
                    warn!("Fallo al aplicar actualización v{}: {err:#}", info.version);
                    if let Ok(mut guard) = state.lock() {
                        *guard = UpdateState::Error(format!("{err}"));
                    }
                    ctx.request_repaint();
                }
            }
        });
    }
}

async fn fetch_latest_release() -> Result<Option<ReleaseInfo>> {
    let client = build_http_client(Duration::from_secs(15))?;
    let resp = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("sin conexión con GitHub Releases")?;

    if !resp.status().is_success() {
        return Err(anyhow!("GitHub respondió HTTP {}", resp.status().as_u16()));
    }

    let raw_bytes = resp
        .bytes()
        .await
        .context("error leyendo respuesta de GitHub")?;
    let rel: GithubRelease =
        serde_json::from_slice(&raw_bytes).context("respuesta JSON inválida de GitHub")?;
    if rel.draft || rel.prerelease {
        return Ok(None);
    }
    if !is_newer_version(env!("CARGO_PKG_VERSION"), &rel.tag_name) {
        return Ok(None);
    }

    let asset = rel
        .assets
        .into_iter()
        .find(|a| a.name.eq_ignore_ascii_case(RELEASE_ASSET_NAME))
        .ok_or_else(|| anyhow!("la Release {} no incluye {}", rel.tag_name, RELEASE_ASSET_NAME))?;

    let clean_ver = rel.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    Ok(Some(ReleaseInfo {
        version: clean_ver,
        tag: rel.tag_name,
        download_url: asset.browser_download_url,
        asset_size: asset.size,
    }))
}

async fn download_and_spawn_updater(
    info: &ReleaseInfo,
    reinstall_service: bool,
    state: &Arc<Mutex<UpdateState>>,
    ctx: &egui::Context,
) -> Result<()> {
    let client = build_http_client(Duration::from_secs(120))?;
    let resp = client
        .get(&info.download_url)
        .send()
        .await
        .context("error descargando actualización")?;

    if !resp.status().is_success() {
        return Err(anyhow!(
            "descarga rechazada (HTTP {})",
            resp.status().as_u16()
        ));
    }

    let total_bytes = resp.content_length().unwrap_or(info.asset_size);
    let temp_exe = std::env::temp_dir().join(format!("{TEMP_UPDATE_PREFIX}{}.exe", info.version));
    if temp_exe.exists() {
        let _ = fs::remove_file(&temp_exe);
    }

    let download_res: Result<u64> = async {
        let mut file = File::create(&temp_exe).context("no se pudo crear temporal en %TEMP%")?;
        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut last_pct: u8 = 0;

        while let Some(chunk) = stream.next().await {
            let data = chunk.context("descarga interrumpida")?;
            file.write_all(&data).context("error escribiendo temporal")?;
            downloaded += data.len() as u64;

            if let Some(raw_pct) = downloaded.saturating_mul(100).checked_div(total_bytes) {
                let pct = raw_pct.min(99) as u8;
                if pct != last_pct {
                    last_pct = pct;
                    if let Ok(mut guard) = state.lock() {
                        *guard = UpdateState::Downloading {
                            version: info.version.clone(),
                            percent: pct,
                        };
                    }
                    ctx.request_repaint();
                }
            }
        }
        file.flush().context("error sincronizando temporal")?;
        Ok(downloaded)
    }
    .await;

    let downloaded = match download_res {
        Ok(n) => n,
        Err(err) => {
            let _ = fs::remove_file(&temp_exe);
            return Err(err);
        }
    };

    if downloaded < MIN_VALID_EXE_BYTES || (info.asset_size > 0 && downloaded != info.asset_size) {
        let _ = fs::remove_file(&temp_exe);
        return Err(anyhow!(
            "tamaño inválido tras descargar ({downloaded} bytes)"
        ));
    }

    let mut header = [0u8; 2];
    let valid_pe = File::open(&temp_exe)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_ok()
        && is_valid_pe_header(&header);
    if !valid_pe {
        let _ = fs::remove_file(&temp_exe);
        return Err(anyhow!("el binario descargado no es un ejecutable PE válido"));
    }

    if let Ok(mut guard) = state.lock() {
        *guard = UpdateState::Applying {
            version: info.version.clone(),
        };
    }
    ctx.request_repaint();

    let target_exe = std::env::current_exe().context("no se pudo resolver el ejecutable actual")?;
    let current_pid = std::process::id();
    let need_elevation = (reinstall_service || !can_write_in_dir(target_exe.parent()))
        && !randydesk_host::input::is_process_elevated();

    spawn_apply_process(&temp_exe, &target_exe, current_pid, reinstall_service, need_elevation)?;
    info!(
        "Actualizador v{} lanzado hacia {}",
        info.version,
        target_exe.display()
    );
    Ok(())
}

fn build_http_client(timeout: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!("RandyDesk/{}", env!("CARGO_PKG_VERSION")))
        .timeout(timeout)
        .build()
        .context("no se pudo inicializar cliente HTTP TLS")
}

fn can_write_in_dir(dir: Option<&Path>) -> bool {
    let Some(dir) = dir else {
        return false;
    };
    let probe = dir.join(format!(".rd_write_test_{}", std::process::id()));
    match File::create(&probe) {
        Ok(f) => {
            drop(f);
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn quote_win_arg(arg: &str) -> String {
    format!("\"{}\"", arg.replace('"', "\\\""))
}

fn spawn_apply_process(
    temp_exe: &Path,
    target_exe: &Path,
    wait_pid: u32,
    reinstall_service: bool,
    elevate: bool,
) -> Result<()> {
    let target_str = target_exe.to_string_lossy().to_string();
    let pid_str = wait_pid.to_string();

    if elevate {
        let mut params = format!(
            "--apply-update --target {} --wait-pid {}",
            quote_win_arg(&target_str),
            pid_str
        );
        if reinstall_service {
            params.push_str(" --reinstall-service");
        }
        let exe_w = HSTRING::from(temp_exe.as_os_str());
        let params_w = HSTRING::from(params);
        let ret = unsafe {
            ShellExecuteW(
                HWND::default(),
                w!("runas"),
                PCWSTR::from_raw(exe_w.as_ptr()),
                PCWSTR::from_raw(params_w.as_ptr()),
                PCWSTR::null(),
                SW_HIDE,
            )
        };
        if (ret.0 as isize) <= 32 {
            return Err(anyhow!(
                "elevación UAC cancelada o fallida (código {})",
                ret.0 as isize
            ));
        }
        return Ok(());
    }

    let mut cmd = std::process::Command::new(temp_exe);
    cmd.args(["--apply-update", "--target", &target_str, "--wait-pid", &pid_str]);
    if reinstall_service {
        cmd.arg("--reinstall-service");
    }
    cmd.spawn()
        .context("no se pudo iniciar el proceso de reemplazo")?;
    Ok(())
}

/// Espera al cierre del proceso anterior, reemplaza el ejecutable, actualiza `SYSTEM` si aplica y relanza.
pub fn run_apply_update(target: &Path, wait_pid: u32, reinstall_service: bool) -> Result<()> {
    if wait_pid != 0 {
        unsafe {
            if let Ok(proc_h) = OpenProcess(PROCESS_SYNCHRONIZE, false, wait_pid) {
                let _ = WaitForSingleObject(proc_h, 12_000);
                let _ = CloseHandle(proc_h);
            }
        }
    }

    let new_exe = std::env::current_exe().context("no se pudo leer ruta del actualizador")?;
    let mut backup_path = PathBuf::from(target.as_os_str());
    let mut os = backup_path.into_os_string();
    os.push(".old");
    backup_path = PathBuf::from(os);

    let mut replaced = false;
    let mut last_err = None;
    for _ in 0..30 {
        if backup_path.exists() {
            let _ = fs::remove_file(&backup_path);
        }
        if target.exists() {
            if let Err(err) = fs::rename(target, &backup_path) {
                last_err = Some(err);
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        }
        match fs::copy(&new_exe, target) {
            Ok(_) => {
                replaced = true;
                break;
            }
            Err(err) => {
                last_err = Some(err);
                if backup_path.exists() && !target.exists() {
                    let _ = fs::rename(&backup_path, target);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }

    if !replaced {
        return Err(anyhow!(
            "no se pudo reemplazar {}: {:?}",
            target.display(),
            last_err
        ));
    }

    if backup_path.exists() {
        let _ = fs::remove_file(&backup_path);
    }

    if reinstall_service {
        if let Err(err) = randydesk_host::win_service::install_service_now(None, None) {
            warn!("No se pudo actualizar RandyDeskService tras actualizar: {err:#}");
        }
    }

    let _ = std::process::Command::new(target)
        .arg("--cleanup-file")
        .arg(&new_exe)
        .spawn();
    Ok(())
}

/// Elimina en segundo plano el binario temporal descargado en `%TEMP%` tras reiniciar ya actualizado.
pub fn run_cleanup_temp_file(path: PathBuf) {
    if !is_safe_temp_cleanup_path(&path) {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..15 {
            if !path.exists() || fs::remove_file(&path).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_compares_semver_tags() {
        assert_eq!(parse_semver("v0.5.3"), Some((0, 5, 3)));
        assert_eq!(parse_semver("V1.2.10-rc1"), Some((1, 2, 10)));
        assert_eq!(parse_semver("0.5.2+build9"), Some((0, 5, 2)));
        assert_eq!(parse_semver("0.5"), None);
        assert_eq!(parse_semver("abc"), None);

        assert!(is_newer_version("0.5.2", "v0.5.3"));
        assert!(is_newer_version("0.5.3", "0.6.0"));
        assert!(!is_newer_version("0.5.3", "v0.5.3"));
        assert!(!is_newer_version("0.5.4", "v0.5.3"));
        assert!(!is_newer_version("0.5.3", "invalid"));
    }

    #[test]
    fn validates_pe_magic_and_safe_cleanup_path() {
        assert!(is_valid_pe_header(&[b'M', b'Z', 0x90]));
        assert!(!is_valid_pe_header(b"M"));
        assert!(!is_valid_pe_header(b"<!DOCTYPE html>"));

        let safe = std::env::temp_dir().join("RandyDesk_update_0.5.4.exe");
        assert!(is_safe_temp_cleanup_path(&safe));

        let wrong_name = std::env::temp_dir().join("RandyDesk.exe");
        assert!(!is_safe_temp_cleanup_path(&wrong_name));

        let wrong_dir = PathBuf::from(r"C:\Windows\System32\RandyDesk_update_0.5.4.exe");
        assert!(!is_safe_temp_cleanup_path(&wrong_dir));
    }
}
