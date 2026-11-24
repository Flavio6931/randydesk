//! Interfaz gráfica unificada de RandyDesk en español (recibir conexiones + controlar equipos).

use crate::config::{load_hosts, normalize_address, save_hosts, SavedHost};
use crate::host_panel::HostPanelState;
use crate::keymap::{is_control_or_nav_key, map_egui_key_to_vk, map_mouse_button, normalize_pos};
use crate::net::{ConnectRequest, ConnectionState, NetCommand, NetworkHandle};
use arboard::Clipboard;
use eframe::egui;
use randydesk_proto::{CodecKind, InputEvent, StreamSettings};
use std::time::Duration;

/// Aplicación principal de RandyDesk (anfitrión + visor remoto en una ventana).
pub struct RandyDeskApp {
    rt_handle: tokio::runtime::Handle,
    host_panel: HostPanelState,
    net: NetworkHandle,
    hosts: Vec<SavedHost>,
    selected_host_idx: Option<usize>,

    // Formulario de conexión remota
    name_input: String,
    address_input: String,
    password_input: String,
    codec_input: CodecKind,
    quality_input: u8,
    fps_input: u8,
    expected_fp_input: Option<String>,
    ui_notice: Option<String>,

    // Renderizado del escritorio remoto
    screen_texture: Option<egui::TextureHandle>,
    last_uploaded_seq: u64,
    fullscreen: bool,
    pending_save_fp_for_address: Option<String>,
}

impl RandyDeskApp {
    /// Inicializa la aplicación cargando la libreta local y arrancando el receptor local.
    pub fn new(cc: &eframe::CreationContext<'_>, rt_handle: tokio::runtime::Handle) -> Self {
        configure_dark_theme(&cc.egui_ctx);
        let hosts = load_hosts();
        let host_panel = HostPanelState::new(&rt_handle);
        Self {
            net: NetworkHandle::new(rt_handle.clone()),
            rt_handle,
            host_panel,
            hosts,
            selected_host_idx: None,
            name_input: "PC Oficina".to_string(),
            address_input: "127.0.0.1:21118".to_string(),
            password_input: String::new(),
            codec_input: CodecKind::Jpeg,
            quality_input: 75,
            fps_input: 30,
            expected_fp_input: None,
            ui_notice: None,
            screen_texture: None,
            last_uploaded_seq: 0,
            fullscreen: false,
            pending_save_fp_for_address: None,
        }
    }

    fn start_connection(&mut self, ctx: &egui::Context) {
        if self.password_input.is_empty() {
            self.ui_notice = Some("Ingresa la contraseña o PIN de sesión del equipo remoto.".into());
            return;
        }
        let normalized = normalize_address(&self.address_input);
        self.address_input = normalized.clone();
        self.ui_notice = None;
        self.pending_save_fp_for_address = Some(normalized.clone());

        self.net.connect(
            ConnectRequest {
                address: normalized,
                password: self.password_input.clone(),
                expected_fingerprint: self.expected_fp_input.clone(),
                settings: StreamSettings {
                    codec: self.codec_input,
                    jpeg_quality: self.quality_input,
                    target_fps: self.fps_input,
                },
            },
            ctx.clone(),
        );
    }

    fn load_host_into_form(&mut self, idx: usize) {
        if let Some(h) = self.hosts.get(idx).cloned() {
            self.selected_host_idx = Some(idx);
            self.name_input = h.name;
            self.address_input = h.address;
            self.codec_input = h.codec;
            self.quality_input = h.quality;
            self.fps_input = h.fps;
            self.expected_fp_input = h.fingerprint_sha256;
            self.ui_notice = None;
        }
    }

    fn save_current_to_book(&mut self) {
        let normalized = normalize_address(&self.address_input);
        let name = if self.name_input.trim().is_empty() {
            normalized.clone()
        } else {
            self.name_input.trim().to_string()
        };

        if let Some(idx) = self.selected_host_idx {
            if let Some(h) = self.hosts.get_mut(idx) {
                h.name = name;
                h.address = normalized;
                h.codec = self.codec_input;
                h.quality = self.quality_input;
                h.fps = self.fps_input;
                h.fingerprint_sha256 = self.expected_fp_input.clone();
            }
        } else {
            let mut host = SavedHost::new(name, normalized);
            host.codec = self.codec_input;
            host.quality = self.quality_input;
            host.fps = self.fps_input;
            host.fingerprint_sha256 = self.expected_fp_input.clone();
            self.hosts.push(host);
            self.selected_host_idx = Some(self.hosts.len() - 1);
        }

        match save_hosts(&self.hosts) {
            Ok(()) => self.ui_notice = Some("Equipo guardado en tu libreta local.".into()),
            Err(err) => self.ui_notice = Some(format!("No se pudo guardar: {err}")),
        }
    }

    fn sync_tofu_fingerprint(&mut self, observed: Option<&str>) {
        let Some(fp) = observed else { return };
        let Some(addr) = self.pending_save_fp_for_address.clone() else {
            return;
        };
        if let Some(host) = self.hosts.iter_mut().find(|h| h.address == addr) {
            if host.fingerprint_sha256.is_none() {
                host.fingerprint_sha256 = Some(fp.to_string());
                let _ = save_hosts(&self.hosts);
            }
        }
        if self.expected_fp_input.is_none() {
            self.expected_fp_input = Some(fp.to_string());
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
                self.ui_notice = Some("Portapapeles recibido del equipo remoto.".into());
            }
        }

        if matches!(status, ConnectionState::Connected { .. }) {
            self.sync_tofu_fingerprint(stats.fingerprint_sha256.as_deref());
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
                    self.ui_notice = Some(format!("Error de conexión: {msg}"));
                }
                self.render_lobby(ctx);
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
                    &self.address_input.clone(),
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
            .default_width(315.0)
            .show(ctx, |ui| {
                self.host_panel.ui(ui, &self.rt_handle);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.heading("Controlar escritorio remoto");
            ui.label(
                egui::RichText::new("Sesiones directas cifradas QUIC TLS 1.3 — sin límite de tiempo.")
                    .small()
                    .weak(),
            );
            ui.add_space(10.0);

            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::same(14.0))
                .show(ui, |ui| {
                    egui::Grid::new("connect_grid")
                        .num_columns(2)
                        .spacing([14.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Nombre:");
                            ui.text_edit_singleline(&mut self.name_input);
                            ui.end_row();

                            ui.label("IP / Tailscale:");
                            ui.text_edit_singleline(&mut self.address_input);
                            ui.end_row();

                            ui.label("Contraseña / PIN:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.password_input)
                                    .password(true)
                                    .hint_text("PIN temporal o contraseña fija"),
                            );
                            ui.end_row();

                            ui.label("Modo de video:");
                            ui.horizontal(|ui| {
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
                            });
                            ui.end_row();

                            ui.label("Calidad / FPS:");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::Slider::new(&mut self.quality_input, 35..=95)
                                        .suffix("%"),
                                );
                                ui.add(
                                    egui::Slider::new(&mut self.fps_input, 5..=60).suffix(" fps"),
                                );
                            });
                            ui.end_row();

                            if let Some(fp) = self.expected_fp_input.clone() {
                                ui.label("Huella TLS:");
                                ui.horizontal(|ui| {
                                    let short: String = fp.chars().take(26).collect();
                                    ui.label(egui::RichText::new(format!("{short}…")).monospace().small());
                                    if ui.small_button("Olvidar").clicked() {
                                        self.expected_fp_input = None;
                                    }
                                });
                                ui.end_row();
                            }
                        });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized([170.0, 32.0], egui::Button::new("🔌 Conectar ahora"))
                            .clicked()
                        {
                            self.start_connection(ctx);
                        }
                        if ui
                            .add_sized([170.0, 32.0], egui::Button::new("💾 Guardar en libreta"))
                            .clicked()
                        {
                            self.save_current_to_book();
                        }
                        if ui.button("➕ Nuevo").clicked() {
                            self.selected_host_idx = None;
                            self.name_input = "Nuevo Equipo".into();
                            self.address_input = "100.x.y.z:21118".into();
                            self.expected_fp_input = None;
                        }
                    });
                });

            if let Some(notice) = &self.ui_notice {
                ui.add_space(8.0);
                ui.colored_label(egui::Color32::from_rgb(250, 200, 90), notice);
            }

            ui.add_space(14.0);
            ui.heading("Mis Equipos guardados");
            ui.separator();

            if self.hosts.is_empty() {
                ui.label(
                    egui::RichText::new("Aún no guardaste equipos. Usa \"Guardar en libreta\" para conectar en 1 clic.")
                        .small()
                        .weak(),
                );
            } else {
                let mut connect_idx: Option<usize> = None;
                let mut edit_idx: Option<usize> = None;
                let mut delete_idx: Option<usize> = None;

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (idx, host) in self.hosts.iter().enumerate() {
                            let is_selected = self.selected_host_idx == Some(idx);
                            egui::Frame::group(ui.style())
                                .fill(if is_selected {
                                    egui::Color32::from_rgb(36, 52, 74)
                                } else {
                                    egui::Color32::from_rgb(26, 30, 38)
                                })
                                .inner_margin(egui::Margin::same(10.0))
                                .show(ui, |ui| {
                                    ui.set_min_width(240.0);
                                    ui.label(egui::RichText::new(format!("🖥 {}", host.name)).strong());
                                    ui.label(egui::RichText::new(&host.address).small().weak());
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        if ui.small_button("🔌 Conectar").clicked() {
                                            connect_idx = Some(idx);
                                        }
                                        if ui.small_button("✏ Cargar").clicked() {
                                            edit_idx = Some(idx);
                                        }
                                        if ui.small_button("🗑").on_hover_text("Eliminar").clicked() {
                                            delete_idx = Some(idx);
                                        }
                                    });
                                });
                        }
                    });
                });

                if let Some(idx) = edit_idx {
                    self.load_host_into_form(idx);
                }
                if let Some(idx) = connect_idx {
                    self.load_host_into_form(idx);
                    self.start_connection(ctx);
                }
                if let Some(idx) = delete_idx {
                    self.hosts.remove(idx);
                    self.selected_host_idx = None;
                    let _ = save_hosts(&self.hosts);
                }
            }
        });
    }

    fn render_connecting(&mut self, ctx: &egui::Context, address: &str) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.add_space(10.0);
                    ui.heading(format!("Conectando a {address}..."));
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
                        egui::Color32::from_rgb(245, 165, 66),
                        format!("⚠ Reconectando (intento #{attempt}): {reason}"),
                    );
                } else {
                    ui.colored_label(
                        egui::Color32::from_rgb(90, 220, 130),
                        format!("● {host_name} ({width}×{height})"),
                    );
                }

                ui.separator();
                ui.label(format!("⏱ {}", format_duration(stats.elapsed_secs)));
                ui.label(format!("🎞 {:.1} fps", stats.fps));
                if stats.kbps >= 1024.0 {
                    ui.label(format!("📶 {:.2} MB/s", stats.kbps / 8192.0));
                } else {
                    ui.label(format!("📶 {:.0} kbps", stats.kbps));
                }
                if let Some(rtt) = stats.rtt_ms {
                    ui.label(format!("⚡ {rtt} ms"));
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
                if ui
                    .button("📋 Enviar")
                    .on_hover_text("Enviar mi portapapeles al host")
                    .clicked()
                {
                    if let Ok(text) = Clipboard::new().and_then(|mut c| c.get_text()) {
                        self.net.send_cmd(NetCommand::ClipboardSet(text));
                    }
                }
                if ui
                    .button("📥 Traer")
                    .on_hover_text("Traer portapapeles del host")
                    .clicked()
                {
                    self.net.send_cmd(NetCommand::ClipboardGet);
                }
                if ui.button("🔄 Refrescar").clicked() {
                    self.net.send_cmd(NetCommand::RequestKeyframe);
                }
                if ui
                    .button(if self.fullscreen {
                        "🗗 Ventana"
                    } else {
                        "⛶ Pantalla completa"
                    })
                    .clicked()
                {
                    self.fullscreen = !self.fullscreen;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✖ Desconectar").clicked() {
                        if self.fullscreen {
                            self.fullscreen = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                        }
                        self.net.disconnect();
                    }
                });
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
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
                    egui::Color32::WHITE,
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

fn configure_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_rounding = egui::Rounding::same(8.0);
    ctx.set_visuals(visuals);
}
