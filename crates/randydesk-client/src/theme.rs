//! Paleta de colores, estilo oscuro inspirado en RustDesk y gráficos vectoriales limpios.

use eframe::egui::{self, Color32, Pos2, Rect, Rounding, Stroke, Vec2};

pub const BG_MAIN: Color32 = Color32::from_rgb(0x15, 0x17, 0x1C);
pub const BG_SIDEBAR: Color32 = Color32::from_rgb(0x1E, 0x21, 0x28);
pub const BG_CARD: Color32 = Color32::from_rgb(0x20, 0x23, 0x2B);
pub const BG_CARD_FOOTER: Color32 = Color32::from_rgb(0x25, 0x29, 0x32);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x2E, 0x33, 0x3E);
pub const ACCENT_BLUE: Color32 = Color32::from_rgb(0x00, 0x78, 0xFF);
pub const ACCENT_TEAL: Color32 = Color32::from_rgb(0x1D, 0xB9, 0x7C);
pub const STATUS_ORANGE: Color32 = Color32::from_rgb(0xE8, 0x9A, 0x3C);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x9A, 0xA2, 0xB1);

/// Paleta de 7 tonos sobrios para los banners superiores de las tarjetas de equipos.
pub const BANNER_COLORS: [Color32; 7] = [
    Color32::from_rgb(0x7E, 0x2E, 0x79), // Morado magenta
    Color32::from_rgb(0x1A, 0x41, 0x85), // Azul profundo
    Color32::from_rgb(0x35, 0x23, 0x6B), // Violeta oscuro
    Color32::from_rgb(0x2F, 0x50, 0x70), // Azul acero
    Color32::from_rgb(0x4C, 0x4F, 0x29), // Oliva oscuro
    Color32::from_rgb(0x44, 0x3B, 0x54), // Gris púrpura
    Color32::from_rgb(0x32, 0x1B, 0x3A), // Ciruela oscuro
];

/// Aplica el tema oscuro limpio sobre todo el contexto `egui`.
pub fn apply_rustdesk_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(14.0);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG_MAIN;
    visuals.window_fill = BG_SIDEBAR;
    visuals.window_rounding = Rounding::same(8.0);
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.extreme_bg_color = Color32::from_rgb(0x12, 0x14, 0x19);

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.rounding = Rounding::same(6.0);

    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x28, 0x2C, 0x36);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.rounding = Rounding::same(6.0);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x32, 0x38, 0x45);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_BLUE);
    visuals.widgets.hovered.rounding = Rounding::same(6.0);

    visuals.widgets.active.bg_fill = Color32::from_rgb(0x24, 0x4B, 0x7A);
    visuals.widgets.active.rounding = Rounding::same(6.0);

    visuals.selection.bg_fill = ACCENT_BLUE;
    ctx.set_visuals(visuals);
    ctx.set_style(style);
}

/// Dibuja un logotipo minimalista de Windows (4 cuadrantes blancos) en el banner de tarjeta.
pub fn draw_windows_logo(painter: &egui::Painter, center: Pos2, size: f32) {
    let gap = (size * 0.08).max(1.5);
    let tile = (size - gap) * 0.5;
    let half = size * 0.5;
    let color = Color32::from_rgb(0xF5, 0xF7, 0xFA);

    let offsets = [
        Vec2::new(-half, -half),
        Vec2::new(gap * 0.5, -half),
        Vec2::new(-half, gap * 0.5),
        Vec2::new(gap * 0.5, gap * 0.5),
    ];

    for off in offsets {
        let rect = Rect::from_min_size(center + off, Vec2::splat(tile));
        painter.rect_filled(rect, Rounding::same(1.5), color);
    }
}
