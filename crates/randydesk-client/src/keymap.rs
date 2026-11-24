//! Traducción de eventos de puntero y teclado de `egui` hacia eventos Win32 remotos.

use eframe::egui;
use randydesk_proto::MouseButton;

/// Normaliza una posición de puntero al rango `[0.0, 1.0]` dentro del rectángulo de video.
pub fn normalize_pos(pos: egui::Pos2, img_rect: egui::Rect) -> Option<(f32, f32)> {
    if img_rect.width() <= 1.0 || img_rect.height() <= 1.0 {
        return None;
    }
    let x = ((pos.x - img_rect.min.x) / img_rect.width()).clamp(0.0, 1.0);
    let y = ((pos.y - img_rect.min.y) / img_rect.height()).clamp(0.0, 1.0);
    Some((x, y))
}

/// Convierte un botón de puntero `egui` al enumerado del protocolo.
pub fn map_mouse_button(btn: egui::PointerButton) -> Option<MouseButton> {
    match btn {
        egui::PointerButton::Primary => Some(MouseButton::Left),
        egui::PointerButton::Secondary => Some(MouseButton::Right),
        egui::PointerButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

/// Indica si la tecla debe enviarse como Virtual-Key directa (navegación, control o función).
pub fn is_control_or_nav_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ArrowDown
            | egui::Key::ArrowLeft
            | egui::Key::ArrowRight
            | egui::Key::ArrowUp
            | egui::Key::Escape
            | egui::Key::Tab
            | egui::Key::Backspace
            | egui::Key::Enter
            | egui::Key::Insert
            | egui::Key::Delete
            | egui::Key::Home
            | egui::Key::End
            | egui::Key::PageUp
            | egui::Key::PageDown
            | egui::Key::F1
            | egui::Key::F2
            | egui::Key::F3
            | egui::Key::F4
            | egui::Key::F5
            | egui::Key::F6
            | egui::Key::F7
            | egui::Key::F8
            | egui::Key::F9
            | egui::Key::F10
            | egui::Key::F11
            | egui::Key::F12
    )
}

/// Mapea una tecla `egui` al par `(Virtual-Key Win32, es_tecla_extendida)`.
pub fn map_egui_key_to_vk(key: egui::Key) -> Option<(u16, bool)> {
    use egui::Key;
    let res = match key {
        Key::ArrowDown => (0x28, true),
        Key::ArrowLeft => (0x25, true),
        Key::ArrowRight => (0x27, true),
        Key::ArrowUp => (0x26, true),
        Key::Escape => (0x1B, false),
        Key::Tab => (0x09, false),
        Key::Backspace => (0x08, false),
        Key::Enter => (0x0D, false),
        Key::Space => (0x20, false),
        Key::Insert => (0x2D, true),
        Key::Delete => (0x2E, true),
        Key::Home => (0x24, true),
        Key::End => (0x23, true),
        Key::PageUp => (0x21, true),
        Key::PageDown => (0x22, true),
        Key::F1 => (0x70, false),
        Key::F2 => (0x71, false),
        Key::F3 => (0x72, false),
        Key::F4 => (0x73, false),
        Key::F5 => (0x74, false),
        Key::F6 => (0x75, false),
        Key::F7 => (0x76, false),
        Key::F8 => (0x77, false),
        Key::F9 => (0x78, false),
        Key::F10 => (0x79, false),
        Key::F11 => (0x7A, false),
        Key::F12 => (0x7B, false),
        Key::Num0 => (0x30, false),
        Key::Num1 => (0x31, false),
        Key::Num2 => (0x32, false),
        Key::Num3 => (0x33, false),
        Key::Num4 => (0x34, false),
        Key::Num5 => (0x35, false),
        Key::Num6 => (0x36, false),
        Key::Num7 => (0x37, false),
        Key::Num8 => (0x38, false),
        Key::Num9 => (0x39, false),
        Key::A => (0x41, false),
        Key::B => (0x42, false),
        Key::C => (0x43, false),
        Key::D => (0x44, false),
        Key::E => (0x45, false),
        Key::F => (0x46, false),
        Key::G => (0x47, false),
        Key::H => (0x48, false),
        Key::I => (0x49, false),
        Key::J => (0x4A, false),
        Key::K => (0x4B, false),
        Key::L => (0x4C, false),
        Key::M => (0x4D, false),
        Key::N => (0x4E, false),
        Key::O => (0x4F, false),
        Key::P => (0x50, false),
        Key::Q => (0x51, false),
        Key::R => (0x52, false),
        Key::S => (0x53, false),
        Key::T => (0x54, false),
        Key::U => (0x55, false),
        Key::V => (0x56, false),
        Key::W => (0x57, false),
        Key::X => (0x58, false),
        Key::Y => (0x59, false),
        Key::Z => (0x5A, false),
        _ => return None,
    };
    Some(res)
}
