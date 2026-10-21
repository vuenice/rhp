use egui::{vec2, Color32, CornerRadius, Stroke, Ui};

use crate::models::AppItem;
use crate::theme::*;

pub struct SidebarRail<'a> {
    pub apps: &'a [AppItem],
    pub active_connection: Option<usize>,
    pub on_select_standalone: &'a mut bool,
    pub on_select_app: &'a mut Option<usize>,
    pub on_create_click: &'a mut bool,
}

impl<'a> SidebarRail<'a> {
    pub fn show(self, ui: &mut Ui) {
        let width = 48.0;

        ui.allocate_ui_with_layout(
            vec2(width, ui.available_height()),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                let rect = ui.available_rect_before_wrap();
                let center_x = rect.min.x + width / 2.0;

                // Fill sidebar background
                ui.painter().rect_filled(rect, CornerRadius::ZERO, COLOR_BG_SIDEBAR);
                // Right border
                ui.painter().line_segment(
                    [rect.right_top(), rect.right_bottom()],
                    Stroke::new(1.0f32, COLOR_BORDER),
                );

                let mut current_y = rect.top() + 10.0;

                // 1. Standalone RHP Playground Play Button (▶)
                let is_standalone_active = self.active_connection.is_none();
                let play_btn_rect = egui::Rect::from_center_size(
                    egui::pos2(center_x, current_y + 16.0),
                    vec2(32.0, 32.0),
                );
                let play_id = ui.id().with("standalone_play_btn");
                let play_resp = ui.interact(play_btn_rect, play_id, egui::Sense::click());
                let play_hovered = play_resp.hovered();

                let play_bg = if is_standalone_active {
                    Color32::from_rgb(16, 185, 129) // Emerald active
                } else if play_hovered {
                    Color32::from_rgb(38, 37, 52)
                } else {
                    Color32::from_rgb(28, 27, 38)
                };

                let play_stroke = if is_standalone_active {
                    Stroke::new(1.5f32, Color32::WHITE)
                } else if play_hovered {
                    Stroke::new(1.0f32, COLOR_EMERALD)
                } else {
                    Stroke::new(1.0f32, COLOR_BORDER)
                };

                ui.painter().rect(
                    play_btn_rect,
                    CornerRadius::same(8),
                    play_bg,
                    play_stroke,
                    egui::StrokeKind::Inside,
                );

                ui.painter().text(
                    play_btn_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "▶",
                    egui::FontId::proportional(13.0),
                    if is_standalone_active { Color32::WHITE } else if play_hovered { COLOR_EMERALD } else { COLOR_TEXT_MUTED },
                );

                if play_resp.on_hover_text("RHP Playground (Standalone Script)").clicked() {
                    *self.on_select_standalone = true;
                }

                current_y += 40.0;

                // 2. Create Connection Button (+)
                let create_btn_rect = egui::Rect::from_center_size(
                    egui::pos2(center_x, current_y + 16.0),
                    vec2(32.0, 32.0),
                );
                let create_id = ui.id().with("create_conn_btn");
                let response = ui.interact(create_btn_rect, create_id, egui::Sense::click());
                let is_hovered = response.hovered();

                let bg_color = if is_hovered {
                    Color32::from_rgb(38, 37, 52)
                } else {
                    Color32::from_rgb(28, 27, 38)
                };
                let border_color = if is_hovered {
                    COLOR_ACCENT
                } else {
                    COLOR_BORDER
                };

                ui.painter().rect(
                    create_btn_rect,
                    CornerRadius::same(8),
                    bg_color,
                    Stroke::new(1.0f32, border_color),
                    egui::StrokeKind::Inside,
                );

                ui.painter().text(
                    create_btn_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "+",
                    egui::FontId::proportional(17.0),
                    if is_hovered { Color32::WHITE } else { COLOR_TEXT_MUTED },
                );

                if response.on_hover_text("New Project Connection").clicked() {
                    *self.on_create_click = true;
                }

                current_y += 42.0;

                // Divider line if there are custom connections
                if !self.apps.is_empty() {
                    ui.painter().line_segment(
                        [egui::pos2(rect.min.x + 8.0, current_y), egui::pos2(rect.max.x - 8.0, current_y)],
                        Stroke::new(1.0f32, COLOR_BORDER),
                    );
                    current_y += 10.0;
                }

                // 3. User Project Connections List
                for (idx, app) in self.apps.iter().enumerate() {
                    let is_active = self.active_connection == Some(idx);
                    let letter = app.initial_letter();

                    let item_rect = egui::Rect::from_center_size(
                        egui::pos2(center_x, current_y + 16.0),
                        vec2(32.0, 32.0),
                    );
                    let item_id = ui.id().with(("sidebar_app_badge", idx, &app.id));
                    let item_resp = ui.interact(item_rect, item_id, egui::Sense::click());
                    let hovered = item_resp.hovered();

                    let (badge_bg, badge_fg) = match app.color_idx % 4 {
                        0 => (Color32::from_rgb(42, 41, 56), Color32::WHITE),   // Charcoal
                        1 => (Color32::from_rgb(224, 36, 118), Color32::WHITE), // Pink/Magenta
                        2 => (Color32::from_rgb(139, 92, 246), Color32::WHITE), // Purple
                        _ => (Color32::from_rgb(14, 165, 233), Color32::WHITE), // Sky
                    };

                    let stroke = if is_active {
                        Stroke::new(1.5f32, Color32::WHITE)
                    } else if hovered {
                        Stroke::new(1.0f32, COLOR_ACCENT)
                    } else {
                        Stroke::new(1.0f32, COLOR_BORDER)
                    };

                    ui.painter().rect(
                        item_rect,
                        CornerRadius::same(8),
                        badge_bg,
                        stroke,
                        egui::StrokeKind::Inside,
                    );

                    ui.painter().text(
                        item_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        letter,
                        egui::FontId::proportional(14.0),
                        badge_fg,
                    );

                    let tooltip = format!("{}\nProject: {}", app.name, app.project_path);
                    if item_resp.on_hover_text(tooltip).clicked() {
                        *self.on_select_app = Some(idx);
                    }

                    current_y += 40.0;
                }
            },
        );
    }
}
