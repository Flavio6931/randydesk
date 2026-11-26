//! Barra lateral minimalista estilo RustDesk ("Tu escritorio") con acceso directo sin contraseña.

use crate::config::display_address;
use crate::theme::{ACCENT_BLUE, ACCENT_TEAL, STATUS_ORANGE, TEXT_MUTED};
use arboard::Clipboard;
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
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

    /// Dibuja la barra lateral minimalista "Tu escritorio".
    pub fn ui(&mut self, ui: &mut egui::Ui, rt_handle: &tokio::runtime::Handle) {
        self.refresh_service_cache(rt_handle, false);

        ui.add_space(10.0);
        ui.label(egui::RichText::new("Tu escritorio").size(16.0).strong());
        ui.label(
            egui::RichText::new("Comparte este ID para que accedan directo a tu PC.")
                .size(11.5)
                .color(TEXT_MUTED),
        );
        ui.add_space(16.0);

        let info = self.runtime_info();
        let is_running = info.as_ref().is_some_and(|i| i.running);

        // Bloque único principal: Tu ID + menú avanzado "..."
        render_accent_block(ui, ACCENT_BLUE, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Tu ID").size(12.0).color(TEXT_MUTED));
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
                                    copy_to_clipboard(&lan_ep, &mut self.status_msg, "IP LAN copiada");
                                    ui.close_menu();
                                }
                            }
                            if let Some(ts) = &i.tailscale_ip {
                                let ts_ep = display_address(&format!("{ts}:{}", i.port));
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

                        if ui.button("Configurar contraseña opcional").clicked() {
                            self.editing_password = !self.editing_password;
                            ui.close_menu();
                        }
                        if !self.password.is_empty() && ui.button("Quitar contraseña").clicked() {
                            self.password.clear();
                            self.start_host(rt_handle);
                            self.status_msg = Some("Acceso directo sin contraseña activado.".into());
                            ui.close_menu();
                        }
                        ui.separator();

                        ui.horizontal(|ui| {
                            ui.label("Puerto LAN:");
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

            if let Some(ref i) = info {
                let id_fmt = format_host_id(&i.host_id);
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(&id_fmt)
                            .size(21.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .sense(egui::Sense::click()),
                );
                if resp.on_hover_text("Clic para copiar tu ID").clicked() {
                    copy_to_clipboard(&id_fmt, &mut self.status_msg, "ID copiado");
                }

                ui.add_space(4.0);
                if i.relay_connected {
                    ui.horizontal(|ui| {
                        let (dot_rect, _) =
                            ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 3.5, ACCENT_TEAL);
                        ui.label(
                            egui::RichText::new("Listo en Internet (Relay)")
                                .size(11.0)
                                .color(ACCENT_TEAL),
                        );
                    });
                } else if i.relay_url.is_some() {
                    ui.horizontal(|ui| {
                        let (dot_rect, _) =
                            ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(dot_rect.center(), 3.5, STATUS_ORANGE);
                        ui.label(
                            egui::RichText::new("Conectando al servidor Relay...")
                                .size(11.0)
                                .color(STATUS_ORANGE),
                        );
                    });
                }

                if let Some(lan) = &i.lan_ip {
                    let lan_txt = display_address(&format!("{lan}:{}", i.port));
                    let lan_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("Red local: {lan_txt}"))
                                .size(11.0)
                                .color(TEXT_MUTED),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if lan_resp.on_hover_text("Clic para copiar IP local").clicked() {
                        copy_to_clipboard(&lan_txt, &mut self.status_msg, "IP local copiada");
                    }
                }

                ui.add_space(4.0);
                if self.password.is_empty() {
                    ui.label(
                        egui::RichText::new("Acceso directo sin contraseña")
                            .size(11.0)
                            .color(TEXT_MUTED),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("Protegido con contraseña opcional")
                            .size(11.0)
                            .color(ACCENT_TEAL),
                    );
                }

                ui.add_space(6.0);
                if self.cached_svc_status == ServiceStatusKind::Running {
                    ui.horizontal(|ui| {
                        let (dot_rect, _) =
                            ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 3.5, ACCENT_TEAL);
                        ui.label(
                            egui::RichText::new("Servicio SYSTEM activo (Control total)")
                                .size(11.0)
                                .color(ACCENT_TEAL),
                        );
                    });
                    if let Some(ref svc_state) = self.cached_svc_state {
                        if svc_state.version != env!("CARGO_PKG_VERSION")
                            && ui
                                .small_button("Actualizar servicio SYSTEM")
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
                                    self.status_msg = Some("Actualizando servicio SYSTEM...".into());
                                    self.last_svc_poll = Instant::now() - Duration::from_secs(2);
                                }
                                Err(err) => self.status_msg = Some(format!("{err}")),
                            }
                        }
                    }
                    if ui
                        .small_button("Desinstalar servicio")
                        .on_hover_text("Detiene y elimina RandyDeskService de Windows")
                        .clicked()
                    {
                        match request_uninstall_service_elevated() {
                            Ok(()) => {
                                self.status_msg = Some("Desinstalando servicio SYSTEM...".into());
                                self.last_svc_poll = Instant::now() - Duration::from_secs(2);
                            }
                            Err(err) => self.status_msg = Some(format!("{err}")),
                        }
                    }
                } else {
                    if i.is_elevated {
                        ui.horizontal(|ui| {
                            let (dot_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                            ui.painter().circle_filled(dot_rect.center(), 3.5, ACCENT_TEAL);
                            ui.label(
                                egui::RichText::new("Modo Administrador activo")
                                    .size(11.0)
                                    .color(ACCENT_TEAL),
                            );
                        });
                    } else if i.uipi_warning {
                        ui.label(
                            egui::RichText::new("Ventana Admin detectada: instala servicio SYSTEM")
                                .size(10.5)
                                .color(STATUS_ORANGE),
                        );
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Instalar servicio (SYSTEM)")
                                    .size(11.5)
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

                    if !i.is_elevated
                        && ui
                            .small_button("Ejecutar solo esta vez como Admin")
                            .on_hover_text("Alternativa temporal sin instalar servicio permanente")
                            .clicked()
                    {
                        match randydesk_host::input::relaunch_as_admin() {
                            Ok(()) => std::process::exit(0),
                            Err(err) => self.status_msg = Some(format!("{err}")),
                        }
                    }
                }
            } else {
                ui.label(egui::RichText::new("Detenido").size(18.0).color(TEXT_MUTED));
            }
        });

        if self.editing_password {
            ui.add_space(14.0);
            render_accent_block(ui, ACCENT_TEAL, |ui| {
                ui.label(
                    egui::RichText::new("Contraseña opcional")
                        .size(12.0)
                        .color(TEXT_MUTED),
                );
                ui.add_space(4.0);
                ui.add_sized(
                    [180.0, 26.0],
                    egui::TextEdit::singleline(&mut self.password),
                );
                ui.add_space(4.0);
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

        if let Some(msg) = &self.status_msg {
            ui.add_space(14.0);
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
    let bar = Rect::from_min_size(
        Pos2::new(rect.min.x, rect.min.y + 2.0),
        Vec2::new(3.0, (rect.height() - 4.0).max(16.0)),
    );
    ui.painter().rect_filled(bar, 1.5, accent);
}

fn copy_to_clipboard(text: &str, status: &mut Option<String>, ok_msg: &str) {
    match Clipboard::new().and_then(|mut cb| cb.set_text(text.to_owned())) {
        Ok(()) => *status = Some(ok_msg.to_string()),
        Err(err) => *status = Some(format!("Error copiando: {err}")),
    }
}
