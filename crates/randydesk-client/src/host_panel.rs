//! Panel lateral estilo RustDesk ("Tu Escritorio") para recibir conexiones remotas en este equipo.

use arboard::Clipboard;
use eframe::egui;
use randydesk_host::service::HostService;
use randydesk_proto::{generate_session_pin, StreamSettings, DEFAULT_PORT};

/// Estado del servidor host embebido dentro de la aplicación gráfica.
pub struct HostPanelState {
    service: Option<HostService>,
    port: u16,
    port_text: String,
    password: String,
    password_dirty: bool,
    show_password: bool,
    monitor_index: usize,
    status_msg: Option<String>,
}

impl HostPanelState {
    /// Crea el estado e intenta iniciar automáticamente el listener para recibir conexiones.
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
            password_dirty: false,
            show_password: false,
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
            self.status_msg = Some("La contraseña/PIN no puede estar vacía.".into());
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
                self.password_dirty = false;
                self.status_msg = None;
            }
            Err(err) => {
                self.status_msg = Some(format!("No se pudo iniciar recepción: {err:#}"));
            }
        }
    }

    /// Detiene el listener local si estaba activo.
    pub fn stop_host(&mut self) {
        if let Some(mut svc) = self.service.take() {
            svc.stop();
        }
    }

    /// Dibuja la tarjeta "Tu Escritorio (Este Equipo)" en el panel lateral izquierdo.
    pub fn ui(&mut self, ui: &mut egui::Ui, rt_handle: &tokio::runtime::Handle) {
        ui.add_space(6.0);
        ui.heading("Tu Escritorio");
        ui.label(
            egui::RichText::new("Comparte esta dirección y PIN para que se conecten a esta PC.")
                .small()
                .weak(),
        );
        ui.add_space(8.0);

        let info = self.service.as_ref().map(|s| s.info());
        let is_running = info.as_ref().is_some_and(|i| i.running);

        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(12.0))
            .fill(egui::Color32::from_rgb(24, 29, 38))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if is_running {
                        ui.colored_label(
                            egui::Color32::from_rgb(80, 220, 120),
                            "● Listo para recibir",
                        );
                    } else {
                        ui.colored_label(egui::Color32::from_rgb(170, 170, 180), "○ Detenido");
                    }
                    if let Some(ref i) = info {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!("Sesiones: {}", i.active_clients))
                                    .small()
                                    .weak(),
                            );
                        });
                    }
                });

                ui.separator();

                if let Some(ref i) = info {
                    // IP LAN
                    if let Some(lan) = &i.lan_ip {
                        let endpoint = format!("{lan}:{}", i.port);
                        ui.label(egui::RichText::new("Red local (LAN):").small().weak());
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&endpoint).monospace().strong());
                            if ui.small_button("📋").on_hover_text("Copiar IP LAN").clicked() {
                                copy_to_clipboard(&endpoint, &mut self.status_msg, "IP LAN copiada");
                            }
                        });
                        ui.add_space(4.0);
                    }

                    // IP Tailscale
                    ui.label(egui::RichText::new("Internet (Tailscale):").small().weak());
                    if let Some(ts) = &i.tailscale_ip {
                        let endpoint = format!("{ts}:{}", i.port);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&endpoint).monospace().strong());
                            if ui
                                .small_button("📋")
                                .on_hover_text("Copiar IP Tailscale")
                                .clicked()
                            {
                                copy_to_clipboard(
                                    &endpoint,
                                    &mut self.status_msg,
                                    "IP Tailscale copiada",
                                );
                            }
                        });
                    } else {
                        ui.label(
                            egui::RichText::new("No detectado (instala Tailscale gratis para Internet)")
                                .small()
                                .weak(),
                        );
                    }
                } else {
                    ui.label(egui::RichText::new("Servicio de recepción apagado.").small().weak());
                }

                ui.add_space(8.0);
                ui.label(egui::RichText::new("Contraseña / PIN de acceso:").small().weak());
                ui.horizontal(|ui| {
                    let edit = egui::TextEdit::singleline(&mut self.password)
                        .password(!self.show_password)
                        .desired_width(145.0);
                    if ui.add(edit).changed() {
                        self.password_dirty = true;
                    }
                    if ui
                        .small_button(if self.show_password { "🙈" } else { "👁" })
                        .on_hover_text("Mostrar / ocultar")
                        .clicked()
                    {
                        self.show_password = !self.show_password;
                    }
                    if ui.small_button("📋").on_hover_text("Copiar PIN").clicked() {
                        copy_to_clipboard(&self.password, &mut self.status_msg, "PIN copiado");
                    }
                    if ui.small_button("🔄").on_hover_text("Generar nuevo PIN").clicked() {
                        self.password = generate_session_pin();
                        self.start_host(rt_handle);
                        self.status_msg = Some("Nuevo PIN aplicado.".into());
                    }
                });

                if self.password_dirty {
                    ui.add_space(4.0);
                    if ui.button("✔ Aplicar nueva contraseña").clicked() {
                        self.start_host(rt_handle);
                        if self.status_msg.is_none() {
                            self.status_msg = Some("Contraseña actualizada.".into());
                        }
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Puerto UDP:").small().weak());
                    let port_edit = egui::TextEdit::singleline(&mut self.port_text).desired_width(55.0);
                    if ui.add(port_edit).lost_focus() {
                        if let Ok(p) = self.port_text.trim().parse::<u16>() {
                            if p > 0 && p != self.port {
                                self.port = p;
                                if is_running {
                                    self.start_host(rt_handle);
                                }
                            }
                        } else {
                            self.port_text = self.port.to_string();
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if is_running {
                            if ui.button("⏹ Detener").clicked() {
                                self.stop_host();
                            }
                        } else if ui.button("▶ Activar").clicked() {
                            self.start_host(rt_handle);
                        }
                    });
                });

                if let Some(ref i) = info {
                    ui.add_space(6.0);
                    let short_fp: String = i.fingerprint_sha256.chars().take(23).collect();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("TLS: {short_fp}…"))
                                .monospace()
                                .small()
                                .weak(),
                        );
                        if ui
                            .small_button("📋")
                            .on_hover_text("Copiar huella TLS completa")
                            .clicked()
                        {
                            copy_to_clipboard(
                                &i.fingerprint_sha256,
                                &mut self.status_msg,
                                "Huella TLS copiada",
                            );
                        }
                    });
                }
            });

        if let Some(msg) = &self.status_msg {
            ui.add_space(6.0);
            ui.colored_label(egui::Color32::from_rgb(130, 200, 255), msg);
        }
    }
}

impl Drop for HostPanelState {
    fn drop(&mut self) {
        self.stop_host();
    }
}

fn copy_to_clipboard(text: &str, status: &mut Option<String>, ok_msg: &str) {
    match Clipboard::new().and_then(|mut cb| cb.set_text(text.to_owned())) {
        Ok(()) => *status = Some(ok_msg.to_string()),
        Err(err) => *status = Some(format!("Error copiando: {err}")),
    }
}
