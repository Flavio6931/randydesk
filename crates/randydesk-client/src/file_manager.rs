//! Gestor visual dual de archivos (PC local <-> PC remota) y seguimiento de transferencias cifradas.

use crate::net::{NetCommand, SharedSessionState};
use crate::theme::{
    styled_text_input, ACCENT_BLUE, ACCENT_DANGER_TEXT, BG_CARD, BG_SIDEBAR,
    BG_SURFACE_ELEVATED, BORDER_HOVER, BORDER_SUBTLE, PILL_GREEN_TEXT, TEXT_MUTED, TEXT_PRIMARY,
    TEXT_SECONDARY,
};
use directories::UserDirs;
use eframe::egui::{self, Rounding, Stroke};
use randydesk_host::files::{default_browse_dir, unique_dest_path, DRIVES_SENTINEL};
use randydesk_proto::{ClientMsg, FileEntry, HostMsg, FILE_CHUNK_SIZE};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

/// Sentido de la transferencia de archivos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// Estado actual de una transferencia individual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferStatus {
    InProgress,
    Done(String),
    Error(String),
    Cancelled,
}

/// Registro de progreso compartido entre red y UI para una transferencia.
#[derive(Debug, Clone)]
pub struct TransferItem {
    pub id: u64,
    pub direction: TransferDirection,
    pub file_name: String,
    pub transferred: u64,
    pub total: u64,
    pub status: TransferStatus,
}

/// Último listado de directorio recibido del equipo remoto.
#[derive(Debug, Clone)]
pub struct RemoteDirListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FileEntry>,
    pub error: Option<String>,
}

/// Acciones disparadas por la ventana dual de transferencia de archivos.
#[derive(Debug, Clone)]
pub enum FileManagerAction {
    ListRemote(String),
    MkdirRemote {
        parent: String,
        name: String,
    },
    UploadFile {
        local_path: PathBuf,
        remote_dir: String,
    },
    DownloadFile {
        remote_path: String,
        local_dir: PathBuf,
    },
    CancelTransfer(u64),
    ClearCompleted,
}

/// Estado local de navegación de la ventana de transferencia de archivos.
pub struct FileManagerWindowState {
    pub local_dir: PathBuf,
    showing_local_drives: bool,
    local_path_input: String,
    remote_path_input: String,
    new_remote_folder: String,
    cached_local_dir: PathBuf,
    cached_local_drives: bool,
    cached_local_entries: Vec<FileEntry>,
    local_error: Option<String>,
    last_synced_remote_path: String,
    local_loaded: bool,
}

impl Default for FileManagerWindowState {
    fn default() -> Self {
        Self::new()
    }
}

impl FileManagerWindowState {
    /// Inicializa el explorador local apuntando al Escritorio del usuario.
    pub fn new() -> Self {
        let dir = default_browse_dir();
        let path_str = dir.to_string_lossy().to_string();
        Self {
            local_dir: dir.clone(),
            showing_local_drives: false,
            local_path_input: path_str,
            remote_path_input: String::new(),
            new_remote_folder: String::new(),
            cached_local_dir: dir,
            cached_local_drives: false,
            cached_local_entries: Vec::new(),
            local_error: None,
            last_synced_remote_path: String::new(),
            local_loaded: false,
        }
    }

    /// Fuerza la relectura del directorio local actual.
    pub fn refresh_local(&mut self) {
        self.local_loaded = true;
        self.cached_local_dir = self.local_dir.clone();
        self.cached_local_drives = self.showing_local_drives;
        if self.showing_local_drives {
            self.local_path_input = "Unidades".to_string();
            self.local_error = None;
            self.cached_local_entries = list_local_drives();
            return;
        }
        self.local_path_input = self.local_dir.to_string_lossy().to_string();
        match list_local_entries(&self.local_dir) {
            Ok(entries) => {
                self.cached_local_entries = entries;
                self.local_error = None;
            }
            Err(err) => {
                self.cached_local_entries.clear();
                self.local_error = Some(err);
            }
        }
    }

    /// Devuelve el directorio remoto activo o cadena vacía si aún no se cargó.
    pub fn current_remote_dir(&self, remote: Option<&RemoteDirListing>) -> String {
        remote
            .map(|r| r.path.clone())
            .filter(|p| !p.is_empty() && p != DRIVES_SENTINEL)
            .unwrap_or_default()
    }

    /// Renderiza la ventana flotante dual y devuelve las acciones solicitadas.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        open: &mut bool,
        remote: Option<&RemoteDirListing>,
        transfers: &[TransferItem],
    ) -> Vec<FileManagerAction> {
        let mut actions = Vec::new();
        if !*open {
            return actions;
        }

        if !self.local_loaded
            || self.cached_local_dir != self.local_dir
            || self.cached_local_drives != self.showing_local_drives
        {
            self.refresh_local();
        }

        if let Some(r) = remote {
            if r.path != self.last_synced_remote_path {
                self.last_synced_remote_path = r.path.clone();
                self.remote_path_input = if r.path == DRIVES_SENTINEL {
                    "Unidades".to_string()
                } else {
                    r.path.clone()
                };
            }
        }

        let mut win_open = *open;
        egui::Window::new("Transferencia de archivos")
            .open(&mut win_open)
            .resizable(true)
            .default_size([880.0, 540.0])
            .min_size([680.0, 400.0])
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(BG_SIDEBAR)
                    .stroke(Stroke::new(1.0_f32, BORDER_HOVER))
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::same(14.0)),
            )
            .show(ctx, |ui| {
                let remote_target_dir = self.current_remote_dir(remote);

                ui.columns(2, |cols| {
                    self.render_local_pane(&mut cols[0], &remote_target_dir, &mut actions);
                    self.render_remote_pane(&mut cols[1], remote, &mut actions);
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                render_transfers_footer(ui, transfers, &mut actions);
            });

        if !win_open {
            *open = false;
        }
        actions
    }

    fn render_local_pane(
        &mut self,
        ui: &mut egui::Ui,
        remote_target_dir: &str,
        actions: &mut Vec<FileManagerAction>,
    ) {
        egui::Frame::none()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Tu equipo (Local)")
                            .size(13.0)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Refrescar").clicked() {
                            self.refresh_local();
                        }
                        if ui.small_button("Unidades").clicked() {
                            self.showing_local_drives = true;
                            self.refresh_local();
                        }
                        if ui.small_button("Subir").clicked() && !self.showing_local_drives {
                            if let Some(parent) = self.local_dir.parent() {
                                self.local_dir = parent.to_path_buf();
                            } else {
                                self.showing_local_drives = true;
                            }
                            self.refresh_local();
                        }
                        if let Some(dirs) = UserDirs::new() {
                            if let Some(dl) = dirs.download_dir() {
                                if ui.small_button("Descargas").clicked() {
                                    self.showing_local_drives = false;
                                    self.local_dir = dl.to_path_buf();
                                    self.refresh_local();
                                }
                            }
                            if let Some(desk) = dirs.desktop_dir() {
                                if ui.small_button("Escritorio").clicked() {
                                    self.showing_local_drives = false;
                                    self.local_dir = desk.to_path_buf();
                                    self.refresh_local();
                                }
                            }
                        }
                    });
                });

                ui.add_space(6.0);
                let w = ui.available_width();
                let resp = styled_text_input(
                    ui,
                    &mut self.local_path_input,
                    "Ruta local...",
                    w,
                    28.0,
                    12.0,
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let cand = PathBuf::from(self.local_path_input.trim());
                    if cand.is_dir() {
                        self.showing_local_drives = false;
                        self.local_dir = cand;
                        self.refresh_local();
                    }
                }

                if let Some(err) = &self.local_error {
                    ui.add_space(4.0);
                    ui.colored_label(ACCENT_DANGER_TEXT, err);
                }

                ui.add_space(6.0);
                let entries = self.cached_local_entries.clone();
                let mut open_dir: Option<PathBuf> = None;
                let mut upload_file: Option<PathBuf> = None;

                egui::ScrollArea::vertical()
                    .id_salt("local_files_scroll")
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for entry in &entries {
                            ui.horizontal(|ui| {
                                let icon = if entry.is_dir { "[DIR]" } else { "     " };
                                ui.label(
                                    egui::RichText::new(icon)
                                        .size(11.0)
                                        .color(if entry.is_dir {
                                            ACCENT_BLUE
                                        } else {
                                            TEXT_MUTED
                                        }),
                                );
                                if entry.is_dir {
                                    if ui
                                        .selectable_label(
                                            false,
                                            egui::RichText::new(&entry.name)
                                                .size(12.0)
                                                .color(TEXT_PRIMARY),
                                        )
                                        .clicked()
                                    {
                                        open_dir = Some(if self.showing_local_drives {
                                            PathBuf::from(&entry.name)
                                        } else {
                                            self.local_dir.join(&entry.name)
                                        });
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new(&entry.name)
                                            .size(12.0)
                                            .color(TEXT_SECONDARY),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Enviar ->")
                                                            .size(10.5)
                                                            .color(TEXT_PRIMARY),
                                                    )
                                                    .fill(BG_SURFACE_ELEVATED)
                                                    .stroke(Stroke::new(1.0_f32, BORDER_HOVER)),
                                                )
                                                .clicked()
                                            {
                                                upload_file = Some(self.local_dir.join(&entry.name));
                                            }
                                            ui.label(
                                                egui::RichText::new(format_bytes(entry.size))
                                                    .size(10.5)
                                                    .color(TEXT_MUTED),
                                            );
                                        },
                                    );
                                }
                            });
                        }
                    });

                if let Some(next_dir) = open_dir {
                    self.showing_local_drives = false;
                    self.local_dir = next_dir;
                    self.refresh_local();
                }
                if let Some(path) = upload_file {
                    actions.push(FileManagerAction::UploadFile {
                        local_path: path,
                        remote_dir: remote_target_dir.to_string(),
                    });
                }
            });
    }

    fn render_remote_pane(
        &mut self,
        ui: &mut egui::Ui,
        remote: Option<&RemoteDirListing>,
        actions: &mut Vec<FileManagerAction>,
    ) {
        egui::Frame::none()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Equipo remoto")
                            .size(13.0)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Refrescar").clicked() {
                            let cur = remote.map(|r| r.path.clone()).unwrap_or_default();
                            actions.push(FileManagerAction::ListRemote(cur));
                        }
                        if ui.small_button("Unidades").clicked() {
                            actions.push(FileManagerAction::ListRemote(
                                DRIVES_SENTINEL.to_string(),
                            ));
                        }
                        if let Some(parent) = remote.and_then(|r| r.parent.as_ref()) {
                            if ui.small_button("Subir").clicked() {
                                actions.push(FileManagerAction::ListRemote(parent.clone()));
                            }
                        }
                        if ui.small_button("Inicio").clicked() {
                            actions.push(FileManagerAction::ListRemote(String::new()));
                        }
                    });
                });

                ui.add_space(6.0);
                let w = ui.available_width();
                let resp = styled_text_input(
                    ui,
                    &mut self.remote_path_input,
                    "Ruta remota...",
                    w,
                    28.0,
                    12.0,
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let typed = self.remote_path_input.trim().to_string();
                    actions.push(FileManagerAction::ListRemote(typed));
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let fw = (ui.available_width() - 96.0).max(100.0);
                    let f_resp = styled_text_input(
                        ui,
                        &mut self.new_remote_folder,
                        "Nueva carpeta remota...",
                        fw,
                        26.0,
                        11.5,
                    );
                    let enter = f_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui.button("+ Carpeta").clicked() || enter)
                        && !self.new_remote_folder.trim().is_empty()
                    {
                        let parent = self.current_remote_dir(remote);
                        let name = self.new_remote_folder.trim().to_string();
                        self.new_remote_folder.clear();
                        actions.push(FileManagerAction::MkdirRemote { parent, name });
                    }
                });

                if let Some(err) = remote.and_then(|r| r.error.as_ref()) {
                    ui.add_space(4.0);
                    ui.colored_label(ACCENT_DANGER_TEXT, err);
                }

                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("remote_files_scroll")
                    .max_height(232.0)
                    .show(ui, |ui| {
                        let Some(listing) = remote else {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Cargando archivos del equipo remoto...");
                            });
                            return;
                        };
                        for entry in &listing.entries {
                            ui.horizontal(|ui| {
                                let icon = if entry.is_dir { "[DIR]" } else { "     " };
                                ui.label(
                                    egui::RichText::new(icon)
                                        .size(11.0)
                                        .color(if entry.is_dir {
                                            ACCENT_BLUE
                                        } else {
                                            TEXT_MUTED
                                        }),
                                );
                                if entry.is_dir {
                                    if ui
                                        .selectable_label(
                                            false,
                                            egui::RichText::new(&entry.name)
                                                .size(12.0)
                                                .color(TEXT_PRIMARY),
                                        )
                                        .clicked()
                                    {
                                        let next = if listing.path == DRIVES_SENTINEL {
                                            entry.name.clone()
                                        } else {
                                            PathBuf::from(&listing.path)
                                                .join(&entry.name)
                                                .to_string_lossy()
                                                .to_string()
                                        };
                                        actions.push(FileManagerAction::ListRemote(next));
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new(&entry.name)
                                            .size(12.0)
                                            .color(TEXT_SECONDARY),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("<- Bajar")
                                                            .size(10.5)
                                                            .color(TEXT_PRIMARY),
                                                    )
                                                    .fill(ACCENT_BLUE),
                                                )
                                                .clicked()
                                            {
                                                let remote_path = PathBuf::from(&listing.path)
                                                    .join(&entry.name)
                                                    .to_string_lossy()
                                                    .to_string();
                                                actions.push(FileManagerAction::DownloadFile {
                                                    remote_path,
                                                    local_dir: self.local_dir.clone(),
                                                });
                                            }
                                            ui.label(
                                                egui::RichText::new(format_bytes(entry.size))
                                                    .size(10.5)
                                                    .color(TEXT_MUTED),
                                            );
                                        },
                                    );
                                }
                            });
                        }
                    });
            });
    }
}

fn render_transfers_footer(
    ui: &mut egui::Ui,
    transfers: &[TransferItem],
    actions: &mut Vec<FileManagerAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Cola de transferencias (puedes arrastrar archivos sobre la sesión)")
                .size(12.0)
                .strong()
                .color(TEXT_PRIMARY),
        );
        if transfers
            .iter()
            .any(|t| !matches!(t.status, TransferStatus::InProgress))
        {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Limpiar finalizadas").clicked() {
                    actions.push(FileManagerAction::ClearCompleted);
                }
            });
        }
    });
    ui.add_space(4.0);

    if transfers.is_empty() {
        ui.label(
            egui::RichText::new("Sin transferencias en curso.")
                .size(11.5)
                .color(TEXT_MUTED),
        );
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("transfers_scroll")
        .max_height(110.0)
        .show(ui, |ui| {
            for t in transfers.iter().rev() {
                ui.horizontal(|ui| {
                    let dir = match t.direction {
                        TransferDirection::Upload => "Subiendo",
                        TransferDirection::Download => "Bajando",
                    };
                    ui.label(
                        egui::RichText::new(format!("{dir}: {}", t.file_name))
                            .size(11.5)
                            .color(TEXT_PRIMARY),
                    );
                    let frac = if t.total == 0 {
                        if matches!(t.status, TransferStatus::Done(_)) {
                            1.0
                        } else {
                            0.0
                        }
                    } else {
                        (t.transferred as f32 / t.total as f32).clamp(0.0, 1.0)
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(150.0)
                            .text(format!(
                                "{} / {}",
                                format_bytes(t.transferred),
                                format_bytes(t.total)
                            )),
                    );
                    match &t.status {
                        TransferStatus::InProgress => {
                            if ui.small_button("Cancelar").clicked() {
                                actions.push(FileManagerAction::CancelTransfer(t.id));
                            }
                        }
                        TransferStatus::Done(_) => {
                            ui.colored_label(PILL_GREEN_TEXT, "Completado");
                        }
                        TransferStatus::Error(err) => {
                            ui.colored_label(ACCENT_DANGER_TEXT, format!("Error: {err}"));
                        }
                        TransferStatus::Cancelled => {
                            ui.colored_label(TEXT_MUTED, "Cancelado");
                        }
                    }
                });
            }
        });
}

fn list_local_drives() -> Vec<FileEntry> {
    let mut out = Vec::new();
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if Path::new(&root).exists() {
            out.push(FileEntry {
                name: root,
                is_dir: true,
                size: 0,
                modified_unix: 0,
            });
        }
    }
    out
}

fn list_local_entries(dir: &Path) -> std::result::Result<Vec<FileEntry>, String> {
    let rd = fs::read_dir(dir).map_err(|e| format!("No se pudo leer la carpeta local: {e}"))?;
    let mut entries = Vec::new();
    for item in rd.flatten() {
        let name = item.file_name().to_string_lossy().to_string();
        let (is_dir, size, modified_unix) = match item.metadata() {
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
    Ok(entries)
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

struct ActiveDownload {
    temp_path: PathBuf,
    local_dir: PathBuf,
    file_name: String,
    file: File,
    written: u64,
    total_size: u64,
    completed: bool,
}

impl Drop for ActiveDownload {
    fn drop(&mut self) {
        if !self.completed {
            let _ = fs::remove_file(&self.temp_path);
        }
    }
}

/// Resultado de despachar un `NetCommand` relacionado con archivos.
pub(crate) enum HandledFileCommand {
    Direct(ClientMsg),
    Spawned,
    Other,
}

/// Gestor compartido entre QUIC y Relay E2E para subidas y descargas del cliente.
pub(crate) struct ClientFileTransfers {
    pending_downloads: HashMap<u64, PathBuf>,
    active_downloads: HashMap<u64, ActiveDownload>,
    cancelled_uploads: Arc<Mutex<HashSet<u64>>>,
}

impl ClientFileTransfers {
    pub fn new() -> Self {
        Self {
            pending_downloads: HashMap::new(),
            active_downloads: HashMap::new(),
            cancelled_uploads: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn handle_command(
        &mut self,
        cmd: NetCommand,
        xfer_tx: &mpsc::Sender<ClientMsg>,
        shared: &Arc<Mutex<SharedSessionState>>,
        ctx: &egui::Context,
    ) -> HandledFileCommand {
        match cmd {
            NetCommand::FileListReq(path) => {
                HandledFileCommand::Direct(ClientMsg::FileListReq { path })
            }
            NetCommand::FileMkdir { parent_dir, name } => {
                HandledFileCommand::Direct(ClientMsg::FileMkdir { parent_dir, name })
            }
            NetCommand::UploadLocalFile {
                transfer_id,
                local_path,
                remote_dir,
            } => {
                self.spawn_upload(
                    transfer_id,
                    local_path,
                    remote_dir,
                    xfer_tx.clone(),
                    Arc::clone(shared),
                    ctx.clone(),
                );
                HandledFileCommand::Spawned
            }
            NetCommand::DownloadRemoteFile {
                transfer_id,
                remote_path,
                local_dir,
            } => {
                self.register_download(transfer_id, local_dir, &remote_path, shared, ctx);
                HandledFileCommand::Direct(ClientMsg::FileDownloadReq {
                    transfer_id,
                    remote_path,
                })
            }
            NetCommand::CancelTransfer(transfer_id) => {
                self.cancel(transfer_id, shared, ctx);
                HandledFileCommand::Direct(ClientMsg::FileTransferCancel { transfer_id })
            }
            _ => HandledFileCommand::Other,
        }
    }

    fn register_download(
        &mut self,
        transfer_id: u64,
        local_dir: PathBuf,
        remote_path: &str,
        shared: &Arc<Mutex<SharedSessionState>>,
        ctx: &egui::Context,
    ) {
        self.pending_downloads.insert(transfer_id, local_dir);
        let file_name = Path::new(remote_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archivo")
            .to_string();
        if let Ok(mut g) = shared.lock() {
            g.transfers.push(TransferItem {
                id: transfer_id,
                direction: TransferDirection::Download,
                file_name,
                transferred: 0,
                total: 0,
                status: TransferStatus::InProgress,
            });
        }
        ctx.request_repaint();
    }

    fn spawn_upload(
        &self,
        transfer_id: u64,
        local_path: PathBuf,
        remote_dir: String,
        out_tx: mpsc::Sender<ClientMsg>,
        shared: Arc<Mutex<SharedSessionState>>,
        ctx: egui::Context,
    ) {
        let cancelled = Arc::clone(&self.cancelled_uploads);
        tokio::spawn(async move {
            let file_name = local_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("archivo")
                .to_string();
            let mut file = match tokio::fs::File::open(&local_path).await {
                Ok(f) => f,
                Err(err) => {
                    push_upload_error(&shared, &ctx, transfer_id, &file_name, format!("{err}"));
                    return;
                }
            };
            let total_size = match file.metadata().await {
                Ok(m) if m.is_file() => m.len(),
                Ok(_) => {
                    push_upload_error(
                        &shared,
                        &ctx,
                        transfer_id,
                        &file_name,
                        "Solo se pueden enviar archivos".to_string(),
                    );
                    return;
                }
                Err(err) => {
                    push_upload_error(&shared, &ctx, transfer_id, &file_name, format!("{err}"));
                    return;
                }
            };

            if let Ok(mut g) = shared.lock() {
                g.transfers.push(TransferItem {
                    id: transfer_id,
                    direction: TransferDirection::Upload,
                    file_name: file_name.clone(),
                    transferred: 0,
                    total: total_size,
                    status: TransferStatus::InProgress,
                });
            }
            ctx.request_repaint();

            if out_tx
                .send(ClientMsg::FileUploadStart {
                    transfer_id,
                    remote_dir,
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
                    return;
                }
                match file.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        if out_tx
                            .send(ClientMsg::FileUploadChunk {
                                transfer_id,
                                offset,
                                data,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                        offset = offset.saturating_add(n as u64);
                        if let Ok(mut g) = shared.lock() {
                            if let Some(item) = g.transfers.iter_mut().find(|t| t.id == transfer_id) {
                                item.transferred = offset;
                            }
                        }
                        ctx.request_repaint();
                    }
                    Err(err) => {
                        set_transfer_status(
                            &shared,
                            &ctx,
                            transfer_id,
                            TransferStatus::Error(format!("{err}")),
                        );
                        return;
                    }
                }
            }

            let _ = out_tx.send(ClientMsg::FileUploadFinish { transfer_id }).await;
        });
    }

    fn cancel(
        &mut self,
        transfer_id: u64,
        shared: &Arc<Mutex<SharedSessionState>>,
        ctx: &egui::Context,
    ) {
        self.pending_downloads.remove(&transfer_id);
        self.active_downloads.remove(&transfer_id);
        if let Ok(mut s) = self.cancelled_uploads.lock() {
            s.insert(transfer_id);
        }
        set_transfer_status(shared, ctx, transfer_id, TransferStatus::Cancelled);
    }

    pub fn handle_host_msg(
        &mut self,
        msg: HostMsg,
        shared: &Arc<Mutex<SharedSessionState>>,
        ctx: &egui::Context,
    ) -> Option<HostMsg> {
        match msg {
            HostMsg::FileListResp {
                path,
                parent,
                entries,
                error,
            } => {
                if let Ok(mut g) = shared.lock() {
                    g.remote_dir = Some(RemoteDirListing {
                        path,
                        parent,
                        entries,
                        error,
                    });
                }
                ctx.request_repaint();
                None
            }
            HostMsg::FileDownloadMeta {
                transfer_id,
                file_name,
                total_size,
            } => {
                let dir = self
                    .pending_downloads
                    .remove(&transfer_id)
                    .unwrap_or_else(default_browse_dir);
                let temp_path = dir.join(format!(".randydesk-dl-{transfer_id}.rdpart"));
                match OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&temp_path)
                {
                    Ok(file) => {
                        self.active_downloads.insert(
                            transfer_id,
                            ActiveDownload {
                                temp_path,
                                local_dir: dir,
                                file_name: file_name.clone(),
                                file,
                                written: 0,
                                total_size,
                                completed: false,
                            },
                        );
                        if let Ok(mut g) = shared.lock() {
                            if let Some(item) = g.transfers.iter_mut().find(|t| t.id == transfer_id)
                            {
                                item.file_name = file_name;
                                item.total = total_size;
                            }
                        }
                        ctx.request_repaint();
                    }
                    Err(err) => {
                        set_transfer_status(
                            shared,
                            ctx,
                            transfer_id,
                            TransferStatus::Error(format!("{err}")),
                        );
                    }
                }
                None
            }
            HostMsg::FileDownloadChunk {
                transfer_id,
                offset,
                data,
            } => {
                if let Some(dl) = self.active_downloads.get_mut(&transfer_id) {
                    if offset != dl.written {
                        self.active_downloads.remove(&transfer_id);
                        set_transfer_status(
                            shared,
                            ctx,
                            transfer_id,
                            TransferStatus::Error("Bloque de descarga fuera de orden".to_string()),
                        );
                        return None;
                    }
                    if let Err(err) = dl.file.write_all(&data) {
                        self.active_downloads.remove(&transfer_id);
                        set_transfer_status(
                            shared,
                            ctx,
                            transfer_id,
                            TransferStatus::Error(format!("{err}")),
                        );
                        return None;
                    }
                    dl.written = dl.written.saturating_add(data.len() as u64);
                    let written = dl.written;
                    if let Ok(mut g) = shared.lock() {
                        if let Some(item) = g.transfers.iter_mut().find(|t| t.id == transfer_id) {
                            item.transferred = written;
                        }
                    }
                    ctx.request_repaint();
                }
                None
            }
            HostMsg::FileTransferDone {
                transfer_id,
                saved_path,
            } => {
                if let Some(mut dl) = self.active_downloads.remove(&transfer_id) {
                    if dl.written != dl.total_size {
                        set_transfer_status(
                            shared,
                            ctx,
                            transfer_id,
                            TransferStatus::Error(format!(
                                "Descarga incompleta ({} de {} bytes)",
                                dl.written, dl.total_size
                            )),
                        );
                        return None;
                    }
                    let _ = dl.file.flush();
                    let dest = unique_dest_path(&dl.local_dir, &dl.file_name);
                    if let Err(err) = fs::rename(&dl.temp_path, &dest) {
                        set_transfer_status(
                            shared,
                            ctx,
                            transfer_id,
                            TransferStatus::Error(format!("{err}")),
                        );
                    } else {
                        dl.completed = true;
                        let dest_str = dest.to_string_lossy().to_string();
                        if let Ok(mut g) = shared.lock() {
                            if let Some(item) = g.transfers.iter_mut().find(|t| t.id == transfer_id)
                            {
                                item.transferred = item.total;
                                item.status = TransferStatus::Done(dest_str);
                            }
                        }
                        ctx.request_repaint();
                    }
                } else {
                    if let Ok(mut g) = shared.lock() {
                        if let Some(item) = g.transfers.iter_mut().find(|t| t.id == transfer_id) {
                            item.transferred = item.total;
                            item.status = TransferStatus::Done(saved_path);
                        }
                    }
                    ctx.request_repaint();
                }
                None
            }
            HostMsg::FileTransferError {
                transfer_id,
                message,
            } => {
                self.pending_downloads.remove(&transfer_id);
                self.active_downloads.remove(&transfer_id);
                set_transfer_status(shared, ctx, transfer_id, TransferStatus::Error(message));
                None
            }
            other => Some(other),
        }
    }
}

fn push_upload_error(
    shared: &Arc<Mutex<SharedSessionState>>,
    ctx: &egui::Context,
    id: u64,
    file_name: &str,
    err: String,
) {
    if let Ok(mut g) = shared.lock() {
        g.transfers.push(TransferItem {
            id,
            direction: TransferDirection::Upload,
            file_name: file_name.to_string(),
            transferred: 0,
            total: 0,
            status: TransferStatus::Error(err),
        });
    }
    ctx.request_repaint();
}

fn set_transfer_status(
    shared: &Arc<Mutex<SharedSessionState>>,
    ctx: &egui::Context,
    transfer_id: u64,
    status: TransferStatus,
) {
    if let Ok(mut g) = shared.lock() {
        if let Some(item) = g.transfers.iter_mut().find(|t| t.id == transfer_id) {
            item.status = status;
        }
    }
    ctx.request_repaint();
}
