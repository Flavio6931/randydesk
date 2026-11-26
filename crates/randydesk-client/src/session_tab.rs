//! Estado y renderizado encapsulado de cada pestana de sesion remota independiente.

use crate::config::{display_address, normalize_address, save_host_thumbnail};
use crate::decoder::FrameUpdate;
use crate::file_manager::{
    FileManagerAction, FileManagerWindowState, RemoteDirListing, TransferItem, TransferStatus,
};
use crate::keymap::{is_control_or_nav_key, map_egui_key_to_vk, map_mouse_button, normalize_pos};
use crate::net::{ConnectRequest, ConnectionState, NetCommand, NetworkHandle, SessionStats};
use crate::session_bar::{
    render_session_overlay, QualityPreset, ScaleMode, SessionBarInput, SessionBarState,
};
use crate::theme::{ACCENT_BLUE, ACCENT_TEAL, BG_CARD, BORDER_SUBTLE, STATUS_ORANGE, TEXT_MUTED, TEXT_PRIMARY};
use crate::tray::TrayController;
use crate::win_hook::{KeyboardHookGuard, KeyboardMode};
use arboard::Clipboard;
use eframe::egui::{self, Color32, Vec2};
use randydesk_host::clipboard::ClipboardWatcher;
use randydesk_host::input::{append_input_debug_log, set_input_debug_logging};
use randydesk_proto::{CodecKind, CursorKind, InputEvent, MonitorInfo, StreamSettings};
use std::time::{Duration, Instant};

fn map_remote_cursor(kind: CursorKind) -> egui::CursorIcon {
    match kind {
        CursorKind::Default => egui::CursorIcon::Default,
        CursorKind::Text => egui::CursorIcon::Text,
        CursorKind::PointingHand => egui::CursorIcon::PointingHand,
        CursorKind::Crosshair => egui::CursorIcon::Crosshair,
        CursorKind::Move => egui::CursorIcon::Move,
        CursorKind::ResizeHorizontal => egui::CursorIcon::ResizeHorizontal,
        CursorKind::ResizeVertical => egui::CursorIcon::ResizeVertical,
        CursorKind::ResizeNeSw => egui::CursorIcon::ResizeNeSw,
        CursorKind::ResizeNwSe => egui::CursorIcon::ResizeNwSe,
        CursorKind::NotAllowed => egui::CursorIcon::NotAllowed,
        CursorKind::Wait => egui::CursorIcon::Wait,
        CursorKind::Progress => egui::CursorIcon::Progress,
        CursorKind::Hidden => egui::CursorIcon::None,
    }
}

/// Configuracion inicial usada al abrir una pestana de sesion remota.
#[derive(Clone)]
pub struct TabLaunchParams {
    pub id: u64,
    pub title: String,
    pub address: String,
    pub password: String,
    pub expected_fp: Option<String>,
    pub relay_url: Option<String>,
    pub codec: CodecKind,
    pub quality: u8,
    pub fps: u8,
    pub stream_scale: u8,
    pub quality_preset: QualityPreset,
    pub show_remote_cursor: bool,
    pub keyboard_grab_enabled: bool,
    pub keyboard_mode: KeyboardMode,
    pub invert_wheel: bool,
    pub swap_mouse_buttons: bool,
    pub auto_clipboard: bool,
    pub audio_enabled: bool,
    pub audio_volume: u8,
}

/// Snapshot por cuadro del estado de red de una pestana.
pub struct TabPollSnapshot {
    pub status: ConnectionState,
    pub stats: SessionStats,
    pub monitors: Vec<MonitorInfo>,
    pub active_monitor: u8,
    pub remote_cursor: CursorKind,
    pub remote_dir: Option<RemoteDirListing>,
    pub transfers: Vec<TransferItem>,
    pub thumb_updated_addr: Option<String>,
    pub need_recent_upsert: Option<(String, Option<String>)>,
    pub auth_prompt_needed: bool,
}

/// Accion devuelta por el renderizado del cuerpo de una pestana.
pub enum SessionTabOutcome {
    None,
    CloseTab,
    Reconnect,
    ReconnectWithPrompt,
}

/// Sesion remota aislada con su propio `NetworkHandle`, textura GPU y barra flotante.
pub struct RemoteSessionTab {
    pub id: u64,
    pub title: String,
    pub target_address: String,
    pub expected_fp: Option<String>,
    pub codec_input: CodecKind,
    pub quality_input: u8,
    pub fps_input: u8,
    pub net: NetworkHandle,
    pub screen_texture: Option<egui::TextureHandle>,
    pub last_frame_rgba: Vec<u8>,
    pub last_frame_w: u32,
    pub last_frame_h: u32,
    pub last_thumb_save: Instant,
    pub fullscreen: bool,
    pub scale_mode: ScaleMode,
    pub custom_scale_pct: f32,
    pub stream_scale: u8,
    pub quality_preset: QualityPreset,
    pub show_monitor_stats: bool,
    pub show_remote_cursor: bool,
    pub keyboard_grab_enabled: bool,
    pub keyboard_mode: KeyboardMode,
    pub invert_wheel: bool,
    pub swap_mouse_buttons: bool,
    pub view_only: bool,
    pub bar_collapsed: bool,
    pub bar_pinned: bool,
    pub auto_clipboard: bool,
    pub input_debug_log: bool,
    pub clip_watcher: ClipboardWatcher,
    pub last_clip_poll: Instant,
    pub audio_enabled: bool,
    pub audio_volume: u8,
    pub show_file_manager: bool,
    pub file_mgr_ui: FileManagerWindowState,
    pub next_transfer_id: u64,
    pub was_foreground: bool,
    pub focused_on_connect: bool,
    pub remote_ctrl_down: bool,
    pub remote_alt_down: bool,
    pub remote_shift_down: bool,
    pub recorded_recent: bool,
    pub last_error_handled: Option<String>,
}

impl RemoteSessionTab {
    /// Crea e inicia una nueva pestana conectada al equipo remoto indicado.
    pub fn launch(
        params: TabLaunchParams,
        rt_handle: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) -> Self {
        let mut net = NetworkHandle::new(rt_handle.clone());
        net.set_audio_volume(params.audio_volume);
        net.connect(
            ConnectRequest {
                address: params.address.clone(),
                password: params.password,
                expected_fingerprint: params.expected_fp.clone(),
                relay_url: params.relay_url,
                settings: StreamSettings {
                    codec: params.codec,
                    jpeg_quality: params.quality,
                    target_fps: params.fps,
                },
                stream_scale: params.stream_scale,
                monitor_index: 0,
                audio_enabled: params.audio_enabled,
            },
            ctx.clone(),
        );

        Self {
            id: params.id,
            title: params.title,
            target_address: params.address,
            expected_fp: params.expected_fp,
            codec_input: params.codec,
            quality_input: params.quality,
            fps_input: params.fps,
            net,
            screen_texture: None,
            last_frame_rgba: Vec::new(),
            last_frame_w: 0,
            last_frame_h: 0,
            last_thumb_save: Instant::now() - Duration::from_secs(45),
            fullscreen: false,
            scale_mode: ScaleMode::Adaptive,
            custom_scale_pct: 100.0,
            stream_scale: params.stream_scale,
            quality_preset: params.quality_preset,
            show_monitor_stats: true,
            show_remote_cursor: params.show_remote_cursor,
            keyboard_grab_enabled: params.keyboard_grab_enabled,
            keyboard_mode: params.keyboard_mode,
            invert_wheel: params.invert_wheel,
            swap_mouse_buttons: params.swap_mouse_buttons,
            view_only: false,
            bar_collapsed: false,
            bar_pinned: true,
            auto_clipboard: params.auto_clipboard,
            input_debug_log: false,
            clip_watcher: ClipboardWatcher::new(),
            last_clip_poll: Instant::now(),
            audio_enabled: params.audio_enabled,
            audio_volume: params.audio_volume,
            show_file_manager: false,
            file_mgr_ui: FileManagerWindowState::new(),
            next_transfer_id: 1,
            was_foreground: false,
            focused_on_connect: false,
            remote_ctrl_down: false,
            remote_alt_down: false,
            remote_shift_down: false,
            recorded_recent: false,
            last_error_handled: None,
        }
    }

    /// Reintenta la conexion sobre la misma pestana con una contrasena nueva.
    pub fn reconnect_with_password(
        &mut self,
        password: String,
        relay_url: Option<String>,
        ctx: &egui::Context,
    ) {
        self.recorded_recent = false;
        self.focused_on_connect = false;
        self.last_error_handled = None;
        self.clip_watcher.reset();
        self.net.set_audio_volume(self.audio_volume);
        self.net.connect(
            ConnectRequest {
                address: self.target_address.clone(),
                password,
                expected_fingerprint: self.expected_fp.clone(),
                relay_url,
                settings: StreamSettings {
                    codec: self.codec_input,
                    jpeg_quality: self.quality_input,
                    target_fps: self.fps_input,
                },
                stream_scale: self.stream_scale,
                monitor_index: 0,
                audio_enabled: self.audio_enabled,
            },
            ctx.clone(),
        );
    }

    /// Libera cualquier modificador remoto presionado en esta pestana.
    pub fn release_remote_modifiers(&mut self) {
        if self.remote_ctrl_down {
            self.net.send_input(InputEvent::Key {
                vk: 0xA2,
                scan: 0x1D,
                extended: false,
                down: false,
            });
            self.remote_ctrl_down = false;
        }
        if self.remote_alt_down {
            self.net.send_input(InputEvent::Key {
                vk: 0xA4,
                scan: 0x38,
                extended: false,
                down: false,
            });
            self.remote_alt_down = false;
        }
        if self.remote_shift_down {
            self.net.send_input(InputEvent::Key {
                vk: 0xA0,
                scan: 0x2A,
                extended: false,
                down: false,
            });
            self.remote_shift_down = false;
        }
    }

    fn send_remote_ctrl_shortcut(&mut self, vk: u16, scan: u16) {
        let need_temp_ctrl = !self.remote_ctrl_down;
        if need_temp_ctrl {
            self.net.send_input(InputEvent::Key {
                vk: 0xA2,
                scan: 0x1D,
                extended: false,
                down: true,
            });
        }
        self.net.send_input(InputEvent::Key {
            vk,
            scan,
            extended: false,
            down: true,
        });
        self.net.send_input(InputEvent::Key {
            vk,
            scan,
            extended: false,
            down: false,
        });
        if need_temp_ctrl {
            self.net.send_input(InputEvent::Key {
                vk: 0xA2,
                scan: 0x1D,
                extended: false,
                down: false,
            });
        }
    }

    /// Guarda la ultima miniatura en disco y devuelve la direccion normalizada si hubo cambios.
    pub fn save_thumbnail_now(&mut self) -> Option<String> {
        if self.last_frame_w == 0 || self.last_frame_h == 0 || self.last_frame_rgba.is_empty() {
            return None;
        }
        if save_host_thumbnail(
            &self.target_address,
            &self.last_frame_rgba,
            self.last_frame_w as usize,
            self.last_frame_h as usize,
        )
        .is_ok()
        {
            self.last_thumb_save = Instant::now();
            return Some(normalize_address(&self.target_address));
        }
        None
    }

    /// Desconecta la sesion y guarda su miniatura mas reciente antes de cerrar la pestana.
    pub fn close_and_save(&mut self, ctx: &egui::Context) -> Option<String> {
        if self.fullscreen {
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        self.release_remote_modifiers();
        let saved = self.save_thumbnail_now();
        self.net.disconnect();
        saved
    }

    /// Sondea el estado de red, actualiza la textura GPU aunque este en segundo plano y maneja portapapeles activo.
    pub fn poll(
        &mut self,
        ctx: &egui::Context,
        is_active: bool,
        hidden_to_tray: bool,
    ) -> TabPollSnapshot {
        let (
            status,
            stats,
            updates,
            incoming_clip,
            monitors,
            active_monitor,
            remote_cursor,
            remote_dir,
            transfers,
        ) = {
            let mut guard = self.net.shared.lock().expect("lock estado red");
            let clip = guard.incoming_clipboard.take();
            let updates: Vec<FrameUpdate> = guard.pending_updates.drain(..).collect();
            (
                guard.status.clone(),
                guard.stats.clone(),
                updates,
                clip,
                guard.monitors.clone(),
                guard.active_monitor,
                guard.remote_cursor,
                guard.remote_dir.clone(),
                guard.transfers.clone(),
            )
        };

        if let Some(clip_text) = incoming_clip {
            if is_active {
                if let Ok(mut cb) = Clipboard::new() {
                    let _ = cb.set_text(clip_text.clone());
                }
                self.clip_watcher.mark_applied(&clip_text);
            }
        }

        if is_active
            && matches!(status, ConnectionState::Connected { .. })
            && self.auto_clipboard
            && !self.view_only
            && !hidden_to_tray
            && self.last_clip_poll.elapsed() >= Duration::from_millis(350)
        {
            self.last_clip_poll = Instant::now();
            if let Some(text) = self.clip_watcher.poll_changed() {
                self.net.send_cmd(NetCommand::ClipboardSet(text));
            }
        }

        let mut need_recent_upsert = None;
        if let ConnectionState::Connected { ref host_name, .. } = status {
            self.title = host_name.clone();
            if !self.recorded_recent {
                self.recorded_recent = true;
                need_recent_upsert =
                    Some((host_name.clone(), stats.fingerprint_sha256.clone()));
            }
            if is_active && !hidden_to_tray {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }

        let mut auth_prompt_needed = false;
        if let ConnectionState::Error(ref msg) = status {
            if self.last_error_handled.as_deref() != Some(msg.as_str()) {
                self.last_error_handled = Some(msg.clone());
                if msg.contains("Contraseña") || msg.contains("PIN") {
                    auth_prompt_needed = true;
                }
            }
        }

        let mut thumb_updated_addr = None;
        if !updates.is_empty() {
            self.apply_frame_updates_to_gpu(ctx, updates);
            if self.last_thumb_save.elapsed() >= Duration::from_secs(60) {
                thumb_updated_addr = self.save_thumbnail_now();
            }
        }

        TabPollSnapshot {
            status,
            stats,
            monitors,
            active_monitor,
            remote_cursor,
            remote_dir,
            transfers,
            thumb_updated_addr,
            need_recent_upsert,
            auth_prompt_needed,
        }
    }

    fn apply_frame_updates_to_gpu(&mut self, ctx: &egui::Context, updates: Vec<FrameUpdate>) {
        for update in updates {
            let uw = update.width;
            let uh = update.height;
            let target_size = [uw as usize, uh as usize];
            let needs_alloc = match &self.screen_texture {
                Some(tex) => tex.size() != target_size,
                None => true,
            };

            if needs_alloc {
                let blank = egui::ColorImage::new(target_size, Color32::BLACK);
                self.screen_texture = Some(ctx.load_texture(
                    format!("remote-screen-{}", self.id),
                    blank,
                    egui::TextureOptions::LINEAR,
                ));
                self.last_frame_w = uw;
                self.last_frame_h = uh;
                self.last_frame_rgba
                    .resize((uw as usize) * (uh as usize) * 4, 0);
            }

            for tile in update.tiles {
                let tw = tile.width as usize;
                let th = tile.height as usize;
                let tx = tile.x as usize;
                let ty = tile.y as usize;
                let fw = self.last_frame_w as usize;
                let fh = self.last_frame_h as usize;

                if tx + tw <= fw && ty + th <= fh && tile.rgba.len() >= tw * th * 4 {
                    for row in 0..th {
                        let src_off = row * tw * 4;
                        let dst_off = ((ty + row) * fw + tx) * 4;
                        self.last_frame_rgba[dst_off..dst_off + tw * 4]
                            .copy_from_slice(&tile.rgba[src_off..src_off + tw * 4]);
                    }
                }

                if let Some(tex) = &mut self.screen_texture {
                    let tile_img = egui::ColorImage::from_rgba_unmultiplied([tw, th], &tile.rgba);
                    tex.set_partial([tx, ty], tile_img, egui::TextureOptions::LINEAR);
                }
            }
        }
    }

    /// Devuelve el color de estado compacto para la pestana en la barra superior.
    pub fn tab_dot_color(&self) -> Color32 {
        if let Ok(g) = self.net.shared.lock() {
            match g.status {
                ConnectionState::Connected { .. } => ACCENT_TEAL,
                ConnectionState::Connecting { .. } | ConnectionState::Reconnecting { .. } => {
                    STATUS_ORANGE
                }
                ConnectionState::Error(_) => Color32::from_rgb(0xEF, 0x44, 0x44),
                ConnectionState::Disconnected => TEXT_MUTED,
            }
        } else {
            TEXT_MUTED
        }
    }

    /// Renderiza el contenido de la pestana activa (conectando, error o escritorio remoto).
    #[allow(clippy::too_many_arguments)]
    pub fn render_active(
        &mut self,
        ctx: &egui::Context,
        rt_handle: &tokio::runtime::Handle,
        kb_hook: &mut KeyboardHookGuard,
        tray: Option<&TrayController>,
        hidden_to_tray: bool,
        snap: &TabPollSnapshot,
    ) -> SessionTabOutcome {
        match &snap.status {
            ConnectionState::Connecting { address } => {
                self.focused_on_connect = false;
                self.release_remote_modifiers();
                kb_hook.set_capture(false, None);
                let mut close = false;
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.add_space(10.0);
                            ui.heading(format!("Conectando a {}...", display_address(address)));
                            ui.add_space(10.0);
                            if ui.button("Cancelar").clicked() {
                                close = true;
                            }
                        });
                    });
                });
                if close {
                    return SessionTabOutcome::CloseTab;
                }
                SessionTabOutcome::None
            }
            ConnectionState::Error(msg) => {
                self.focused_on_connect = false;
                self.release_remote_modifiers();
                kb_hook.set_capture(false, None);
                let mut outcome = SessionTabOutcome::None;
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        egui::Frame::none()
                            .fill(BG_CARD)
                            .stroke(egui::Stroke::new(1.0_f32, BORDER_SUBTLE))
                            .rounding(egui::Rounding::same(12.0))
                            .inner_margin(egui::Margin::same(22.0))
                            .show(ui, |ui| {
                                ui.set_max_width(420.0);
                                ui.label(
                                    egui::RichText::new("No se pudo establecer la sesion")
                                        .strong()
                                        .size(16.0)
                                        .color(TEXT_PRIMARY),
                                );
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(msg).color(TEXT_MUTED));
                                ui.add_space(14.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new("Reintentar")
                                                .fill(ACCENT_BLUE)
                                                .min_size(Vec2::new(100.0, 32.0)),
                                        )
                                        .clicked()
                                    {
                                        if msg.contains("Contraseña") || msg.contains("PIN") {
                                            outcome = SessionTabOutcome::ReconnectWithPrompt;
                                        } else {
                                            outcome = SessionTabOutcome::Reconnect;
                                        }
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new("Cerrar pestaña")
                                                .min_size(Vec2::new(110.0, 32.0)),
                                        )
                                        .clicked()
                                    {
                                        outcome = SessionTabOutcome::CloseTab;
                                    }
                                });
                            });
                    });
                });
                outcome
            }
            ConnectionState::Disconnected => {
                kb_hook.set_capture(false, None);
                SessionTabOutcome::CloseTab
            }
            ConnectionState::Connected {
                host_name,
                width,
                height,
            } => self.render_remote_canvas(
                ctx,
                rt_handle,
                kb_hook,
                tray,
                hidden_to_tray,
                host_name,
                *width,
                *height,
                &snap.stats,
                None,
                &snap.monitors,
                snap.active_monitor,
                snap.remote_cursor,
                snap.remote_dir.as_ref(),
                &snap.transfers,
            ),
            ConnectionState::Reconnecting { attempt, reason } => {
                self.release_remote_modifiers();
                kb_hook.set_capture(false, None);
                self.render_remote_canvas(
                    ctx,
                    rt_handle,
                    kb_hook,
                    tray,
                    hidden_to_tray,
                    &self.title.clone(),
                    0,
                    0,
                    &snap.stats,
                    Some((*attempt, reason.as_str())),
                    &snap.monitors,
                    snap.active_monitor,
                    snap.remote_cursor,
                    snap.remote_dir.as_ref(),
                    &snap.transfers,
                )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_remote_canvas(
        &mut self,
        ctx: &egui::Context,
        rt_handle: &tokio::runtime::Handle,
        kb_hook: &mut KeyboardHookGuard,
        tray: Option<&TrayController>,
        hidden_to_tray: bool,
        host_name: &str,
        width: u32,
        height: u32,
        stats: &SessionStats,
        reconnecting: Option<(u32, &str)>,
        monitors: &[MonitorInfo],
        active_monitor: u8,
        remote_cursor: CursorKind,
        remote_dir: Option<&RemoteDirListing>,
        transfers: &[TransferItem],
    ) -> SessionTabOutcome {
        let mut img_rect = egui::Rect::NOTHING;
        let mut pointer_in_canvas = false;
        let mut has_texture = false;

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::BLACK))
            .show(ctx, |ui| {
                let Some(tex) = &self.screen_texture else {
                    kb_hook.set_capture(false, None);
                    ui.centered_and_justified(|ui| {
                        ui.label("Esperando primer cuadro del equipo remoto...");
                    });
                    return;
                };
                has_texture = true;

                let tex_size = tex.size_vec2();
                match self.scale_mode {
                    ScaleMode::Adaptive => {
                        let avail = ui.available_size();
                        let scale =
                            (avail.x / tex_size.x.max(1.0)).min(avail.y / tex_size.y.max(1.0));
                        let draw_size = tex_size * scale;
                        let (rect, response) =
                            ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
                        img_rect = egui::Rect::from_center_size(rect.center(), draw_size);
                        pointer_in_canvas = response.hovered() || response.dragged();
                        ui.painter().image(
                            tex.id(),
                            img_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    ScaleMode::Original => {
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let size = tex_size.max(ui.available_size());
                                let (rect, response) =
                                    ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                                img_rect = egui::Rect::from_center_size(rect.center(), tex_size);
                                pointer_in_canvas = response.hovered() || response.dragged();
                                ui.painter().image(
                                    tex.id(),
                                    img_rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    Color32::WHITE,
                                );
                            });
                    }
                    ScaleMode::Custom(pct) => {
                        let factor = (pct / 100.0).clamp(0.5, 2.0);
                        let draw_size = tex_size * factor;
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let size = draw_size.max(ui.available_size());
                                let (rect, response) =
                                    ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                                img_rect = egui::Rect::from_center_size(rect.center(), draw_size);
                                pointer_in_canvas = response.hovered() || response.dragged();
                                ui.painter().image(
                                    tex.id(),
                                    img_rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    Color32::WHITE,
                                );
                            });
                    }
                }
            });

        if has_texture && pointer_in_canvas {
            if self.view_only {
                ctx.set_cursor_icon(egui::CursorIcon::NotAllowed);
            } else if !self.show_remote_cursor {
                ctx.set_cursor_icon(egui::CursorIcon::None);
            } else {
                ctx.set_cursor_icon(map_remote_cursor(remote_cursor));
            }
        }

        let mut bar_state = SessionBarState {
            codec: self.codec_input,
            quality: self.quality_input,
            fps: self.fps_input,
            scale_mode: self.scale_mode,
            custom_scale_pct: self.custom_scale_pct,
            stream_scale: self.stream_scale,
            quality_preset: self.quality_preset,
            show_monitor_stats: self.show_monitor_stats,
            show_remote_cursor: self.show_remote_cursor,
            fullscreen: self.fullscreen,
            keyboard_grab_enabled: self.keyboard_grab_enabled,
            keyboard_mode: self.keyboard_mode,
            invert_wheel: self.invert_wheel,
            swap_mouse_buttons: self.swap_mouse_buttons,
            view_only: self.view_only,
            bar_collapsed: self.bar_collapsed,
            bar_pinned: self.bar_pinned,
            auto_clipboard: self.auto_clipboard,
            input_debug_log: self.input_debug_log,
            audio_enabled: self.audio_enabled,
            audio_volume: self.audio_volume,
            show_file_manager: self.show_file_manager,
        };
        let bar_actions = render_session_overlay(
            ctx,
            SessionBarInput {
                host_name,
                width,
                height,
                stats,
                reconnecting,
                hook_installed: kb_hook.is_installed(),
                hook_hits: kb_hook.sent_key_count(),
                monitors,
                active_monitor,
            },
            &mut bar_state,
        );

        self.codec_input = bar_state.codec;
        self.quality_input = bar_state.quality;
        self.fps_input = bar_state.fps;
        self.scale_mode = bar_state.scale_mode;
        self.custom_scale_pct = bar_state.custom_scale_pct;
        self.stream_scale = bar_state.stream_scale;
        self.quality_preset = bar_state.quality_preset;
        self.show_monitor_stats = bar_state.show_monitor_stats;
        self.show_remote_cursor = bar_state.show_remote_cursor;
        self.fullscreen = bar_state.fullscreen;
        self.keyboard_grab_enabled = bar_state.keyboard_grab_enabled;
        self.keyboard_mode = bar_state.keyboard_mode;
        self.invert_wheel = bar_state.invert_wheel;
        self.swap_mouse_buttons = bar_state.swap_mouse_buttons;
        self.view_only = bar_state.view_only;
        self.bar_collapsed = bar_state.bar_collapsed;
        self.bar_pinned = bar_state.bar_pinned;
        self.auto_clipboard = bar_state.auto_clipboard;
        self.input_debug_log = bar_state.input_debug_log;
        self.audio_enabled = bar_state.audio_enabled;
        self.audio_volume = bar_state.audio_volume;

        if bar_actions.settings_changed {
            self.net.send_cmd(NetCommand::UpdateSettings(StreamSettings {
                codec: self.codec_input,
                jpeg_quality: self.quality_input,
                target_fps: self.fps_input,
            }));
        }
        if bar_actions.stream_scale_changed {
            self.net
                .send_cmd(NetCommand::SetStreamScale(self.stream_scale));
        }
        if let Some(mon_idx) = bar_actions.select_monitor {
            self.net.send_cmd(NetCommand::SelectMonitor(mon_idx));
        }
        if bar_actions.debug_log_changed {
            set_input_debug_logging(self.input_debug_log);
        }
        if bar_actions.audio_changed {
            self.net.set_audio_muted(!self.audio_enabled);
            self.net.set_audio_volume(self.audio_volume);
            self.net
                .send_cmd(NetCommand::SetAudioEnabled(self.audio_enabled));
        }
        if bar_actions.toggle_file_manager {
            self.show_file_manager = !self.show_file_manager;
            if self.show_file_manager && remote_dir.is_none() {
                self.net.send_cmd(NetCommand::FileListReq(String::new()));
            }
        }
        if bar_actions.lock_remote_screen {
            self.net.send_cmd(NetCommand::LockWorkStation);
        }
        if bar_actions.send_sas {
            self.net.send_cmd(NetCommand::SendSas);
        }
        if bar_actions.send_win_key {
            if let Some(tx) = self.net.cmd_sender() {
                rt_handle.spawn(async move {
                    let _ = tx
                        .send(NetCommand::Input(InputEvent::Key {
                            vk: 0x5B,
                            scan: 0x5B,
                            extended: true,
                            down: true,
                        }))
                        .await;
                    tokio::time::sleep(Duration::from_millis(35)).await;
                    let _ = tx
                        .send(NetCommand::Input(InputEvent::Key {
                            vk: 0x5B,
                            scan: 0x5B,
                            extended: true,
                            down: false,
                        }))
                        .await;
                });
            }
        }
        if bar_actions.send_task_manager {
            if let Some(tx) = self.net.cmd_sender() {
                rt_handle.spawn(async move {
                    for (vk, scan) in [(0xA2u16, 0x1Du16), (0xA0, 0x2A), (0x1B, 0x01)] {
                        let _ = tx
                            .send(NetCommand::Input(InputEvent::Key {
                                vk,
                                scan,
                                extended: false,
                                down: true,
                            }))
                            .await;
                    }
                    tokio::time::sleep(Duration::from_millis(35)).await;
                    for (vk, scan) in [(0x1Bu16, 0x01u16), (0xA0, 0x2A), (0xA2, 0x1D)] {
                        let _ = tx
                            .send(NetCommand::Input(InputEvent::Key {
                                vk,
                                scan,
                                extended: false,
                                down: false,
                            }))
                            .await;
                    }
                });
            }
        }
        if bar_actions.send_clipboard {
            if let Ok(text) = Clipboard::new().and_then(|mut c| c.get_text()) {
                self.clip_watcher.mark_applied(&text);
                self.net.send_cmd(NetCommand::ClipboardSet(text));
            }
        }
        if bar_actions.fetch_clipboard {
            self.net.send_cmd(NetCommand::ClipboardGet);
        }
        if bar_actions.request_keyframe {
            self.net.send_cmd(NetCommand::RequestKeyframe);
        }
        if bar_actions.toggle_fullscreen {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if bar_actions.keyboard_cfg_changed {
            self.release_remote_modifiers();
            kb_hook.release_modifiers_now();
            kb_hook.set_mode(self.keyboard_mode);
            if !self.keyboard_grab_enabled || self.view_only {
                kb_hook.set_capture(false, None);
            } else {
                if let Some(tray) = tray {
                    tray.ensure_foreground();
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        if bar_actions.disconnect {
            self.show_file_manager = false;
            self.focused_on_connect = false;
            kb_hook.set_capture(false, None);
            return SessionTabOutcome::CloseTab;
        }

        if !self.view_only {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            if !dropped.is_empty() {
                let target_remote = self.file_mgr_ui.current_remote_dir(remote_dir);
                let mut uploaded_any = false;
                for file in dropped {
                    if let Some(path) = file.path {
                        if path.is_file() {
                            let id = self.next_transfer_id;
                            self.next_transfer_id += 1;
                            self.net.send_cmd(NetCommand::UploadLocalFile {
                                transfer_id: id,
                                local_path: path,
                                remote_dir: target_remote.clone(),
                            });
                            uploaded_any = true;
                        }
                    }
                }
                if uploaded_any {
                    self.show_file_manager = true;
                    if remote_dir.is_none() {
                        self.net.send_cmd(NetCommand::FileListReq(String::new()));
                    }
                }
            }
        }

        let fm_actions = self.file_mgr_ui.show(
            ctx,
            &mut self.show_file_manager,
            remote_dir,
            transfers,
        );
        for act in fm_actions {
            match act {
                FileManagerAction::ListRemote(path) => {
                    self.net.send_cmd(NetCommand::FileListReq(path));
                }
                FileManagerAction::MkdirRemote { parent, name } => {
                    self.net
                        .send_cmd(NetCommand::FileMkdir { parent_dir: parent, name });
                }
                FileManagerAction::UploadFile {
                    local_path,
                    remote_dir,
                } => {
                    let id = self.next_transfer_id;
                    self.next_transfer_id += 1;
                    self.net.send_cmd(NetCommand::UploadLocalFile {
                        transfer_id: id,
                        local_path,
                        remote_dir,
                    });
                }
                FileManagerAction::DownloadFile {
                    remote_path,
                    local_dir,
                } => {
                    let id = self.next_transfer_id;
                    self.next_transfer_id += 1;
                    self.net.send_cmd(NetCommand::DownloadRemoteFile {
                        transfer_id: id,
                        remote_path,
                        local_dir,
                    });
                }
                FileManagerAction::CancelTransfer(id) => {
                    self.net.send_cmd(NetCommand::CancelTransfer(id));
                }
                FileManagerAction::ClearCompleted => {
                    if let Ok(mut g) = self.net.shared.lock() {
                        g.transfers
                            .retain(|t| matches!(t.status, TransferStatus::InProgress));
                    }
                }
            }
        }

        if !has_texture {
            return SessionTabOutcome::None;
        }

        if !self.focused_on_connect && reconnecting.is_none() && !hidden_to_tray {
            self.focused_on_connect = true;
            if let Some(tray) = tray {
                tray.ensure_foreground();
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        let egui_focused = ctx.input(|i| i.focused);
        kb_hook.set_window_focused(egui_focused);
        let is_fg = KeyboardHookGuard::is_window_foreground();
        let typing_in_local_ui = ctx.wants_keyboard_input();
        let should_capture_kb = reconnecting.is_none()
            && self.keyboard_grab_enabled
            && !hidden_to_tray
            && !self.view_only
            && !typing_in_local_ui;
        if self.was_foreground && !is_fg {
            self.release_remote_modifiers();
            if should_capture_kb {
                kb_hook.release_modifiers_now();
            }
        }
        self.was_foreground = is_fg;

        kb_hook.set_mode(self.keyboard_mode);
        kb_hook.set_capture(
            should_capture_kb,
            if should_capture_kb {
                self.net.cmd_sender()
            } else {
                None
            },
        );

        if self.view_only || typing_in_local_ui {
            self.release_remote_modifiers();
            return SessionTabOutcome::None;
        }

        let hook_blocking_egui = kb_hook.is_capturing_all_keys();

        if !hook_blocking_egui && is_fg && reconnecting.is_none() {
            let mods = ctx.input(|i| i.modifiers);
            let altgr = mods.ctrl && mods.alt && KeyboardHookGuard::is_altgr_down();
            let eff_ctrl = mods.ctrl && !altgr;
            let eff_alt = mods.alt && !altgr;
            if eff_ctrl != self.remote_ctrl_down {
                self.net.send_input(InputEvent::Key {
                    vk: 0xA2,
                    scan: 0x1D,
                    extended: false,
                    down: eff_ctrl,
                });
                self.remote_ctrl_down = eff_ctrl;
            }
            if eff_alt != self.remote_alt_down {
                self.net.send_input(InputEvent::Key {
                    vk: 0xA4,
                    scan: 0x38,
                    extended: false,
                    down: eff_alt,
                });
                self.remote_alt_down = eff_alt;
            }
            if mods.shift != self.remote_shift_down {
                self.net.send_input(InputEvent::Key {
                    vk: 0xA0,
                    scan: 0x2A,
                    extended: false,
                    down: mods.shift,
                });
                self.remote_shift_down = mods.shift;
            }
        } else {
            self.release_remote_modifiers();
        }

        let events = ctx.input(|i| i.events.clone());

        for ev in events {
            match ev {
                egui::Event::PointerMoved(pos) if pointer_in_canvas => {
                    if let Some((x_norm, y_norm)) = normalize_pos(pos, img_rect) {
                        self.net
                            .send_input(InputEvent::MouseMove { x_norm, y_norm });
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } if (pointer_in_canvas && img_rect.contains(pos)) || !pressed => {
                    if pressed && pointer_in_canvas && img_rect.contains(pos) {
                        if let Some(tray) = tray {
                            tray.ensure_foreground();
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    if let Some((x_norm, y_norm)) = normalize_pos(pos, img_rect) {
                        self.net
                            .send_input(InputEvent::MouseMove { x_norm, y_norm });
                    }
                    let mapped_btn = if self.swap_mouse_buttons {
                        match button {
                            egui::PointerButton::Primary => egui::PointerButton::Secondary,
                            egui::PointerButton::Secondary => egui::PointerButton::Primary,
                            other => other,
                        }
                    } else {
                        button
                    };
                    if let Some(mb) = map_mouse_button(mapped_btn) {
                        self.net.send_input(InputEvent::MouseButton {
                            button: mb,
                            down: pressed,
                        });
                    }
                }
                egui::Event::MouseWheel { delta, .. } if pointer_in_canvas => {
                    let sign = if self.invert_wheel { -1 } else { 1 };
                    let dx = (delta.x * 3.0).round() as i16 * 40 * sign;
                    let dy = (delta.y * 3.0).round() as i16 * 40 * sign;
                    if dx != 0 || dy != 0 {
                        self.net.send_input(InputEvent::MouseWheel {
                            delta_x: dx,
                            delta_y: dy,
                        });
                    }
                }
                egui::Event::Copy if !hook_blocking_egui => {
                    append_input_debug_log("[CLIENT-EGUI] shortcut Copy (Ctrl+C)");
                    self.send_remote_ctrl_shortcut(0x43, 0x2E);
                }
                egui::Event::Cut if !hook_blocking_egui => {
                    append_input_debug_log("[CLIENT-EGUI] shortcut Cut (Ctrl+X)");
                    self.send_remote_ctrl_shortcut(0x58, 0x2D);
                }
                egui::Event::Paste(_) if !hook_blocking_egui => {
                    append_input_debug_log("[CLIENT-EGUI] shortcut Paste (Ctrl+V)");
                    self.send_remote_ctrl_shortcut(0x56, 0x2F);
                }
                egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } if !hook_blocking_egui => {
                    let altgr = modifiers.ctrl && modifiers.alt && KeyboardHookGuard::is_altgr_down();
                    let is_combo = (modifiers.ctrl || modifiers.alt || modifiers.command) && !altgr;
                    if is_combo || is_control_or_nav_key(key) {
                        if let Some((vk, extended)) = map_egui_key_to_vk(key) {
                            append_input_debug_log(&format!(
                                "[CLIENT-EGUI] key vk=0x{vk:04X} ext={extended} down={pressed} combo={is_combo}"
                            ));
                            self.net.send_input(InputEvent::Key {
                                vk,
                                scan: 0,
                                extended,
                                down: pressed,
                            });
                        }
                    }
                }
                egui::Event::Text(text) if !hook_blocking_egui => {
                    let mods = ctx.input(|i| i.modifiers);
                    let altgr = mods.ctrl && mods.alt && KeyboardHookGuard::is_altgr_down();
                    if (!mods.ctrl && !mods.alt && !mods.command) || altgr {
                        if altgr {
                            self.release_remote_modifiers();
                        }
                        let chars: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
                        if !chars.is_empty() {
                            append_input_debug_log(&format!(
                                "[CLIENT-EGUI] text count={} altgr={altgr}",
                                chars.len()
                            ));
                        }
                        for ch in chars {
                            self.net
                                .send_input(InputEvent::UnicodeChar { ch, down: true });
                            self.net
                                .send_input(InputEvent::UnicodeChar { ch, down: false });
                        }
                    }
                }
                _ => {}
            }
        }
        SessionTabOutcome::None
    }
}
