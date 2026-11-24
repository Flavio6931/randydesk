//! Interfaz gráfica unificada de RandyDesk estilo RustDesk (LAN directa + ID por Internet E2E).

use crate::cards::{render_host_cards, CardAction, HostTab};
use crate::config::{
    display_address, load_hosts, load_settings, normalize_address, save_hosts, save_settings,
    upsert_recent, AppSettings, SavedHost,
};
use crate::host_panel::HostPanelState;
use crate::keymap::{is_control_or_nav_key, map_egui_key_to_vk, map_mouse_button, normalize_pos};
use crate::net::{ConnectRequest, ConnectionState, NetCommand, NetworkHandle};
use crate::theme::{
    apply_rustdesk_theme, ACCENT_BLUE, ACCENT_TEAL, BANNER_COLORS, BG_CARD, BG_MAIN, BG_SIDEBAR,
    BORDER_SUBTLE, STATUS_ORANGE, TEXT_MUTED,
};
use arboard::Clipboard;
use eframe::egui::{self, Color32, Rounding, Stroke, Vec2};
use randydesk_proto::{
    normalize_relay_ws_url, CodecKind, InputEvent, StreamSettings, DEFAULT_RELAY_URL,
};
use std::time::Duration;

/// Datos temporales del modal para pedir contraseña al conectar.
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

/// Aplicación principal de RandyDesk (anfitrión + visor remoto en una ventana).
pub struct RandyDeskApp {
    rt_handle: tokio::runtime::Handle,
    host_panel: HostPanelState,
    net: NetworkHandle,
    hosts: Vec<SavedHost>,
    settings: AppSettings,

    // Estado de vista principal (Lobby)
    quick_address: String,
    active_tab: HostTab,
    search_query: String,
    codec_input: CodecKind,
    quality_input: u8,
    fps_input: u8,
    ui_notice: Option<String>,

    // Modales
    password_prompt: Option<PasswordPrompt>,
    host_editor: Option<HostEditor>,
    show_relay_modal: bool,
    relay_url_input: String,
    relay_modal_msg: Option<String>,
    show_guide: bool,

    // Renderizado del escritorio remoto
    screen_texture: Option<egui::TextureHandle>,
    last_uploaded_seq: u64,
    fullscreen: bool,
    active_target_address: Option<String>,
    recorded_recent_for_session: bool,
}

impl RandyDeskApp {
    /// Inicializa la aplicación cargando el tema, la libreta, los ajustes de Relay y el receptor local.
    pub fn new(cc: &eframe::CreationContext<'_>, rt_handle: tokio::runtime::Handle) -> Self {
        apply_rustdesk_theme(&cc.egui_ctx);
        let hosts = load_hosts();
        let settings = load_settings();
        let relay_url_input = settings.relay_url.clone();
        let host_panel = HostPanelState::new(&rt_handle, settings.normalized_relay_url());
        Self {
            net: NetworkHandle::new(rt_handle.clone()),
            rt_handle,
            host_panel,
            hosts,
            settings,
            quick_address: String::new(),
            active_tab: HostTab::Recent,
            search_query: String::new(),
            codec_input: CodecKind::Jpeg,
            quality_input: 75,
            fps_input: 30,
            ui_notice: None,
            password_prompt: None,
            host_editor: None,
            show_relay_modal: false,
            relay_url_input,
            relay_modal_msg: None,
            show_guide: false,
            screen_texture: None,
            last_uploaded_seq: 0,
            fullscreen: false,
            active_target_address: None,
            recorded_recent_for_session: false,
        }
    }

    fn open_password_prompt_for_address(&mut self, raw_address: &str) {
        let trimmed = raw_address.trim();
        if trimmed.is_empty() {
            self.ui_notice = Some("Escribe el ID de 9 dígitos o la IP local del equipo destino.".into());
            return;
        }
        let normalized = normalize_address(trimmed);
        if let Some(idx) = self.hosts.iter().position(|h| h.address == normalized) {
            self.open_password_prompt_for_host(idx);
            return;
        }
        self.ui_notice = None;
        self.password_prompt = Some(PasswordPrompt {
            title: display_address(&normalized),
            address: normalized,
            password: String::new(),
            expected_fp: None,
            codec: self.codec_input,
            quality: self.quality_input,
            fps: self.fps_input,
            just_opened: true,
        });
    }

    fn open_password_prompt_for_host(&mut self, idx: usize) {
        let Some(host) = self.hosts.get(idx) else {
            return;
        };
        self.ui_notice = None;
        self.codec_input = host.codec;
        self.quality_input = host.quality;
        self.fps_input = host.fps;
        self.password_prompt = Some(PasswordPrompt {
            title: host.name.clone(),
            address: host.address.clone(),
            password: String::new(),
            expected_fp: host.fingerprint_sha256.clone(),
            codec: host.codec,
            quality: host.quality,
            fps: host.fps,
            just_opened: true,
        });
    }

    fn launch_connection(&mut self, prompt: PasswordPrompt, ctx: &egui::Context) {
        self.active_target_address = Some(prompt.address.clone());
        self.recorded_recent_for_session = false;
        self.ui_notice = None;
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
            },
            ctx.clone(),
        );
    }

    fn handle_card_action(&mut self, act: CardAction) {
        match act {
            CardAction::Connect(idx) => self.open_password_prompt_for_host(idx),
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
}

impl eframe::App for RandyDeskApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (status, stats, maybe_frame, frame_seq, incoming_clip) = {
            let mut guard = self.net.shared.lock().expect("lock estado red");
            let clip = guard.incoming_clipboard.take();
            (
                guard.status.clone(),
                guard.stats.clone(),
                guard.latest_frame.clone(),
                guard.frame_seq,
                clip,
            )
        };

        if let Some(clip_text) = incoming_clip {
            if let Ok(mut cb) = Clipboard::new() {
                let _ = cb.set_text(clip_text);
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
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        if let Some(decoded) = maybe_frame {
            if frame_seq != self.last_uploaded_seq {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [decoded.width as usize, decoded.height as usize],
                    &decoded.rgba,
                );
                match &mut self.screen_texture {
                    Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                    None => {
                        self.screen_texture = Some(ctx.load_texture(
                            "remote-screen",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
                self.last_uploaded_seq = frame_seq;
            }
        }

        match status {
            ConnectionState::Disconnected | ConnectionState::Error(_) => {
                if let ConnectionState::Error(msg) = &status {
                    self.ui_notice = Some(format!("No se pudo conectar: {msg}"));
                }
                self.render_lobby(ctx);
                self.render_modals(ctx);
            }
            ConnectionState::Connecting { ref address } => {
                self.render_connecting(ctx, address);
            }
            ConnectionState::Connected {
                ref host_name,
                width,
                height,
            } => {
                self.render_remote_session(ctx, host_name, width, height, &stats, None);
            }
            ConnectionState::Reconnecting {
                attempt,
                ref reason,
            } => {
                self.render_remote_session(
                    ctx,
                    &self.quick_address.clone(),
                    0,
                    0,
                    &stats,
                    Some((attempt, reason.as_str())),
                );
            }
        }
    }
}

impl Drop for RandyDeskApp {
    fn drop(&mut self) {
        self.net.disconnect();
        self.host_panel.stop_host();
    }
}

impl RandyDeskApp {
    fn render_lobby(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("este_equipo_panel")
            .resizable(false)
            .exact_width(240.0)
            .frame(
                egui::Frame::none()
                    .fill(BG_SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                self.host_panel.ui(ui, &self.rt_handle);
            });

        egui::TopBottomPanel::bottom("status_footer")
            .exact_height(34.0)
            .frame(
                egui::Frame::none()
                    .fill(BG_SIDEBAR)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(14.0, 6.0)),
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
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 4.0, dot_color);

                    let state_txt = if relay_ok {
                        "Listo (Internet y red local)."
                    } else if has_relay {
                        "Conectando al servidor Relay..."
                    } else if running {
                        "Solo red local."
                    } else {
                        "Recepción detenida."
                    };
                    ui.label(egui::RichText::new(state_txt).size(12.0).color(TEXT_MUTED));

                    if ui
                        .link(egui::RichText::new("Configurar servidor Relay").size(12.0))
                        .clicked()
                    {
                        self.relay_url_input = self.settings.relay_url.clone();
                        self.relay_modal_msg = None;
                        self.show_relay_modal = true;
                    }

                    ui.label(egui::RichText::new("|").size(12.0).color(BORDER_SUBTLE));

                    if ui.link(egui::RichText::new("Guía rápida").size(12.0)).clicked() {
                        self.show_guide = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("RandyDesk v{}", env!("CARGO_PKG_VERSION")))
                                .size(11.5)
                                .color(TEXT_MUTED),
                        );
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG_MAIN)
                    .inner_margin(egui::Margin::symmetric(22.0, 16.0)),
            )
            .show(ctx, |ui| {
                // Tarjeta superior compacta "Controlar escritorio remoto"
                egui::Frame::none()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::same(16.0))
                    .show(ui, |ui| {
                        ui.set_width(390.0);
                        ui.label(
                            egui::RichText::new("Controlar escritorio remoto")
                                .size(15.0)
                                .strong(),
                        );
                        ui.add_space(10.0);

                        let input_resp = ui.add_sized(
                            [370.0, 38.0],
                            egui::TextEdit::singleline(&mut self.quick_address)
                                .hint_text("Escribe el ID (ej. 482 910 331) o IP local")
                                .font(egui::FontId::proportional(15.0)),
                        );
                        let pressed_enter =
                            input_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.menu_button("Opciones", |ui| {
                                    ui.label("Modo de video:");
                                    ui.radio_value(
                                        &mut self.codec_input,
                                        CodecKind::Jpeg,
                                        "Fluido (JPEG)",
                                    );
                                    ui.radio_value(
                                        &mut self.codec_input,
                                        CodecKind::Lz4Rgba,
                                        "Nítido (LZ4)",
                                    );
                                    ui.separator();
                                    ui.add(
                                        egui::Slider::new(&mut self.quality_input, 35..=95)
                                            .text("Calidad %"),
                                    );
                                    ui.add(
                                        egui::Slider::new(&mut self.fps_input, 5..=60).text("FPS"),
                                    );
                                });

                                let connect_btn = ui.add_sized(
                                    [110.0, 32.0],
                                    egui::Button::new(
                                        egui::RichText::new("Conectar")
                                            .color(Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(ACCENT_BLUE),
                                );
                                if connect_btn.clicked() || pressed_enter {
                                    let addr = self.quick_address.clone();
                                    self.open_password_prompt_for_address(&addr);
                                }
                            });
                        });
                    });

                if let Some(notice) = &self.ui_notice {
                    ui.add_space(8.0);
                    ui.colored_label(Color32::from_rgb(245, 195, 90), notice);
                }

                ui.add_space(18.0);
                ui.separator();
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.active_tab, HostTab::Recent, "Recientes");
                    ui.selectable_value(&mut self.active_tab, HostTab::Favorites, "Favoritos");
                    ui.selectable_value(&mut self.active_tab, HostTab::All, "Todos");
                    ui.add_space(8.0);
                    if ui.button("+ Agregar").clicked() {
                        self.host_editor = Some(HostEditor {
                            editing_idx: None,
                            name: String::new(),
                            address: String::new(),
                            color_idx: (self.hosts.len() % 7) as u8,
                        });
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_sized(
                            [190.0, 26.0],
                            egui::TextEdit::singleline(&mut self.search_query)
                                .hint_text("Buscar equipo..."),
                        );
                    });
                });

                ui.add_space(12.0);
                if let Some(act) =
                    render_host_cards(ui, &self.hosts, self.active_tab, &self.search_query)
                {
                    self.handle_card_action(act);
                }
            });
    }

    fn render_modals(&mut self, ctx: &egui::Context) {
        // 1. Modal de contraseña al conectar
        let mut launch: Option<PasswordPrompt> = None;
        let mut close_prompt = false;
        if let Some(prompt) = &mut self.password_prompt {
            egui::Window::new(format!("Conectar a {}", prompt.title))
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
                    ui.label("Contraseña o PIN del equipo remoto:");
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
                                launch = Some(PasswordPrompt {
                                    title: prompt.title.clone(),
                                    address: prompt.address.clone(),
                                    password: prompt.password.clone(),
                                    expected_fp: prompt.expected_fp.clone(),
                                    codec: prompt.codec,
                                    quality: prompt.quality,
                                    fps: prompt.fps,
                                    just_opened: false,
                                });
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
                        egui::TextEdit::singleline(&mut ed.address).hint_text("482 910 331 o 192.168.1.25"),
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
                            "Pega la URL de tu servidor Relay (ej. https://tu-relay.onrender.com o https://xxxx.trycloudflare.com). Todo el tráfico viaja cifrado E2E.",
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
                    ui.label("1. Por Internet sin Tailscale (ID de 9 dígitos):");
                    ui.label(
                        egui::RichText::new(
                            "Pega la URL de tu servidor Relay (Render Free o cloudflared) en 'Configurar servidor Relay' en ambos equipos y conéctate solo usando el ID numérico.",
                        )
                        .color(TEXT_MUTED),
                    );
                    ui.add_space(8.0);
                    ui.label("2. En la misma casa u oficina (LAN):");
                    ui.label(
                        egui::RichText::new(
                            "No necesitas servidor Relay: escribe directo la IP local (192.168.x.x) que aparece debajo de 'Tu ID'.",
                        )
                        .color(TEXT_MUTED),
                    );
                    ui.add_space(8.0);
                    ui.label("3. Sesiones largas (2+ horas):");
                    ui.label(
                        egui::RichText::new(
                            "En la PC remota configura Windows en 'Suspender el equipo: Nunca'.",
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

    fn render_remote_session(
        &mut self,
        ctx: &egui::Context,
        host_name: &str,
        width: u32,
        height: u32,
        stats: &crate::net::SessionStats,
        reconnecting: Option<(u32, &str)>,
    ) {
        egui::TopBottomPanel::top("session_toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if let Some((attempt, reason)) = reconnecting {
                    ui.colored_label(
                        Color32::from_rgb(245, 165, 66),
                        format!("Reconectando (#{attempt}): {reason}"),
                    );
                } else {
                    ui.colored_label(
                        ACCENT_TEAL,
                        format!("● {host_name} ({width}x{height})"),
                    );
                }

                ui.separator();
                ui.label(format_duration(stats.elapsed_secs));
                ui.label(format!("{:.1} fps", stats.fps));
                if stats.kbps >= 1024.0 {
                    ui.label(format!("{:.2} MB/s", stats.kbps / 8192.0));
                } else {
                    ui.label(format!("{:.0} kbps", stats.kbps));
                }
                if let Some(rtt) = stats.rtt_ms {
                    ui.label(format!("{rtt} ms"));
                }

                ui.separator();
                let mut changed = false;
                if ui
                    .selectable_label(self.codec_input == CodecKind::Jpeg, "JPEG")
                    .clicked()
                {
                    self.codec_input = CodecKind::Jpeg;
                    changed = true;
                }
                if ui
                    .selectable_label(self.codec_input == CodecKind::Lz4Rgba, "Nítido")
                    .clicked()
                {
                    self.codec_input = CodecKind::Lz4Rgba;
                    changed = true;
                }
                if self.codec_input == CodecKind::Jpeg
                    && ui
                        .add(egui::Slider::new(&mut self.quality_input, 35..=95).show_value(false))
                        .changed()
                {
                    changed = true;
                }
                if changed {
                    self.net.send_cmd(NetCommand::UpdateSettings(StreamSettings {
                        codec: self.codec_input,
                        jpeg_quality: self.quality_input,
                        target_fps: self.fps_input,
                    }));
                }

                ui.separator();
                if ui.button("Enviar portapapeles").clicked() {
                    if let Ok(text) = Clipboard::new().and_then(|mut c| c.get_text()) {
                        self.net.send_cmd(NetCommand::ClipboardSet(text));
                    }
                }
                if ui.button("Traer").clicked() {
                    self.net.send_cmd(NetCommand::ClipboardGet);
                }
                if ui.button("Refrescar").clicked() {
                    self.net.send_cmd(NetCommand::RequestKeyframe);
                }
                if ui
                    .button(if self.fullscreen {
                        "Ventana"
                    } else {
                        "Pantalla completa"
                    })
                    .clicked()
                {
                    self.fullscreen = !self.fullscreen;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                }
                if ui
                    .button(
                        egui::RichText::new("Desconectar")
                            .color(Color32::from_rgb(245, 115, 115)),
                    )
                    .clicked()
                {
                    if self.fullscreen {
                        self.fullscreen = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                    }
                    self.net.disconnect();
                }
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::BLACK))
            .show(ctx, |ui| {
                let Some(tex) = &self.screen_texture else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Esperando primer cuadro del equipo remoto...");
                    });
                    return;
                };

                let avail = ui.available_size();
                let tex_size = tex.size_vec2();
                let scale = (avail.x / tex_size.x.max(1.0)).min(avail.y / tex_size.y.max(1.0));
                let draw_size = tex_size * scale;

                let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
                let img_rect = egui::Rect::from_center_size(rect.center(), draw_size);

                ui.painter().image(
                    tex.id(),
                    img_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );

                let pointer_in_canvas = response.hovered() || response.dragged();
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
                        } if img_rect.contains(pos) || !pressed => {
                            if let Some((x_norm, y_norm)) = normalize_pos(pos, img_rect) {
                                self.net
                                    .send_input(InputEvent::MouseMove { x_norm, y_norm });
                            }
                            if let Some(mb) = map_mouse_button(button) {
                                self.net.send_input(InputEvent::MouseButton {
                                    button: mb,
                                    down: pressed,
                                });
                            }
                        }
                        egui::Event::MouseWheel { delta, .. } if pointer_in_canvas => {
                            let dx = (delta.x * 3.0).round() as i16 * 40;
                            let dy = (delta.y * 3.0).round() as i16 * 40;
                            if dx != 0 || dy != 0 {
                                self.net.send_input(InputEvent::MouseWheel {
                                    delta_x: dx,
                                    delta_y: dy,
                                });
                            }
                        }
                        egui::Event::Key {
                            key,
                            pressed,
                            modifiers,
                            ..
                        } => {
                            let is_combo = modifiers.ctrl || modifiers.alt || modifiers.command;
                            if is_combo || is_control_or_nav_key(key) {
                                if let Some((vk, extended)) = map_egui_key_to_vk(key) {
                                    self.net.send_input(InputEvent::Key {
                                        vk,
                                        scan: 0,
                                        extended,
                                        down: pressed,
                                    });
                                }
                            }
                        }
                        egui::Event::Text(text) => {
                            let mods = ctx.input(|i| i.modifiers);
                            if !mods.ctrl && !mods.alt && !mods.command {
                                for ch in text.chars().filter(|c| !c.is_control()) {
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
            });
    }
}

fn format_duration(total_secs: u64) -> String {
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}
