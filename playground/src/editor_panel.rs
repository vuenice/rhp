use egui::{CornerRadius, FontId, ScrollArea, Ui};
use egui_extras::syntax_highlighting::CodeTheme;

use crate::theme::*;

pub struct EditorPanel<'a> {
    pub code: &'a mut String,
}

impl<'a> EditorPanel<'a> {
    pub fn show(self, ui: &mut Ui) {
        let code_theme = CodeTheme::dark(13.0);
        let mut layouter = |ui: &Ui, text: &str, _wrap_width: f32| {
            let layout_job = egui_extras::syntax_highlighting::highlight(
                ui.ctx(),
                ui.style(),
                &code_theme,
                text,
                "PHP Source",
            );
            ui.fonts(|f| f.layout_job(layout_job))
        };

        let panel_rect = ui.available_rect_before_wrap();
        let gutter_width = 46.0;

        // Fill panel background
        ui.painter().rect_filled(panel_rect, CornerRadius::ZERO, COLOR_BG_PANEL);

        // Gutter right border line extending full panel height
        let gutter_rail = egui::Rect::from_min_max(
            panel_rect.min,
            egui::pos2(panel_rect.min.x + gutter_width, panel_rect.max.y),
        );
        ui.painter().line_segment(
            [gutter_rail.right_top(), gutter_rail.right_bottom()],
            egui::Stroke::new(1.0_f32, COLOR_BORDER),
        );

        ScrollArea::vertical()
            .id_salt("editor_scroll_area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    // Indent text edit past gutter rail
                    ui.add_space(gutter_width + 12.0);

                    let text_edit = egui::TextEdit::multiline(self.code)
                        .id_salt("editor_text_edit")
                        .font(FontId::monospace(13.5))
                        .desired_width(f32::INFINITY)
                        .desired_rows(50)
                        .frame(false)
                        .layouter(&mut layouter);

                    let output = text_edit.show(ui);
                    let galley_pos = output.galley_pos;
                    let painter = ui.painter();

                    let mut line_no = 1;
                    let mut is_new_line = true;

                    if output.galley.rows.is_empty() {
                        painter.text(
                            egui::pos2(gutter_rail.left() + gutter_width - 10.0, galley_pos.y),
                            egui::Align2::RIGHT_TOP,
                            "1",
                            FontId::monospace(12.5),
                            COLOR_GUTTER_TEXT,
                        );
                    } else {
                        for row in &output.galley.rows {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scroll_area_gutter_calculation() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .vertical_scroll_offset(50.0)
                    .show(ui, |ui| {
                        let gutter_width = 44.0;
                        let start_pos = ui.cursor().min;
                        let mut text = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\n".to_string();

                        let output = ui.horizontal_top(|ui| {
                            ui.add_space(gutter_width + 8.0);
                            egui::TextEdit::multiline(&mut text)
                                .font(egui::TextStyle::Monospace)
                                .frame(false)
                                .show(ui)
                        }).inner;

                        let galley_pos = output.galley_pos;
                        println!("scrolled start_pos={:?}, galley_pos={:?}", start_pos, galley_pos);
                        let mut line_no = 1;
                        let mut is_new_line = true;
                        let mut line_coords = Vec::new();
                        for row in &output.galley.rows {
                            if is_new_line {
                                let y = galley_pos.y + row.rect.min.y;
                                line_coords.push((line_no, y));
                                line_no += 1;
                            }
                            is_new_line = row.ends_with_newline;
                        }
                        println!("start_pos={:?}, line_coords={:?}", start_pos, line_coords);
                    });
            });
        });
    }

    #[test]
    fn test_read_only_text_edit() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut output = "Array\n(\n    [0] => Hello\n    [1] => World\n)\n";
                let te = egui::TextEdit::multiline(&mut output)
                    .font(egui::TextStyle::Monospace)
                    .frame(false)
                    .show(ui);
                assert_eq!(te.galley.rows.len(), 6);
            });
        });
    }

    #[test]
    fn test_egui_extras_highlight_php_source() {
        let ctx = egui::Context::default();
        let code_theme = CodeTheme::dark(13.0);
        let code = "$data = [ \"name\" => \"RHP Playground\" ];\nprint_r($data);\n";
        let layout_job = egui_extras::syntax_highlighting::highlight(
            &ctx,
            &egui::Style::default(),
            &code_theme,
            code,
            "PHP Source",
        );
        assert!(layout_job.sections.len() > 10);
    }
}
