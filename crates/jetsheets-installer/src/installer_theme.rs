use eframe::egui::{self, Color32, CornerRadius, Stroke};

pub const BACKGROUND: Color32 = Color32::from_rgb(16, 21, 28);
pub const SURFACE: Color32 = Color32::from_rgb(27, 34, 43);
pub const SURFACE_ALT: Color32 = Color32::from_rgb(34, 42, 53);
pub const BORDER: Color32 = Color32::from_rgb(47, 59, 73);
pub const BLUE: Color32 = Color32::from_rgb(59, 130, 246);
pub const TEXT: Color32 = Color32::from_rgb(239, 244, 249);
pub const MUTED: Color32 = Color32::from_rgb(164, 177, 192);
pub const DIM: Color32 = Color32::from_rgb(123, 139, 156);
pub const SUCCESS: Color32 = Color32::from_rgb(34, 197, 94);
pub const ERROR: Color32 = Color32::from_rgb(239, 90, 90);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = SURFACE_ALT;
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(MUTED);
    visuals.selection.bg_fill = Color32::from_rgb(32, 75, 134);
    visuals.selection.stroke = Stroke::new(1.0, BLUE);
    visuals.widgets.inactive.bg_fill = SURFACE_ALT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 62, 82);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BLUE);
    visuals.widgets.active.bg_fill = Color32::from_rgb(35, 75, 128);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, BLUE);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.corner_radius = CornerRadius::same(7);
    }
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(9.0, 9.0);
    ctx.set_style_of(egui::Theme::Dark, style);
}
