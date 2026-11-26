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

/// Píldoras opacas pre-mezcladas para mantener contraste nítido en `egui`.
pub const PILL_GREEN_BG: Color32 = Color32::from_rgb(0x0E, 0x26, 0x22);
pub const PILL_GREEN_BORDER: Color32 = Color32::from_rgb(0x16, 0x5C, 0x48);
pub const PILL_GREEN_TEXT: Color32 = Color32::from_rgb(0x34, 0xD3, 0x99);

pub const PILL_BLUE_BG: Color32 = Color32::from_rgb(0x14, 0x24, 0x3E);
pub const PILL_BLUE_BORDER: Color32 = Color32::from_rgb(0x28, 0x4D, 0x8A);
pub const PILL_BLUE_TEXT: Color32 = Color32::from_rgb(0x79, 0xB8, 0xFF);

pub const PILL_AMBER_BG: Color32 = Color32::from_rgb(0x2B, 0x21, 0x10);
pub const PILL_AMBER_BORDER: Color32 = Color32::from_rgb(0x78, 0x4B, 0x14);
pub const PILL_AMBER_TEXT: Color32 = Color32::from_rgb(0xFB, 0xBF, 0x24);

pub const PILL_RED_BG: Color32 = Color32::from_rgb(0x2D, 0x13, 0x18);
pub const PILL_RED_BORDER: Color32 = Color32::from_rgb(0x7F, 0x23, 0x2D);

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

/// Pares `(arriba, abajo)` para degradados verticales suaves en banners de tarjetas.
pub const BANNER_GRADIENTS: [(Color32, Color32); 7] = [
    (Color32::from_rgb(0x1E, 0x3A, 0x6E), Color32::from_rgb(0x10, 0x1F, 0x3D)),
    (Color32::from_rgb(0x16, 0x47, 0x42), Color32::from_rgb(0x0D, 0x25, 0x24)),
    (Color32::from_rgb(0x31, 0x28, 0x6B), Color32::from_rgb(0x19, 0x15, 0x39)),
    (Color32::from_rgb(0x24, 0x3F, 0x5A), Color32::from_rgb(0x13, 0x21, 0x31)),
    (Color32::from_rgb(0x4A, 0x23, 0x52), Color32::from_rgb(0x26, 0x12, 0x2C)),
    (Color32::from_rgb(0x2D, 0x31, 0x3C), Color32::from_rgb(0x18, 0x1B, 0x22)),
    (Color32::from_rgb(0x19, 0x3D, 0x52), Color32::from_rgb(0x0D, 0x21, 0x2D)),
];

/// Aplica el tema Obsidian Craft sobre todo el contexto `egui`.
pub fn apply_rustdesk_theme(ctx: &egui::Context) {
    #[cfg(target_os = "windows")]
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "segoe_ui".to_owned(),
            egui::FontData::from_owned(bytes),
        );
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "segoe_ui".to_owned());
        }
        ctx.set_fonts(fonts);
    }
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

/// Pinta un degradado vertical suave con esquinas superiores redondeadas usando una malla de vértices.
pub fn paint_vertical_gradient_banner(
    painter: &egui::Painter,
    rect: Rect,
    top_rounding: f32,
    top: Color32,
    bottom: Color32,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let r = top_rounding.min(rect.height() * 0.5).min(rect.width() * 0.5);
    let lerp = |t: f32| -> Color32 {
        let t = t.clamp(0.0, 1.0);
        Color32::from_rgb(
            (top.r() as f32 + (bottom.r() as f32 - top.r() as f32) * t).round() as u8,
            (top.g() as f32 + (bottom.g() as f32 - top.g() as f32) * t).round() as u8,
            (top.b() as f32 + (bottom.b() as f32 - top.b() as f32) * t).round() as u8,
        )
    };

    let mut mesh = egui::Mesh::default();
    let arc_steps = 8usize;
    let mut y_rows: Vec<(f32, f32)> = Vec::with_capacity(arc_steps + 2);
    for i in 0..=arc_steps {
        let frac = i as f32 / arc_steps as f32;
        let angle = frac * std::f32::consts::FRAC_PI_2;
        let dy = r * (1.0 - angle.cos());
        let inset = r * (1.0 - angle.sin());
        y_rows.push((rect.min.y + dy, inset));
    }
    y_rows.push((rect.max.y, 0.0));

    let h = rect.height().max(1.0);
    for (idx, (y, inset)) in y_rows.iter().enumerate() {
        let t = (*y - rect.min.y) / h;
        let col = lerp(t);
        mesh.colored_vertex(Pos2::new(rect.min.x + inset, *y), col);
        mesh.colored_vertex(Pos2::new(rect.max.x - inset, *y), col);
        if idx > 0 {
            let prev = ((idx - 1) * 2) as u32;
            let curr = (idx * 2) as u32;
            mesh.add_triangle(prev, prev + 1, curr);
            mesh.add_triangle(prev + 1, curr + 1, curr);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// Renderiza un campo de texto de una línea con borde visible, relleno `BG_INPUT` y centrado vertical.
pub fn styled_text_input(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    width: f32,
    height: f32,
    font_size: f32,
) -> egui::Response {
    let (rect, box_resp) =
        ui.allocate_exact_size(Vec2::new(width.max(60.0), height), egui::Sense::click());
    let id = ui.make_persistent_id(("styled_input", hint));
    let focused = ui.memory(|m| m.has_focus(id));
    let hovered = box_resp.hovered();
    let border_col = if focused {
        ACCENT_BLUE
    } else if hovered {
        Color32::from_rgb(0x43, 0x50, 0x6B)
    } else {
        BORDER_HOVER
    };

    ui.painter().rect_filled(rect, Rounding::same(8.0), BG_INPUT);
    ui.painter().rect_stroke(
        rect,
        Rounding::same(8.0),
        Stroke::new(if focused { 1.4_f32 } else { 1.0_f32 }, border_col),
    );

    let inner_rect = rect.shrink2(Vec2::new(12.0, 2.0));
    let mut inner_resp: Option<egui::Response> = None;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let edit = egui::TextEdit::singleline(text)
                .id(id)
                .frame(false)
                .desired_width(inner_rect.width())
                .hint_text(egui::RichText::new(hint).size(font_size).color(TEXT_MUTED))
                .font(egui::FontId::proportional(font_size))
                .text_color(TEXT_PRIMARY);
            inner_resp = Some(ui.add(edit));
        });
    });

    let resp = inner_resp.expect("allocate_new_ui siempre ejecuta su cuerpo");
    if box_resp.clicked() && !resp.has_focus() {
        resp.request_focus();
    }
    resp
}
