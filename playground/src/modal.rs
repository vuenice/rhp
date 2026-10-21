use egui::{vec2, Color32, CornerRadius, RichText, Stroke, Window};

use crate::models::AppItem;
use crate::theme::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ConnectionTab {
    Local,
    Docker,
    Remote,
}

pub struct CreateModal {
    pub open: bool,
    pub active_tab: ConnectionTab,
    pub name: String,
    pub project_path: String,
    pub rhp_path: String,
    pub error_message: Option<String>,
}

impl Default for CreateModal {
    fn default() -> Self {
        Self {
            open: false,
            active_tab: ConnectionTab::Local,
            name: String::new(),
            project_path: String::new(),
            rhp_path: crate::runner::resolve_rhp_binary("").to_string_lossy().to_string(),
            error_message: None,
        }
    }
}

impl CreateModal {
    pub fn show(&mut self, ctx: &egui::Context, on_create: &mut Option<AppItem>) {
        if !self.open {
            return;
        }

        let mut is_open = self.open;
        let mut close_requested = false;

        let window_frame = egui::Frame::window(&ctx.style())
            .fill(Color32::from_rgb(28, 27, 39))
            .stroke(Stroke::new(1.0f32, COLOR_BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(20, 18));

        Window::new("New Connection")
            .id(egui::Id::new("new_connection_dialog"))
            .open(&mut is_open)
            .frame(window_frame)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.add_space(2.0);

                // Segmented Tab Bar: Local | Docker | Remote
                let tab_bar_rect = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 34.0));
                ui.painter().rect(
                    tab_bar_rect,
                    CornerRadius::same(8),
                    Color32::from_rgb(20, 19, 28),
                    Stroke::new(1.0f32, COLOR_BORDER),
                    egui::StrokeKind::Inside,
                );

                let tab_width = tab_bar_rect.width() / 3.0;
                let tabs = [
                    (ConnectionTab::Local, "Local"),
                    (ConnectionTab::Docker, "Docker"),
                    (ConnectionTab::Remote, "Remote"),
                ];

                for (idx, (tab, label)) in tabs.iter().enumerate() {
                    let tab_rect = egui::Rect::from_min_size(
                        egui::pos2(tab_bar_rect.min.x + idx as f32 * tab_width + 2.0, tab_bar_rect.min.y + 2.0),
                        vec2(tab_width - 4.0, 30.0),
                    );
                    let is_active = self.active_tab == *tab;
                    let tab_id = ui.id().with(("modal_tab", idx));
                    let resp = ui.interact(tab_rect, tab_id, egui::Sense::click());

                    if is_active {
                        ui.painter().rect_filled(
                            tab_rect,
                            CornerRadius::same(6),
                            COLOR_ACCENT,
                        );
                    } else if resp.hovered() {
                        ui.painter().rect_filled(
                            tab_rect,
                            CornerRadius::same(6),
                            Color32::from_rgb(34, 33, 48),
                        );
                    }

                    ui.painter().text(
                        tab_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        *label,
                        egui::FontId::proportional(12.5),
                        if is_active { Color32::WHITE } else { COLOR_TEXT_MUTED },
                    );

                    if resp.clicked() {
                        self.active_tab = *tab;
                    }
                }

                ui.advance_cursor_after_rect(tab_bar_rect);
                ui.add_space(14.0);

                match self.active_tab {
                    ConnectionTab::Local => {
                        self.render_local_tab(ui, on_create, &mut close_requested);
                    }
                    ConnectionTab::Docker => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(24.0);
                            ui.label(RichText::new("Docker Connection").color(COLOR_TEXT_PRIMARY).size(13.5).strong());
                            ui.add_space(6.0);
                            ui.label(RichText::new("Docker containerized REPL execution will be available in future releases.").color(COLOR_TEXT_MUTED).size(11.5));
                            ui.add_space(24.0);
                        });
                    }
                    ConnectionTab::Remote => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(24.0);
                            ui.label(RichText::new("Remote SSH Connection").color(COLOR_TEXT_PRIMARY).size(13.5).strong());
                            ui.add_space(6.0);
                            ui.label(RichText::new("Execute PHP code securely on remote staging or production servers over SSH.").color(COLOR_TEXT_MUTED).size(11.5));
                            ui.add_space(24.0);
                        });
                    }
                }
            });

        self.open = is_open && !close_requested;
    }

    fn render_local_tab(&mut self, ui: &mut egui::Ui, on_create: &mut Option<AppItem>, close_requested: &mut bool) {
        ui.vertical(|ui| {
            // App Name
            ui.label(RichText::new("App Name").size(12.0).color(COLOR_TEXT_MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut self.name)
                    .id_salt("modal_field_app_name")
                    .hint_text("e.g. My App or Laravel Project")
                    .desired_width(f32::INFINITY),
            );

            ui.add_space(10.0);

            // Project Path
            ui.label(RichText::new("Project Path").size(12.0).color(COLOR_TEXT_MUTED));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.project_path)
                        .id_salt("modal_field_project_path")
                        .hint_text("/path/to/project")
                        .desired_width(ui.available_width() - 86.0),
                );

                let browse_btn = egui::Button::new(RichText::new("Browse...").size(12.0))
                    .fill(Color32::from_rgb(38, 37, 52))
                    .corner_radius(CornerRadius::same(6));

                if ui.add(browse_btn).clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        let path_str = folder.to_string_lossy().to_string();
                        if self.name.trim().is_empty() {
                            if let Some(folder_name) = folder.file_name() {
                                self.name = folder_name.to_string_lossy().to_string();
                            }
                        }
                        self.project_path = path_str;
                    }
                }
            });

            ui.add_space(10.0);

            // RHP Path
            ui.label(RichText::new("RHP Binary Path").size(12.0).color(COLOR_TEXT_MUTED));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.rhp_path)
                        .id_salt("modal_field_rhp_path")
                        .hint_text("/path/to/rhp or rhp")
                        .desired_width(ui.available_width() - 86.0),
                );

                let browse_btn = egui::Button::new(RichText::new("Browse...").size(12.0))
                    .fill(Color32::from_rgb(38, 37, 52))
                    .corner_radius(CornerRadius::same(6));

                if ui.add(browse_btn).clicked() {
                    if let Some(file) = rfd::FileDialog::new().pick_file() {
                        self.rhp_path = file.to_string_lossy().to_string();
                    }
                }
            });

            if let Some(err) = &self.error_message {
                ui.add_space(6.0);
                ui.label(RichText::new(err).color(COLOR_ROSE).size(11.5));
            }

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let create_btn = egui::Button::new(
                        RichText::new("Create Connection").color(Color32::WHITE).strong().size(12.5),
                    )
                    .fill(COLOR_ACCENT)
                    .corner_radius(CornerRadius::same(6));

                    if ui.add(create_btn).clicked() {
                        if self.project_path.trim().is_empty() {
                            self.error_message = Some("Please specify a project path.".to_string());
                            return;
                        }

                        let app_name = if self.name.trim().is_empty() {
                            std::path::Path::new(&self.project_path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("App")
                                .to_string()
                        } else {
                            self.name.trim().to_string()
                        };

                        let new_app = AppItem {
                            id: format!("app-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                            name: app_name,
                            project_path: self.project_path.clone(),
                            rhp_path: self.rhp_path.clone(),
                            code: "echo \"Connected to RHP environment!\\n\";\n".to_string(),
                            output: String::new(),
                            color_idx: (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() % 4) as usize,
                        };

                        *on_create = Some(new_app);
                        *close_requested = true;
                        self.name.clear();
                        self.project_path.clear();
                        self.error_message = None;
                    }

                    let cancel_btn = egui::Button::new(
                        RichText::new("Cancel").color(COLOR_TEXT_MUTED).size(12.5),
                    )
                    .fill(Color32::TRANSPARENT);

                    if ui.add(cancel_btn).clicked() {
                        *close_requested = true;
                        self.error_message = None;
                    }
                });
            });
        });
    }
}

#[derive(Default)]
pub struct SettingsModal {
    pub open: bool,
    pub connection_idx: usize,
    pub name: String,
    pub project_path: String,
    pub rhp_path: String,
    pub error_message: Option<String>,
}

impl SettingsModal {
    pub fn open_for(&mut self, idx: usize, app: &AppItem) {
        self.open = true;
        self.connection_idx = idx;
        self.name = app.name.clone();
        self.project_path = app.project_path.clone();
        self.rhp_path = app.rhp_path.clone();
        self.error_message = None;
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        apps: &mut Vec<AppItem>,
        active_connection: &mut Option<usize>,
    ) {
        if !self.open || self.connection_idx >= apps.len() {
            self.open = false;
            return;
        }

        let mut is_open = self.open;
        let mut close_requested = false;
        let mut delete_requested = false;

        let window_frame = egui::Frame::window(&ctx.style())
            .fill(Color32::from_rgb(28, 27, 39))
            .stroke(Stroke::new(1.0f32, COLOR_BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(20, 18));

        Window::new("Project Settings")
            .id(egui::Id::new("project_settings_dialog"))
            .open(&mut is_open)
            .frame(window_frame)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.add_space(2.0);

                // App Name
                ui.label(RichText::new("App Name").size(12.0).color(COLOR_TEXT_MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut self.name)
                        .id_salt("settings_field_app_name")
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(10.0);

                // Project Path
                ui.label(RichText::new("Project Path").size(12.0).color(COLOR_TEXT_MUTED));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.project_path)
                            .id_salt("settings_field_project_path")
                            .desired_width(ui.available_width() - 86.0),
                    );

                    let browse_btn = egui::Button::new(RichText::new("Browse...").size(12.0))
                        .fill(Color32::from_rgb(38, 37, 52))
                        .corner_radius(CornerRadius::same(6));

                    if ui.add(browse_btn).clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.project_path = folder.to_string_lossy().to_string();
                        }
                    }
                });

                ui.add_space(10.0);

                // RHP Path
                ui.label(RichText::new("RHP Binary Path").size(12.0).color(COLOR_TEXT_MUTED));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.rhp_path)
                            .id_salt("settings_field_rhp_path")
                            .desired_width(ui.available_width() - 86.0),
                    );

                    let browse_btn = egui::Button::new(RichText::new("Browse...").size(12.0))
                        .fill(Color32::from_rgb(38, 37, 52))
                        .corner_radius(CornerRadius::same(6));

                    if ui.add(browse_btn).clicked() {
                        if let Some(file) = rfd::FileDialog::new().pick_file() {
                            self.rhp_path = file.to_string_lossy().to_string();
                        }
                    }
                });

                if let Some(err) = &self.error_message {
                    ui.add_space(6.0);
                    ui.label(RichText::new(err).color(COLOR_ROSE).size(11.5));
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    let delete_btn = egui::Button::new(
                        RichText::new("Delete Connection").color(COLOR_ROSE).size(12.0),
                    )
                    .fill(Color32::from_rgb(45, 25, 32))
                    .corner_radius(CornerRadius::same(6));

                    if ui.add(delete_btn).on_hover_text("Remove this connection").clicked() {
                        delete_requested = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let save_btn = egui::Button::new(
                            RichText::new("Save Changes").color(Color32::WHITE).strong().size(12.5),
                        )
                        .fill(COLOR_ACCENT)
                        .corner_radius(CornerRadius::same(6));

                        if ui.add(save_btn).clicked() {
                            if self.project_path.trim().is_empty() {
                                self.error_message = Some("Project path cannot be empty.".to_string());
                                return;
                            }
                            let app = &mut apps[self.connection_idx];
                            app.name = self.name.trim().to_string();
                            app.project_path = self.project_path.trim().to_string();
                            app.rhp_path = self.rhp_path.trim().to_string();
                            crate::models::save_apps(apps);
                            close_requested = true;
                        }

                        let cancel_btn = egui::Button::new(
                            RichText::new("Cancel").color(COLOR_TEXT_MUTED).size(12.5),
                        )
                        .fill(Color32::TRANSPARENT);

                        if ui.add(cancel_btn).clicked() {
                            close_requested = true;
                        }
                    });
                });
            });

        if delete_requested {
            apps.remove(self.connection_idx);
            crate::models::save_apps(apps);
            *active_connection = None; // Switch back to standalone playground
            self.open = false;
        } else if !is_open || close_requested {
            self.open = false;
        }
    }
}
