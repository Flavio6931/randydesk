//! Cuadrícula visual de tarjetas de equipos guardados y recientes estilo RustDesk.

use crate::config::{display_address, SavedHost};
use crate::theme::{
    draw_windows_logo, ACCENT_TEAL, BANNER_COLORS, BG_CARD_FOOTER, BORDER_SUBTLE, STATUS_ORANGE,
    TEXT_MUTED,
};
use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, Rounding, Sense, Stroke, Vec2};

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
                HostTab::Recent => "No hay conexiones recientes todavía. Conéctate arriba o pulsa + Agregar.",
                HostTab::All => "No hay equipos guardados todavía. Pulsa + Agregar para guardar tus PCs.",
            };
            ui.label(egui::RichText::new(empty_msg).color(TEXT_MUTED));
        });
        return None;
    }

    let mut action: Option<CardAction> = None;
    ui.spacing_mut().item_spacing = Vec2::new(14.0, 14.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for idx in indices {
                if let Some(act) = render_single_card(ui, idx, &hosts[idx]) {
                    action = Some(act);
                }
            }
        });
    });

    action
}

fn render_single_card(ui: &mut egui::Ui, idx: usize, host: &SavedHost) -> Option<CardAction> {
    let card_size = Vec2::new(216.0, 126.0);
    let banner_h = 84.0;
    let footer_h = card_size.y - banner_h;
    let mut action: Option<CardAction> = None;

    ui.allocate_ui(card_size, |ui| {
        let card_rect = ui.max_rect();
        let banner_rect = Rect::from_min_size(card_rect.min, Vec2::new(card_size.x, banner_h));
        let footer_rect = Rect::from_min_size(
            Pos2::new(card_rect.min.x, card_rect.min.y + banner_h),
            Vec2::new(card_size.x, footer_h),
        );

        let click_area = Rect::from_min_size(card_rect.min, Vec2::new(card_size.x - 36.0, card_size.y));
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
        let banner_color = BANNER_COLORS[(host.color_idx as usize) % BANNER_COLORS.len()];
        painter.rect_filled(
            banner_rect,
            Rounding {
                nw: 8.0,
                ne: 8.0,
                sw: 0.0,
                se: 0.0,
            },
            banner_color,
        );
        painter.rect_filled(
            footer_rect,
            Rounding {
                nw: 0.0,
                ne: 0.0,
                sw: 8.0,
                se: 8.0,
            },
            BG_CARD_FOOTER,
        );

        // Logotipo Windows centrado + nombre debajo
        draw_windows_logo(
            painter,
            Pos2::new(banner_rect.center().x, banner_rect.min.y + 32.0),
            30.0,
        );
        let label_text = truncate_text(&host.name, 24);
        painter.text(
            Pos2::new(banner_rect.center().x, banner_rect.max.y - 16.0),
            egui::Align2::CENTER_CENTER,
            label_text,
            egui::FontId::proportional(12.0),
            Color32::from_rgb(0xEC, 0xEF, 0xF5),
        );

        // Pie de tarjeta: punto de estado + IP limpia
        let dot_center = Pos2::new(footer_rect.min.x + 14.0, footer_rect.center().y);
        let dot_color = if host.fingerprint_sha256.is_some() {
            ACCENT_TEAL
        } else {
            STATUS_ORANGE
        };
        painter.circle_filled(dot_center, 3.5, dot_color);

        let clean_addr = display_address(&host.address);
        painter.text(
            Pos2::new(footer_rect.min.x + 25.0, footer_rect.center().y),
            egui::Align2::LEFT_CENTER,
            truncate_text(&clean_addr, 20),
            egui::FontId::proportional(13.0),
            Color32::WHITE,
        );

        // Borde suave (resaltado en azul al pasar el puntero)
        let stroke_color = if hovered {
            Color32::from_rgb(0x4C, 0x95, 0xF5)
        } else {
            BORDER_SUBTLE
        };
        painter.rect_stroke(
            card_rect,
            Rounding::same(8.0),
            Stroke::new(1.0_f32, stroke_color),
        );

        // Botón menú "..." en la esquina derecha del footer
        let menu_rect = Rect::from_min_size(
            Pos2::new(footer_rect.max.x - 34.0, footer_rect.min.y + 6.0),
            Vec2::new(28.0, footer_h - 12.0),
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
                        .button(egui::RichText::new("Eliminar").color(Color32::from_rgb(240, 100, 100)))
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
    });

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
