//! Barra flotante superior Obsidian Craft colapsable en sesión remota.

use crate::net::SessionStats;
use crate::theme::{
    ACCENT_BLUE, ACCENT_DANGER_TEXT, ACCENT_TEAL, BG_SURFACE_ELEVATED, BORDER_HOVER,
    PILL_AMBER_BG, PILL_AMBER_TEXT, PILL_GREEN_BG, PILL_GREEN_BORDER,
    PILL_GREEN_TEXT, PILL_RED_BG, PILL_RED_BORDER, STATUS_ORANGE, TEXT_MUTED, TEXT_PRIMARY,
    TEXT_SECONDARY,
};
use crate::win_hook::{KeyboardHookGuard, KeyboardMode};
use eframe::egui::{self, Color32, Rounding, Shape, Stroke, Vec2};
use randydesk_proto::{CodecKind, MonitorInfo};

/// Modo de escalado del escritorio remoto dentro de la ventana del visor.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ScaleMode {
    /// Ajusta la imagen al área disponible preservando la relación de aspecto.
    #[default]
    Adaptive,
    /// Muestra píxeles 1:1 con barras de desplazamiento si excede la ventana.
    Original,
    /// Porcentaje manual de zoom en el visor (`50%..=200%`).
    Custom(f32),
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

/// Datos de solo lectura mostrados en la barra flotante de sesión.
pub struct SessionBarInput<'a> {
    pub host_name: &'a str,
    pub stats: &'a SessionStats,
    pub reconnecting: Option<(u32, &'a str)>,
    pub hook_installed: bool,
    pub hook_hits: u64,
    pub monitors: &'a [MonitorInfo],
    pub active_monitor: u8,
}

/// Estado mutable compartido entre la UI de sesión y la barra flotante.
pub struct SessionBarState {
    pub codec: CodecKind,
    pub quality: u8,
    pub fps: u8,
    pub scale_mode: ScaleMode,
    pub custom_scale_pct: f32,
    pub stream_scale: u8,
    pub quality_preset: QualityPreset,
    pub show_remote_cursor: bool,
    pub fullscreen: bool,
    pub keyboard_grab_enabled: bool,
    pub keyboard_mode: KeyboardMode,
    pub invert_wheel: bool,
    pub swap_mouse_buttons: bool,
    pub view_only: bool,
    pub bar_collapsed: bool,
    pub bar_pinned: bool,
    pub auto_clipboard: bool,
    pub input_debug_log: bool,
    pub audio_enabled: bool,
    pub audio_volume: u8,
    pub show_file_manager: bool,
}

/// Acciones disparadas por el usuario desde la barra flotante durante un frame.
#[derive(Default)]
pub struct SessionBarActions {
    pub settings_changed: bool,
    pub stream_scale_changed: bool,
    pub select_monitor: Option<u8>,
    pub send_win_key: bool,
    pub send_task_manager: bool,
    pub send_sas: bool,
    pub lock_remote_screen: bool,
    pub send_clipboard: bool,
    pub fetch_clipboard: bool,
    pub request_keyframe: bool,
    pub toggle_fullscreen: bool,
    pub disconnect: bool,
    pub keyboard_cfg_changed: bool,
    pub debug_log_changed: bool,
    pub audio_changed: bool,
    pub toggle_file_manager: bool,
}

/// Normaliza un porcentaje de escala de transmisión al rango permitido (`50..=100`).
pub fn clamp_stream_scale(pct: u8) -> u8 {
    pct.clamp(50, 100)
}

/// Renderiza la barra flotante superior (expandida o colapsada como pestaña) y el monitor de calidad.
pub fn render_session_overlay(
    ctx: &egui::Context,
    input: SessionBarInput<'_>,
    state: &mut SessionBarState,
) -> SessionBarActions {
    let mut actions = SessionBarActions::default();
    state.stream_scale = clamp_stream_scale(state.stream_scale);

    if state.bar_collapsed {
        let resp = egui::Area::new(egui::Id::new("session_floating_tab"))
            .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 2.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let fill = Color32::from_rgba_unmultiplied(16, 19, 25, 240);
                let frame_resp = egui::Frame::none()
                    .fill(fill)
                    .stroke(Stroke::new(1.0_f32, BORDER_HOVER))
                    .rounding(Rounding {
                        nw: 0.0,
                        ne: 0.0,
                        sw: 8.0,
                        se: 8.0,
                    })
                    .inner_margin(egui::Margin::symmetric(14.0, 4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let (dot_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                            let dot_color = if input.reconnecting.is_some() {
                                STATUS_ORANGE
                            } else {
                                ACCENT_TEAL
                            };
                            ui.painter().circle_filled(dot_rect.center(), 3.5, dot_color);
                            ui.label(
                                egui::RichText::new("RandyDesk")
                                    .size(11.5)
                                    .strong()
                                    .color(TEXT_PRIMARY),
                            );
                            let (tri_rect, _) =
                                ui.allocate_exact_size(Vec2::new(9.0, 6.0), egui::Sense::hover());
                            let c = tri_rect.center();
                            ui.painter().add(Shape::convex_polygon(
                                vec![
                                    egui::pos2(c.x - 4.0, c.y - 2.0),
                                    egui::pos2(c.x + 4.0, c.y - 2.0),
                                    egui::pos2(c.x, c.y + 3.0),
                                ],
                                TEXT_SECONDARY,
                                Stroke::NONE,
                            ));
                        });
                    });
                frame_resp.response.interact(egui::Sense::click())
            })
            .inner;

        if resp.clicked() || (!state.bar_pinned && resp.hovered()) {
            state.bar_collapsed = false;
        }
    } else {
        let bar_fill = Color32::from_rgba_unmultiplied(16, 19, 25, 240);
        let bar_resp = egui::Area::new(egui::Id::new("session_floating_bar"))
            .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 8.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(bar_fill)
                    .stroke(Stroke::new(1.0_f32, BORDER_HOVER))
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 6.0))
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
                                        .color(TEXT_PRIMARY),
                                );

                                if !input.stats.transport_label.is_empty()
                                    || input.stats.rtt_ms.is_some()
                                {
                                    let t_label = match (
                                        input.stats.transport_label.is_empty(),
                                        input.stats.rtt_ms,
                                    ) {
                                        (false, Some(ms)) => {
                                            format!("{} · {ms} ms", input.stats.transport_label)
                                        }
                                        (false, None) => input.stats.transport_label.clone(),
                                        (true, Some(ms)) => format!("{ms} ms"),
                                        (true, None) => String::new(),
                                    };
                                    if !t_label.is_empty() {
                                        egui::Frame::none()
                                            .fill(PILL_GREEN_BG)
                                            .stroke(Stroke::new(1.0_f32, PILL_GREEN_BORDER))
                                            .rounding(Rounding::same(99.0))
                                            .inner_margin(egui::Margin::symmetric(7.0, 2.0))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    egui::RichText::new(t_label)
                                                        .size(10.0)
                                                        .strong()
                                                        .color(PILL_GREEN_TEXT),
                                                );
                                            });
                                    }
                                }
                            }

                            ui.separator();

                            ui.menu_button("Pantalla", |ui| {
                                ui.set_min_width(240.0);
                                if ui
                                    .selectable_label(
                                        matches!(state.scale_mode, ScaleMode::Adaptive),
                                        "Escala adaptativa",
                                    )
                                    .clicked()
                                {
                                    state.scale_mode = ScaleMode::Adaptive;
                                    ui.close_menu();
                                }
                                if ui
                                    .selectable_label(
                                        matches!(state.scale_mode, ScaleMode::Original),
                                        "Escala original",
                                    )
                                    .clicked()
                                {
                                    state.scale_mode = ScaleMode::Original;
                                    ui.close_menu();
                                }
                                if ui
                                    .selectable_label(
                                        matches!(state.scale_mode, ScaleMode::Custom(_)),
                                        "Escala personalizada",
                                    )
                                    .clicked()
                                {
                                    state.scale_mode =
                                        ScaleMode::Custom(state.custom_scale_pct.clamp(50.0, 200.0));
                                }
                                if matches!(state.scale_mode, ScaleMode::Custom(_))
                                    && ui
                                        .add(
                                            egui::Slider::new(
                                                &mut state.custom_scale_pct,
                                                50.0..=200.0,
                                            )
                                            .step_by(5.0)
                                            .suffix("%")
                                            .text("Zoom"),
                                        )
                                        .changed()
                                {
                                    state.scale_mode = ScaleMode::Custom(state.custom_scale_pct);
                                }

                                ui.separator();

                                ui.menu_button("Resolución de transmisión", |ui| {
                                    ui.set_min_width(195.0);
                                    for (pct, label) in [
                                        (100u8, "100% (Nativa)"),
                                        (75u8, "75% (Fluida)"),
                                        (50u8, "50% (Ultra rápida)"),
                                    ] {
                                        if ui
                                            .selectable_label(state.stream_scale == pct, label)
                                            .clicked()
                                        {
                                            if state.stream_scale != pct {
                                                state.stream_scale = pct;
                                                actions.stream_scale_changed = true;
                                            }
                                            ui.close_menu();
                                        }
                                    }
                                });

                                ui.menu_button("Calidad de imagen", |ui| {
                                    ui.set_min_width(235.0);
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
                                        if state.stream_scale != 100 {
                                            state.stream_scale = 100;
                                            actions.stream_scale_changed = true;
                                        }
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
                                        if state.stream_scale != 100 {
                                            state.stream_scale = 100;
                                            actions.stream_scale_changed = true;
                                        }
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
                                        if state.stream_scale != 75 {
                                            state.stream_scale = 75;
                                            actions.stream_scale_changed = true;
                                        }
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
                                    ui.set_min_width(215.0);
                                    if ui
                                        .selectable_label(
                                            state.codec == CodecKind::H264,
                                            "H.264 (Fluido / Bajo consumo)",
                                        )
                                        .clicked()
                                    {
                                        state.codec = CodecKind::H264;
                                        actions.settings_changed = true;
                                        ui.close_menu();
                                    }
                                    if ui
                                        .selectable_label(
                                            state.codec == CodecKind::Jpeg,
                                            "Automático (JPEG por bloques)",
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

                                if !input.monitors.is_empty() {
                                    ui.menu_button("Monitor", |ui| {
                                        ui.set_min_width(215.0);
                                        for m in input.monitors {
                                            let suffix = if m.primary { " · Principal" } else { "" };
                                            let lbl = format!(
                                                "{} ({}x{}{suffix})",
                                                m.name, m.width, m.height
                                            );
                                            if ui
                                                .selectable_label(
                                                    m.index == input.active_monitor,
                                                    lbl,
                                                )
                                                .clicked()
                                            {
                                                actions.select_monitor = Some(m.index);
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                    ui.separator();
                                }

                                ui.checkbox(
                                    &mut state.show_remote_cursor,
                                    "Mostrar cursor remoto",
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
                                ui.set_min_width(250.0);
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
                                if ui.button("Bloquear pantalla remota").clicked() {
                                    actions.lock_remote_screen = true;
                                    ui.close_menu();
                                }
                                if ui
                                    .button("Enviar Ctrl + Alt + Del (SAS)")
                                    .on_hover_text("Requiere servicio SYSTEM activo en la PC remota")
                                    .clicked()
                                {
                                    actions.send_sas = true;
                                    ui.close_menu();
                                }
                                ui.separator();
                                ui.checkbox(
                                    &mut state.auto_clipboard,
                                    "Sincronizar portapapeles automático",
                                );
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

                            ui.menu_button("Teclado y ratón", |ui| {
                                ui.set_min_width(288.0);
                                if ui
                                    .selectable_label(
                                        state.keyboard_mode == KeyboardMode::Map,
                                        "Modo mapa (Teclado físico · Recomendado)",
                                    )
                                    .clicked()
                                {
                                    state.keyboard_mode = KeyboardMode::Map;
                                    actions.keyboard_cfg_changed = true;
                                    ui.close_menu();
                                }
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
                                ui.checkbox(&mut state.invert_wheel, "Invertir dirección de rueda");
                                ui.checkbox(
                                    &mut state.swap_mouse_buttons,
                                    "Intercambiar botones de ratón",
                                );
                                if ui
                                    .checkbox(
                                        &mut state.view_only,
                                        "Solo visualización (bloquear entrada)",
                                    )
                                    .changed()
                                {
                                    actions.keyboard_cfg_changed = true;
                                }
                                if ui
                                    .checkbox(
                                        &mut state.input_debug_log,
                                        "Activar log de diagnóstico de teclado",
                                    )
                                    .changed()
                                {
                                    actions.debug_log_changed = true;
                                }

                                ui.separator();
                                let (status_txt, status_col, pill_bg) = if state.view_only {
                                    (
                                        "Estado entrada: Solo visualización".to_string(),
                                        PILL_AMBER_TEXT,
                                        PILL_AMBER_BG,
                                    )
                                } else if !state.keyboard_grab_enabled {
                                    (
                                        "Estado hook: Desactivado".to_string(),
                                        TEXT_MUTED,
                                        BG_SURFACE_ELEVATED,
                                    )
                                } else if input.hook_installed {
                                    if KeyboardHookGuard::is_window_foreground() {
                                        let label = match state.keyboard_mode {
                                            KeyboardMode::Map => format!(
                                                "Teclado: PC remota (Modo físico, {} teclas)",
                                                input.hook_hits
                                            ),
                                            KeyboardMode::Translate => format!(
                                                "Teclado: PC remota (Modo traducción, {} atajos)",
                                                input.hook_hits
                                            ),
                                        };
                                        (label, PILL_GREEN_TEXT, PILL_GREEN_BG)
                                    } else {
                                        (
                                            "Teclado: Tu PC (haz clic aquí para controlar)"
                                                .to_string(),
                                            TEXT_SECONDARY,
                                            BG_SURFACE_ELEVATED,
                                        )
                                    }
                                } else {
                                    (
                                        "Estado hook: Hook no disponible".to_string(),
                                        PILL_AMBER_TEXT,
                                        PILL_AMBER_BG,
                                    )
                                };

                                egui::Frame::none()
                                    .fill(pill_bg)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(
                                            egui::RichText::new(status_txt)
                                                .size(11.0)
                                                .color(status_col),
                                        );
                                    });
                            });

                            ui.separator();

                            ui.menu_button("Audio", |ui| {
                                ui.set_min_width(210.0);
                                if ui
                                    .checkbox(&mut state.audio_enabled, "Escuchar audio remoto")
                                    .changed()
                                {
                                    actions.audio_changed = true;
                                }
                                if ui
                                    .add_enabled(
                                        state.audio_enabled,
                                        egui::Slider::new(&mut state.audio_volume, 0..=100)
                                            .suffix("%")
                                            .text("Volumen"),
                                    )
                                    .changed()
                                {
                                    actions.audio_changed = true;
                                }
                            });

                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Archivos").size(11.5).color(
                                            if state.show_file_manager {
                                                Color32::WHITE
                                            } else {
                                                TEXT_PRIMARY
                                            },
                                        ),
                                    )
                                    .fill(if state.show_file_manager {
                                        ACCENT_BLUE
                                    } else {
                                        BG_SURFACE_ELEVATED
                                    })
                                    .rounding(Rounding::same(6.0)),
                                )
                                .on_hover_text("Abrir gestor de transferencia de archivos (o arrastra archivos aquí)")
                                .clicked()
                            {
                                actions.toggle_file_manager = true;
                            }

                            ui.separator();

                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Fijar").size(11.5).color(
                                            if state.bar_pinned {
                                                Color32::WHITE
                                            } else {
                                                TEXT_SECONDARY
                                            },
                                        ),
                                    )
                                    .fill(if state.bar_pinned {
                                        ACCENT_BLUE
                                    } else {
                                        BG_SURFACE_ELEVATED
                                    })
                                    .rounding(Rounding::same(6.0)),
                                )
                                .on_hover_text(
                                    "Mantener barra fijada arriba (si la desactivas, se oculta al alejar el puntero)",
                                )
                                .clicked()
                            {
                                state.bar_pinned = !state.bar_pinned;
                            }

                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Ocultar")
                                            .size(11.5)
                                            .color(TEXT_SECONDARY),
                                    )
                                    .fill(BG_SURFACE_ELEVATED)
                                    .rounding(Rounding::same(6.0)),
                                )
                                .on_hover_text(
                                    "Contraer la barra en una pestaña superior para no tapar pestañas",
                                )
                                .clicked()
                            {
                                state.bar_collapsed = true;
                            }

                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Desconectar")
                                            .size(11.5)
                                            .color(ACCENT_DANGER_TEXT)
                                            .strong(),
                                    )
                                    .fill(PILL_RED_BG)
                                    .stroke(Stroke::new(1.0_f32, PILL_RED_BORDER))
                                    .rounding(Rounding::same(6.0)),
                                )
                                .clicked()
                            {
                                actions.disconnect = true;
                            }
                        });
                    });
            });

        if !state.bar_pinned && !ctx.memory(|m| m.any_popup_open()) {
            let safe_zone = bar_resp.response.rect.expand2(Vec2::new(20.0, 18.0));
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                if !safe_zone.contains(pos) {
                    state.bar_collapsed = true;
                }
            }
        }
    }

    actions
}

/// Formatea una duración en segundos como `HH:MM:SS`.
#[allow(dead_code)]
pub fn format_duration(total_secs: u64) -> String {
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_session_duration_and_clamps_stream_scale() {
        assert_eq!(format_duration(0), "00:00:00");
        assert_eq!(format_duration(3661), "01:01:01");
        assert_eq!(clamp_stream_scale(10), 50);
        assert_eq!(clamp_stream_scale(75), 75);
        assert_eq!(clamp_stream_scale(150), 100);
    }
}
