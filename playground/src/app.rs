use egui::{vec2, CentralPanel, Color32, Context, CornerRadius, RichText, Stroke, UiBuilder};
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::editor_panel::EditorPanel;
use crate::modal::{CreateModal, SettingsModal};
use crate::models::{default_standalone_code, load_saved_apps, save_apps, AppItem};
use crate::output_panel::OutputPanel;
use crate::runner::{run_script_async, CancelHandle};
use crate::sidebar::SidebarRail;
use crate::theme::*;

pub struct RhpPlaygroundApp {
    pub apps: Vec<AppItem>,
    pub active_connection: Option<usize>, // None = Standalone, Some(idx) = App
    pub standalone_code: String,
    pub standalone_output: String,
    pub modal: CreateModal,
    pub settings_modal: SettingsModal,
    pub splitter_ratio: f32,

    pub is_running: bool,
    pub cancel_handle: Option<CancelHandle>,

    pub sender: Sender<(Option<usize>, Result<String, String>)>,
    pub receiver: Receiver<(Option<usize>, Result<String, String>)>,

    pub runtime: tokio::runtime::Runtime,
}

impl RhpPlaygroundApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (sender, receiver) = channel();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to initialize Tokio runtime");

        let apps = load_saved_apps();
        let active_connection = if apps.is_empty() { None } else { Some(0) };

        Self {
            apps,
            active_connection,
            standalone_code: default_standalone_code(),
            standalone_output: String::new(),
            modal: CreateModal::default(),
            settings_modal: SettingsModal::default(),
            splitter_ratio: 0.50,
            is_running: false,
            cancel_handle: None,
            sender,
            receiver,
            runtime,
        }
    }

    pub fn trigger_run(&mut self) {
        if self.is_running {
            return;
        }

        let (target_id, code, project_path, rhp_path) = match self.active_connection {
            None => (
                None,
                self.standalone_code.clone(),
                String::new(),
                String::new(),
            ),
            Some(idx) => {
                if idx >= self.apps.len() {
                    return;
                }
                let app = &self.apps[idx];
                (
                    Some(idx),
                    app.code.clone(),
                    app.project_path.clone(),
                    app.rhp_path.clone(),
                )
            }
        };

        let (cancel_handle, cancelled_flag) = CancelHandle::new();
        self.cancel_handle = Some(cancel_handle);
        self.is_running = true;

        let tx = self.sender.clone();
        self.runtime.spawn(async move {
            let res = run_script_async(code, project_path, rhp_path, cancelled_flag).await;
            let _ = tx.send((target_id, res));
        });
    }

    pub fn trigger_cancel(&mut self) {
        if let Some(handle) = &self.cancel_handle {
            handle.cancel();
        }
        self.is_running = false;
        self.cancel_handle = None;
    }

    pub fn render(&mut self, ctx: &Context) {
        apply_theme(ctx);

        // Check for completed background runs
        while let Ok((target_id, res)) = self.receiver.try_recv() {
            self.is_running = false;
            self.cancel_handle = None;
            match target_id {
                None => match res {
                    Ok(out) => self.standalone_output = out,
                    Err(err) => self.standalone_output = format!("[ERROR]:\n{}", err),
                },
                Some(idx) => {
                    if idx < self.apps.len() {
                        match res {
                            Ok(out) => self.apps[idx].output = out,
                            Err(err) => self.apps[idx].output = format!("[ERROR]:\n{}", err),
                        }
                        save_apps(&self.apps);
                    }
                }
            }
        }

        // Keyboard Shortcut: Ctrl+Enter or Cmd+Enter to Run
        let run_pressed = ctx.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::Enter)
        });
        if run_pressed {
            self.trigger_run();
        }

        let mut on_select_standalone = false;
        let mut on_select_app: Option<usize> = None;
        let mut on_create_click = false;
        let mut new_app_created: Option<AppItem> = None;

        CentralPanel::default()
            .frame(egui::Frame::NONE.fill(COLOR_BG_APP))
            .show(ctx, |ui| {
                let total_rect = ui.available_rect_before_wrap();
                let height = total_rect.height();
                let sidebar_width = 48.0;

                // 1. Sidebar Rail (Full Height)
                let sidebar_rect = egui::Rect::from_min_size(
                    total_rect.min,
                    vec2(sidebar_width, height),
                );
                ui.scope_builder(UiBuilder::new().max_rect(sidebar_rect), |ui| {
                    let sidebar = SidebarRail {
                        apps: &self.apps,
                        active_connection: self.active_connection,
                        on_select_standalone: &mut on_select_standalone,
                        on_select_app: &mut on_select_app,
                        on_create_click: &mut on_create_click,
                    };
                    sidebar.show(ui);
                });

                let remaining_x = total_rect.min.x + sidebar_width;
                let remaining_w = (total_rect.width() - sidebar_width).max(100.0);

                // 2. Header Bar above both panels
                let header_height = 42.0;
                let header_rect = egui::Rect::from_min_size(
                    egui::pos2(remaining_x, total_rect.min.y),
                    vec2(remaining_w, header_height),
                );

                ui.scope_builder(UiBuilder::new().max_rect(header_rect), |ui| {
                    ui.painter().rect_filled(header_rect, CornerRadius::ZERO, COLOR_BG_SIDEBAR);
                    ui.painter().line_segment(
                        [header_rect.left_bottom(), header_rect.right_bottom()],
                        Stroke::new(1.0f32, COLOR_BORDER),
                    );

                    ui.horizontal_centered(|ui| {
                        ui.add_space(14.0);

                        // Left side: Project path or Standalone title
                        match self.active_connection {
                            None => {
                                ui.label(
                                    RichText::new("⚡ RHP Playground")
                                        .color(COLOR_TEXT_PRIMARY)
                                        .size(13.5)
                                        .strong(),
                                );
                                ui.add_space(4.0);
                                let badge_rect = egui::Rect::from_min_size(
                                    ui.cursor().min + vec2(0.0, 3.0),
                                    vec2(74.0, 18.0),
                                );
                                ui.painter().rect(
                                    badge_rect,
                                    CornerRadius::same(4),
                                    Color32::from_rgb(32, 31, 44),
                                    Stroke::new(1.0f32, COLOR_BORDER),
                                    egui::StrokeKind::Inside,
                                );
                                ui.painter().text(
                                    badge_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Standalone",
                                    egui::FontId::proportional(11.0),
                                    COLOR_TEXT_MUTED,
                                );
                                ui.add_space(80.0);
                            }
                            Some(idx) => {
                                if idx < self.apps.len() {
                                    let app = &self.apps[idx];
                                    ui.label(
                                        RichText::new(&app.name)
                                            .color(COLOR_TEXT_PRIMARY)
                                            .size(13.5)
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new("•")
                                            .color(COLOR_TEXT_MUTED)
                                            .size(12.0),
                                    );
                                    let path_label = ui.label(
                                        RichText::new(format!("📁 {}", app.project_path))
                                            .color(COLOR_TEXT_MUTED)
                                            .size(12.0)
                                            .monospace(),
                                    );
                                    path_label.on_hover_text(&app.project_path);
                                }
                            }
                        }

                        // Right side: Settings button + Run/Cancel button
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(14.0);

                            // Run / Cancel Button
                            if self.is_running {
                                let cancel_btn = egui::Button::new(
                                    RichText::new("⏹ Cancel").color(Color32::WHITE).strong().size(12.0),
                                )
                                .fill(COLOR_ROSE)
                                .corner_radius(CornerRadius::same(6));

                                if ui.add(cancel_btn).clicked() {
                                    self.trigger_cancel();
                                }

                                let run_btn = egui::Button::new(
                                    RichText::new("⏳ Running...").color(Color32::WHITE).size(12.0),
                                )
                                .fill(Color32::from_rgb(45, 43, 60))
                                .corner_radius(CornerRadius::same(6));

                                ui.add_enabled(false, run_btn);
                            } else {
                                ui.label(
                                    RichText::new("Ctrl+↵")
                                        .color(COLOR_TEXT_MUTED)
                                        .size(11.0)
                                        .monospace(),
                                );

                                let run_btn = egui::Button::new(
                                    RichText::new("▶  Run").color(Color32::WHITE).strong().size(12.0),
                                )
                                .fill(COLOR_EMERALD)
                                .corner_radius(CornerRadius::same(6));

                                if ui.add(run_btn).on_hover_text("Execute code (Ctrl+Enter)").clicked() {
                                    self.trigger_run();
                                }
                            }

                            // Settings Button (if a connection is active)
                            if let Some(idx) = self.active_connection {
                                ui.add_space(8.0);
                                let settings_btn = egui::Button::new(
                                    RichText::new("⚙ Settings").color(COLOR_TEXT_PRIMARY).size(12.0),
                                )
                                .fill(Color32::from_rgb(34, 33, 46))
                                .corner_radius(CornerRadius::same(6));

                                if ui.add(settings_btn).on_hover_text("Configure project settings").clicked() {
                                    if idx < self.apps.len() {
                                        self.settings_modal.open_for(idx, &self.apps[idx]);
                                    }
                                }
                            }
                        });
                    });
                });

                // 3. Panels Area (from header_rect.bottom() down to bottom of window)
                let panels_y = header_rect.bottom();
                let panels_height = total_rect.bottom() - panels_y;

                let left_w = (remaining_w * self.splitter_ratio).clamp(180.0, (remaining_w - 180.0).max(180.0));
                let splitter_x = remaining_x + left_w;

                // Left Panel (Editor)
                let left_rect = egui::Rect::from_min_size(
                    egui::pos2(remaining_x, panels_y),
                    vec2(left_w, panels_height),
                );

                let code_ref = match self.active_connection {
                    None => &mut self.standalone_code,
                    Some(idx) => {
                        if idx < self.apps.len() {
                            &mut self.apps[idx].code
                        } else {
                            &mut self.standalone_code
                        }
                    }
                };

                ui.scope_builder(UiBuilder::new().max_rect(left_rect), |ui| {
                    let editor = EditorPanel {
                        code: code_ref,
                    };
                    editor.show(ui);
                });

                // Draggable Splitter
                let splitter_rect = egui::Rect::from_center_size(
                    egui::pos2(splitter_x, panels_y + panels_height / 2.0),
                    vec2(8.0, panels_height),
                );
                let splitter_id = ui.id().with("main_window_splitter");
                let splitter_resp = ui.interact(splitter_rect, splitter_id, egui::Sense::drag());

                if splitter_resp.hovered() || splitter_resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
                }

                if splitter_resp.dragged() {
                    let delta_ratio = splitter_resp.drag_delta().x / remaining_w;
                    self.splitter_ratio = (self.splitter_ratio + delta_ratio).clamp(0.15, 0.85);
                }

                let splitter_color = if splitter_resp.hovered() || splitter_resp.dragged() {
                    COLOR_ACCENT
                } else {
                    COLOR_BORDER
                };
                ui.painter().line_segment(
                    [egui::pos2(splitter_x, panels_y), egui::pos2(splitter_x, total_rect.bottom())],
                    Stroke::new(1.0f32, splitter_color),
                );

                // Right Panel (Output)
                let right_x = splitter_x;
                let right_w = (total_rect.max.x - right_x).max(50.0);
                let right_rect = egui::Rect::from_min_size(
                    egui::pos2(right_x, panels_y),
                    vec2(right_w, panels_height),
                );

                let output_ref = match self.active_connection {
                    None => &self.standalone_output,
                    Some(idx) => {
                        if idx < self.apps.len() {
                            &self.apps[idx].output
                        } else {
                            &self.standalone_output
                        }
                    }
                };

                ui.scope_builder(UiBuilder::new().max_rect(right_rect), |ui| {
                    let output = OutputPanel {
                        output: output_ref,
                    };
                    output.show(ui);
                });
            });

        // Handle Standalone selection
        if on_select_standalone {
            self.active_connection = None;
        }

        // Handle App Selection
        if let Some(idx) = on_select_app {
            if idx < self.apps.len() {
                self.active_connection = Some(idx);
            }
        }

        // Handle Create Button Click
        if on_create_click {
            self.modal.open = true;
        }

        // Show Create Modal Popup
        self.modal.show(ctx, &mut new_app_created);

        // Show Settings Modal Popup
        self.settings_modal.show(ctx, &mut self.apps, &mut self.active_connection);

        // Handle New App Created
        if let Some(app) = new_app_created {
            self.apps.push(app);
            self.active_connection = Some(self.apps.len() - 1);
            save_apps(&self.apps);
        }

        // Auto-save on change
        if ctx.input(|i| i.raw.events.iter().any(|e| matches!(e, egui::Event::Key { .. }))) {
            if self.active_connection.is_some() {
                save_apps(&self.apps);
            }
        }

        if self.is_running {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for RhpPlaygroundApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.render(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_frames_and_modal() {
        let (sender, receiver) = channel();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let mut app = RhpPlaygroundApp {
            apps: crate::models::default_apps(),
            active_connection: None,
            standalone_code: default_standalone_code(),
            standalone_output: String::new(),
            modal: CreateModal::default(),
            settings_modal: SettingsModal::default(),
            splitter_ratio: 0.50,
            is_running: false,
            cancel_handle: None,
            sender,
            receiver,
            runtime,
        };

        let ctx = Context::default();

        // 1. Frame with modal closed
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.render(ctx);
        });

        // 2. Frame with modal open
        app.modal.open = true;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.render(ctx);
        });

        // 3. Switch tabs in modal
        app.modal.active_tab = crate::modal::ConnectionTab::Docker;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.render(ctx);
        });
    }
}

