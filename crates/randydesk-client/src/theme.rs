//! Paleta Obsidian Craft (Linear/Raycast Dark), estilos `egui` e iconos vectoriales limpios.

use eframe::egui::{self, Color32, Pos2, Rect, Rounding, Shape, Stroke, Vec2};

pub const BG_MAIN: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x11);
pub const BG_SIDEBAR: Color32 = Color32::from_rgb(0x10, 0x13, 0x19);
pub const BG_CARD: Color32 = Color32::from_rgb(0x14, 0x18, 0x21);
pub const BG_SURFACE_ELEVATED: Color32 = Color32::from_rgb(0x1B, 0x20, 0x2C);
pub const BG_INPUT: Color32 = Color32::from_rgb(0x0D, 0x10, 0x16);

pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x1F, 0x25, 0x33);
pub const BORDER_HOVER: Color32 = Color32::from_rgb(0x31, 0x3B, 0x52);

pub const ACCENT_BLUE: Color32 = Color32::from_rgb(0x2F, 0x81, 0xF7);
pub const ACCENT_TEAL: Color32 = Color32::from_rgb(0x10, 0xB9, 0x81);
pub const ACCENT_TEAL_SOLID: Color32 = Color32::from_rgb(0x05, 0x96, 0x69);
pub const STATUS_ORANGE: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
pub const ACCENT_DANGER_TEXT: Color32 = Color32::from_rgb(0xFC, 0xA5, 0xA5);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xF3, 0xF5, 0xF9);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x9C, 0xA3, 0xB5);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x64, 0x6D, 0x82);

/// Paleta de 7 tonos profundos Obsidian Craft para los banners superiores de tarjetas.
pub const BANNER_COLORS: [Color32; 7] = [
    Color32::from_rgb(0x16, 0x2C, 0x54), // Cobalto profundo
    Color32::from_rgb(0x12, 0x36, 0x33), // Esmeralda pizarra
    Color32::from_rgb(0x26, 0x1F, 0x54), // Índigo oscuro
    Color32::from_rgb(0x1E, 0x33, 0x48), // Acero azulado
    Color32::from_rgb(0x3C, 0x1D, 0x42), // Ciruela profundo
    Color32::from_rgb(0x23, 0x26, 0x2E), // Grafito sobrio
    Color32::from_rgb(0x14, 0x30, 0x40), // Petróleo nocturno
];

/// Aplica el tema Obsidian Craft sobre todo el contexto `egui`.
pub fn apply_rustdesk_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(14.0);

    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.panel_fill = BG_MAIN;
    visuals.window_fill = BG_SIDEBAR;
    visuals.window_rounding = Rounding::same(12.0);
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER_HOVER);
    visuals.extreme_bg_color = BG_INPUT;

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.rounding = Rounding::same(8.0);

    visuals.widgets.inactive.bg_fill = BG_SURFACE_ELEVATED;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.rounding = Rounding::same(8.0);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x23, 0x2A, 0x3A);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_HOVER);
    visuals.widgets.hovered.rounding = Rounding::same(8.0);

    visuals.widgets.active.bg_fill = Color32::from_rgb(0x1E, 0x3A, 0x63);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_BLUE);
    visuals.widgets.active.rounding = Rounding::same(8.0);

    visuals.selection.bg_fill = ACCENT_BLUE;
    ctx.set_visuals(visuals);
    ctx.set_style(style);
}

/// Dibuja un logotipo minimalista de Windows (4 cuadrantes blancos) en el banner de tarjeta.
pub fn draw_windows_logo(painter: &egui::Painter, center: Pos2, size: f32) {
    let gap = (size * 0.10).max(2.0);
    let tile = (size - gap) * 0.5;
    let half = size * 0.5;
    let color = Color32::from_rgba_unmultiplied(0xF3, 0xF5, 0xF9, 235);

    let offsets = [
        Vec2::new(-half, -half),
        Vec2::new(gap * 0.5, -half),
        Vec2::new(-half, gap * 0.5),
        Vec2::new(gap * 0.5, gap * 0.5),
    ];

    for off in offsets {
        let rect = Rect::from_min_size(center + off, Vec2::splat(tile));
        painter.rect_filled(rect, Rounding::same(2.0), color);
    }
}

/// Dibuja una estrella vectorial de 5 puntas centrada en `center` sin depender de fuentes Unicode.
pub fn draw_star(painter: &egui::Painter, center: Pos2, outer_r: f32, color: Color32) {
    let inner_r = outer_r * 0.42;
    let mut ring = Vec::with_capacity(10);
    for i in 0..10 {
        let angle = -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::PI / 5.0;
        let r = if i % 2 == 0 { outer_r } else { inner_r };
        ring.push(Pos2::new(
            center.x + r * angle.cos(),
            center.y + r * angle.sin(),
        ));
    }
    for i in 0..10 {
        let next = (i + 1) % 10;
        painter.add(Shape::convex_polygon(
            vec![center, ring[i], ring[next]],
            color,
            Stroke::NONE,
        ));
    }
}

/// Dibuja un candado vectorial limpio de 12x12 para el indicador de cifrado E2E.
pub fn draw_lock_icon(painter: &egui::Painter, center: Pos2, color: Color32) {
    let body = Rect::from_center_size(center + Vec2::new(0.0, 2.0), Vec2::new(9.0, 6.5));
    painter.rect_stroke(body, Rounding::same(1.5), Stroke::new(1.4_f32, color));
    let arc_pts = vec![
        center + Vec2::new(-2.6, -1.2),
        center + Vec2::new(-2.6, -3.4),
        center + Vec2::new(-1.3, -4.6),
        center + Vec2::new(1.3, -4.6),
        center + Vec2::new(2.6, -3.4),
        center + Vec2::new(2.6, -1.2),
    ];
    painter.add(Shape::line(arc_pts, Stroke::new(1.4_f32, color)));
}

/// Renderiza una píldora compacta de estado con punto indicador, relleno translúcido y borde sutil.
pub fn draw_status_pill(
    ui: &mut egui::Ui,
    label: &str,
    dot_color: Color32,
    text_color: Color32,
    bg: Color32,
    stroke: Color32,
) {
    egui::Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, stroke))
        .rounding(Rounding::same(99.0))
        .inner_margin(egui::Margin::symmetric(9.0, 3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(7.0), egui::Sense::hover());
                ui.painter().circle_filled(dot_rect.center(), 3.0, dot_color);
                ui.label(egui::RichText::new(label).size(11.0).strong().color(text_color));
            });
        });
}
