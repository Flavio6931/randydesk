//! Cuadrícula visual Obsidian Craft de tarjetas de equipos guardados y recientes.

use crate::config::{display_address, normalize_address, SavedHost};
use crate::theme::{
    draw_star, draw_windows_logo, paint_vertical_gradient_banner, ACCENT_BLUE, ACCENT_TEAL,
    BANNER_GRADIENTS, BG_CARD, BORDER_SUBTLE, STATUS_ORANGE, TEXT_MUTED, TEXT_PRIMARY,
    TEXT_SECONDARY,
};
use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
use randydesk_proto::parse_numeric_id;
use std::collections::HashMap;

/// Pestaña activa para filtrar la lista de tarjetas de equipos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTab {
    Recent,
    Favorites,
    All,
}

/// Acción disparada por el usuario sobre una tarjeta de equipo (índice sobre `hosts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAction {
    Connect(usize),
    ToggleFavorite(usize),
    Edit(usize),
    Delete(usize),
}

/// Renderiza la cuadrícula de tarjetas filtrada y ordenada; devuelve la acción seleccionada si hubo clic.
pub fn render_host_cards(
    ui: &mut egui::Ui,
    hosts: &[SavedHost],
    tab: HostTab,
    search: &str,
    online_map: &HashMap<String, bool>,
    thumbnails: &HashMap<String, egui::TextureHandle>,
) -> Option<CardAction> {
    let query = search.trim().to_lowercase();
    let mut indices: Vec<usize> = hosts
        .iter()
        .enumerate()
        .filter(|(_, h)| match tab {
            HostTab::Recent => h.last_connected > 0 || hosts.len() <= 8,
            HostTab::Favorites => h.favorite,
            HostTab::All => true,
        })
        .filter(|(_, h)| {
            query.is_empty()
                || h.name.to_lowercase().contains(&query)
                || h.address.to_lowercase().contains(&query)
        })
        .map(|(idx, _)| idx)
        .collect();

    if tab == HostTab::Recent {
        indices.sort_by(|&a, &b| hosts[b].last_connected.cmp(&hosts[a].last_connected));
    }

    if indices.is_empty() {
        ui.add_space(28.0);
        ui.vertical_centered(|ui| {
            let empty_msg = match tab {
                HostTab::Favorites => "No hay equipos marcados como favoritos.",
                HostTab::Recent => {
                    "No hay conexiones recientes todavía. Conéctate arriba o pulsa + Agregar."
                }
                HostTab::All => {
                    "No hay equipos guardados todavía. Pulsa + Agregar para guardar tus PCs."
                }
            };
            ui.label(egui::RichText::new(empty_msg).color(TEXT_MUTED));
        });
        return None;
    }

    let mut action: Option<CardAction> = None;
    let card_w = 272.0_f32;
    let gap = 16.0_f32;
    let avail = ui.available_width().max(card_w);
    let cols = (((avail + gap) / (card_w + gap)).floor() as usize).max(1);

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
        for row in indices.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &idx in row {
                    if let Some(act) =
                        render_single_card(ui, idx, &hosts[idx], online_map, thumbnails)
                    {
                        action = Some(act);
                    }
                }
            });
        }
    });

    action
}

fn render_single_card(
    ui: &mut egui::Ui,
    idx: usize,
    host: &SavedHost,
    online_map: &HashMap<String, bool>,
    thumbnails: &HashMap<String, egui::TextureHandle>,
) -> Option<CardAction> {
    let card_size = Vec2::new(272.0, 152.0);
    let banner_h = 90.0;
    let footer_h = card_size.y - banner_h;
    let mut action: Option<CardAction> = None;

    let (card_rect, _) = ui.allocate_exact_size(card_size, Sense::hover());
    let banner_rect = Rect::from_min_size(card_rect.min, Vec2::new(card_size.x, banner_h));
    let footer_rect = Rect::from_min_size(
        Pos2::new(card_rect.min.x, card_rect.min.y + banner_h),
        Vec2::new(card_size.x, footer_h),
    );

    let click_area =
        Rect::from_min_size(card_rect.min, Vec2::new(card_size.x - 42.0, card_size.y));
    let click_resp = ui.interact(
        click_area,
        ui.id().with(("card_click", idx)),
        Sense::click(),
    );
    let banner_only_resp = ui.interact(
        banner_rect,
        ui.id().with(("card_banner", idx)),
        Sense::click(),
    );
    let hovered = click_resp.hovered() || banner_only_resp.hovered();

    let painter = ui.painter();
    let norm_addr = normalize_address(&host.address);
    if let Some(tex) = thumbnails.get(&norm_addr) {
        let inset_rect = banner_rect.shrink2(Vec2::new(1.0, 1.0));
        painter.rect_filled(
            banner_rect,
            Rounding {
                nw: 12.0,
                ne: 12.0,
                sw: 0.0,
                se: 0.0,
            },
            BG_CARD,
        );
        painter.image(
            tex.id(),
            inset_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_filled(
            inset_rect,
            Rounding::ZERO,
            Color32::from_rgba_unmultiplied(8, 12, 20, 56),
        );
    } else {
        let (top_col, bot_col) = BANNER_GRADIENTS[(host.color_idx as usize) % BANNER_GRADIENTS.len()];
        paint_vertical_gradient_banner(painter, banner_rect, 12.0, top_col, bot_col);
    }

    painter.rect_filled(
        footer_rect,
        Rounding {
            nw: 0.0,
            ne: 0.0,
            sw: 12.0,
            se: 12.0,
        },
        BG_CARD,
    );

    // Pill superior izquierda opaca de transporte esperado (Relay E2E vs QUIC LAN)
    let is_direct_ip = host.address.contains('.') || host.address.contains(':');
    let badge_text = if is_direct_ip { "QUIC LAN" } else { "Relay E2E" };
    let badge_rect = Rect::from_min_size(
        Pos2::new(banner_rect.min.x + 10.0, banner_rect.min.y + 10.0),
        Vec2::new(if is_direct_ip { 68.0 } else { 72.0 }, 20.0),
    );
    painter.rect_filled(
        badge_rect,
        Rounding::same(99.0),
        Color32::from_rgb(0x0B, 0x10, 0x1A),
    );
    painter.rect_stroke(
        badge_rect,
        Rounding::same(99.0),
        Stroke::new(1.0_f32, Color32::from_rgb(0x2A, 0x35, 0x4A)),
    );
    painter.text(
        badge_rect.center(),
        egui::Align2::CENTER_CENTER,
        badge_text,
        egui::FontId::proportional(10.5),
        TEXT_PRIMARY,
    );

    // Estrella vectorial superior derecha si es favorito
    if host.favorite {
        draw_star(
            painter,
            Pos2::new(banner_rect.max.x - 18.0, banner_rect.min.y + 19.0),
            7.2,
            STATUS_ORANGE,
        );
    }

    // Logotipo Windows centrado en el banner solo si no hay miniatura real
    if !thumbnails.contains_key(&norm_addr) {
        draw_windows_logo(
            painter,
            Pos2::new(banner_rect.center().x, banner_rect.min.y + 53.0),
            30.0,
        );
    }

    // Pie de tarjeta: nombre arriba con punto de estado + ID/IP abajo
    let name_y = footer_rect.min.y + 21.0;
    let online_state = parse_numeric_id(&host.address).and_then(|id| online_map.get(&id).copied());
    let dot_color = match online_state {
        Some(true) => Color32::from_rgb(0x22, 0xC5, 0x5E),
        Some(false) => Color32::from_rgb(0x64, 0x74, 0x8B),
        None => {
            if host.fingerprint_sha256.is_some() {
                ACCENT_TEAL
            } else {
                STATUS_ORANGE
            }
        }
    };
    painter.circle_filled(
        Pos2::new(footer_rect.min.x + 16.0, name_y),
        3.5,
        dot_color,
    );
    painter.text(
        Pos2::new(footer_rect.min.x + 26.0, name_y),
        egui::Align2::LEFT_CENTER,
        truncate_text(&host.name, 22),
        egui::FontId::proportional(13.0),
        TEXT_PRIMARY,
    );

    let clean_addr = display_address(&host.address);
    painter.text(
        Pos2::new(footer_rect.min.x + 14.0, footer_rect.min.y + 43.0),
        egui::Align2::LEFT_CENTER,
        truncate_text(&clean_addr, 24),
        egui::FontId::proportional(11.5),
        TEXT_SECONDARY,
    );

    // Borde fino (cobalto al pasar el puntero)
    let stroke = if hovered {
        Stroke::new(1.2_f32, ACCENT_BLUE)
    } else {
        Stroke::new(1.0_f32, BORDER_SUBTLE)
    };
    painter.rect_stroke(card_rect, Rounding::same(12.0), stroke);

    // Botón menú "..." limpio sin doble recuadro detrás
    let menu_rect = Rect::from_min_size(
        Pos2::new(footer_rect.max.x - 40.0, footer_rect.min.y + 17.0),
        Vec2::new(28.0, 28.0),
    );
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(menu_rect), |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.menu_button("...", |ui| {
                if ui.button("Conectar").clicked() {
                    action = Some(CardAction::Connect(idx));
                    ui.close_menu();
                }
                let fav_label = if host.favorite {
                    "Quitar de favoritos"
                } else {
                    "Añadir a favoritos"
                };
                if ui.button(fav_label).clicked() {
                    action = Some(CardAction::ToggleFavorite(idx));
                    ui.close_menu();
                }
                if ui.button("Editar").clicked() {
                    action = Some(CardAction::Edit(idx));
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .button(
                        egui::RichText::new("Eliminar")
                            .color(Color32::from_rgb(240, 100, 100)),
                    )
                    .clicked()
                {
                    action = Some(CardAction::Delete(idx));
                    ui.close_menu();
                }
            });
        });
    });

    if action.is_none() && (click_resp.clicked() || banner_only_resp.clicked()) {
        action = Some(CardAction::Connect(idx));
    }

    action
}

fn truncate_text(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{head}...")
    }
}
