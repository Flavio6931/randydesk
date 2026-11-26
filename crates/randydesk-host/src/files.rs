//! Exploración segura de archivos del host y transferencias por bloques cifrados (`128 KiB`).

use anyhow::{anyhow, Context, Result};
use directories::UserDirs;
use randydesk_proto::{FileEntry, HostMsg, FILE_CHUNK_SIZE};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

/// Ruta especial que indica listar las unidades lógicas de Windows (`C:\`, `D:\`, etc.).
pub const DRIVES_SENTINEL: &str = "__DRIVES__";

/// Devuelve el directorio inicial seguro para abrir el navegador remoto de archivos.
pub fn default_browse_dir() -> PathBuf {
    if let Some(dirs) = UserDirs::new() {
        if let Some(desk) = dirs.desktop_dir() {
            if desk.exists() {
                return desk.to_path_buf();
            }
        }
        if dirs.home_dir().exists() {
            return dirs.home_dir().to_path_buf();
        }
    }
    let public_desk = PathBuf::from(r"C:\Users\Public\Desktop");
    if public_desk.exists() {
        return public_desk;
    }
    PathBuf::from(r"C:\")
}

/// Valida que un nombre de archivo o carpeta sea un componente simple sin path traversal.
pub fn sanitize_file_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(anyhow!("Nombre de archivo o carpeta inválido"));
    }
    if trimmed.chars().any(|c| {
        matches!(
            c,
            '/' | '\\' | ':' | '\0' | '*' | '?' | '"' | '<' | '>' | '|'
        )
    }) {
        return Err(anyhow!("El nombre contiene caracteres no permitidos"));
    }
    Ok(trimmed.to_string())
}

/// Calcula una ruta destino sin sobrescribir archivos existentes (`archivo (1).ext`).
pub fn unique_dest_path(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(file_name);
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("archivo");
    let ext = p.extension().and_then(|e| e.to_str());
    for n in 1..=9999u32 {
        let next_name = match ext {
            Some(e) if !e.is_empty() => format!("{stem} ({n}).{e}"),
            _ => format!("{stem} ({n})"),
        };
        let next = dir.join(next_name);
        if !next.exists() {
            return next;
        }
    }
    candidate
}

/// Enumera un directorio o lista de unidades del equipo host y construye `HostMsg::FileListResp`.
pub fn list_directory(raw_path: &str) -> HostMsg {
    let trimmed = raw_path.trim();
    if trimmed == DRIVES_SENTINEL {
        let mut entries = Vec::new();
        for letter in b'A'..=b'Z' {
            let root = format!("{}:\\", letter as char);
            if Path::new(&root).exists() {
                entries.push(FileEntry {
                    name: root,
                    is_dir: true,
                    size: 0,
                    modified_unix: 0,
                });
            }
        }
        return HostMsg::FileListResp {
            path: DRIVES_SENTINEL.to_string(),
            parent: None,
            entries,
            error: None,
        };
    }

    let dir = if trimmed.is_empty() {
        default_browse_dir()
    } else {
        PathBuf::from(trimmed)
    };
    let display_path = dir.to_string_lossy().to_string();
    let parent = match dir.parent() {
        Some(p) if !p.as_os_str().is_empty() => Some(p.to_string_lossy().to_string()),
        _ => Some(DRIVES_SENTINEL.to_string()),
    };

    let read_dir = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(err) => {
            return HostMsg::FileListResp {
                path: display_path,
                parent,
                entries: Vec::new(),
                error: Some(format!("No se pudo leer la carpeta: {err}")),
            };
        }
    };

    let mut entries = Vec::new();
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata();
        let (is_dir, size, modified_unix) = match meta {
            Ok(m) => {
                let ts = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                (m.is_dir(), if m.is_dir() { 0 } else { m.len() }, ts)
            }
            Err(_) => (false, 0, 0),
        };
        entries.push(FileEntry {
            name,
            is_dir,
            size,
            modified_unix,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    HostMsg::FileListResp {
        path: display_path,
        parent,
        entries,
        error: None,
    }
}

/// Crea una subcarpeta dentro de `parent_dir` y devuelve el listado actualizado.
pub fn create_directory(parent_dir: &str, name: &str) -> HostMsg {
    let base = if parent_dir.trim().is_empty() {
        default_browse_dir()
    } else {
        PathBuf::from(parent_dir.trim())
    };
    let clean = match sanitize_file_name(name) {
        Ok(c) => c,
        Err(err) => {
            return HostMsg::FileListResp {
                path: base.to_string_lossy().to_string(),
                parent: base.parent().map(|p| p.to_string_lossy().to_string()),
                entries: Vec::new(),
                error: Some(format!("{err}")),
            };
        }
    };
    let target = base.join(clean);
    if let Err(err) = fs::create_dir_all(&target) {
        return HostMsg::FileListResp {
            path: base.to_string_lossy().to_string(),
            parent: base.parent().map(|p| p.to_string_lossy().to_string()),
            entries: Vec::new(),
            error: Some(format!("No se pudo crear la carpeta: {err}")),
        };
    }
    list_directory(&base.to_string_lossy())
}

struct ActiveUpload {
    temp_path: PathBuf,
    final_dir: PathBuf,
    file_name: String,
    file: File,
    written: u64,
    total_size: u64,
    completed: bool,
}

impl Drop for ActiveUpload {
    fn drop(&mut self) {
        if !self.completed {
            let _ = fs::remove_file(&self.temp_path);
        }
    }
}

/// Gestiona subidas entrantes (`.rdpart`) y descargas salientes en bloques de `128 KiB`.
pub struct HostFileManager {
    active_uploads: HashMap<u64, ActiveUpload>,
    cancelled_downloads: Arc<Mutex<HashSet<u64>>>,
}

impl Default for HostFileManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HostFileManager {
    /// Crea un gestor vacío de transferencias para una sesión activa.
    pub fn new() -> Self {
        Self {
            active_uploads: HashMap::new(),
            cancelled_downloads: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Inicia una recepción de archivo escribiendo en un archivo temporal `.rdpart`.
    pub fn start_upload(
        &mut self,
        transfer_id: u64,
        remote_dir: &str,
        file_name: &str,
        total_size: u64,
    ) -> std::result::Result<(), String> {
        let clean_name = sanitize_file_name(file_name).map_err(|e| e.to_string())?;
        let dir = if remote_dir.trim().is_empty() || remote_dir.trim() == DRIVES_SENTINEL {
            default_browse_dir()
        } else {
            PathBuf::from(remote_dir.trim())
        };
        if !dir.is_dir() {
            return Err("La carpeta de destino remota no existe".to_string());
        }

        let temp_path = dir.join(format!(".randydesk-{transfer_id}.rdpart"));
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)
            .with_context(|| format!("no se pudo crear {}", temp_path.display()))
            .map_err(|e| e.to_string())?;

        self.active_uploads.insert(
            transfer_id,
            ActiveUpload {
                temp_path,
                final_dir: dir,
                file_name: clean_name,
                file,
                written: 0,
                total_size,
                completed: false,
            },
        );
        Ok(())
    }

    /// Escribe un bloque secuencial del archivo en subida verificando su desplazamiento.
    pub fn write_upload_chunk(
        &mut self,
        transfer_id: u64,
        offset: u64,
        data: &[u8],
    ) -> std::result::Result<(), String> {
        let Some(up) = self.active_uploads.get_mut(&transfer_id) else {
            return Err("Transferencia no iniciada o cancelada".to_string());
        };
        if offset != up.written {
            return Err(format!(
                "Desplazamiento inválido (esperado {}, recibido {})",
                up.written, offset
            ));
        }
        up.file
            .write_all(data)
            .map_err(|e| format!("Error escribiendo archivo: {e}"))?;
        up.written = up.written.saturating_add(data.len() as u64);
        Ok(())
    }

    /// Finaliza y renombra el archivo `.rdpart` evitando sobrescribir archivos existentes.
    pub fn finish_upload(&mut self, transfer_id: u64) -> std::result::Result<String, String> {
        let mut up = self
            .active_uploads
            .remove(&transfer_id)
            .ok_or_else(|| "Transferencia no encontrada".to_string())?;
        if up.written != up.total_size {
            return Err(format!(
                "Tamaño incompleto ({} de {} bytes)",
                up.written, up.total_size
            ));
        }
        let _ = up.file.flush();
        let dest = unique_dest_path(&up.final_dir, &up.file_name);
        fs::rename(&up.temp_path, &dest)
            .map_err(|e| format!("Error al guardar archivo final: {e}"))?;
        up.completed = true;
        Ok(dest.to_string_lossy().to_string())
    }

    /// Cancela una subida o descarga activa y limpia temporales `.rdpart`.
    pub fn cancel_transfer(&mut self, transfer_id: u64) {
        self.active_uploads.remove(&transfer_id);
        if let Ok(mut set) = self.cancelled_downloads.lock() {
            set.insert(transfer_id);
        }
    }

    /// Procesa un mensaje de control de archivos; devuelve `None` si fue atendido o `Some(msg)` si es de otro tipo.
    pub fn handle_client_msg(
        &mut self,
        msg: randydesk_proto::ClientMsg,
        out_tx: &mpsc::Sender<HostMsg>,
    ) -> Option<randydesk_proto::ClientMsg> {
        use randydesk_proto::ClientMsg;
        match msg {
            ClientMsg::FileListReq { path } => {
                let _ = out_tx.try_send(list_directory(&path));
                None
            }
            ClientMsg::FileMkdir { parent_dir, name } => {
                let _ = out_tx.try_send(create_directory(&parent_dir, &name));
                None
            }
            ClientMsg::FileUploadStart {
                transfer_id,
                remote_dir,
                file_name,
                total_size,
            } => {
                if let Err(message) = self.start_upload(transfer_id, &remote_dir, &file_name, total_size) {
                    let _ = out_tx.try_send(HostMsg::FileTransferError {
                        transfer_id,
                        message,
                    });
                }
                None
            }
            ClientMsg::FileUploadChunk {
                transfer_id,
                offset,
                data,
            } => {
                if let Err(message) = self.write_upload_chunk(transfer_id, offset, &data) {
                    self.active_uploads.remove(&transfer_id);
                    let _ = out_tx.try_send(HostMsg::FileTransferError {
                        transfer_id,
                        message,
                    });
                }
                None
            }
            ClientMsg::FileUploadFinish { transfer_id } => {
                let resp = match self.finish_upload(transfer_id) {
                    Ok(saved_path) => HostMsg::FileTransferDone {
                        transfer_id,
                        saved_path,
                    },
                    Err(message) => HostMsg::FileTransferError {
                        transfer_id,
                        message,
                    },
                };
                let _ = out_tx.try_send(resp);
                None
            }
            ClientMsg::FileDownloadReq {
                transfer_id,
                remote_path,
            } => {
                self.spawn_download(transfer_id, remote_path, out_tx.clone());
                None
            }
            ClientMsg::FileTransferCancel { transfer_id } => {
                self.cancel_transfer(transfer_id);
                None
            }
            other => Some(other),
        }
    }

    /// Lanza en segundo plano la lectura asíncrona y envío por bloques de un archivo remoto.
    pub fn spawn_download(
        &self,
        transfer_id: u64,
        remote_path: String,
        out_tx: mpsc::Sender<HostMsg>,
    ) {
        let cancelled = Arc::clone(&self.cancelled_downloads);
        tokio::spawn(async move {
            let path = PathBuf::from(&remote_path);
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) if !n.is_empty() => n.to_string(),
                _ => {
                    let _ = out_tx
                        .send(HostMsg::FileTransferError {
                            transfer_id,
                            message: "Ruta de archivo inválida".to_string(),
                        })
                        .await;
                    return;
                }
            };
            let mut file = match tokio::fs::File::open(&path).await {
                Ok(f) => f,
                Err(err) => {
                    let _ = out_tx
                        .send(HostMsg::FileTransferError {
                            transfer_id,
                            message: format!("No se pudo abrir archivo: {err}"),
                        })
                        .await;
                    return;
                }
            };
            let total_size = match file.metadata().await {
                Ok(m) if m.is_file() => m.len(),
                Ok(_) => {
                    let _ = out_tx
                        .send(HostMsg::FileTransferError {
                            transfer_id,
                            message: "Solo se pueden descargar archivos".to_string(),
                        })
                        .await;
                    return;
                }
                Err(err) => {
                    let _ = out_tx
                        .send(HostMsg::FileTransferError {
                            transfer_id,
                            message: format!("Error leyendo metadatos: {err}"),
                        })
                        .await;
                    return;
                }
            };

            if out_tx
                .send(HostMsg::FileDownloadMeta {
                    transfer_id,
                    file_name,
                    total_size,
                })
                .await
                .is_err()
            {
                return;
            }

            let mut offset = 0u64;
            let mut buf = vec![0u8; FILE_CHUNK_SIZE];
            loop {
                if cancelled
                    .lock()
                    .map(|mut s| s.remove(&transfer_id))
                    .unwrap_or(false)
                {
                    let _ = out_tx
                        .send(HostMsg::FileTransferError {
                            transfer_id,
                            message: "Transferencia cancelada".to_string(),
                        })
                        .await;
                    return;
                }

                match file.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        if out_tx
                            .send(HostMsg::FileDownloadChunk {
                                transfer_id,
                                offset,
                                data: chunk,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                        offset = offset.saturating_add(n as u64);
                    }
                    Err(err) => {
                        let _ = out_tx
                            .send(HostMsg::FileTransferError {
                                transfer_id,
                                message: format!("Error leyendo archivo: {err}"),
                            })
                            .await;
                        return;
                    }
                }
            }

            let _ = out_tx
                .send(HostMsg::FileTransferDone {
                    transfer_id,
                    saved_path: remote_path,
                })
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_blocks_traversal_and_reserved_chars() {
        assert_eq!(sanitize_file_name("informe.pdf").unwrap(), "informe.pdf");
        assert!(sanitize_file_name("").is_err());
        assert!(sanitize_file_name("..").is_err());
        assert!(sanitize_file_name("../secreto.txt").is_err());
        assert!(sanitize_file_name(r"a\b").is_err());
        assert!(sanitize_file_name("C:test").is_err());
    }

    #[test]
    fn upload_lifecycle_renames_safely_and_cancel_cleans_rdpart() {
        let base = std::env::temp_dir().join(format!("randydesk-files-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let dir_str = base.to_string_lossy().to_string();

        let mut mgr = HostFileManager::new();
        mgr.start_upload(1, &dir_str, "nota.txt", 10).unwrap();
        mgr.write_upload_chunk(1, 0, b"hola ").unwrap();
        mgr.write_upload_chunk(1, 5, b"mundo").unwrap();
        let saved1 = mgr.finish_upload(1).unwrap();
        assert_eq!(fs::read_to_string(&saved1).unwrap(), "hola mundo");

        // Segunda subida con mismo nombre no sobrescribe la primera
        mgr.start_upload(2, &dir_str, "nota.txt", 4).unwrap();
        mgr.write_upload_chunk(2, 0, b"1234").unwrap();
        let saved2 = mgr.finish_upload(2).unwrap();
        assert_ne!(saved1, saved2);
        assert!(saved2.contains("nota (1).txt"));

        // Cancelación limpia temporal .rdpart
        mgr.start_upload(3, &dir_str, "abortado.bin", 100).unwrap();
        mgr.write_upload_chunk(3, 0, b"abc").unwrap();
        mgr.cancel_transfer(3);
        assert!(!base.join(".randydesk-3.rdpart").exists());

        let HostMsg::FileListResp { entries, error, .. } = list_directory(&dir_str) else {
            panic!("se esperaba FileListResp");
        };
        assert!(error.is_none());
        assert_eq!(entries.len(), 2);

        let _ = fs::remove_dir_all(&base);
    }
}
