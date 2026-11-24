//! Persistencia atómica y retrocompatible de equipos guardados en `%APPDATA%\RandyDesk\hosts.json`.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use randydesk_proto::{CodecKind, DEFAULT_PORT};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

/// Normaliza una dirección ingresada por el usuario añadiendo `:21118` si falta el puerto.
pub fn normalize_address(raw: &str) -> String {
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

/// Devuelve la dirección legible ocultando `:21118` cuando usa el puerto por defecto.
pub fn display_address(addr: &str) -> String {
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("no se pudo crear {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(hosts)?;
    fs::write(&tmp, json).with_context(|| format!("error escribiendo {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("error renombrando {}", path.display()))?;
    Ok(())
}

fn hosts_file_path() -> Result<PathBuf> {
    let proj = ProjectDirs::from("com", "RandyDesk", "RandyDesk")
        .context("no se pudo resolver directorio de configuración")?;
    Ok(proj.config_dir().join("hosts.json"))
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

        upsert_recent(
            &mut hosts,
            "192.168.1.50:21118",
            "Equipo-A",
            None,
            CodecKind::Lz4Rgba,
            90,
            60,
        );
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].codec, CodecKind::Lz4Rgba);
        assert_eq!(hosts[0].fingerprint_sha256.as_deref(), Some("AA:BB"));
    }
}
