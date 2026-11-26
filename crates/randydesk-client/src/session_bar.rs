//! Barra flotante superior estilo RustDesk y tarjeta de métricas en sesión remota.

use crate::net::SessionStats;
use crate::theme::{ACCENT_BLUE, ACCENT_TEAL, BORDER_SUBTLE, STATUS_ORANGE, TEXT_MUTED};
use crate::win_hook::KeyboardMode;
use eframe::egui::{self, Color32, Rounding, Stroke, Vec2};
use randydesk_proto::CodecKind;

/// Modo de escalado del escritorio remoto dentro de la ventana del visor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScaleMode {
    /// Ajusta la imagen al área disponible preservando la relación de aspecto.
    #[default]
    Adaptive,
    /// Muestra píxeles 1:1 con barras de desplazamiento si excede la ventana.
    Original,
}

/// Perfil predefinido de calidad/latencia de imagen estilo RustDesk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QualityPreset {
    /// Prioriza nitidez de detalles en JPEG.
    High,
    /// Equilibrio recomendado entre nitidez y fluidez sobre Internet.
    #[default]
    Balanced,
    /// Prioriza mínima latencia y máxima tasa de cuadros.
    Fast,
    /// Configuración manual mediante deslizadores.
    Custom,
}

/// Datos de solo lectura mostrados en la barra flotante y el monitor de calidad.
pub struct SessionBarInput<'a> {
    pub host_name: &'a str,
    pub width: u32,
    pub height: u32,
    pub stats: &'a SessionStats,
    pub reconnecting: Option<(u32, &'a str)>,
    pub hook_installed: bool,
    pub hook_hits: u64,
}

/// Estado mutable compartido entre la UI de sesión y la barra flotante.
pub struct SessionBarState {
    pub codec: CodecKind,
    pub quality: u8,
    pub fps: u8,
    pub scale_mode: ScaleMode,
    pub quality_preset: QualityPreset,
    pub show_monitor_stats: bool,
    pub fullscreen: bool,
    pub keyboard_grab_enabled: bool,
    pub keyboard_mode: KeyboardMode,
}

/// Acciones disparadas por el usuario desde la barra flotante durante un frame.
#[derive(Default)]
pub struct SessionBarActions {
    pub settings_changed: bool,
    pub send_win_key: bool,
    pub send_task_manager: bool,
    pub send_clipboard: bool,
    pub fetch_clipboard: bool,
    pub request_keyframe: bool,
    pub toggle_fullscreen: bool,
    pub disconnect: bool,
    pub keyboard_cfg_changed: bool,
}

/// Renderiza la barra flotante superior centrada y, si está activa, la tarjeta de métricas.
pub fn render_session_overlay(
    ctx: &egui::Context,
    input: SessionBarInput<'_>,
    state: &mut SessionBarState,
) -> SessionBarActions {
    let mut actions = SessionBarActions::default();

    let bar_fill = Color32::from_rgba_unmultiplied(24, 28, 36, 240);
    egui::Area::new(egui::Id::new("session_floating_bar"))
        .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 6.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(bar_fill)
                .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;

                        let (dot_rect, _) =
                            ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                        let dot_color = if input.reconnecting.is_some() {
                            STATUS_ORANGE
                        } else {
                            ACCENT_TEAL
                        };
                        ui.painter().circle_filled(dot_rect.center(), 4.0, dot_color);

                        if let Some((attempt, _)) = input.reconnecting {
                            ui.colored_label(
                                STATUS_ORANGE,
                                format!("Reconectando #{attempt}"),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new(input.host_name)
                                    .size(12.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                        }

                        ui.separator();

                        ui.menu_button("Pantalla", |ui| {
                            ui.set_min_width(210.0);
                            if ui
                                .selectable_label(
                                    state.scale_mode == ScaleMode::Adaptive,
                                    "Escala adaptativa",
                                )
                                .clicked()
                            {
                                state.scale_mode = ScaleMode::Adaptive;
                                ui.close_menu();
                            }
                            if ui
                                .selectable_label(
                                    state.scale_mode == ScaleMode::Original,
                                    "Escala original",
                                )
                                .clicked()
                            {
                                state.scale_mode = ScaleMode::Original;
                                ui.close_menu();
                            }

                            ui.separator();

                            ui.menu_button("Calidad de imagen", |ui| {
                                ui.set_min_width(225.0);
                                if ui
                                    .selectable_label(
                                        state.quality_preset == QualityPreset::High
                                            && state.codec == CodecKind::Jpeg,
                                        "Buena calidad de imagen",
                                    )
                                    .clicked()
                                {
                                    state.codec = CodecKind::Jpeg;
                                    state.quality = 88;
                                    state.fps = 30;
                                    state.quality_preset = QualityPreset::High;
                                    actions.settings_changed = true;
                                    ui.close_menu();
                                }
                                if ui
                                    .selectable_label(
                                        state.quality_preset == QualityPreset::Balanced
                                            && state.codec == CodecKind::Jpeg,
                                        "Equilibrado",
                                    )
                                    .clicked()
                                {
                                    state.codec = CodecKind::Jpeg;
                                    state.quality = 72;
                                    state.fps = 30;
                                    state.quality_preset = QualityPreset::Balanced;
                                    actions.settings_changed = true;
                                    ui.close_menu();
                                }
                                if ui
                                    .selectable_label(
                                        state.quality_preset == QualityPreset::Fast
                                            && state.codec == CodecKind::Jpeg,
                                        "Optimizar el tiempo de reacción",
                                    )
                                    .clicked()
                                {
                                    state.codec = CodecKind::Jpeg;
                                    state.quality = 50;
                                    state.fps = 30;
                                    state.quality_preset = QualityPreset::Fast;
                                    actions.settings_changed = true;
                                    ui.close_menu();
                                }

                                ui.separator();
                                ui.label(
                                    egui::RichText::new("Personalizado")
                                        .size(11.5)
                                        .color(TEXT_MUTED),
                                );
                                if ui
                                    .add(
                                        egui::Slider::new(&mut state.quality, 35..=95)
                                            .text("Calidad JPEG"),
                                    )
                                    .changed()
                                {
                                    state.codec = CodecKind::Jpeg;
                                    state.quality_preset = QualityPreset::Custom;
                                    actions.settings_changed = true;
                                }
                                if ui
                                    .add(egui::Slider::new(&mut state.fps, 5..=60).text("FPS"))
                                    .changed()
                                {
                                    state.quality_preset = QualityPreset::Custom;
                                    actions.settings_changed = true;
                                }
                            });

                            ui.menu_button("Códec", |ui| {
                                ui.set_min_width(185.0);
                                if ui
                                    .selectable_label(
                                        state.codec == CodecKind::Jpeg,
                                        "Automático (JPEG)",
                                    )
                                    .clicked()
                                {
                                    state.codec = CodecKind::Jpeg;
                                    actions.settings_changed = true;
                                    ui.close_menu();
                                }
                                if ui
                                    .selectable_label(
                                        state.codec == CodecKind::Lz4Rgba,
                                        "Sin pérdida (LZ4 Nítido)",
                                    )
                                    .clicked()
                                {
                                    state.codec = CodecKind::Lz4Rgba;
                                    actions.settings_changed = true;
                                    ui.close_menu();
                                }
                            });

                            ui.separator();

                            ui.checkbox(
                                &mut state.show_monitor_stats,
                                "Mostrar calidad del monitor",
                            );
                            if ui
                                .checkbox(&mut state.fullscreen, "Pantalla completa")
                                .changed()
                            {
                                actions.toggle_fullscreen = true;
                                ui.close_menu();
                            }
                        });

                        ui.menu_button("Acciones", |ui| {
                            ui.set_min_width(235.0);
                            if ui.button("Enviar tecla Windows (Inicio)").clicked() {
                                actions.send_win_key = true;
                                ui.close_menu();
                            }
                            if ui
                                .button("Administrador de tareas (Ctrl+Shift+Esc)")
                                .clicked()
                            {
                                actions.send_task_manager = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Enviar portapapeles").clicked() {
                                actions.send_clipboard = true;
                                ui.close_menu();
                            }
                            if ui.button("Traer portapapeles").clicked() {
                                actions.fetch_clipboard = true;
                                ui.close_menu();
                            }
                            if ui.button("Actualizar pantalla").clicked() {
                                actions.request_keyframe = true;
                                ui.close_menu();
                            }
                        });

                        ui.menu_button("Teclado", |ui| {
                            ui.set_min_width(230.0);
                            if ui
                                .selectable_label(
                                    state.keyboard_mode == KeyboardMode::Translate,
                                    "Modo traducción (Texto + Atajos)",
                                )
                                .clicked()
                            {
                                state.keyboard_mode = KeyboardMode::Translate;
                                actions.keyboard_cfg_changed = true;
                                ui.close_menu();
                            }
                            if ui
                                .selectable_label(
                                    state.keyboard_mode == KeyboardMode::Map,
                                    "Modo mapa (Teclado físico)",
                                )
                                .clicked()
                            {
                                state.keyboard_mode = KeyboardMode::Map;
                                actions.keyboard_cfg_changed = true;
                                ui.close_menu();
                            }

                            ui.separator();

                            if ui
                                .checkbox(
                                    &mut state.keyboard_grab_enabled,
                                    "Capturar tecla Win y Alt+Tab",
                                )
                                .changed()
                            {
                                actions.keyboard_cfg_changed = true;
                            }

                            let (status_txt, status_col) = if !state.keyboard_grab_enabled {
                                ("Estado hook: Desactivado".to_string(), TEXT_MUTED)
                            } else if input.hook_installed {
                                (
                                    format!("Estado hook: Activo ({})", input.hook_hits),
                                    ACCENT_TEAL,
                                )
                            } else {
                                ("Estado hook: Hook no disponible".to_string(), STATUS_ORANGE)
                            };
                            ui.label(egui::RichText::new(status_txt).size(11.5).color(status_col));
                        });

                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Desconectar")
                                        .size(12.0)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(Color32::from_rgb(198, 56, 56)),
                            )
                            .clicked()
                        {
                            actions.disconnect = true;
                        }
                    });
                });
        });

    if state.show_monitor_stats {
        egui::Area::new(egui::Id::new("session_monitor_card"))
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-12.0, 52.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(18, 21, 28, 215))
                    .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        let bw = if input.stats.kbps >= 1024.0 {
                            format!("{:.2} MB/s", input.stats.kbps / 8192.0)
                        } else {
                            format!("{:.0} kbps", input.stats.kbps)
                        };
                        let rtt = input
                            .stats
                            .rtt_ms
                            .map(|r| format!("{r} ms"))
                            .unwrap_or_else(|| "-- ms".to_string());
                        let codec_str = match state.codec {
                            CodecKind::Jpeg => format!("JPEG {}%", state.quality),
                            CodecKind::Lz4Rgba => "LZ4 Nítido".to_string(),
                        };

                        ui.label(
                            egui::RichText::new(format!(
                                "{}x{} | {:.1} fps | {bw} | {rtt}",
                                input.width, input.height, input.stats.fps
                            ))
                            .size(11.5)
                            .color(ACCENT_BLUE),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "{codec_str} | {}",
                                format_duration(input.stats.elapsed_secs)
                            ))
                            .size(11.0)
                            .color(TEXT_MUTED),
                        );
                    });
            });
    }

    actions
}

/// Formatea una duración en segundos como `HH:MM:SS`.
pub fn format_duration(total_secs: u64) -> String {
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}
