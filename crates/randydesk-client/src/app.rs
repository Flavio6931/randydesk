//! Interfaz gráfica unificada de RandyDesk (bandeja, hook tecla Win, multi-sesión por pestañas y GPU parcial).

use crate::cards::{render_host_cards, CardAction, HostTab};
use crate::config::{
    display_address, load_host_thumbnail, load_hosts, load_settings, normalize_address, save_hosts,
    save_settings, upsert_recent, AppSettings, SavedHost,
};
use crate::host_panel::HostPanelState;
use crate::net::NetCommand;
use crate::presence::PresenceTracker;
use crate::session_bar::QualityPreset;
use crate::session_tab::{RemoteSessionTab, SessionTabOutcome, TabLaunchParams, TabPollSnapshot};
use crate::theme::{
    apply_rustdesk_theme, styled_text_input, ACCENT_BLUE, ACCENT_TEAL, ACCENT_TEAL_SOLID,
    BANNER_COLORS, BG_CARD, BG_INPUT, BG_MAIN, BG_SIDEBAR, BG_SURFACE_ELEVATED, BORDER_HOVER,
    BORDER_SUBTLE, STATUS_ORANGE, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};
use crate::tray::TrayController;
use crate::updater::{UpdateState, UpdaterHandle};
use crate::win_hook::{KeyboardHookGuard, KeyboardMode};
use eframe::egui::{self, Color32, Rounding, Stroke, Vec2};
use randydesk_proto::{normalize_relay_ws_url, CodecKind, DEFAULT_RELAY_URL};
use std::collections::HashMap;
use std::time::{Duration, Instant};

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
    retry_tab_id: Option<u64>,
}

/// Datos temporales del modal para agregar o editar un equipo guardado.
struct HostEditor {
    editing_idx: Option<usize>,
    name: String,
    address: String,
    color_idx: u8,
}

/// Aplicación principal de RandyDesk (anfitrión + visor multi-sesión por pestañas con bandeja).
pub struct RandyDeskApp {
    rt_handle: tokio::runtime::Handle,
    host_panel: HostPanelState,
    hosts: Vec<SavedHost>,
    settings: AppSettings,
    tray: Option<TrayController>,
    updater: UpdaterHandle,
    kb_hook: KeyboardHookGuard,
    presence: PresenceTracker,
    thumbnails: HashMap<String, egui::TextureHandle>,

    // Multi-sesión por pestañas (`None` = vista Lobby / Inicio)
    tabs: Vec<RemoteSessionTab>,
    active_session_tab: Option<usize>,
    next_tab_id: u64,

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

    // Preferencias por defecto aplicadas a cada nueva pestaña
    stream_scale: u8,
    quality_preset: QualityPreset,
    show_remote_cursor: bool,
    keyboard_grab_enabled: bool,
    keyboard_mode: KeyboardMode,
    invert_wheel: bool,
    swap_mouse_buttons: bool,
    auto_clipboard: bool,
    audio_enabled: bool,
    audio_volume: u8,
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
        let presence = PresenceTracker::new(rt_handle.clone());

        Self {
            rt_handle,
            host_panel,
            hosts,
            settings,
            tray,
            updater,
            kb_hook,
            presence,
            thumbnails: HashMap::new(),
            tabs: Vec::new(),
            active_session_tab: None,
            next_tab_id: 1,
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
            stream_scale: 100,
            quality_preset: QualityPreset::Balanced,
            show_remote_cursor: true,
            keyboard_grab_enabled: true,
            keyboard_mode: KeyboardMode::default(),
            invert_wheel: false,
            swap_mouse_buttons: false,
            auto_clipboard: true,
            audio_enabled: true,
            audio_volume: 100,
        }
    }

    fn sync_thumbnails(&mut self, ctx: &egui::Context) {
        for host in &self.hosts {
            let norm = normalize_address(&host.address);
            if self.thumbnails.contains_key(&norm) {
                continue;
            }
            if let Some(img) = load_host_thumbnail(&norm) {
                let tex = ctx.load_texture(
                    format!("thumb-{norm}"),
                    img,
                    egui::TextureOptions::LINEAR,
                );
                self.thumbnails.insert(norm, tex);
            }
        }
    }

    fn refresh_thumbnail(&mut self, norm_addr: &str, ctx: &egui::Context) {
        if let Some(img) = load_host_thumbnail(norm_addr) {
            let tex = ctx.load_texture(
                format!("thumb-{norm_addr}"),
                img,
                egui::TextureOptions::LINEAR,
            );
            self.thumbnails.insert(norm_addr.to_string(), tex);
        }
    }

    fn release_all_tabs_modifiers(&mut self) {
        for tab in &mut self.tabs {
            tab.release_remote_modifiers();
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
            retry_tab_id: None,
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
            retry_tab_id: None,
        };
        self.launch_connection(attempt, ctx);
    }

    fn launch_connection(&mut self, prompt: PasswordPrompt, ctx: &egui::Context) {
        self.ui_notice = None;
        self.last_attempt = Some(prompt.clone());
        self.release_all_tabs_modifiers();
        self.kb_hook.set_capture(false, None);

        if let Some(tab_id) = prompt.retry_tab_id {
            if let Some(pos) = self.tabs.iter().position(|t| t.id == tab_id) {
                self.tabs[pos].reconnect_with_password(
                    prompt.password,
                    self.settings.normalized_relay_url(),
                    ctx,
                );
                self.active_session_tab = Some(pos);
                return;
            }
        }

        let norm = normalize_address(&prompt.address);
        if let Some(existing_idx) = self
            .tabs
            .iter()
            .position(|t| normalize_address(&t.target_address) == norm)
        {
            self.active_session_tab = Some(existing_idx);
            return;
        }

        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = RemoteSessionTab::launch(
            TabLaunchParams {
                id: tab_id,
                title: prompt.title,
                address: prompt.address,
                password: prompt.password,
                expected_fp: prompt.expected_fp,
                relay_url: self.settings.normalized_relay_url(),
                codec: prompt.codec,
                quality: prompt.quality,
                fps: prompt.fps,
                stream_scale: self.stream_scale,
                quality_preset: self.quality_preset,
                show_remote_cursor: self.show_remote_cursor,
                keyboard_grab_enabled: self.keyboard_grab_enabled,
                keyboard_mode: self.keyboard_mode,
                invert_wheel: self.invert_wheel,
                swap_mouse_buttons: self.swap_mouse_buttons,
                auto_clipboard: self.auto_clipboard,
                audio_enabled: self.audio_enabled,
                audio_volume: self.audio_volume,
            },
            &self.rt_handle,
            ctx,
        );
        self.tabs.push(tab);
        self.active_session_tab = Some(self.tabs.len() - 1);
    }

    fn close_tab(&mut self, idx: usize, ctx: &egui::Context) {
        if idx >= self.tabs.len() {
            return;
        }
        self.kb_hook.set_capture(false, None);
        let mut removed = self.tabs.remove(idx);
        if let Some(addr) = removed.close_and_save(ctx) {
            self.refresh_thumbnail(&addr, ctx);
        }
        if self.tabs.is_empty() {
            self.active_session_tab = None;
        } else if let Some(active) = self.active_session_tab {
            if active == idx {
                self.active_session_tab = if idx > 0 { Some(idx - 1) } else { Some(0) };
            } else if active > idx {
                self.active_session_tab = Some(active - 1);
            }
        }
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

    fn render_tab_strip(&mut self, ctx: &egui::Context) {
        if self.tabs.is_empty() {
            return;
        }
        if let Some(active) = self.active_session_tab {
            if self.tabs.get(active).is_some_and(|t| t.fullscreen) {
                return;
            }
        }

        let mut next_active = self.active_session_tab;
        let mut close_idx: Option<usize> = None;

        egui::TopBottomPanel::top("session_tabs_bar")
            .exact_height(34.0)
            .frame(
                egui::Frame::none()
                    .fill(BG_SIDEBAR)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let lobby_active = self.active_session_tab.is_none();
                    let lobby_btn = egui::Button::new(
                        egui::RichText::new("Inicio").size(12.0).color(if lobby_active {
                            Color32::WHITE
                        } else {
                            TEXT_SECONDARY
                        }),
                    )
                    .fill(if lobby_active { ACCENT_BLUE } else { BG_CARD })
                    .stroke(Stroke::new(
                        1.0_f32,
                        if lobby_active { ACCENT_BLUE } else { BORDER_SUBTLE },
                    ))
                    .rounding(Rounding::same(6.0))
                    .min_size(Vec2::new(58.0, 24.0));
                    if ui.add(lobby_btn).clicked() {
                        next_active = None;
                    }

                    egui::ScrollArea::horizontal()
                        .id_salt("tabs_scroll")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for (idx, tab) in self.tabs.iter().enumerate() {
                                    let selected = self.active_session_tab == Some(idx);
                                    let bg = if selected {
                                        BG_SURFACE_ELEVATED
                                    } else {
                                        BG_CARD
                                    };
                                    let border = if selected {
                                        ACCENT_BLUE
                                    } else {
                                        BORDER_SUBTLE
                                    };
                                    egui::Frame::none()
                                        .fill(bg)
                                        .stroke(Stroke::new(1.0_f32, border))
                                        .rounding(Rounding::same(6.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                let (dot_r, _) = ui.allocate_exact_size(
                                                    Vec2::splat(8.0),
                                                    egui::Sense::hover(),
                                                );
                                                ui.painter().circle_filled(
                                                    dot_r.center(),
                                                    3.5,
                                                    tab.tab_dot_color(),
                                                );
                                                let short_title = if tab.title.chars().count() > 18
                                                {
                                                    let head: String =
                                                        tab.title.chars().take(17).collect();
                                                    format!("{head}...")
                                                } else {
                                                    tab.title.clone()
                                                };
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new(short_title)
                                                                .size(12.0)
                                                                .color(if selected {
                                                                    TEXT_PRIMARY
                                                                } else {
                                                                    TEXT_SECONDARY
                                                                }),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .clicked()
                                                {
                                                    next_active = Some(idx);
                                                }
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new("x")
                                                                .size(11.5)
                                                                .color(TEXT_MUTED),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .on_hover_text("Cerrar sesion")
                                                    .clicked()
                                                {
                                                    close_idx = Some(idx);
                                                }
                                            });
                                        });
                                }
                            });
                        });
                });
            });

        if next_active != self.active_session_tab {
            self.release_all_tabs_modifiers();
            self.kb_hook.set_capture(false, None);
            self.active_session_tab = next_active;
        }
        if let Some(idx) = close_idx {
            self.close_tab(idx, ctx);
        }
    }
}

impl eframe::App for RandyDeskApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.updater.take_exit_requested() {
            self.kb_hook.set_capture(false, None);
            for mut tab in self.tabs.drain(..) {
                let _ = tab.close_and_save(ctx);
            }
            self.host_panel.stop_host();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // 1. Gestión de minimizar / restaurar desde la bandeja del sistema
        if let Some(tray) = self.tray.as_ref() {
            if tray.take_restore_requested() {
                self.hidden_to_tray = false;
                self.restored_at = Instant::now();
                if let Some(active) = self.active_session_tab {
                    if let Some(tab) = self.tabs.get(active) {
                        tab.net.send_cmd(NetCommand::RequestKeyframe);
                    }
                }
            } else if !self.hidden_to_tray {
                tray.cache_main_hwnd();
                if self.restored_at.elapsed() >= Duration::from_millis(450)
                    && tray.is_window_minimized()
                    && tray.hide_to_tray()
                {
                    self.hidden_to_tray = true;
                    self.release_all_tabs_modifiers();
                    self.kb_hook.set_capture(false, None);
                }
            }
        }

        // 2. Presencia online y miniaturas cuando estamos en Lobby (o periodicamente)
        let addresses: Vec<String> = self.hosts.iter().map(|h| h.address.clone()).collect();
        self.presence.maybe_refresh(
            self.settings.normalized_relay_url().as_deref(),
            &addresses,
            ctx,
        );
        self.sync_thumbnails(ctx);

        // 3. Sondeo de todas las pestañas abiertas
        let active_idx = self.active_session_tab;
        let mut snapshots: Vec<TabPollSnapshot> = Vec::with_capacity(self.tabs.len());
        let mut refreshed_thumbs: Vec<String> = Vec::new();
        let mut recent_updates: Vec<(usize, String, Option<String>)> = Vec::new();
        let mut auth_tab: Option<usize> = None;

        for (idx, tab) in self.tabs.iter_mut().enumerate() {
            let is_active = active_idx == Some(idx);
            let snap = tab.poll(ctx, is_active, self.hidden_to_tray);
            if let Some(addr) = &snap.thumb_updated_addr {
                refreshed_thumbs.push(addr.clone());
            }
            if let Some((host_name, fp)) = &snap.need_recent_upsert {
                recent_updates.push((idx, host_name.clone(), fp.clone()));
            }
            if snap.auth_prompt_needed && auth_tab.is_none() {
                auth_tab = Some(idx);
            }
            snapshots.push(snap);
        }

        for addr in refreshed_thumbs {
            self.refresh_thumbnail(&addr, ctx);
        }
        if !recent_updates.is_empty() {
            for (idx, host_name, fp) in recent_updates {
                let tab = &self.tabs[idx];
                upsert_recent(
                    &mut self.hosts,
                    &tab.target_address,
                    &host_name,
                    fp.as_deref(),
                    tab.codec_input,
                    tab.quality_input,
                    tab.fps_input,
                );
            }
            let _ = save_hosts(&self.hosts);
        }
        if let Some(idx) = auth_tab {
            let tab = &self.tabs[idx];
            self.password_prompt = Some(PasswordPrompt {
                title: tab.title.clone(),
                address: tab.target_address.clone(),
                password: String::new(),
                expected_fp: tab.expected_fp.clone(),
                codec: tab.codec_input,
                quality: tab.quality_input,
                fps: tab.fps_input,
                just_opened: true,
                retry_tab_id: Some(tab.id),
            });
            self.active_session_tab = Some(idx);
        }

        if self.hidden_to_tray {
            self.release_all_tabs_modifiers();
            self.kb_hook.set_capture(false, None);
            return;
        }

        // 4. Barra superior de pestañas (visible en cuanto haya >= 1 sesión)
        self.render_tab_strip(ctx);

        // 5. Renderizado de vista activa (Lobby o Pestaña remota)
        if let Some(active) = self.active_session_tab {
            if active < self.tabs.len() {
                let snap = &snapshots[active];
                let outcome = self.tabs[active].render_active(
                    ctx,
                    &self.rt_handle,
                    &mut self.kb_hook,
                    self.tray.as_ref(),
                    self.hidden_to_tray,
                    snap,
                );
                match outcome {
                    SessionTabOutcome::None => {}
                    SessionTabOutcome::CloseTab => self.close_tab(active, ctx),
                    SessionTabOutcome::Reconnect => {
                        let relay_url = self.settings.normalized_relay_url();
                        self.tabs[active].reconnect_with_password(String::new(), relay_url, ctx);
                    }
                    SessionTabOutcome::ReconnectWithPrompt => {
                        let tab = &self.tabs[active];
                        self.password_prompt = Some(PasswordPrompt {
                            title: tab.title.clone(),
                            address: tab.target_address.clone(),
                            password: String::new(),
                            expected_fp: tab.expected_fp.clone(),
                            codec: tab.codec_input,
                            quality: tab.quality_input,
                            fps: tab.fps_input,
                            just_opened: true,
                            retry_tab_id: Some(tab.id),
                        });
                    }
                }
                self.render_modals(ctx);
                return;
            } else {
                self.active_session_tab = None;
            }
        }

        self.kb_hook.set_capture(false, None);
        self.render_lobby(ctx);
        self.render_modals(ctx);
    }
}

impl Drop for RandyDeskApp {
    fn drop(&mut self) {
        self.kb_hook.set_capture(false, None);
        for mut tab in self.tabs.drain(..) {
            let _ = tab.save_thumbnail_now();
            tab.net.disconnect();
        }
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
                let addresses: Vec<String> = self.hosts.iter().map(|h| h.address.clone()).collect();
                let online_map = self.presence.snapshot(&addresses);
                if let Some(act) = render_host_cards(
                    ui,
                    &self.hosts,
                    self.active_tab,
                    &self.search_query,
                    &online_map,
                    &self.thumbnails,
                ) {
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
}
