//! Interfaz gráfica unificada de RandyDesk (bandeja, hook tecla Win, acceso directo y GPU parcial).

use crate::cards::{render_host_cards, CardAction, HostTab};
use crate::config::{
    display_address, load_hosts, load_settings, normalize_address, save_hosts, save_settings,
    upsert_recent, AppSettings, SavedHost,
};
use crate::decoder::FrameUpdate;
use crate::file_manager::{
    FileManagerAction, FileManagerWindowState, RemoteDirListing, TransferItem, TransferStatus,
};
use crate::host_panel::HostPanelState;
use crate::keymap::{is_control_or_nav_key, map_egui_key_to_vk, map_mouse_button, normalize_pos};
use crate::net::{ConnectRequest, ConnectionState, NetCommand, NetworkHandle};
use crate::session_bar::{
    render_session_overlay, QualityPreset, ScaleMode, SessionBarInput, SessionBarState,
};
use crate::theme::{
    apply_rustdesk_theme, styled_text_input, ACCENT_BLUE, ACCENT_TEAL, ACCENT_TEAL_SOLID,
    BANNER_COLORS, BG_CARD, BG_INPUT, BG_MAIN, BG_SIDEBAR, BG_SURFACE_ELEVATED, BORDER_HOVER,
    BORDER_SUBTLE, STATUS_ORANGE, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};
use crate::tray::TrayController;
use crate::updater::{UpdateState, UpdaterHandle};
use crate::win_hook::{KeyboardHookGuard, KeyboardMode};
use arboard::Clipboard;
use eframe::egui::{self, Color32, Rounding, Stroke, Vec2};
use randydesk_host::clipboard::ClipboardWatcher;
use randydesk_host::input::{append_input_debug_log, set_input_debug_logging};
use randydesk_proto::{
    normalize_relay_ws_url, CodecKind, CursorKind, InputEvent, MonitorInfo, StreamSettings,
    DEFAULT_RELAY_URL,
};
use std::time::{Duration, Instant};

fn map_remote_cursor(kind: CursorKind) -> egui::CursorIcon {
    match kind {
        CursorKind::Default => egui::CursorIcon::Default,
        CursorKind::Text => egui::CursorIcon::Text,
        CursorKind::PointingHand => egui::CursorIcon::PointingHand,
        CursorKind::Crosshair => egui::CursorIcon::Crosshair,
        CursorKind::Move => egui::CursorIcon::Move,
        CursorKind::ResizeHorizontal => egui::CursorIcon::ResizeHorizontal,
        CursorKind::ResizeVertical => egui::CursorIcon::ResizeVertical,
        CursorKind::ResizeNeSw => egui::CursorIcon::ResizeNeSw,
        CursorKind::ResizeNwSe => egui::CursorIcon::ResizeNwSe,
        CursorKind::NotAllowed => egui::CursorIcon::NotAllowed,
        CursorKind::Wait => egui::CursorIcon::Wait,
        CursorKind::Progress => egui::CursorIcon::Progress,
        CursorKind::Hidden => egui::CursorIcon::None,
    }
}

/// Datos para pedir contraseña solo si el host remoto tiene contraseña opcional activada.
#[derive(Clone)]
struct PasswordPrompt {
    title: String,
    address: String,
    password: String,
    expected_fp: Option<String>,
    codec: CodecKind,
    quality: u8,
    fps: u8,
    just_opened: bool,
}

/// Datos temporales del modal para agregar o editar un equipo guardado.
struct HostEditor {
    editing_idx: Option<usize>,
    name: String,
    address: String,
    color_idx: u8,
}

/// Aplicación principal de RandyDesk (anfitrión + visor remoto con bandeja del sistema).
pub struct RandyDeskApp {
    rt_handle: tokio::runtime::Handle,
    host_panel: HostPanelState,
    net: NetworkHandle,
    hosts: Vec<SavedHost>,
    settings: AppSettings,
    tray: Option<TrayController>,
    updater: UpdaterHandle,
    kb_hook: KeyboardHookGuard,

    // Estado de bandeja
    hidden_to_tray: bool,
    restored_at: Instant,

    // Estado de vista principal (Lobby)
    quick_address: String,
    active_tab: HostTab,
    search_query: String,
    codec_input: CodecKind,
    quality_input: u8,
    fps_input: u8,
    ui_notice: Option<String>,

    // Modales
    last_attempt: Option<PasswordPrompt>,
    password_prompt: Option<PasswordPrompt>,
    host_editor: Option<HostEditor>,
    show_relay_modal: bool,
    relay_url_input: String,
    relay_modal_msg: Option<String>,
    show_guide: bool,

    // Renderizado incremental del escritorio remoto
    screen_texture: Option<egui::TextureHandle>,
    fullscreen: bool,
    scale_mode: ScaleMode,
    custom_scale_pct: f32,
    stream_scale: u8,
    quality_preset: QualityPreset,
    show_monitor_stats: bool,
    show_remote_cursor: bool,
    keyboard_grab_enabled: bool,
    keyboard_mode: KeyboardMode,
    invert_wheel: bool,
    swap_mouse_buttons: bool,
    view_only: bool,
    bar_collapsed: bool,
    bar_pinned: bool,
    auto_clipboard: bool,
    input_debug_log: bool,
    clip_watcher: ClipboardWatcher,
    last_clip_poll: Instant,
    audio_enabled: bool,
    audio_volume: u8,
    show_file_manager: bool,
    file_mgr_ui: FileManagerWindowState,
    next_transfer_id: u64,
    was_foreground: bool,
    focused_on_connect: bool,
    remote_ctrl_down: bool,
    remote_alt_down: bool,
    remote_shift_down: bool,
    active_target_address: Option<String>,
    recorded_recent_for_session: bool,
    last_error_handled: Option<String>,
}

impl RandyDeskApp {
    /// Inicializa la aplicación con bandeja del sistema, hook de teclado Win32 y servidor Relay.
    pub fn new(cc: &eframe::CreationContext<'_>, rt_handle: tokio::runtime::Handle) -> Self {
        apply_rustdesk_theme(&cc.egui_ctx);
        let hosts = load_hosts();
        let settings = load_settings();
        let relay_url_input = settings.relay_url.clone();
        let host_panel = HostPanelState::new(&rt_handle, settings.normalized_relay_url());
        let tray = TrayController::new(cc.egui_ctx.clone());
        let updater = UpdaterHandle::new(rt_handle.clone(), cc.egui_ctx.clone());
        let kb_hook = KeyboardHookGuard::new();

        Self {
            net: NetworkHandle::new(rt_handle.clone()),
            rt_handle,
            host_panel,
            hosts,
            settings,
            tray,
            updater,
            kb_hook,
            hidden_to_tray: false,
            restored_at: Instant::now() - Duration::from_secs(5),
            quick_address: String::new(),
            active_tab: HostTab::Recent,
            search_query: String::new(),
            codec_input: CodecKind::Jpeg,
            quality_input: 72,
            fps_input: 30,
            ui_notice: None,
            last_attempt: None,
            password_prompt: None,
            host_editor: None,
            show_relay_modal: false,
            relay_url_input,
            relay_modal_msg: None,
            show_guide: false,
            screen_texture: None,
            fullscreen: false,
            scale_mode: ScaleMode::Adaptive,
            custom_scale_pct: 100.0,
            stream_scale: 100,
            quality_preset: QualityPreset::Balanced,
            show_monitor_stats: true,
            show_remote_cursor: true,
            keyboard_grab_enabled: true,
            keyboard_mode: KeyboardMode::default(),
            invert_wheel: false,
            swap_mouse_buttons: false,
            view_only: false,
            bar_collapsed: false,
            bar_pinned: true,
            auto_clipboard: true,
            input_debug_log: false,
            clip_watcher: ClipboardWatcher::new(),
            last_clip_poll: Instant::now(),
            audio_enabled: true,
            audio_volume: 100,
            show_file_manager: false,
            file_mgr_ui: FileManagerWindowState::new(),
            next_transfer_id: 1,
            was_foreground: false,
            focused_on_connect: false,
            remote_ctrl_down: false,
            remote_alt_down: false,
            remote_shift_down: false,
            active_target_address: None,
            recorded_recent_for_session: false,
            last_error_handled: None,
        }
    }

    fn release_remote_modifiers(&mut self) {
        if self.remote_ctrl_down {
            self.net.send_input(InputEvent::Key {
                vk: 0xA2,
                scan: 0x1D,
                extended: false,
                down: false,
            });
            self.remote_ctrl_down = false;
        }
        if self.remote_alt_down {
            self.net.send_input(InputEvent::Key {
                vk: 0xA4,
                scan: 0x38,
                extended: false,
                down: false,
            });
            self.remote_alt_down = false;
        }
        if self.remote_shift_down {
            self.net.send_input(InputEvent::Key {
                vk: 0xA0,
                scan: 0x2A,
                extended: false,
                down: false,
            });
            self.remote_shift_down = false;
        }
    }

    fn send_remote_ctrl_shortcut(&mut self, vk: u16, scan: u16) {
        let need_temp_ctrl = !self.remote_ctrl_down;
        if need_temp_ctrl {
            self.net.send_input(InputEvent::Key {
                vk: 0xA2,
                scan: 0x1D,
                extended: false,
                down: true,
            });
        }
        self.net.send_input(InputEvent::Key {
            vk,
            scan,
            extended: false,
            down: true,
        });
        self.net.send_input(InputEvent::Key {
            vk,
            scan,
            extended: false,
            down: false,
        });
        if need_temp_ctrl {
            self.net.send_input(InputEvent::Key {
                vk: 0xA2,
                scan: 0x1D,
                extended: false,
                down: false,
            });
        }
    }

    fn connect_direct_address(&mut self, raw_address: &str, ctx: &egui::Context) {
        let trimmed = raw_address.trim();
        if trimmed.is_empty() {
            self.ui_notice = Some("Escribe el ID de 9 dígitos o la IP local del equipo destino.".into());
            return;
        }
        let normalized = normalize_address(trimmed);
        if let Some(idx) = self.hosts.iter().position(|h| h.address == normalized) {
            self.connect_direct_host(idx, ctx);
            return;
        }
        let attempt = PasswordPrompt {
            title: display_address(&normalized),
            address: normalized,
            password: String::new(),
            expected_fp: None,
            codec: self.codec_input,
            quality: self.quality_input,
            fps: self.fps_input,
            just_opened: true,
        };
        self.launch_connection(attempt, ctx);
    }

    fn connect_direct_host(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(host) = self.hosts.get(idx) else {
            return;
        };
        self.codec_input = host.codec;
        self.quality_input = host.quality;
        self.fps_input = host.fps;
        let attempt = PasswordPrompt {
            title: host.name.clone(),
            address: host.address.clone(),
            password: String::new(),
            expected_fp: host.fingerprint_sha256.clone(),
            codec: host.codec,
            quality: host.quality,
            fps: host.fps,
            just_opened: true,
        };
        self.launch_connection(attempt, ctx);
    }

    fn launch_connection(&mut self, prompt: PasswordPrompt, ctx: &egui::Context) {
        self.active_target_address = Some(prompt.address.clone());
        self.recorded_recent_for_session = false;
        self.focused_on_connect = false;
        self.last_error_handled = None;
        self.ui_notice = None;
        self.show_file_manager = false;
        self.clip_watcher.reset();
        self.last_clip_poll = Instant::now();
        self.last_attempt = Some(prompt.clone());
        self.net.set_audio_volume(self.audio_volume);
        self.net.connect(
            ConnectRequest {
                address: prompt.address,
                password: prompt.password,
                expected_fingerprint: prompt.expected_fp,
                relay_url: self.settings.normalized_relay_url(),
                settings: StreamSettings {
                    codec: prompt.codec,
                    jpeg_quality: prompt.quality,
                    target_fps: prompt.fps,
                },
                stream_scale: self.stream_scale,
                monitor_index: 0,
                audio_enabled: self.audio_enabled,
            },
            ctx.clone(),
        );
    }

    fn handle_card_action(&mut self, act: CardAction, ctx: &egui::Context) {
        match act {
            CardAction::Connect(idx) => self.connect_direct_host(idx, ctx),
            CardAction::ToggleFavorite(idx) => {
                if let Some(h) = self.hosts.get_mut(idx) {
                    h.favorite = !h.favorite;
                    let _ = save_hosts(&self.hosts);
                }
            }
            CardAction::Edit(idx) => {
                if let Some(h) = self.hosts.get(idx) {
                    self.host_editor = Some(HostEditor {
                        editing_idx: Some(idx),
                        name: h.name.clone(),
                        address: display_address(&h.address),
                        color_idx: h.color_idx,
                    });
                }
            }
            CardAction::Delete(idx) => {
                if idx < self.hosts.len() {
                    self.hosts.remove(idx);
                    let _ = save_hosts(&self.hosts);
                }
            }
        }
    }

    fn apply_frame_updates_to_gpu(&mut self, ctx: &egui::Context, updates: Vec<FrameUpdate>) {
        for update in updates {
            let target_size = [update.width as usize, update.height as usize];
            let needs_alloc = match &self.screen_texture {
                Some(tex) => tex.size() != target_size,
                None => true,
            };

            if needs_alloc {
                let blank = egui::ColorImage::new(target_size, Color32::BLACK);
                self.screen_texture = Some(ctx.load_texture(
                    "remote-screen",
                    blank,
                    egui::TextureOptions::LINEAR,
                ));
            }

            if let Some(tex) = &mut self.screen_texture {
                for tile in update.tiles {
                    let tile_img = egui::ColorImage::from_rgba_unmultiplied(
                        [tile.width as usize, tile.height as usize],
                        &tile.rgba,
                    );
                    tex.set_partial(
                        [tile.x as usize, tile.y as usize],
                        tile_img,
                        egui::TextureOptions::LINEAR,
                    );
                }
            }
        }
    }
}

impl eframe::App for RandyDeskApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.updater.take_exit_requested() {
            self.kb_hook.set_capture(false, None);
            self.net.disconnect();
            self.host_panel.stop_host();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // 1. Gestión de minimizar / restaurar desde la bandeja del sistema
        if let Some(tray) = self.tray.as_ref() {
            if tray.take_restore_requested() {
                self.hidden_to_tray = false;
                self.restored_at = Instant::now();
                self.net.send_cmd(NetCommand::RequestKeyframe);
            } else if !self.hidden_to_tray {
                tray.cache_main_hwnd();
                if self.restored_at.elapsed() >= Duration::from_millis(450)
                    && tray.is_window_minimized()
                    && tray.hide_to_tray()
                {
                    self.hidden_to_tray = true;
                    self.release_remote_modifiers();
                    self.kb_hook.set_capture(false, None);
                }
            }
        }

        // 2. Lectura del estado compartido de red
        let (
            status,
            stats,
            updates,
            incoming_clip,
            monitors,
            active_monitor,
            remote_cursor,
            remote_dir,
            transfers,
        ) = {
            let mut guard = self.net.shared.lock().expect("lock estado red");
            let clip = guard.incoming_clipboard.take();
            let updates: Vec<FrameUpdate> = guard.pending_updates.drain(..).collect();
            (
                guard.status.clone(),
                guard.stats.clone(),
                updates,
                clip,
                guard.monitors.clone(),
                guard.active_monitor,
                guard.remote_cursor,
                guard.remote_dir.clone(),
                guard.transfers.clone(),
            )
        };

        if let Some(clip_text) = incoming_clip {
            if let Ok(mut cb) = Clipboard::new() {
                let _ = cb.set_text(clip_text.clone());
            }
            self.clip_watcher.mark_applied(&clip_text);
        }

        if matches!(status, ConnectionState::Connected { .. })
            && self.auto_clipboard
            && !self.view_only
            && !self.hidden_to_tray
            && self.last_clip_poll.elapsed() >= Duration::from_millis(350)
        {
            self.last_clip_poll = Instant::now();
            if let Some(text) = self.clip_watcher.poll_changed() {
                self.net.send_cmd(NetCommand::ClipboardSet(text));
            }
        }

        if let ConnectionState::Connected { ref host_name, .. } = status {
            if !self.recorded_recent_for_session {
                if let Some(addr) = &self.active_target_address {
                    upsert_recent(
                        &mut self.hosts,
                        addr,
                        host_name,
                        stats.fingerprint_sha256.as_deref(),
                        self.codec_input,
                        self.quality_input,
                        self.fps_input,
                    );
                    let _ = save_hosts(&self.hosts);
                }
                self.recorded_recent_for_session = true;
            }
            if !self.hidden_to_tray {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }

        if !updates.is_empty() {
            self.apply_frame_updates_to_gpu(ctx, updates);
        }

        if self.hidden_to_tray {
            self.release_remote_modifiers();
            self.kb_hook.set_capture(false, None);
            return;
        }

        match status {
            ConnectionState::Disconnected | ConnectionState::Error(_) => {
                self.focused_on_connect = false;
                self.release_remote_modifiers();
                self.kb_hook.set_capture(false, None);
                if let ConnectionState::Error(msg) = &status {
                    if self.last_error_handled.as_deref() != Some(msg.as_str()) {
                        self.last_error_handled = Some(msg.clone());
                        if msg.contains("Contraseña") || msg.contains("PIN") {
                            if let Some(mut attempt) = self.last_attempt.clone() {
                                attempt.password.clear();
                                attempt.just_opened = true;
                                self.password_prompt = Some(attempt);
                            }
                        } else {
                            self.ui_notice = Some(format!("No se pudo conectar: {msg}"));
                        }
                    }
                }
                self.render_lobby(ctx);
                self.render_modals(ctx);
            }
            ConnectionState::Connecting { ref address } => {
                self.focused_on_connect = false;
                self.release_remote_modifiers();
                self.kb_hook.set_capture(false, None);
                self.render_connecting(ctx, address);
            }
            ConnectionState::Connected {
                ref host_name,
                width,
                height,
            } => {
                self.render_remote_session(
                    ctx,
                    host_name,
                    width,
                    height,
                    &stats,
                    None,
                    &monitors,
                    active_monitor,
                    remote_cursor,
                    remote_dir.as_ref(),
                    &transfers,
                );
            }
            ConnectionState::Reconnecting {
                attempt,
                ref reason,
            } => {
                self.release_remote_modifiers();
                self.kb_hook.set_capture(false, None);
                self.render_remote_session(
                    ctx,
                    &self.quick_address.clone(),
                    0,
                    0,
                    &stats,
                    Some((attempt, reason.as_str())),
                    &monitors,
                    active_monitor,
                    remote_cursor,
                    remote_dir.as_ref(),
                    &transfers,
                );
            }
        }
    }
}

impl Drop for RandyDeskApp {
    fn drop(&mut self) {
        self.kb_hook.set_capture(false, None);
        self.net.disconnect();
        self.host_panel.stop_host();
    }
}

impl RandyDeskApp {
    fn render_lobby(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("este_equipo_panel")
            .resizable(false)
            .exact_width(272.0)
            .frame(
                egui::Frame::none()
                    .fill(BG_SIDEBAR)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(16.0, 14.0)),
            )
            .show(ctx, |ui| {
                self.host_panel.ui(ui, &self.rt_handle);
            });

        egui::TopBottomPanel::bottom("status_footer")
            .exact_height(38.0)
            .frame(
                egui::Frame::none()
                    .fill(BG_SIDEBAR)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(16.0, 7.0)),
            )
            .show(ctx, |ui| {
                let info = self.host_panel.runtime_info();
                let running = info.as_ref().is_some_and(|i| i.running);
                let relay_ok = info.as_ref().is_some_and(|i| i.relay_connected);
                let has_relay = info.as_ref().and_then(|i| i.relay_url.as_ref()).is_some();

                ui.horizontal(|ui| {
                    let dot_color = if relay_ok {
                        ACCENT_TEAL
                    } else if running {
                        STATUS_ORANGE
                    } else {
                        TEXT_MUTED
                    };
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 4.0, dot_color);

                    let state_txt = if relay_ok && info.as_ref().is_some_and(|i| i.is_system) {
                        "Listo · Internet (Relay E2E) y red local · SYSTEM"
                    } else if relay_ok {
                        "Listo · Internet (Relay E2E) y red local"
                    } else if has_relay {
                        "Conectando al servidor Relay..."
                    } else if running {
                        "Solo red local"
                    } else {
                        "Recepción detenida"
                    };
                    ui.label(
                        egui::RichText::new(state_txt)
                            .size(11.5)
                            .color(TEXT_SECONDARY),
                    );

                    ui.label(egui::RichText::new("|").size(11.5).color(BORDER_SUBTLE));

                    if ui
                        .link(
                            egui::RichText::new("Servidor Relay")
                                .size(11.5)
                                .color(ACCENT_BLUE),
                        )
                        .clicked()
                    {
                        self.relay_url_input = self.settings.relay_url.clone();
                        self.relay_modal_msg = None;
                        self.show_relay_modal = true;
                    }

                    ui.label(egui::RichText::new("|").size(11.5).color(BORDER_SUBTLE));

                    if ui
                        .link(
                            egui::RichText::new("Guía rápida")
                                .size(11.5)
                                .color(ACCENT_BLUE),
                        )
                        .clicked()
                    {
                        self.show_guide = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("RandyDesk v{}", env!("CARGO_PKG_VERSION")))
                                .size(11.5)
                                .color(TEXT_MUTED),
                        );

                        ui.label(egui::RichText::new("|").size(11.5).color(BORDER_SUBTLE));

                        match self.updater.state() {
                            UpdateState::Idle => {
                                if ui
                                    .link(
                                        egui::RichText::new("Buscar actualización")
                                            .size(11.5)
                                            .color(ACCENT_BLUE),
                                    )
                                    .clicked()
                                {
                                    self.updater.check_now(false);
                                }
                            }
                            UpdateState::Checking => {
                                ui.label(
                                    egui::RichText::new("Buscando...")
                                        .size(11.5)
                                        .color(TEXT_MUTED),
                                );
                            }
                            UpdateState::UpToDate => {
                                if ui
                                    .link(
                                        egui::RichText::new("Al día · Buscar")
                                            .size(11.5)
                                            .color(TEXT_SECONDARY),
                                    )
                                    .clicked()
                                {
                                    self.updater.check_now(false);
                                }
                            }
                            UpdateState::Available(info) => {
                                let label = format!("Actualizar a v{}", info.version);
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(label)
                                                .size(11.0)
                                                .strong()
                                                .color(Color32::WHITE),
                                        )
                                        .fill(ACCENT_TEAL_SOLID)
                                        .rounding(Rounding::same(6.0)),
                                    )
                                    .on_hover_text("Descargar y aplicar actualización desde GitHub")
                                    .clicked()
                                {
                                    self.updater.download_and_apply(
                                        info,
                                        self.host_panel.is_service_installed(),
                                    );
                                }
                            }
                            UpdateState::Downloading { version, percent } => {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Descargando v{version}... {percent}%"
                                    ))
                                    .size(11.5)
                                    .color(ACCENT_TEAL),
                                );
                            }
                            UpdateState::Applying { version } => {
                                ui.label(
                                    egui::RichText::new(format!("Aplicando v{version}..."))
                                        .size(11.5)
                                        .color(ACCENT_TEAL),
                                );
                            }
                            UpdateState::Error(msg) => {
                                if ui
                                    .link(
                                        egui::RichText::new("Error al actualizar (reintentar)")
                                            .size(11.5)
                                            .color(STATUS_ORANGE),
                                    )
                                    .on_hover_text(msg)
                                    .clicked()
                                {
                                    self.updater.check_now(false);
                                }
                            }
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG_MAIN)
                    .inner_margin(egui::Margin::symmetric(24.0, 20.0)),
            )
            .show(ctx, |ui| {
                let content_w = ui.available_width().min(1180.0);
                ui.set_max_width(content_w);

                // Tarjeta Hero Obsidian Craft "Controlar escritorio remoto"
                egui::Frame::none()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0_f32, BORDER_HOVER))
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::same(18.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Controlar escritorio remoto")
                                    .size(15.0)
                                    .strong()
                                    .color(TEXT_PRIMARY),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.menu_button("Ajustes", |ui| {
                                        ui.label("Calidad / Fluidez:");
                                        ui.add(
                                            egui::Slider::new(&mut self.quality_input, 35..=95)
                                                .text("Calidad %"),
                                        );
                                        ui.add(
                                            egui::Slider::new(&mut self.fps_input, 5..=60)
                                                .text("FPS"),
                                        );
                                    });

                                    egui::Frame::none()
                                        .fill(BG_INPUT)
                                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                                        .rounding(Rounding::same(8.0))
                                        .inner_margin(egui::Margin::same(3.0))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 3.0;
                                                for (kind, lbl) in [
                                                    (CodecKind::Lz4Rgba, "LZ4 Nítido"),
                                                    (CodecKind::Jpeg, "JPEG"),
                                                    (CodecKind::H264, "H.264"),
                                                ] {
                                                    let active = self.codec_input == kind;
                                                    let resp = ui.add(
                                                        egui::Button::new(
                                                            egui::RichText::new(lbl)
                                                                .size(11.0)
                                                                .color(if active {
                                                                    TEXT_PRIMARY
                                                                } else {
                                                                    TEXT_SECONDARY
                                                                }),
                                                        )
                                                        .fill(if active {
                                                            ACCENT_BLUE
                                                        } else {
                                                            Color32::TRANSPARENT
                                                        })
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(6.0))
                                                        .min_size(Vec2::new(48.0, 22.0)),
                                                    );
                                                    if resp.clicked() {
                                                        self.codec_input = kind;
                                                    }
                                                }
                                            });
                                        });
                                },
                            );
                        });

                        ui.add_space(12.0);
                        let mut pressed_enter = false;
                        let mut connect_clicked = false;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            let input_w = (ui.available_width() - 134.0).max(220.0);
                            let input_resp = styled_text_input(
                                ui,
                                &mut self.quick_address,
                                "Escribe el ID de 9 dígitos o IP local (ej. 482 910 331)...",
                                input_w,
                                42.0,
                                14.0,
                            );
                            pressed_enter = input_resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));

                            connect_clicked = ui
                                .add_sized(
                                    [122.0, 42.0],
                                    egui::Button::new(
                                        egui::RichText::new("Conectar")
                                            .size(13.5)
                                            .color(Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(ACCENT_BLUE)
                                    .rounding(Rounding::same(8.0)),
                                )
                                .clicked();
                        });

                        if connect_clicked || pressed_enter {
                            let addr = self.quick_address.clone();
                            self.connect_direct_address(&addr, ctx);
                        }
                    });

                if let Some(notice) = &self.ui_notice {
                    ui.add_space(8.0);
                    ui.colored_label(STATUS_ORANGE, notice);
                }

                ui.add_space(16.0);

                // Barra de pestañas segmentadas + Buscador + "+ Agregar"
                ui.horizontal(|ui| {
                    egui::Frame::none()
                        .fill(BG_CARD)
                        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::same(3.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (tab, lbl) in [
                                    (HostTab::Recent, "Recientes"),
                                    (HostTab::Favorites, "Favoritos"),
                                    (HostTab::All, "Todos"),
                                ] {
                                    let active = self.active_tab == tab;
                                    let btn = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(lbl).size(12.0).color(if active {
                                                TEXT_PRIMARY
                                            } else {
                                                TEXT_SECONDARY
                                            }),
                                        )
                                        .fill(if active {
                                            BG_SURFACE_ELEVATED
                                        } else {
                                            Color32::TRANSPARENT
                                        })
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(6.0))
                                        .min_size(Vec2::new(72.0, 26.0)),
                                    );
                                    if btn.clicked() {
                                        self.active_tab = tab;
                                    }
                                }
                            });
                        });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized(
                                [96.0, 34.0],
                                egui::Button::new(
                                    egui::RichText::new("+ Agregar")
                                        .size(12.5)
                                        .color(TEXT_PRIMARY),
                                )
                                .fill(BG_SURFACE_ELEVATED)
                                .stroke(Stroke::new(1.0_f32, BORDER_HOVER))
                                .rounding(Rounding::same(8.0)),
                            )
                            .clicked()
                        {
                            self.host_editor = Some(HostEditor {
                                editing_idx: None,
                                name: String::new(),
                                address: String::new(),
                                color_idx: (self.hosts.len() % 7) as u8,
                            });
                        }

                        styled_text_input(
                            ui,
                            &mut self.search_query,
                            "Buscar equipo...",
                            220.0,
                            34.0,
                            12.5,
                        );
                    });
                });

                ui.add_space(14.0);
                if let Some(act) =
                    render_host_cards(ui, &self.hosts, self.active_tab, &self.search_query)
                {
                    self.handle_card_action(act, ctx);
                }
            });
    }

    fn render_modals(&mut self, ctx: &egui::Context) {
        // 1. Modal de contraseña (solo si el equipo destino configuró contraseña opcional)
        let mut launch: Option<PasswordPrompt> = None;
        let mut close_prompt = false;
        if let Some(prompt) = &mut self.password_prompt {
            egui::Window::new(format!("Contraseña requerida — {}", prompt.title))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(320.0);
                    ui.label(
                        egui::RichText::new(format!("Destino: {}", display_address(&prompt.address)))
                            .small()
                            .color(TEXT_MUTED),
                    );
                    ui.add_space(8.0);
                    ui.label("Este equipo solicita contraseña opcional:");
                    let resp = ui.add_sized(
                        [310.0, 30.0],
                        egui::TextEdit::singleline(&mut prompt.password).password(true),
                    );
                    if prompt.just_opened {
                        resp.request_focus();
                        prompt.just_opened = false;
                    }
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let ok = ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Conectar").color(Color32::WHITE),
                                    )
                                    .fill(ACCENT_BLUE),
                                )
                                .clicked();
                            if (ok || enter) && !prompt.password.is_empty() {
                                launch = Some(prompt.clone());
                            }
                            if ui.button("Cancelar").clicked()
                                || ui.input(|i| i.key_pressed(egui::Key::Escape))
                            {
                                close_prompt = true;
                            }
                        });
                    });
                });
        }
        if close_prompt {
            self.password_prompt = None;
        }
        if let Some(prompt) = launch {
            self.password_prompt = None;
            self.launch_connection(prompt, ctx);
        }

        // 2. Modal Agregar / Editar equipo
        let mut save_editor = false;
        let mut close_editor = false;
        if let Some(ed) = &mut self.host_editor {
            let title = if ed.editing_idx.is_some() {
                "Editar equipo"
            } else {
                "Agregar equipo"
            };
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_min_width(330.0);
                    ui.label("Nombre:");
                    ui.add_sized([320.0, 28.0], egui::TextEdit::singleline(&mut ed.name));
                    ui.add_space(6.0);
                    ui.label("ID de 9 dígitos o IP local:");
                    ui.add_sized(
                        [320.0, 28.0],
                        egui::TextEdit::singleline(&mut ed.address)
                            .hint_text("482 910 331 o 192.168.1.25"),
                    );
                    ui.add_space(8.0);
                    ui.label("Color de tarjeta:");
                    ui.horizontal(|ui| {
                        for (i, &c) in BANNER_COLORS.iter().enumerate() {
                            let selected = ed.color_idx as usize == i;
                            let (rect, resp) =
                                ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::click());
                            ui.painter().rect_filled(rect, Rounding::same(4.0), c);
                            if selected {
                                ui.painter().rect_stroke(
                                    rect,
                                    Rounding::same(4.0),
                                    Stroke::new(2.0_f32, Color32::WHITE),
                                );
                            }
                            if resp.clicked() {
                                ed.color_idx = i as u8;
                            }
                        }
                    });

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Guardar").color(Color32::WHITE),
                                    )
                                    .fill(ACCENT_BLUE),
                                )
                                .clicked()
                                && !ed.address.trim().is_empty()
                            {
                                save_editor = true;
                            }
                            if ui.button("Cancelar").clicked() {
                                close_editor = true;
                            }
                        });
                    });
                });
        }
        if save_editor {
            if let Some(ed) = self.host_editor.take() {
                let normalized = normalize_address(&ed.address);
                let name = if ed.name.trim().is_empty() {
                    display_address(&normalized)
                } else {
                    ed.name.trim().to_string()
                };
                if let Some(idx) = ed.editing_idx {
                    if let Some(h) = self.hosts.get_mut(idx) {
                        h.name = name;
                        h.address = normalized;
                        h.color_idx = ed.color_idx;
                    }
                } else {
                    let mut h = SavedHost::new(name, normalized);
                    h.color_idx = ed.color_idx;
                    self.hosts.push(h);
                }
                let _ = save_hosts(&self.hosts);
            }
        } else if close_editor {
            self.host_editor = None;
        }

        // 3. Modal Configurar servidor Relay
        if self.show_relay_modal {
            let mut open = true;
            let mut apply_relay: Option<String> = None;
            egui::Window::new("Configurar servidor Relay (Internet)")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_max_width(430.0);
                    ui.label(
                        egui::RichText::new(
                            "RandyDesk ya incluye su servidor por defecto. Todo el tráfico viaja cifrado E2E.",
                        )
                        .color(TEXT_MUTED),
                    );
                    ui.add_space(8.0);
                    ui.add_sized(
                        [410.0, 30.0],
                        egui::TextEdit::singleline(&mut self.relay_url_input)
                            .hint_text("wss://randydesk-relay.onrender.com"),
                    );
                    if let Some(msg) = &self.relay_modal_msg {
                        ui.add_space(6.0);
                        ui.colored_label(ACCENT_TEAL, msg);
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Guardar y conectar").color(Color32::WHITE),
                                )
                                .fill(ACCENT_BLUE),
                            )
                            .clicked()
                        {
                            apply_relay = Some(self.relay_url_input.trim().to_string());
                        }
                        if ui.button("Usar por defecto").clicked() {
                            self.relay_url_input = DEFAULT_RELAY_URL.to_string();
                            apply_relay = Some(DEFAULT_RELAY_URL.to_string());
                        }
                        if ui.button("Solo red local").clicked() {
                            self.relay_url_input.clear();
                            apply_relay = Some(String::new());
                        }
                    });
                });

            if let Some(raw) = apply_relay {
                let normalized = normalize_relay_ws_url(&raw).unwrap_or_default();
                self.settings.relay_disabled = normalized.is_empty();
                self.settings.relay_url = normalized.clone();
                self.relay_url_input = normalized;
                match save_settings(&self.settings) {
                    Ok(()) => {
                        self.host_panel
                            .set_relay_url(self.settings.normalized_relay_url(), &self.rt_handle);
                        self.relay_modal_msg = Some(if self.settings.relay_disabled {
                            "Relay desactivado (modo solo red local).".into()
                        } else {
                            "Servidor Relay guardado y conectando...".into()
                        });
                    }
                    Err(err) => self.relay_modal_msg = Some(format!("Error guardando: {err}")),
                }
            }
            if !open {
                self.show_relay_modal = false;
            }
        }

        // 4. Modal Guía rápida
        if self.show_guide {
            let mut open = true;
            egui::Window::new("Guía rápida — Conexión ilimitada")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.set_max_width(430.0);
                    ui.label("1. Conexión directa sin contraseña:");
                    ui.label(
                        egui::RichText::new(
                            "Escribe el ID de 9 dígitos del otro equipo y pulsa Conectar (o haz clic en su tarjeta).",
                        )
                        .color(TEXT_MUTED),
                    );
                    ui.add_space(8.0);
                    ui.label("2. Bandeja del sistema:");
                    ui.label(
                        egui::RichText::new(
                            "Al minimizar la ventana queda en la bandeja de Windows (junto al reloj) manteniendo tu conexión activa.",
                        )
                        .color(TEXT_MUTED),
                    );
                    ui.add_space(8.0);
                    ui.label("3. Tecla Windows y nitidez:");
                    ui.label(
                        egui::RichText::new(
                            "Con el puntero sobre el escritorio remoto, la tecla Win y Alt+Tab actúan directo en la otra PC. Si ves texto pequeño en un IDE, prueba modo 'Nítido'.",
                        )
                        .color(TEXT_MUTED),
                    );
                });
            if !open {
                self.show_guide = false;
            }
        }
    }

    fn render_connecting(&mut self, ctx: &egui::Context, address: &str) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.add_space(10.0);
                    ui.heading(format!("Conectando a {}...", display_address(address)));
                    ui.add_space(10.0);
                    if ui.button("Cancelar").clicked() {
                        self.net.disconnect();
                    }
                });
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_remote_session(
        &mut self,
        ctx: &egui::Context,
        host_name: &str,
        width: u32,
        height: u32,
        stats: &crate::net::SessionStats,
        reconnecting: Option<(u32, &str)>,
        monitors: &[MonitorInfo],
        active_monitor: u8,
        remote_cursor: CursorKind,
        remote_dir: Option<&RemoteDirListing>,
        transfers: &[TransferItem],
    ) {
        let mut img_rect = egui::Rect::NOTHING;
        let mut pointer_in_canvas = false;
        let mut has_texture = false;

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::BLACK))
            .show(ctx, |ui| {
                let Some(tex) = &self.screen_texture else {
                    self.kb_hook.set_capture(false, None);
                    ui.centered_and_justified(|ui| {
                        ui.label("Esperando primer cuadro del equipo remoto...");
                    });
                    return;
                };
                has_texture = true;

                let tex_size = tex.size_vec2();
                match self.scale_mode {
                    ScaleMode::Adaptive => {
                        let avail = ui.available_size();
                        let scale =
                            (avail.x / tex_size.x.max(1.0)).min(avail.y / tex_size.y.max(1.0));
                        let draw_size = tex_size * scale;
                        let (rect, response) =
                            ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
                        img_rect = egui::Rect::from_center_size(rect.center(), draw_size);
                        pointer_in_canvas = response.hovered() || response.dragged();
                        ui.painter().image(
                            tex.id(),
                            img_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    ScaleMode::Original => {
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let size = tex_size.max(ui.available_size());
                                let (rect, response) =
                                    ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                                img_rect = egui::Rect::from_center_size(rect.center(), tex_size);
                                pointer_in_canvas = response.hovered() || response.dragged();
                                ui.painter().image(
                                    tex.id(),
                                    img_rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    Color32::WHITE,
                                );
                            });
                    }
                    ScaleMode::Custom(pct) => {
                        let factor = (pct / 100.0).clamp(0.5, 2.0);
                        let draw_size = tex_size * factor;
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let size = draw_size.max(ui.available_size());
                                let (rect, response) =
                                    ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                                img_rect = egui::Rect::from_center_size(rect.center(), draw_size);
                                pointer_in_canvas = response.hovered() || response.dragged();
                                ui.painter().image(
                                    tex.id(),
                                    img_rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    Color32::WHITE,
                                );
                            });
                    }
                }
            });

        if has_texture && pointer_in_canvas {
            if self.view_only {
                ctx.set_cursor_icon(egui::CursorIcon::NotAllowed);
            } else if !self.show_remote_cursor {
                ctx.set_cursor_icon(egui::CursorIcon::None);
            } else {
                ctx.set_cursor_icon(map_remote_cursor(remote_cursor));
            }
        }

        let mut bar_state = SessionBarState {
            codec: self.codec_input,
            quality: self.quality_input,
            fps: self.fps_input,
            scale_mode: self.scale_mode,
            custom_scale_pct: self.custom_scale_pct,
            stream_scale: self.stream_scale,
            quality_preset: self.quality_preset,
            show_monitor_stats: self.show_monitor_stats,
            show_remote_cursor: self.show_remote_cursor,
            fullscreen: self.fullscreen,
            keyboard_grab_enabled: self.keyboard_grab_enabled,
            keyboard_mode: self.keyboard_mode,
            invert_wheel: self.invert_wheel,
            swap_mouse_buttons: self.swap_mouse_buttons,
            view_only: self.view_only,
            bar_collapsed: self.bar_collapsed,
            bar_pinned: self.bar_pinned,
            auto_clipboard: self.auto_clipboard,
            input_debug_log: self.input_debug_log,
            audio_enabled: self.audio_enabled,
            audio_volume: self.audio_volume,
            show_file_manager: self.show_file_manager,
        };
        let bar_actions = render_session_overlay(
            ctx,
            SessionBarInput {
                host_name,
                width,
                height,
                stats,
                reconnecting,
                hook_installed: self.kb_hook.is_installed(),
                hook_hits: self.kb_hook.sent_key_count(),
                monitors,
                active_monitor,
            },
            &mut bar_state,
        );

        self.codec_input = bar_state.codec;
        self.quality_input = bar_state.quality;
        self.fps_input = bar_state.fps;
        self.scale_mode = bar_state.scale_mode;
        self.custom_scale_pct = bar_state.custom_scale_pct;
        self.stream_scale = bar_state.stream_scale;
        self.quality_preset = bar_state.quality_preset;
        self.show_monitor_stats = bar_state.show_monitor_stats;
        self.show_remote_cursor = bar_state.show_remote_cursor;
        self.fullscreen = bar_state.fullscreen;
        self.keyboard_grab_enabled = bar_state.keyboard_grab_enabled;
        self.keyboard_mode = bar_state.keyboard_mode;
        self.invert_wheel = bar_state.invert_wheel;
        self.swap_mouse_buttons = bar_state.swap_mouse_buttons;
        self.view_only = bar_state.view_only;
        self.bar_collapsed = bar_state.bar_collapsed;
        self.bar_pinned = bar_state.bar_pinned;
        self.auto_clipboard = bar_state.auto_clipboard;
        self.input_debug_log = bar_state.input_debug_log;
        self.audio_enabled = bar_state.audio_enabled;
        self.audio_volume = bar_state.audio_volume;

        if bar_actions.settings_changed {
            self.net.send_cmd(NetCommand::UpdateSettings(StreamSettings {
                codec: self.codec_input,
                jpeg_quality: self.quality_input,
                target_fps: self.fps_input,
            }));
        }
        if bar_actions.stream_scale_changed {
            self.net
                .send_cmd(NetCommand::SetStreamScale(self.stream_scale));
        }
        if let Some(mon_idx) = bar_actions.select_monitor {
            self.net.send_cmd(NetCommand::SelectMonitor(mon_idx));
        }
        if bar_actions.debug_log_changed {
            set_input_debug_logging(self.input_debug_log);
        }
        if bar_actions.audio_changed {
            self.net.set_audio_muted(!self.audio_enabled);
            self.net.set_audio_volume(self.audio_volume);
            self.net
                .send_cmd(NetCommand::SetAudioEnabled(self.audio_enabled));
        }
        if bar_actions.toggle_file_manager {
            self.show_file_manager = !self.show_file_manager;
            if self.show_file_manager && remote_dir.is_none() {
                self.net.send_cmd(NetCommand::FileListReq(String::new()));
            }
        }
        if bar_actions.lock_remote_screen {
            self.net.send_cmd(NetCommand::LockWorkStation);
        }
        if bar_actions.send_sas {
            self.net.send_cmd(NetCommand::SendSas);
        }
        if bar_actions.send_win_key {
            if let Some(tx) = self.net.cmd_sender() {
                self.rt_handle.spawn(async move {
                    let _ = tx
                        .send(NetCommand::Input(InputEvent::Key {
                            vk: 0x5B,
                            scan: 0x5B,
                            extended: true,
                            down: true,
                        }))
                        .await;
                    tokio::time::sleep(Duration::from_millis(35)).await;
                    let _ = tx
                        .send(NetCommand::Input(InputEvent::Key {
                            vk: 0x5B,
                            scan: 0x5B,
                            extended: true,
                            down: false,
                        }))
                        .await;
                });
            }
        }
        if bar_actions.send_task_manager {
            if let Some(tx) = self.net.cmd_sender() {
                self.rt_handle.spawn(async move {
                    for (vk, scan) in [(0xA2u16, 0x1Du16), (0xA0, 0x2A), (0x1B, 0x01)] {
                        let _ = tx
                            .send(NetCommand::Input(InputEvent::Key {
                                vk,
                                scan,
                                extended: false,
                                down: true,
                            }))
                            .await;
                    }
                    tokio::time::sleep(Duration::from_millis(35)).await;
                    for (vk, scan) in [(0x1Bu16, 0x01u16), (0xA0, 0x2A), (0xA2, 0x1D)] {
                        let _ = tx
                            .send(NetCommand::Input(InputEvent::Key {
                                vk,
                                scan,
                                extended: false,
                                down: false,
                            }))
                            .await;
                    }
                });
            }
        }
        if bar_actions.send_clipboard {
            if let Ok(text) = Clipboard::new().and_then(|mut c| c.get_text()) {
                self.clip_watcher.mark_applied(&text);
                self.net.send_cmd(NetCommand::ClipboardSet(text));
            }
        }
        if bar_actions.fetch_clipboard {
            self.net.send_cmd(NetCommand::ClipboardGet);
        }
        if bar_actions.request_keyframe {
            self.net.send_cmd(NetCommand::RequestKeyframe);
        }
        if bar_actions.toggle_fullscreen {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if bar_actions.keyboard_cfg_changed {
            self.release_remote_modifiers();
            self.kb_hook.release_modifiers_now();
            self.kb_hook.set_mode(self.keyboard_mode);
            if !self.keyboard_grab_enabled || self.view_only {
                self.kb_hook.set_capture(false, None);
            } else {
                if let Some(tray) = self.tray.as_ref() {
                    tray.ensure_foreground();
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        if bar_actions.disconnect {
            if self.fullscreen {
                self.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
            self.show_file_manager = false;
            self.focused_on_connect = false;
            self.release_remote_modifiers();
            self.kb_hook.set_capture(false, None);
            self.net.disconnect();
            return;
        }

        // Arrastrar y soltar archivos locales sobre la ventana para subirlos al equipo remoto
        if !self.view_only {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            if !dropped.is_empty() {
                let target_remote = self.file_mgr_ui.current_remote_dir(remote_dir);
                let mut uploaded_any = false;
                for file in dropped {
                    if let Some(path) = file.path {
                        if path.is_file() {
                            let id = self.next_transfer_id;
                            self.next_transfer_id += 1;
                            self.net.send_cmd(NetCommand::UploadLocalFile {
                                transfer_id: id,
                                local_path: path,
                                remote_dir: target_remote.clone(),
                            });
                            uploaded_any = true;
                        }
                    }
                }
                if uploaded_any {
                    self.show_file_manager = true;
                    if remote_dir.is_none() {
                        self.net.send_cmd(NetCommand::FileListReq(String::new()));
                    }
                }
            }
        }

        let fm_actions = self.file_mgr_ui.show(
            ctx,
            &mut self.show_file_manager,
            remote_dir,
            transfers,
        );
        for act in fm_actions {
            match act {
                FileManagerAction::ListRemote(path) => {
                    self.net.send_cmd(NetCommand::FileListReq(path));
                }
                FileManagerAction::MkdirRemote { parent, name } => {
                    self.net
                        .send_cmd(NetCommand::FileMkdir { parent_dir: parent, name });
                }
                FileManagerAction::UploadFile {
                    local_path,
                    remote_dir,
                } => {
                    let id = self.next_transfer_id;
                    self.next_transfer_id += 1;
                    self.net.send_cmd(NetCommand::UploadLocalFile {
                        transfer_id: id,
                        local_path,
                        remote_dir,
                    });
                }
                FileManagerAction::DownloadFile {
                    remote_path,
                    local_dir,
                } => {
                    let id = self.next_transfer_id;
                    self.next_transfer_id += 1;
                    self.net.send_cmd(NetCommand::DownloadRemoteFile {
                        transfer_id: id,
                        remote_path,
                        local_dir,
                    });
                }
                FileManagerAction::CancelTransfer(id) => {
                    self.net.send_cmd(NetCommand::CancelTransfer(id));
                }
                FileManagerAction::ClearCompleted => {
                    if let Ok(mut g) = self.net.shared.lock() {
                        g.transfers
                            .retain(|t| matches!(t.status, TransferStatus::InProgress));
                    }
                }
            }
        }

        if !has_texture {
            return;
        }

        if !self.focused_on_connect && reconnecting.is_none() && !self.hidden_to_tray {
            self.focused_on_connect = true;
            if let Some(tray) = self.tray.as_ref() {
                tray.ensure_foreground();
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        let egui_focused = ctx.input(|i| i.focused);
        self.kb_hook.set_window_focused(egui_focused);
        let is_fg = KeyboardHookGuard::is_window_foreground();
        let typing_in_local_ui = ctx.wants_keyboard_input();
        let should_capture_kb = reconnecting.is_none()
            && self.keyboard_grab_enabled
            && !self.hidden_to_tray
            && !self.view_only
            && !typing_in_local_ui;
        if self.was_foreground && !is_fg {
            self.release_remote_modifiers();
            if should_capture_kb {
                self.kb_hook.release_modifiers_now();
            }
        }
        self.was_foreground = is_fg;

        self.kb_hook.set_mode(self.keyboard_mode);
        self.kb_hook.set_capture(
            should_capture_kb,
            if should_capture_kb {
                self.net.cmd_sender()
            } else {
                None
            },
        );

        if self.view_only || typing_in_local_ui {
            self.release_remote_modifiers();
            return;
        }

        let hook_blocking_egui = self.kb_hook.is_capturing_all_keys();

        // Sincroniza Ctrl/Alt/Shift con la ventana remota para atajos en Modo traducción (sin AltGr)
        if !hook_blocking_egui && is_fg && reconnecting.is_none() {
            let mods = ctx.input(|i| i.modifiers);
            let altgr = mods.ctrl && mods.alt && KeyboardHookGuard::is_altgr_down();
            let eff_ctrl = mods.ctrl && !altgr;
            let eff_alt = mods.alt && !altgr;
            if eff_ctrl != self.remote_ctrl_down {
                self.net.send_input(InputEvent::Key {
                    vk: 0xA2,
                    scan: 0x1D,
                    extended: false,
                    down: eff_ctrl,
                });
                self.remote_ctrl_down = eff_ctrl;
            }
            if eff_alt != self.remote_alt_down {
                self.net.send_input(InputEvent::Key {
                    vk: 0xA4,
                    scan: 0x38,
                    extended: false,
                    down: eff_alt,
                });
                self.remote_alt_down = eff_alt;
            }
            if mods.shift != self.remote_shift_down {
                self.net.send_input(InputEvent::Key {
                    vk: 0xA0,
                    scan: 0x2A,
                    extended: false,
                    down: mods.shift,
                });
                self.remote_shift_down = mods.shift;
            }
        } else {
            self.release_remote_modifiers();
        }

        let events = ctx.input(|i| i.events.clone());

        for ev in events {
            match ev {
                egui::Event::PointerMoved(pos) if pointer_in_canvas => {
                    if let Some((x_norm, y_norm)) = normalize_pos(pos, img_rect) {
                        self.net
                            .send_input(InputEvent::MouseMove { x_norm, y_norm });
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } if (pointer_in_canvas && img_rect.contains(pos)) || !pressed => {
                    if pressed && pointer_in_canvas && img_rect.contains(pos) {
                        if let Some(tray) = self.tray.as_ref() {
                            tray.ensure_foreground();
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    if let Some((x_norm, y_norm)) = normalize_pos(pos, img_rect) {
                        self.net
                            .send_input(InputEvent::MouseMove { x_norm, y_norm });
                    }
                    let mapped_btn = if self.swap_mouse_buttons {
                        match button {
                            egui::PointerButton::Primary => egui::PointerButton::Secondary,
                            egui::PointerButton::Secondary => egui::PointerButton::Primary,
                            other => other,
                        }
                    } else {
                        button
                    };
                    if let Some(mb) = map_mouse_button(mapped_btn) {
                        self.net.send_input(InputEvent::MouseButton {
                            button: mb,
                            down: pressed,
                        });
                    }
                }
                egui::Event::MouseWheel { delta, .. } if pointer_in_canvas => {
                    let sign = if self.invert_wheel { -1 } else { 1 };
                    let dx = (delta.x * 3.0).round() as i16 * 40 * sign;
                    let dy = (delta.y * 3.0).round() as i16 * 40 * sign;
                    if dx != 0 || dy != 0 {
                        self.net.send_input(InputEvent::MouseWheel {
                            delta_x: dx,
                            delta_y: dy,
                        });
                    }
                }
                egui::Event::Copy if !hook_blocking_egui => {
                    append_input_debug_log("[CLIENT-EGUI] shortcut Copy (Ctrl+C)");
                    self.send_remote_ctrl_shortcut(0x43, 0x2E);
                }
                egui::Event::Cut if !hook_blocking_egui => {
                    append_input_debug_log("[CLIENT-EGUI] shortcut Cut (Ctrl+X)");
                    self.send_remote_ctrl_shortcut(0x58, 0x2D);
                }
                egui::Event::Paste(_) if !hook_blocking_egui => {
                    append_input_debug_log("[CLIENT-EGUI] shortcut Paste (Ctrl+V)");
                    self.send_remote_ctrl_shortcut(0x56, 0x2F);
                }
                egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } if !hook_blocking_egui => {
                    let altgr = modifiers.ctrl && modifiers.alt && KeyboardHookGuard::is_altgr_down();
                    let is_combo = (modifiers.ctrl || modifiers.alt || modifiers.command) && !altgr;
                    if is_combo || is_control_or_nav_key(key) {
                        if let Some((vk, extended)) = map_egui_key_to_vk(key) {
                            append_input_debug_log(&format!(
                                "[CLIENT-EGUI] key vk=0x{vk:04X} ext={extended} down={pressed} combo={is_combo}"
                            ));
                            self.net.send_input(InputEvent::Key {
                                vk,
                                scan: 0,
                                extended,
                                down: pressed,
                            });
                        }
                    }
                }
                egui::Event::Text(text) if !hook_blocking_egui => {
                    let mods = ctx.input(|i| i.modifiers);
                    let altgr = mods.ctrl && mods.alt && KeyboardHookGuard::is_altgr_down();
                    if (!mods.ctrl && !mods.alt && !mods.command) || altgr {
                        if altgr {
                            self.release_remote_modifiers();
                        }
                        let chars: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
                        if !chars.is_empty() {
                            append_input_debug_log(&format!(
                                "[CLIENT-EGUI] text count={} altgr={altgr}",
                                chars.len()
                            ));
                        }
                        for ch in chars {
                            self.net
                                .send_input(InputEvent::UnicodeChar { ch, down: true });
                            self.net
                                .send_input(InputEvent::UnicodeChar { ch, down: false });
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
