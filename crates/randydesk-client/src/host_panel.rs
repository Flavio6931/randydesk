//! Barra lateral Obsidian Craft ("Tu escritorio") con tarjetas estructuradas y gestión del servicio SYSTEM.

use crate::config::display_address;
use crate::theme::{
    draw_lock_icon, draw_status_pill, styled_text_input, ACCENT_BLUE, ACCENT_TEAL, BG_CARD,
    BG_SURFACE_ELEVATED, BORDER_HOVER, BORDER_SUBTLE, PILL_AMBER_BG, PILL_AMBER_BORDER,
    PILL_AMBER_TEXT, PILL_BLUE_BG, PILL_BLUE_BORDER, PILL_BLUE_TEXT, PILL_GREEN_BG,
    PILL_GREEN_BORDER, PILL_GREEN_TEXT, STATUS_ORANGE, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};
use arboard::Clipboard;
use eframe::egui::{self, Color32, Rounding, Stroke, Vec2};
use randydesk_host::service::{HostRuntimeInfo, HostService};
use randydesk_host::win_service::{
    query_service_status, read_service_runtime_state, request_install_service_elevated,
    request_uninstall_service_elevated, save_service_config, ServiceConfig, ServiceRuntimeState,
    ServiceStatusKind,
};
use randydesk_proto::{format_host_id, StreamSettings, DEFAULT_PORT};
use std::time::{Duration, Instant};

/// Estado del servidor host embebido dentro de la aplicación gráfica.
pub struct HostPanelState {
    service: Option<HostService>,
    port: u16,
    port_text: String,
    password: String,
    relay_url: Option<String>,
    editing_password: bool,
    monitor_index: usize,
    status_msg: Option<String>,
    last_svc_poll: Instant,
    cached_svc_status: ServiceStatusKind,
    cached_svc_state: Option<ServiceRuntimeState>,
}

impl HostPanelState {
    /// Crea el estado; si el servicio `SYSTEM` ya corre en segundo plano, cede el puerto al servicio.
    pub fn new(rt_handle: &tokio::runtime::Handle, relay_url: Option<String>) -> Self {
        let initial_pwd = std::env::var("RANDYDESK_PASSWORD").unwrap_or_default();
        let svc_status = query_service_status();
        let svc_state = if svc_status == ServiceStatusKind::Running {
            read_service_runtime_state()
        } else {
            None
        };

        let mut state = Self {
            service: None,
            port: DEFAULT_PORT,
            port_text: DEFAULT_PORT.to_string(),
            password: initial_pwd,
            relay_url,
            editing_password: false,
            monitor_index: 0,
            status_msg: None,
            last_svc_poll: Instant::now(),
            cached_svc_status: svc_status,
            cached_svc_state: svc_state,
        };
        if svc_status != ServiceStatusKind::Running {
            state.start_host(rt_handle);
        }
        state
    }

    fn refresh_service_cache(&mut self, rt_handle: &tokio::runtime::Handle, force: bool) {
        if !force && self.last_svc_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_svc_poll = Instant::now();
        let status = query_service_status();
        self.cached_svc_status = status;
        self.cached_svc_state = if status == ServiceStatusKind::Running {
            self.stop_host();
            read_service_runtime_state()
        } else {
            if self.service.is_none() {
                self.start_host(rt_handle);
            }
            None
        };
    }

    /// Actualiza la URL del servidor Relay y reinicia el servicio host (o guarda `ServiceConfig`).
    pub fn set_relay_url(&mut self, relay_url: Option<String>, rt_handle: &tokio::runtime::Handle) {
        self.relay_url = relay_url.clone();
        if self.cached_svc_status == ServiceStatusKind::Running {
            let _ = save_service_config(&ServiceConfig {
                port: self.port,
                password: self.password.clone(),
                relay_url,
                monitor: self.monitor_index,
            });
            self.status_msg = Some("Configuración guardada para el servicio SYSTEM.".into());
            return;
        }
        self.start_host(rt_handle);
    }

    /// Inicia o reinicia el servicio host con el puerto, contraseña (opcional) y Relay actuales.
    pub fn start_host(&mut self, rt_handle: &tokio::runtime::Handle) {
        self.stop_host();
        if query_service_status() == ServiceStatusKind::Running {
            self.cached_svc_status = ServiceStatusKind::Running;
            self.cached_svc_state = read_service_runtime_state();
            return;
        }
        match HostService::start(
            rt_handle,
            self.port,
            self.password.clone(),
            self.monitor_index,
            StreamSettings::default(),
            self.relay_url.clone(),
        ) {
            Ok(svc) => {
                self.service = Some(svc);
                self.editing_password = false;
                self.status_msg = None;
            }
            Err(err) => {
                self.status_msg = Some(format!("Puerto {} ocupado: {err}", self.port));
            }
        }
    }

    /// Detiene el servicio local si estaba activo.
    pub fn stop_host(&mut self) {
        if let Some(mut svc) = self.service.take() {
            svc.stop();
        }
    }

    /// Indica si el servicio de Windows `RandyDeskService` está instalado en el equipo local.
    pub fn is_service_installed(&self) -> bool {
        self.cached_svc_status != ServiceStatusKind::NotInstalled
    }

    /// Devuelve un resumen vivo del servicio (propio o de `RandyDeskService`) para la barra inferior.
    pub fn runtime_info(&self) -> Option<HostRuntimeInfo> {
        if let Some(ref s) = self.cached_svc_state {
            return Some(HostRuntimeInfo {
                running: true,
                host_id: s.host_id.clone(),
                port: s.port,
                lan_ip: s.lan_ip.clone(),
                tailscale_ip: s.tailscale_ip.clone(),
                relay_url: s.relay_url.clone(),
                relay_connected: s.relay_connected,
                fingerprint_sha256: s.fingerprint_sha256.clone(),
                active_clients: s.active_clients,
                is_elevated: true,
                is_system: s.running_as_system,
                uipi_warning: false,
            });
        }
        self.service.as_ref().map(|s| s.info())
    }

    /// Dibuja la barra lateral Obsidian Craft ("Tu escritorio").
    pub fn ui(&mut self, ui: &mut egui::Ui, rt_handle: &tokio::runtime::Handle) {
        self.refresh_service_cache(rt_handle, false);

        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Tu escritorio")
                .size(16.0)
                .strong()
                .color(TEXT_PRIMARY),
        );
        ui.label(
            egui::RichText::new("Comparte este ID para recibir acceso directo.")
                .size(11.5)
                .color(TEXT_SECONDARY),
        );
        ui.add_space(12.0);

        let info = self.runtime_info();
        let is_running = info.as_ref().is_some_and(|i| i.running);

        // Tarjeta 1: TU ID DE ACCESO
        egui::Frame::none()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(12.0))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("TU ID DE ACCESO")
                            .size(10.0)
                            .strong()
                            .color(TEXT_MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button("...", |ui| {
                            if let Some(ref i) = info {
                                let id_fmt = format_host_id(&i.host_id);
                                if ui.button(format!("Copiar ID ({id_fmt})")).clicked() {
                                    copy_to_clipboard(&id_fmt, &mut self.status_msg, "ID copiado");
                                    ui.close_menu();
                                }
                                if let Some(lan) = &i.lan_ip {
                                    let lan_ep = display_address(&format!("{lan}:{}", i.port));
                                    if ui.button(format!("Copiar IP LAN ({lan_ep})")).clicked() {
                                        copy_to_clipboard(
                                            &lan_ep,
                                            &mut self.status_msg,
                                            "IP LAN copiada",
                                        );
                                        ui.close_menu();
                                    }
                                }
                                if let Some(ts) = &i.tailscale_ip {
                                    let ts_ep = display_address(&format!("{ts}:{}", i.port));
                                    if ui.button(format!("Copiar Tailscale ({ts_ep})")).clicked() {
                                        copy_to_clipboard(
                                            &ts_ep,
                                            &mut self.status_msg,
                                            "IP Tailscale copiada",
                                        );
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
                                    egui::RichText::new(format!(
                                        "Sesiones activas: {}",
                                        i.active_clients
                                    ))
                                    .small()
                                    .color(TEXT_MUTED),
                                );
                                ui.separator();
                            }

                            if ui.button("Configurar contraseña opcional").clicked() {
                                self.editing_password = !self.editing_password;
                                ui.close_menu();
                            }
                            if !self.password.is_empty()
                                && ui.button("Quitar contraseña").clicked()
                            {
                                self.password.clear();
                                self.start_host(rt_handle);
                                self.status_msg =
                                    Some("Acceso directo sin contraseña activado.".into());
                                ui.close_menu();
                            }
                            ui.separator();

                            ui.horizontal(|ui| {
                                ui.label("Puerto LAN:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.port_text)
                                        .desired_width(54.0),
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

                        if let Some(ref i) = info {
                            let id_fmt = format_host_id(&i.host_id);
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Copiar")
                                            .size(10.5)
                                            .strong()
                                            .color(PILL_BLUE_TEXT),
                                    )
                                    .fill(PILL_BLUE_BG)
                                    .stroke(Stroke::new(1.0_f32, PILL_BLUE_BORDER))
                                    .rounding(Rounding::same(5.0))
                                    .min_size(Vec2::new(50.0, 20.0)),
                                )
                                .clicked()
                            {
                                copy_to_clipboard(&id_fmt, &mut self.status_msg, "ID copiado");
                            }
                        }
                    });
                });

                ui.add_space(6.0);
                if let Some(ref i) = info {
                    let id_fmt = format_host_id(&i.host_id);
                    let resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new(&id_fmt)
                                .size(24.0)
                                .strong()
                                .color(TEXT_PRIMARY),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if resp.on_hover_text("Clic para copiar tu ID").clicked() {
                        copy_to_clipboard(&id_fmt, &mut self.status_msg, "ID copiado");
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if i.relay_connected {
                            draw_status_pill(
                                ui,
                                "Relay E2E",
                                ACCENT_TEAL,
                                PILL_GREEN_TEXT,
                                PILL_GREEN_BG,
                                PILL_GREEN_BORDER,
                            );
                        } else if i.relay_url.is_some() {
                            draw_status_pill(
                                ui,
                                "Conectando...",
                                STATUS_ORANGE,
                                PILL_AMBER_TEXT,
                                PILL_AMBER_BG,
                                PILL_AMBER_BORDER,
                            );
                        } else {
                            draw_status_pill(
                                ui,
                                "Solo LAN",
                                TEXT_MUTED,
                                TEXT_SECONDARY,
                                BG_SURFACE_ELEVATED,
                                BORDER_SUBTLE,
                            );
                        }

                        let lan_label = format!("LAN :{}", i.port);
                        let lan_btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new(lan_label)
                                    .size(10.5)
                                    .color(TEXT_SECONDARY),
                            )
                            .fill(BG_SURFACE_ELEVATED)
                            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                            .rounding(Rounding::same(99.0))
                            .min_size(Vec2::new(60.0, 22.0)),
                        );
                        if let Some(lan) = &i.lan_ip {
                            let lan_txt = display_address(&format!("{lan}:{}", i.port));
                            if lan_btn
                                .on_hover_text(format!("IP local: {lan_txt} (clic para copiar)"))
                                .clicked()
                            {
                                copy_to_clipboard(&lan_txt, &mut self.status_msg, "IP local copiada");
                            }
                        }
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        if self.password.is_empty() {
                            ui.label(
                                egui::RichText::new("Sin contraseña")
                                    .size(11.5)
                                    .color(TEXT_SECONDARY),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("PIN activo")
                                    .size(11.5)
                                    .strong()
                                    .color(ACCENT_TEAL),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .link(
                                    egui::RichText::new("Configurar PIN")
                                        .size(11.5)
                                        .color(ACCENT_BLUE),
                                )
                                .clicked()
                            {
                                self.editing_password = !self.editing_password;
                            }
                        });
                    });
                } else {
                    ui.label(
                        egui::RichText::new("Detenido")
                            .size(18.0)
                            .color(TEXT_MUTED),
                    );
                }
            });

        if self.editing_password {
            ui.add_space(10.0);
            egui::Frame::none()
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0_f32, PILL_GREEN_BORDER))
                .rounding(Rounding::same(12.0))
                .inner_margin(egui::Margin::same(12.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Contraseña opcional")
                            .size(12.0)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );
                    ui.add_space(6.0);
                    styled_text_input(
                        ui,
                        &mut self.password,
                        "Sin contraseña...",
                        ui.available_width(),
                        30.0,
                        12.5,
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.small_button("Guardar").clicked() {
                            self.start_host(rt_handle);
                            self.status_msg = Some(if self.password.is_empty() {
                                "Acceso directo sin contraseña.".into()
                            } else {
                                "Contraseña opcional activada.".into()
                            });
                        }
                        if ui.small_button("Cancelar").clicked() {
                            self.editing_password = false;
                        }
                    });
                });
        }

        // Tarjeta 2: Servicio SYSTEM
        ui.add_space(12.0);
        egui::Frame::none()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
            .rounding(Rounding::same(12.0))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let dot_color = if self.cached_svc_status == ServiceStatusKind::Running {
                        ACCENT_TEAL
                    } else if info.as_ref().is_some_and(|i| i.uipi_warning) {
                        STATUS_ORANGE
                    } else {
                        TEXT_MUTED
                    };
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 3.5, dot_color);
                    ui.label(
                        egui::RichText::new("Servicio SYSTEM")
                            .size(12.5)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );

                    if self.cached_svc_status == ServiceStatusKind::Running {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            egui::Frame::none()
                                .fill(PILL_GREEN_BG)
                                .stroke(Stroke::new(1.0_f32, PILL_GREEN_BORDER))
                                .rounding(Rounding::same(4.0))
                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("UAC + SAS")
                                            .size(9.5)
                                            .strong()
                                            .color(PILL_GREEN_TEXT),
                                    );
                                });
                        });
                    }
                });

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Captura pantallas de bloqueo, UAC y Ctrl+Alt+Del automáticamente.",
                    )
                    .size(11.0)
                    .color(TEXT_SECONDARY),
                );
                ui.add_space(8.0);

                if let Some(ref i) = info {
                    if self.cached_svc_status == ServiceStatusKind::Running {
                        ui.horizontal(|ui| {
                            if let Some(ref svc_state) = self.cached_svc_state {
                                if svc_state.version != env!("CARGO_PKG_VERSION")
                                    && ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("Actualizar")
                                                    .size(11.0)
                                                    .color(TEXT_PRIMARY),
                                            )
                                            .fill(BG_SURFACE_ELEVATED)
                                            .stroke(Stroke::new(1.0_f32, BORDER_HOVER)),
                                        )
                                        .on_hover_text("Actualiza el servicio instalado a esta versión")
                                        .clicked()
                                {
                                    match request_install_service_elevated(
                                        &i.host_id,
                                        self.port,
                                        &self.password,
                                        self.relay_url.as_deref(),
                                    ) {
                                        Ok(()) => {
                                            self.status_msg =
                                                Some("Actualizando servicio SYSTEM...".into());
                                            self.last_svc_poll =
                                                Instant::now() - Duration::from_secs(2);
                                        }
                                        Err(err) => self.status_msg = Some(format!("{err}")),
                                    }
                                }
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Desinstalar")
                                            .size(11.0)
                                            .color(TEXT_SECONDARY),
                                    )
                                    .fill(BG_SURFACE_ELEVATED)
                                    .stroke(Stroke::new(1.0_f32, BORDER_HOVER)),
                                )
                                .on_hover_text("Detiene y elimina RandyDeskService de Windows")
                                .clicked()
                            {
                                match request_uninstall_service_elevated() {
                                    Ok(()) => {
                                        self.status_msg =
                                            Some("Desinstalando servicio SYSTEM...".into());
                                        self.last_svc_poll =
                                            Instant::now() - Duration::from_secs(2);
                                    }
                                    Err(err) => self.status_msg = Some(format!("{err}")),
                                }
                            }
                        });
                    } else {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Instalar servicio (SYSTEM)")
                                        .size(11.5)
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(ACCENT_BLUE),
                            )
                            .on_hover_text(
                                "Instala RandyDeskService como NT AUTHORITY\\SYSTEM: control total sobre UAC, pantalla de bloqueo, Ctrl+Alt+Del y apps Admin",
                            )
                            .clicked()
                        {
                            match request_install_service_elevated(
                                &i.host_id,
                                self.port,
                                &self.password,
                                self.relay_url.as_deref(),
                            ) {
                                Ok(()) => {
                                    self.stop_host();
                                    self.status_msg =
                                        Some("Instalando e iniciando servicio SYSTEM...".into());
                                    self.last_svc_poll = Instant::now() - Duration::from_secs(2);
                                }
                                Err(err) => self.status_msg = Some(format!("{err}")),
                            }
                        }

                        if !i.is_elevated {
                            ui.add_space(4.0);
                            if ui
                                .link(
                                    egui::RichText::new("Ejecutar solo esta vez como Admin")
                                        .size(11.0)
                                        .color(TEXT_SECONDARY),
                                )
                                .clicked()
                            {
                                match randydesk_host::input::relaunch_as_admin() {
                                    Ok(()) => std::process::exit(0),
                                    Err(err) => self.status_msg = Some(format!("{err}")),
                                }
                            }
                        }
                    }
                }
            });

        if let Some(msg) = &self.status_msg {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(msg)
                    .size(11.5)
                    .color(Color32::from_rgb(0x60, 0xA5, 0xFA)),
            );
        }

        // Pie de seguridad E2E fijo en la parte inferior del sidebar
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            egui::Frame::none()
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (icon_rect, _) =
                            ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                        draw_lock_icon(ui.painter(), icon_rect.center(), ACCENT_TEAL);
                        ui.label(
                            egui::RichText::new("Cifrado E2E · X25519 + ChaCha20")
                                .size(10.5)
                                .color(TEXT_SECONDARY),
                        );
                    });
                });
        });
    }
}

impl Drop for HostPanelState {
    fn drop(&mut self) {
        self.stop_host();
    }
}

fn copy_to_clipboard(text: &str, status: &mut Option<String>, ok_msg: &str) {
    match Clipboard::new().and_then(|mut cb| cb.set_text(text.to_string())) {
        Ok(()) => *status = Some(format!("{ok_msg}.")),
        Err(err) => *status = Some(format!("No se pudo copiar: {err}")),
    }
}
