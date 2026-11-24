//! Barra lateral minimalista estilo RustDesk ("Tu escritorio") para recibir conexiones.

use crate::config::display_address;
use crate::theme::{ACCENT_BLUE, ACCENT_TEAL, TEXT_MUTED};
use arboard::Clipboard;
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use randydesk_host::service::{HostRuntimeInfo, HostService};
use randydesk_proto::{generate_session_pin, StreamSettings, DEFAULT_PORT};

/// Estado del servidor host embebido dentro de la aplicación gráfica.
pub struct HostPanelState {
    service: Option<HostService>,
    port: u16,
    port_text: String,
    password: String,
    editing_password: bool,
    show_password: bool,
    monitor_index: usize,
    status_msg: Option<String>,
}

impl HostPanelState {
    /// Crea el estado e inicia automáticamente la escucha de conexiones entrantes.
    pub fn new(rt_handle: &tokio::runtime::Handle) -> Self {
        let initial_pwd = std::env::var("RANDYDESK_PASSWORD")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(generate_session_pin);

        let mut state = Self {
            service: None,
            port: DEFAULT_PORT,
            port_text: DEFAULT_PORT.to_string(),
            password: initial_pwd,
            editing_password: false,
            show_password: true,
            monitor_index: 0,
            status_msg: None,
        };
        state.start_host(rt_handle);
        state
    }

    /// Inicia o reinicia el servicio host con el puerto y contraseña actuales.
    pub fn start_host(&mut self, rt_handle: &tokio::runtime::Handle) {
        self.stop_host();
        if self.password.trim().is_empty() {
            self.status_msg = Some("La contraseña no puede estar vacía.".into());
            return;
        }
        match HostService::start(
            rt_handle,
            self.port,
            self.password.clone(),
            self.monitor_index,
            StreamSettings::default(),
        ) {
            Ok(svc) => {
                self.service = Some(svc);
                self.editing_password = false;
                self.status_msg = None;
            }
            Err(err) => {
                self.status_msg = Some(format!("Puerto {} ocupado o inaccesible: {err}", self.port));
            }
        }
    }

    /// Detiene el servicio local si estaba activo.
    pub fn stop_host(&mut self) {
        if let Some(mut svc) = self.service.take() {
            svc.stop();
        }
    }

    /// Devuelve un resumen vivo del servicio para la barra inferior de estado.
    pub fn runtime_info(&self) -> Option<HostRuntimeInfo> {
        self.service.as_ref().map(|s| s.info())
    }

    /// Dibuja la barra lateral minimalista "Tu escritorio".
    pub fn ui(&mut self, ui: &mut egui::Ui, rt_handle: &tokio::runtime::Handle) {
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Tu escritorio").size(16.0).strong());
        ui.label(
            egui::RichText::new("Puedes acceder a tu escritorio con esta dirección y contraseña.")
                .size(11.5)
                .color(TEXT_MUTED),
        );
        ui.add_space(18.0);

        let info = self.service.as_ref().map(|s| s.info());
        let is_running = info.as_ref().is_some_and(|i| i.running);

        // Bloque 1: Dirección (ID) con barra de acento azul
        let primary_addr = info.as_ref().map(|i| {
            let ip = i
                .tailscale_ip
                .as_deref()
                .or(i.lan_ip.as_deref())
                .unwrap_or("127.0.0.1");
            display_address(&format!("{ip}:{}", i.port))
        });

        render_accent_block(ui, ACCENT_BLUE, |ui| {
            ui.horizontal(|ui| {
                let title = if info.as_ref().and_then(|i| i.tailscale_ip.as_ref()).is_some() {
                    "Tu ID (Tailscale)"
                } else {
                    "Tu ID (Red local)"
                };
                ui.label(egui::RichText::new(title).size(12.0).color(TEXT_MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("...", |ui| {
                        if let Some(ref i) = info {
                            if let Some(lan) = &i.lan_ip {
                                let lan_ep = format!("{lan}:{}", i.port);
                                if ui.button(format!("Copiar LAN ({lan_ep})")).clicked() {
                                    copy_to_clipboard(&lan_ep, &mut self.status_msg, "IP LAN copiada");
                                    ui.close_menu();
                                }
                            }
                            if let Some(ts) = &i.tailscale_ip {
                                let ts_ep = format!("{ts}:{}", i.port);
                                if ui.button(format!("Copiar Tailscale ({ts_ep})")).clicked() {
                                    copy_to_clipboard(&ts_ep, &mut self.status_msg, "IP Tailscale copiada");
                                    ui.close_menu();
                                }
                            }
                            if ui.button("Copiar huella TLS").clicked() {
                                copy_to_clipboard(
                                    &i.fingerprint_sha256,
                                    &mut self.status_msg,
                                    "Huella TLS copiada",
                                );
                                ui.close_menu();
                            }
                            ui.label(
                                egui::RichText::new(format!("Sesiones activas: {}", i.active_clients))
                                    .small()
                                    .color(TEXT_MUTED),
                            );
                            ui.separator();
                        }

                        ui.horizontal(|ui| {
                            ui.label("Puerto UDP:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.port_text).desired_width(54.0),
                            );
                            if ui.button("Aplicar").clicked() {
                                if let Ok(p) = self.port_text.trim().parse::<u16>() {
                                    if p > 0 {
                                        self.port = p;
                                        self.start_host(rt_handle);
                                    }
                                } else {
                                    self.port_text = self.port.to_string();
                                }
                                ui.close_menu();
                            }
                        });
                        ui.separator();
                        if is_running {
                            if ui.button("Detener recepción").clicked() {
                                self.stop_host();
                                ui.close_menu();
                            }
                        } else if ui.button("Activar recepción").clicked() {
                            self.start_host(rt_handle);
                            ui.close_menu();
                        }
                    });
                });
            });

            if let Some(addr) = primary_addr {
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(&addr)
                            .size(20.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .sense(egui::Sense::click()),
                );
                if resp.on_hover_text("Clic para copiar dirección").clicked() {
                    copy_to_clipboard(&addr, &mut self.status_msg, "Dirección copiada");
                }
            } else {
                ui.label(egui::RichText::new("Detenido").size(18.0).color(TEXT_MUTED));
            }
        });

        ui.add_space(18.0);

        // Bloque 2: Contraseña de un solo uso con barra de acento verde
        render_accent_block(ui, ACCENT_TEAL, |ui| {
            ui.label(
                egui::RichText::new("Contraseña de acceso")
                    .size(12.0)
                    .color(TEXT_MUTED),
            );

            if self.editing_password {
                ui.horizontal(|ui| {
                    let edit = egui::TextEdit::singleline(&mut self.password).desired_width(128.0);
                    let enter = ui.add(edit).lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if enter || ui.small_button("OK").clicked() {
                        self.start_host(rt_handle);
                        if self.status_msg.is_none() {
                            self.status_msg = Some("Contraseña aplicada.".into());
                        }
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    let shown = if self.show_password {
                        self.password.clone()
                    } else {
                        "••••••••".to_string()
                    };
                    let lbl = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(shown)
                                    .size(16.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_text("Clic para copiar contraseña");
                    if lbl.clicked() {
                        copy_to_clipboard(&self.password, &mut self.status_msg, "Contraseña copiada");
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("Editar")
                            .on_hover_text("Fijar contraseña personalizada")
                            .clicked()
                        {
                            self.editing_password = true;
                        }
                        if ui
                            .small_button("Nuevo")
                            .on_hover_text("Generar nuevo PIN aleatorio")
                            .clicked()
                        {
                            self.password = generate_session_pin();
                            self.start_host(rt_handle);
                            self.status_msg = Some("Nuevo PIN generado.".into());
                        }
                    });
                });

                let toggle_txt = if self.show_password {
                    "Ocultar"
                } else {
                    "Mostrar"
                };
                if ui
                    .small_button(egui::RichText::new(toggle_txt).size(11.0).color(TEXT_MUTED))
                    .clicked()
                {
                    self.show_password = !self.show_password;
                }
            }
        });

        if let Some(msg) = &self.status_msg {
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new(msg)
                    .size(12.0)
                    .color(Color32::from_rgb(120, 205, 255)),
            );
        }
    }
}

impl Drop for HostPanelState {
    fn drop(&mut self) {
        self.stop_host();
    }
}

fn render_accent_block(
    ui: &mut egui::Ui,
    accent: Color32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let resp = ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
    });
    let rect = resp.response.rect;
    let bar = Rect::from_min_size(Pos2::new(rect.min.x, rect.min.y + 2.0), Vec2::new(3.0, (rect.height() - 4.0).max(16.0)));
    ui.painter().rect_filled(bar, 1.5, accent);
}

fn copy_to_clipboard(text: &str, status: &mut Option<String>, ok_msg: &str) {
    match Clipboard::new().and_then(|mut cb| cb.set_text(text.to_owned())) {
        Ok(()) => *status = Some(ok_msg.to_string()),
        Err(err) => *status = Some(format!("Error copiando: {err}")),
    }
}
