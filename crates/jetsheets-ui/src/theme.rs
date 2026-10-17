//! JetSheets colors and shared desktop components.

use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, vec2};

pub const SPACE: f32 = 8.0;
pub const CARD_RADIUS: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JetTheme {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub primary: Color32,
    pub primary_hover: Color32,
    pub accent_text: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub border: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub error_text: Color32,
    pub raised: Color32,
    pub hover_surface: Color32,
    pub header: Color32,
    pub row_even: Color32,
    pub row_odd: Color32,
    pub selection: Color32,
    pub grid: Color32,
}

impl JetTheme {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Jet Dark",
            Self::Light => "Jet Light",
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Dark => Palette {
                background: Color32::from_rgb(0x10, 0x13, 0x18),
                surface: Color32::from_rgb(0x1B, 0x20, 0x29),
                primary: Color32::from_rgb(0x3B, 0x82, 0xF6),
                primary_hover: Color32::from_rgb(0x25, 0x63, 0xEB),
                accent_text: Color32::from_rgb(0x60, 0xA5, 0xFA),
                text: Color32::from_rgb(0xF1, 0xF5, 0xF9),
                text_secondary: Color32::from_rgb(0x92, 0x9C, 0xAA),
                border: Color32::from_rgb(0x2C, 0x34, 0x42),
                success: Color32::from_rgb(0x22, 0xC5, 0x5E),
                warning: Color32::from_rgb(0xF5, 0x9E, 0x0B),
                error: Color32::from_rgb(0xEF, 0x44, 0x44),
                error_text: Color32::from_rgb(0xF8, 0x71, 0x71),
                raised: Color32::from_rgb(0x22, 0x29, 0x34),
                hover_surface: Color32::from_rgb(0x2A, 0x36, 0x48),
                header: Color32::from_rgb(0x23, 0x2B, 0x37),
                row_even: Color32::from_rgb(0x1B, 0x20, 0x29),
                row_odd: Color32::from_rgb(0x20, 0x27, 0x32),
                selection: Color32::from_rgb(0x1D, 0x42, 0x78),
                grid: Color32::from_rgb(0x2C, 0x34, 0x42),
            },
            Self::Light => Palette {
                background: Color32::from_rgb(0xF8, 0xFA, 0xFC),
                surface: Color32::WHITE,
                primary: Color32::from_rgb(0x25, 0x63, 0xEB),
                primary_hover: Color32::from_rgb(0x1D, 0x4E, 0xC6),
                accent_text: Color32::from_rgb(0x25, 0x63, 0xEB),
                text: Color32::from_rgb(0x17, 0x20, 0x33),
                text_secondary: Color32::from_rgb(0x52, 0x60, 0x73),
                border: Color32::from_rgb(0xD6, 0xDF, 0xEA),
                success: Color32::from_rgb(0x15, 0x80, 0x3D),
                warning: Color32::from_rgb(0xB4, 0x60, 0x00),
                error: Color32::from_rgb(0xCF, 0x2E, 0x36),
                error_text: Color32::from_rgb(0xB9, 0x1C, 0x2B),
                raised: Color32::from_rgb(0xF1, 0xF5, 0xF9),
                hover_surface: Color32::from_rgb(0xE8, 0xF0, 0xFF),
                header: Color32::from_rgb(0xEA, 0xF0, 0xF7),
                row_even: Color32::WHITE,
                row_odd: Color32::from_rgb(0xF5, 0xF8, 0xFC),
                selection: Color32::from_rgb(0xDB, 0xEA, 0xFE),
                grid: Color32::from_rgb(0xE0, 0xE7, 0xEF),
            },
        }
    }

    pub fn apply(self, ctx: &egui::Context) {
        let colors = self.palette();
        let egui_theme = match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        };
        let mut visuals = match self {
            Self::Dark => egui::Visuals::dark(),
            Self::Light => egui::Visuals::light(),
        };
        visuals.override_text_color = Some(colors.text);
        visuals.weak_text_color = Some(colors.text_secondary);
        visuals.panel_fill = colors.background;
        visuals.window_fill = colors.surface;
        visuals.window_stroke = Stroke::new(1.0, colors.border);
        visuals.window_corner_radius = CornerRadius::same(CARD_RADIUS);
        visuals.menu_corner_radius = CornerRadius::same(CARD_RADIUS);
        visuals.extreme_bg_color = colors.raised;
        visuals.text_edit_bg_color = Some(colors.raised);
        visuals.code_bg_color = colors.raised;
        visuals.faint_bg_color = colors.row_odd;
        visuals.hyperlink_color = colors.primary;
        visuals.warn_fg_color = colors.warning;
        visuals.error_fg_color = colors.error_text;
        visuals.selection.bg_fill = colors.selection;
        visuals.selection.stroke = Stroke::new(1.0, colors.primary);
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = CornerRadius::same(6);
            widget.bg_stroke = Stroke::new(1.0, colors.border);
            widget.fg_stroke = Stroke::new(1.0, colors.text);
        }
        visuals.widgets.noninteractive.bg_fill = colors.surface;
        visuals.widgets.inactive.bg_fill = colors.raised;
        visuals.widgets.inactive.weak_bg_fill = colors.raised;
        visuals.widgets.hovered.bg_fill = colors.hover_surface;
        visuals.widgets.hovered.weak_bg_fill = colors.hover_surface;
        visuals.widgets.active.bg_fill = colors.selection;
        visuals.widgets.active.weak_bg_fill = colors.selection;
        visuals.widgets.open.bg_fill = colors.raised;
        ctx.set_visuals_of(egui_theme, visuals);
        ctx.style_mut_of(egui_theme, |style| {
            style.spacing.item_spacing = vec2(SPACE, SPACE);
            style.spacing.button_padding = vec2(10.0, 5.0);
            style.spacing.window_margin = egui::Margin::same(12);
            style.spacing.menu_margin = egui::Margin::same(8);
            style.spacing.interact_size.y = 28.0;
        });
        ctx.set_theme(egui_theme);
    }
}

pub fn card<R>(
    ui: &mut egui::Ui,
    colors: Palette,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(CARD_RADIUS)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            content(ui)
        })
}

pub fn section_header(ui: &mut egui::Ui, title: &str, subtitle: &str, colors: Palette) {
    ui.vertical(|ui| {
        ui.label(RichText::new(title).size(17.0).strong().color(colors.text));
        ui.label(
            RichText::new(subtitle)
                .size(12.0)
                .color(colors.text_secondary),
        );
    });
}

pub fn primary_button(ui: &mut egui::Ui, title: &str, colors: Palette) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(title).strong().color(Color32::WHITE))
            .fill(colors.primary_hover),
    )
}

pub fn error_message(ui: &mut egui::Ui, message: &str, colors: Palette) {
    ui.horizontal(|ui| {
        ui.colored_label(colors.error, "●");
        ui.colored_label(colors.error_text, message);
    });
}

pub fn metric(ui: &mut egui::Ui, label: &str, value: &str, colors: Palette) {
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(CARD_RADIUS)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(value).size(19.0).strong().color(colors.text));
                ui.label(RichText::new(label).size(11.0).color(colors.text_secondary));
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_themes_keep_the_requested_brand_colors() {
        let dark = JetTheme::Dark.palette();
        assert_eq!(dark.background, Color32::from_rgb(0x10, 0x13, 0x18));
        assert_eq!(dark.surface, Color32::from_rgb(0x1B, 0x20, 0x29));
        assert_eq!(dark.primary, Color32::from_rgb(0x3B, 0x82, 0xF6));
        let light = JetTheme::Light.palette();
        assert_eq!(light.background, Color32::from_rgb(0xF8, 0xFA, 0xFC));
        assert_eq!(light.surface, Color32::WHITE);
        assert_eq!(light.text, Color32::from_rgb(0x17, 0x20, 0x33));
        assert_eq!(light.primary, Color32::from_rgb(0x25, 0x63, 0xEB));
    }

    #[test]
    fn switching_themes_updates_the_active_egui_style() {
        let ctx = egui::Context::default();
        JetTheme::Light.apply(&ctx);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(
            ctx.global_style().visuals.panel_fill,
            JetTheme::Light.palette().background
        );
        JetTheme::Dark.apply(&ctx);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        assert_eq!(
            ctx.global_style().visuals.panel_fill,
            JetTheme::Dark.palette().background
        );
    }

    #[test]
    fn text_and_selection_have_readable_contrast() {
        fn luminance(color: Color32) -> f32 {
            let [r, g, b, _] = color.to_array();
            let linear = |channel: u8| {
                let value = f32::from(channel) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        }
        fn ratio(a: Color32, b: Color32) -> f32 {
            let (high, low) = (
                luminance(a).max(luminance(b)),
                luminance(a).min(luminance(b)),
            );
            (high + 0.05) / (low + 0.05)
        }
        for theme in [JetTheme::Dark, JetTheme::Light] {
            let colors = theme.palette();
            assert!(ratio(colors.text, colors.surface) >= 4.5);
            assert!(ratio(colors.text_secondary, colors.surface) >= 4.5);
            assert!(ratio(colors.text_secondary, colors.row_odd) >= 4.5);
            assert!(ratio(colors.accent_text, colors.surface) >= 4.5);
            assert!(ratio(colors.success, colors.surface) >= 4.5);
            assert!(ratio(colors.warning, colors.surface) >= 4.5);
            assert!(ratio(colors.error_text, colors.surface) >= 4.5);
            assert!(ratio(Color32::WHITE, colors.primary_hover) >= 4.5);
            assert!(ratio(colors.text, colors.selection) >= 4.5);
        }
    }
}
