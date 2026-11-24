//! Persistencia atómica de equipos (`hosts.json`) y ajustes de servidor (`settings.json`).

use anyhow::{Context, Result};
use directories::ProjectDirs;
use randydesk_proto::{
    format_host_id, normalize_relay_ws_url, parse_numeric_id, CodecKind, DEFAULT_PORT,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Preferencias globales de red del usuario (URL del servidor Relay).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub relay_url: String,
}

impl AppSettings {
    /// Devuelve la URL del Relay normalizada (`ws://` o `wss://`) si está configurada.
    pub fn normalized_relay_url(&self) -> Option<String> {
        normalize_relay_ws_url(&self.relay_url)
    }
}

/// Equipo remoto guardado en la libreta o historial reciente (sin contraseñas en disco).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedHost {
    pub id: String,
    pub name: String,
    pub address: String,
    pub fingerprint_sha256: Option<String>,
    pub codec: CodecKind,
    pub quality: u8,
    pub fps: u8,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub last_connected: u64,
    #[serde(default)]
    pub color_idx: u8,
}

impl SavedHost {
    /// Crea un nuevo registro con valores por defecto seguros.
    pub fn new(name: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            id: format!("pc-{}", now_millis()),
            name: name.into(),
            address: address.into(),
            fingerprint_sha256: None,
            codec: CodecKind::Jpeg,
            quality: 75,
            fps: 30,
            favorite: false,
            last_connected: 0,
            color_idx: 0,
        }
    }
}

/// Normaliza un destino: conserva IDs de 9 dígitos limpios o añade `:21118` a IPs sin puerto.
pub fn normalize_address(raw: &str) -> String {
    if let Some(id) = parse_numeric_id(raw) {
        return id;
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return format!("127.0.0.1:{DEFAULT_PORT}");
    }
    if trimmed.starts_with('[') {
        if trimmed.contains("]:") {
            trimmed.to_string()
        } else {
            format!("{trimmed}:{DEFAULT_PORT}")
        }
    } else if trimmed.contains(':') {
        trimmed.to_string()
    } else {
        format!("{trimmed}:{DEFAULT_PORT}")
    }
}

/// Devuelve el destino legible: `482 910 331` para IDs o IP sin `:21118` cuando usa puerto por defecto.
pub fn display_address(addr: &str) -> String {
    if let Some(id) = parse_numeric_id(addr) {
        return format_host_id(&id);
    }
    let default_suffix = format!(":{DEFAULT_PORT}");
    if let Some(stripped) = addr.strip_suffix(&default_suffix) {
        stripped.to_string()
    } else {
        addr.to_string()
    }
}

/// Registra o actualiza un equipo en el historial reciente tras conectar exitosamente.
pub fn upsert_recent(
    hosts: &mut Vec<SavedHost>,
    address: &str,
    host_name: &str,
    fingerprint: Option<&str>,
    codec: CodecKind,
    quality: u8,
    fps: u8,
) {
    let normalized = normalize_address(address);
    let now = now_secs();

    if let Some(existing) = hosts.iter_mut().find(|h| h.address == normalized) {
        existing.last_connected = now;
        existing.codec = codec;
        existing.quality = quality;
        existing.fps = fps;
        if existing.fingerprint_sha256.is_none() {
            existing.fingerprint_sha256 = fingerprint.map(ToOwned::to_owned);
        }
        if existing.name.trim().is_empty() && !host_name.trim().is_empty() {
            existing.name = host_name.trim().to_string();
        }
        return;
    }

    let label = if host_name.trim().is_empty() {
        display_address(&normalized)
    } else {
        host_name.trim().to_string()
    };
    let color_idx = (hosts.len() % 7) as u8;
    let mut host = SavedHost::new(label, normalized);
    host.fingerprint_sha256 = fingerprint.map(ToOwned::to_owned);
    host.codec = codec;
    host.quality = quality;
    host.fps = fps;
    host.last_connected = now;
    host.color_idx = color_idx;
    hosts.push(host);
}

/// Carga la configuración global (`settings.json`) o lee `RANDYDESK_RELAY_URL` si está vacía.
pub fn load_settings() -> AppSettings {
    let mut settings = settings_file_path()
        .ok()
        .filter(|p| p.exists())
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|json| serde_json::from_str::<AppSettings>(&json).ok())
        .unwrap_or_default();

    if settings.relay_url.trim().is_empty() {
        if let Ok(env_url) = std::env::var("RANDYDESK_RELAY_URL") {
            settings.relay_url = env_url;
        }
    }
    settings
}

/// Guarda la configuración global de forma atómica.
pub fn save_settings(settings: &AppSettings) -> Result<()> {
    let path = settings_file_path()?;
    write_json_atomic(&path, settings)
}

/// Carga la lista de equipos guardados; devuelve lista vacía si aún no existe el archivo.
pub fn load_hosts() -> Vec<SavedHost> {
    let Ok(path) = hosts_file_path() else {
        return Vec::new();
    };
    if !path.exists() {
        return Vec::new();
    }
    match fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Guarda la lista de equipos de forma atómica escribiendo a un temporal y renombrando.
pub fn save_hosts(hosts: &[SavedHost]) -> Result<()> {
    let path = hosts_file_path()?;
    write_json_atomic(&path, hosts)
}

fn write_json_atomic<T: Serialize + ?Sized>(path: &PathBuf, data: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("no se pudo crear {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(data)?;
    fs::write(&tmp, json).with_context(|| format!("error escribiendo {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("error renombrando {}", path.display()))?;
    Ok(())
}

fn hosts_file_path() -> Result<PathBuf> {
    let proj = ProjectDirs::from("com", "RandyDesk", "RandyDesk")
        .context("no se pudo resolver directorio de configuración")?;
    Ok(proj.config_dir().join("hosts.json"))
}

fn settings_file_path() -> Result<PathBuf> {
    let proj = ProjectDirs::from("com", "RandyDesk", "RandyDesk")
        .context("no se pudo resolver directorio de configuración")?;
    Ok(proj.config_dir().join("settings.json"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_v010_hosts_json_with_defaults() {
        let legacy_json = r#"[{
            "id": "pc-1",
            "name": "PC Compañero",
            "address": "100.80.12.4:21118",
            "fingerprint_sha256": null,
            "codec": "Jpeg",
            "quality": 75,
            "fps": 30
        }]"#;
        let parsed: Vec<SavedHost> =
            serde_json::from_str(legacy_json).expect("deserialización retrocompatible");
        assert_eq!(parsed.len(), 1);
        assert!(!parsed[0].favorite);
        assert_eq!(parsed[0].last_connected, 0);
        assert_eq!(parsed[0].color_idx, 0);
        assert_eq!(display_address(&parsed[0].address), "100.80.12.4");
    }

    #[test]
    fn normalizes_and_displays_numeric_ids_and_ips() {
        assert_eq!(normalize_address("482 910 331"), "482910331");
        assert_eq!(display_address("482910331"), "482 910 331");
        assert_eq!(normalize_address("192.168.1.50"), "192.168.1.50:21118");
        assert_eq!(display_address("192.168.1.50:21118"), "192.168.1.50");
    }

    #[test]
    fn upsert_recent_adds_and_updates_without_duplicates() {
        let mut hosts = Vec::new();
        upsert_recent(
            &mut hosts,
            "192.168.1.50",
            "Equipo-A",
            Some("AA:BB"),
            CodecKind::Jpeg,
            75,
            30,
        );
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].address, "192.168.1.50:21118");
        assert!(hosts[0].last_connected > 0);
    }
}
