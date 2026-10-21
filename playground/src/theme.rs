use egui::{Color32, CornerRadius, Margin, Stroke, Style, Visuals};

pub const COLOR_BG_APP: Color32 = Color32::from_rgb(22, 21, 32);       // #161520
pub const COLOR_BG_SIDEBAR: Color32 = Color32::from_rgb(20, 19, 28);   // #14131c
pub const COLOR_BG_PANEL: Color32 = Color32::from_rgb(26, 26, 36);     // #1a1a24
pub const COLOR_BG_OUTPUT: Color32 = Color32::from_rgb(23, 23, 32);    // #171720
pub const COLOR_BG_ACTIVE: Color32 = Color32::from_rgb(36, 35, 48);    // #242330
#[allow(dead_code)]
pub const COLOR_BG_GUTTER: Color32 = Color32::from_rgb(26, 26, 36);    // Seamless with editor panel
#[allow(dead_code)]
pub const COLOR_BG_GUTTER_OUTPUT: Color32 = Color32::from_rgb(23, 23, 32); // Seamless with output panel
pub const COLOR_BORDER: Color32 = Color32::from_rgb(38, 37, 52);       // #262534
#[allow(dead_code)]
pub const COLOR_BORDER_SUBTLE: Color32 = Color32::from_rgb(48, 47, 66); // #302f42

pub const COLOR_ACCENT: Color32 = Color32::from_rgb(168, 85, 247);     // purple-500
#[allow(dead_code)]
pub const COLOR_PINK: Color32 = Color32::from_rgb(236, 72, 153);       // pink-500
pub const COLOR_EMERALD: Color32 = Color32::from_rgb(16, 185, 129);    // emerald-500
pub const COLOR_ROSE: Color32 = Color32::from_rgb(244, 63, 94);        // rose-500

pub const COLOR_TEXT_PRIMARY: Color32 = Color32::from_rgb(228, 228, 235); // #e4e4eb
pub const COLOR_TEXT_MUTED: Color32 = Color32::from_rgb(120, 118, 145);   // #787691
pub const COLOR_GUTTER_TEXT: Color32 = Color32::from_rgb(86, 84, 110);     // #56546e

pub fn apply_theme(ctx: &egui::Context) {
    let mut style = Style::default();
    let mut visuals = Visuals::dark();

    visuals.panel_fill = COLOR_BG_PANEL;
    visuals.window_fill = COLOR_BG_PANEL;
    visuals.faint_bg_color = COLOR_BG_SIDEBAR;
    visuals.extreme_bg_color = COLOR_BG_SIDEBAR;

    visuals.widgets.noninteractive.bg_fill = COLOR_BG_PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0f32, COLOR_BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0f32, COLOR_TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);

    visuals.widgets.inactive.bg_fill = COLOR_BG_ACTIVE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0f32, COLOR_BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0f32, COLOR_TEXT_MUTED);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(6);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(46, 44, 62);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0f32, COLOR_ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0f32, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(6);

    visuals.widgets.active.bg_fill = COLOR_ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0f32, COLOR_ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(6);

    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(168, 85, 247, 70);
    visuals.selection.stroke = Stroke::new(1.0f32, COLOR_ACCENT);

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(6.0, 6.0);
    style.spacing.window_margin = Margin::same(0);

    ctx.set_style(style);
}
