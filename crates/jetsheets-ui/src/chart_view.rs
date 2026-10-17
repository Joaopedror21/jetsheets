use eframe::egui::{self, Align2, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use jetsheets_charts::{ChartKind, ChartPoint, ChartSpec};

use crate::theme::{self, Palette};

/// Draws a live chart from the rows currently visible in the table.
pub fn render(ui: &mut egui::Ui, spec: &ChartSpec, points: &[ChartPoint], colors: Palette) {
    ui.label(
        egui::RichText::new(&spec.title)
            .strong()
            .size(16.0)
            .color(colors.text),
    );
    if points.is_empty() {
        ui.label("Nenhum valor numérico disponível nas linhas visíveis.");
        return;
    }

    let plotted = &points[..points.len().min(48)];
    let desired = vec2(ui.available_width().max(320.0), 260.0);
    let (outer, response) = ui.allocate_exact_size(desired, Sense::hover());
    let painter = ui.painter_at(outer);
    painter.rect_filled(outer, theme::CARD_RADIUS as f32, colors.raised);
    painter.rect_stroke(
        outer,
        theme::CARD_RADIUS as f32,
        Stroke::new(1.0, colors.border),
        StrokeKind::Inside,
    );

    let plot = Rect::from_min_max(
        pos2(outer.left() + 66.0, outer.top() + 22.0),
        pos2(outer.right() - 18.0, outer.bottom() - 43.0),
    );
    let mut low = plotted
        .iter()
        .map(|p| p.value)
        .fold(f64::INFINITY, f64::min);
    let mut high = plotted
        .iter()
        .map(|p| p.value)
        .fold(f64::NEG_INFINITY, f64::max);
    if spec.kind == ChartKind::Bar {
        low = low.min(0.0);
        high = high.max(0.0);
    }
    if (high - low).abs() < f64::EPSILON {
        let padding = high.abs().max(1.0) * 0.1;
        low -= padding;
        high += padding;
    } else if spec.kind == ChartKind::Line {
        let padding = (high - low) * 0.08;
        low -= padding;
        high += padding;
    }
    let y_of = |value: f64| plot.bottom() - ((value - low) / (high - low)) as f32 * plot.height();

    for tick in 0..=4 {
        let fraction = tick as f32 / 4.0;
        let y = plot.bottom() - fraction * plot.height();
        let value = low + (high - low) * f64::from(fraction);
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, colors.grid),
        );
        painter.text(
            pos2(plot.left() - 8.0, y),
            Align2::RIGHT_CENTER,
            compact_number(value),
            FontId::proportional(11.0),
            colors.text_secondary,
        );
    }

    if low <= 0.0 && high >= 0.0 {
        let zero = y_of(0.0);
        painter.line_segment(
            [pos2(plot.left(), zero), pos2(plot.right(), zero)],
            Stroke::new(1.2, colors.text_secondary),
        );
    }

    let slot = plot.width() / plotted.len() as f32;
    let hover = response.hover_pos();
    let mut hovered = None;
    let label_step = (plotted.len() as f32 / 8.0).ceil().max(1.0) as usize;

    match spec.kind {
        ChartKind::Bar => {
            let zero_y = y_of(0.0).clamp(plot.top(), plot.bottom());
            for (index, point) in plotted.iter().enumerate() {
                let center = plot.left() + (index as f32 + 0.5) * slot;
                let bar_width = (slot * 0.68).clamp(2.0, 38.0);
                let y = y_of(point.value);
                let rect = Rect::from_min_max(
                    pos2(center - bar_width / 2.0, y.min(zero_y)),
                    pos2(center + bar_width / 2.0, y.max(zero_y)),
                );
                let active = hover.is_some_and(|p| rect.expand(3.0).contains(p));
                painter.rect_filled(
                    rect,
                    3.0,
                    if active {
                        colors.primary_hover
                    } else {
                        colors.primary
                    },
                );
                if active {
                    hovered = Some(point);
                }
                if index % label_step == 0 {
                    draw_x_label(&painter, center, plot.bottom() + 9.0, &point.label, colors);
                }
            }
        }
        ChartKind::Line => {
            let mut previous = None;
            for (index, point) in plotted.iter().enumerate() {
                let x = if plotted.len() == 1 {
                    plot.center().x
                } else {
                    plot.left() + index as f32 / (plotted.len() - 1) as f32 * plot.width()
                };
                let current = pos2(x, y_of(point.value));
                if let Some(previous) = previous {
                    painter.line_segment([previous, current], Stroke::new(2.5, colors.primary));
                }
                let active = hover.is_some_and(|p| p.distance(current) <= 9.0);
                painter.circle_filled(
                    current,
                    if active { 5.0 } else { 3.5 },
                    if active {
                        colors.primary_hover
                    } else {
                        colors.primary
                    },
                );
                if active {
                    hovered = Some(point);
                }
                if index % label_step == 0 {
                    draw_x_label(&painter, x, plot.bottom() + 9.0, &point.label, colors);
                }
                previous = Some(current);
            }
        }
    }

    if let Some(point) = hovered {
        ui.label(format!(
            "{}  ·  {}  ·  linha {}",
            point.label,
            compact_number(point.value),
            point.row + 1
        ));
    } else if points.len() > plotted.len() {
        ui.label(format!(
            "Exibindo {} de {} pontos visíveis.",
            plotted.len(),
            points.len()
        ));
    } else {
        ui.label(format!("{} pontos visíveis", points.len()));
    }
}

fn draw_x_label(painter: &egui::Painter, x: f32, y: f32, label: &str, colors: Palette) {
    let short: String = label.chars().take(11).collect();
    painter.text(
        pos2(x, y),
        Align2::CENTER_TOP,
        short,
        FontId::proportional(10.0),
        colors.text_secondary,
    );
}

fn compact_number(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 1_000_000.0 {
        format!("{:.1} mi", value / 1_000_000.0)
    } else if magnitude >= 10_000.0 {
        format!("{:.1} mil", value / 1_000.0)
    } else if magnitude >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}
