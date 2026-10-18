use eframe::egui::{self, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};

pub const BACKGROUND: Color32 = Color32::from_rgb(5, 12, 29);
pub const SIDEBAR: Color32 = Color32::from_rgb(4, 18, 43);
pub const SURFACE: Color32 = Color32::from_rgb(8, 23, 48);
pub const SURFACE_LIGHT: Color32 = Color32::from_rgb(11, 31, 62);
pub const BORDER: Color32 = Color32::from_rgb(31, 63, 105);
pub const BLUE: Color32 = Color32::from_rgb(25, 139, 255);
pub const BLUE_LIGHT: Color32 = Color32::from_rgb(73, 178, 255);
pub const TEXT: Color32 = Color32::from_rgb(239, 246, 255);
pub const MUTED: Color32 = Color32::from_rgb(158, 179, 210);
pub const DIM: Color32 = Color32::from_rgb(105, 136, 180);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = Color32::from_rgb(5, 17, 36);
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(MUTED);
    visuals.selection.bg_fill = Color32::from_rgb(19, 79, 145);
    visuals.selection.stroke = Stroke::new(1.0, BLUE_LIGHT);
    visuals.widgets.inactive.bg_fill = SURFACE_LIGHT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(17, 54, 102);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BLUE);
    visuals.widgets.active.bg_fill = Color32::from_rgb(20, 82, 157);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, BLUE_LIGHT);
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub fn paint_neon_border(painter: &Painter, rect: Rect) {
    for (inset, alpha, width) in [(0.0, 32, 8.0), (1.5, 55, 5.0), (3.0, 235, 1.5)] {
        painter.rect_stroke(
            rect.shrink(inset),
            CornerRadius::same(8),
            Stroke::new(width, Color32::from_rgba_unmultiplied(28, 142, 255, alpha)),
            StrokeKind::Inside,
        );
    }
}

pub fn paint_sidebar(painter: &Painter, rect: Rect) {
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, SIDEBAR);
    let glow_center = egui::pos2(rect.right() - 15.0, rect.bottom() - 175.0);
    for (radius, alpha) in [(215.0, 10), (160.0, 16), (105.0, 24), (62.0, 30)] {
        painter.circle_filled(
            glow_center,
            radius,
            Color32::from_rgba_unmultiplied(0, 104, 255, alpha),
        );
    }

    // A perspective spreadsheet made from simple vector lines stays sharp at every DPI.
    let top_left = egui::pos2(rect.left() + 22.0, rect.bottom() - 220.0);
    let top_right = egui::pos2(rect.right() + 22.0, rect.bottom() - 285.0);
    let bottom_left = egui::pos2(rect.left() + 55.0, rect.bottom() - 43.0);
    let bottom_right = egui::pos2(rect.right() + 55.0, rect.bottom() - 109.0);
    painter.add(Shape::convex_polygon(
        vec![top_left, top_right, bottom_right, bottom_left],
        Color32::from_rgb(7, 34, 80),
        Stroke::new(1.3, BLUE),
    ));
    for row in 1..8 {
        let fraction = row as f32 / 8.0;
        let start = top_left.lerp(bottom_left, fraction);
        let end = top_right.lerp(bottom_right, fraction);
        let color = if row == 4 {
            Color32::from_rgb(53, 167, 255)
        } else {
            Color32::from_rgba_unmultiplied(50, 127, 226, 115)
        };
        painter.line_segment(
            [start, end],
            Stroke::new(if row == 4 { 2.5 } else { 1.0 }, color),
        );
    }
    for column in 1..6 {
        let fraction = column as f32 / 6.0;
        painter.line_segment(
            [
                top_left.lerp(top_right, fraction),
                bottom_left.lerp(bottom_right, fraction),
            ],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(48, 123, 228, 100)),
        );
    }
    painter.line_segment(
        [
            Pos2::new(rect.left() - 20.0, rect.bottom() - 140.0),
            Pos2::new(rect.right() + 20.0, rect.bottom() - 245.0),
        ],
        Stroke::new(1.5, Color32::from_rgb(19, 143, 255)),
    );
    painter.line_segment(
        [
            Pos2::new(rect.left() - 20.0, rect.bottom() - 135.0),
            Pos2::new(rect.right() + 20.0, rect.bottom() - 240.0),
        ],
        Stroke::new(6.0, Color32::from_rgba_unmultiplied(15, 113, 255, 35)),
    );
    painter.line_segment(
        [
            Pos2::new(rect.right() - 0.5, rect.top()),
            Pos2::new(rect.right() - 0.5, rect.bottom()),
        ],
        Stroke::new(1.0, Color32::from_rgb(31, 89, 167)),
    );
}
