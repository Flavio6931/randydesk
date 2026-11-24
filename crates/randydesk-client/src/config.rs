//! Persistencia atómica de la libreta de equipos guardados en `%APPDATA%\RandyDesk\hosts.json`.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use randydesk_proto::{CodecKind, DEFAULT_PORT};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Equipo remoto guardado en la libreta del usuario (sin almacenar contraseñas).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedHost {
    pub id: String,
    pub name: String,
    pub address: String,
    pub fingerprint_sha256: Option<String>,
    pub codec: CodecKind,
    pub quality: u8,
    pub fps: u8,
}

impl SavedHost {
    /// Crea un nuevo registro con valores por defecto seguros.
    pub fn new(name: impl Into<String>, address: impl Into<String>) -> Self {
        let now_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Self {
            id: format!("pc-{now_id}"),
            name: name.into(),
            address: address.into(),
            fingerprint_sha256: None,
            codec: CodecKind::Jpeg,
            quality: 75,
            fps: 30,
        }
    }
}

/// Normaliza una dirección ingresada por el usuario añadiendo `:21118` si falta el puerto.
pub fn normalize_address(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return format!("127.0.0.1:{DEFAULT_PORT}");
    }
    // Soporta IPv6 con corchetes `[::1]:21118` o IPv4/host `100.x.y.z`
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
