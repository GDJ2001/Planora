//! Native charts use the same observed samples as the table alternatives.
use crate::history::{HistoryRange, TimeSeries};
use crate::theme::Palette;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use std::time::{Duration, Instant};

pub struct PlotSeries<'a> {
    pub name: &'a str,
    pub samples: &'a TimeSeries,
    pub color: Color32,
    pub dashed: bool,
}

#[derive(Default)]
pub struct PlotState {
    pub cursor: Option<f32>,
    pub pinned: bool,
    axis_max: f32,
    below_half_since: Option<Instant>,
}

/// Paint a bounded, gap-aware plot. The source series owns the truth; this
/// widget only creates screen coordinates from observed values.
pub fn time_plot(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    series: &[PlotSeries<'_>],
    range: HistoryRange,
    now: Instant,
    unit: &str,
    fixed_max: Option<f32>,
    height: f32,
    state: &mut PlotState,
    p: Palette,
) {
    let chart_id = ui.make_persistent_id(id);
    let width = ui.available_width().max(140.0);
    let height = height.max(180.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let response = ui.interact(rect, chart_id, Sense::click_and_drag());
    let canvas = Rect::from_min_max(
        Pos2::new(rect.left() + 48.0, rect.top() + 16.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 30.0),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.surface);
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);

    let gap = Duration::from_secs(3);
    let segments: Vec<_> = series
        .iter()
        .map(|item| item.samples.downsampled_segments(now, range, gap, canvas.width() as usize))
        .collect();
    let observed_max = segments
        .iter()
        .flatten()
        .flatten()
        .fold(0.0f32, |max, (_, value)| max.max(*value));
    let ymax = if let Some(max) = fixed_max { max.max(1.0) } else {
        let target = nice_ceiling(observed_max).max(1.0);
        if target > state.axis_max {
            state.axis_max = target;
            state.below_half_since = None;
        } else if target < state.axis_max * 0.5 {
            let since = state.below_half_since.get_or_insert(now);
            if now.saturating_duration_since(*since) >= Duration::from_secs(10) {
                state.axis_max = target;
                state.below_half_since = None;
            }
        } else {
            state.below_half_since = None;
        }
        state.axis_max.max(1.0)
    };

    for row in 0..=4 {
        let y = canvas.top() + canvas.height() * row as f32 / 4.0;
        painter.line_segment(
            [Pos2::new(canvas.left(), y), Pos2::new(canvas.right(), y)],
            Stroke::new(1.0, p.border),
        );
        painter.text(
            Pos2::new(canvas.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            format_axis(ymax * (4 - row) as f32 / 4.0),
            egui::FontId::proportional(11.0),
            p.secondary,
        );
    }
    for col in 0..=4 {
        let x = canvas.left() + canvas.width() * col as f32 / 4.0;
        painter.line_segment(
            [Pos2::new(x, canvas.top()), Pos2::new(x, canvas.bottom())],
            Stroke::new(1.0, p.border),
        );
    }
    painter.text(
        Pos2::new(canvas.left(), rect.bottom() - 15.0),
        egui::Align2::LEFT_CENTER,
        format!("{} ago", range.label()),
        egui::FontId::proportional(11.0),
        p.secondary,
    );
    painter.text(
        Pos2::new(canvas.right(), rect.bottom() - 15.0),
        egui::Align2::RIGHT_CENTER,
        "Now",
        egui::FontId::proportional(11.0),
        p.secondary,
    );
    painter.text(
        Pos2::new(canvas.left(), rect.top() + 8.0),
        egui::Align2::LEFT_CENTER,
        unit,
        egui::FontId::proportional(11.0),
        p.secondary,
    );

    let to_point = |x: f32, y: f32| {
        Pos2::new(
            canvas.left() + x * canvas.width(),
            canvas.bottom() - (y / ymax).clamp(0.0, 1.0) * canvas.height(),
        )
    };
    for (item, item_segments) in series.iter().zip(&segments) {
        for segment in item_segments {
            let points: Vec<Pos2> = segment.iter().map(|(x, y)| to_point(*x, *y)).collect();
            if points.len() >= 2 && !item.dashed {
                for pair in points.windows(2) {
                    painter.add(Shape::convex_polygon(
                        vec![pair[0], pair[1], Pos2::new(pair[1].x, canvas.bottom()), Pos2::new(pair[0].x, canvas.bottom())],
                        item.color.gamma_multiply(0.12),
                        Stroke::NONE,
                    ));
                }
            }
            if item.dashed {
                for (index, pair) in points.windows(2).enumerate() {
                    if index % 2 == 0 {
                        painter.line_segment([pair[0], pair[1]], Stroke::new(2.0, item.color));
                    }
                }
            } else if points.len() >= 2 {
                painter.add(Shape::line(points, Stroke::new(2.0, item.color)));
            } else if let Some(point) = points.first() {
                painter.circle_filled(*point, 3.5, item.color);
            }
        }
    }

    if response.clicked() {
        response.request_focus();
        state.pinned = !state.pinned;
        if !state.pinned {
            state.cursor = None;
        } else if state.cursor.is_none() {
            state.cursor = response
                .interact_pointer_pos()
                .map(|pos| ((pos.x - canvas.left()) / canvas.width()).clamp(0.0, 1.0));
        }
    }
    if response.hovered() && !state.pinned {
        state.cursor = response
            .hover_pos()
            .map(|pos| ((pos.x - canvas.left()) / canvas.width()).clamp(0.0, 1.0));
    }
    let cutoff = now.checked_sub(range.duration()).unwrap_or(now);
    let mut observed: Vec<f32> = series.iter().flat_map(|item| item.samples.samples.iter())
        .filter(|sample| sample.value.is_some() && sample.time >= cutoff && sample.time <= now)
        .map(|sample| (sample.time.saturating_duration_since(cutoff).as_secs_f32() / range.duration().as_secs_f32()).clamp(0.0, 1.0))
        .collect();
    observed.sort_by(f32::total_cmp);
    observed.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
    if response.has_focus() {
        ui.input(|input| {
            if input.key_pressed(egui::Key::Escape) {
                state.pinned = false;
                state.cursor = None;
            } else if input.key_pressed(egui::Key::Home) {
                state.cursor = observed.first().copied();
                state.pinned = true;
            } else if input.key_pressed(egui::Key::End) {
                state.cursor = observed.last().copied();
                state.pinned = true;
            } else if input.key_pressed(egui::Key::ArrowLeft) {
                if let Some(target) = observed.iter().rev().find(|&&x| x < state.cursor.unwrap_or(1.01) - f32::EPSILON).copied() {
                    state.cursor = Some(target);
                } else if state.cursor.is_none() {
                    state.cursor = observed.last().copied();
                }
                state.pinned = true;
            } else if input.key_pressed(egui::Key::ArrowRight) {
                if let Some(target) = observed.iter().find(|&&x| x > state.cursor.unwrap_or(-0.01) + f32::EPSILON).copied() {
                    state.cursor = Some(target);
                } else if state.cursor.is_none() {
                    state.cursor = observed.first().copied();
                }
                state.pinned = true;
            }
        });
    }
    if let Some(cursor) = state.cursor {
        let x = canvas.left() + cursor * canvas.width();
        painter.line_segment(
            [Pos2::new(x, canvas.top()), Pos2::new(x, canvas.bottom())],
            Stroke::new(1.0, p.focus),
        );
        let mut details = Vec::new();
        for item in series {
            if let Some((sample, age)) = item.samples.query_sample(now, range, cursor) {
                let reading = sample.value.map_or_else(
                    || sample.unavailable_reason.as_ref().map_or_else(|| "Unavailable".to_owned(), ToString::to_string),
                    |v| format!("{} {unit}", format_axis(v)),
                );
                details.push(format!("{}: {} (sample {} ms, {} s ago, {}; {} observations, min {:?}, max {:?})", item.name, reading, sample.timestamp_ms, age.as_secs(), sample.source, sample.count, sample.minimum, sample.maximum));
            }
        }
        if !details.is_empty() {
            response.on_hover_text_at_pointer(details.join("\n"));
            let readout_summary = if state.pinned {
                format!("📌 Pinned: {}", details[0])
            } else {
                details[0].clone()
            };
            painter.text(
                Pos2::new(canvas.left() + 8.0, canvas.top() + 6.0),
                egui::Align2::LEFT_TOP,
                readout_summary,
                egui::FontId::proportional(11.0),
                p.focus,
            );
        }
    }
}

pub fn sparkline(ui: &mut egui::Ui, series: &TimeSeries, now: Instant, color: Color32, p: Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(68.0, 28.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, p.surface_raised);
    let segments = series.downsampled_segments(now, HistoryRange::Sec60, Duration::from_secs(3), 68);
    let max = segments.iter().flatten().fold(1.0f32, |acc, (_, y)| acc.max(*y));
    for segment in segments {
        let points: Vec<_> = segment.into_iter().map(|(x, y)| Pos2::new(rect.left() + x * rect.width(), rect.bottom() - (y / max).clamp(0.0, 1.0) * rect.height())).collect();
        if points.len() >= 2 {
            ui.painter().add(Shape::line(points, Stroke::new(1.4, color)));
        } else if let Some(point) = points.first() {
            ui.painter().circle_filled(*point, 2.0, color);
        }
    }
}

pub fn capacity_bar(ui: &mut egui::Ui, used: Option<u64>, total: Option<u64>, color: Color32, p: Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().max(100.0), 14.0), Sense::hover());
    ui.painter().rect_filled(rect, 4.0, p.surface_raised);
    if let (Some(used), Some(total)) = (used, total) && total > 0 {
        let fraction = (used as f64 / total as f64).clamp(0.0, 1.0) as f32;
        let fill = Rect::from_min_size(rect.min, Vec2::new(rect.width() * fraction, rect.height()));
        ui.painter().rect_filled(fill, 4.0, color);
    }
}

fn nice_ceiling(value: f32) -> f32 {
    if !value.is_finite() || value <= 0.0 {
        return 1.0;
    }
    let step = 10f32.powf(value.log10().floor());
    for multiple in [1.0, 2.0, 5.0, 10.0] {
        if value <= step * multiple {
            return step * multiple;
        }
    }
    step * 10.0
}

fn format_axis(value: f32) -> String {
    if value >= 1_000_000_000.0 {
        format!("{:.1}G", value / 1_000_000_000.0)
    } else if value >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1}k", value / 1_000.0)
    } else if value >= 10.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::nice_ceiling;
    #[test]
    fn axis_bounds_enclose_observed_positive_values() {
        for value in [0.0, 0.001, 1.0, 27.5, 100.0, 1_048_576.0] {
            assert!(nice_ceiling(value) >= value);
        }
    }
}
