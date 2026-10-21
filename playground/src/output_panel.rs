use egui::{CornerRadius, FontId, ScrollArea, Stroke, Ui};

use crate::theme::*;

pub struct OutputPanel<'a> {
    pub output: &'a str,
}

impl<'a> OutputPanel<'a> {
    pub fn show(self, ui: &mut Ui) {
        let panel_rect = ui.available_rect_before_wrap();
        let gutter_width = 46.0;

        // Fill right panel background
        ui.painter().rect_filled(panel_rect, CornerRadius::ZERO, COLOR_BG_OUTPUT);

        // Gutter right border line extending full panel height
        let gutter_rail = egui::Rect::from_min_max(
            panel_rect.min,
            egui::pos2(panel_rect.min.x + gutter_width, panel_rect.max.y),
        );
        ui.painter().line_segment(
            [gutter_rail.right_top(), gutter_rail.right_bottom()],
            Stroke::new(1.0_f32, COLOR_BORDER),
        );

        ScrollArea::vertical()
            .id_salt("output_scroll_area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    // Indent past gutter rail
                    ui.add_space(gutter_width + 12.0);

                    if self.output.is_empty() {
                        // Empty state: line number 1 in gutter, placeholder in output
                        ui.painter().text(
                            egui::pos2(gutter_rail.left() + gutter_width - 10.0, gutter_rail.top() + 8.0),
                            egui::Align2::RIGHT_TOP,
                            "1",
                            FontId::monospace(12.5),
                            COLOR_GUTTER_TEXT,
                        );

                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("// Press Run to execute (Ctrl+Enter)")
                                .font(FontId::monospace(13.5))
                                .color(COLOR_TEXT_MUTED),
                        );
                    } else {
                        // Output present: selectable & copyable, but read-only
                        let mut output_buf = self.output;
                        let text_edit = egui::TextEdit::multiline(&mut output_buf)
                            .id_salt("output_text_edit")
                            .font(FontId::monospace(13.5))
                            .desired_width(f32::INFINITY)
                            .desired_rows(50)
                            .frame(false);

                        let output_res = text_edit.show(ui);
                        let galley_pos = output_res.galley_pos;
                        let painter = ui.painter();

                        let mut line_no = 1;
                        let mut is_new_line = true;
                        for row in &output_res.galley.rows {
                            if is_new_line {
                                let y = galley_pos.y + row.rect.min.y;
                                painter.text(
                                    egui::pos2(gutter_rail.left() + gutter_width - 10.0, y),
                                    egui::Align2::RIGHT_TOP,
                                    format!("{}", line_no),
                                    FontId::monospace(12.5),
                                    COLOR_GUTTER_TEXT,
                                );
                                line_no += 1;
                            }
                            is_new_line = row.ends_with_newline;
                        }
                    }
                });
            });
    }
}

