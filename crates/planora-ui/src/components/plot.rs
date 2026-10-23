//! Native charts use the same observed samples as the table alternatives.
use crate::formatting::{format_bytes, format_rate};
use crate::history::{HistoryRange, Sample, TimeSeries};
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
    cache_key: Option<PlotCacheKey>,
    cache_now: Option<Instant>,
    cached_segments: Vec<Vec<Vec<(f32, f32)>>>,
    bar_cache_key: Option<PlotCacheKey>,
    bar_cache_now: Option<Instant>,
    cached_bars: Vec<Option<ActivityBucket>>,
}

#[derive(PartialEq)]
struct PlotCacheKey {
    range: HistoryRange,
    width: u32,
    scale_bits: u32,
    series: Vec<(usize, u64)>,
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
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );

    let key = PlotCacheKey {
        range,
        width: canvas.width().round() as u32,
        scale_bits: ui.ctx().pixels_per_point().to_bits(),
        series: series
            .iter()
            .map(|item| {
                (
                    item.samples.samples.len(),
                    item.samples
                        .samples
                        .back()
                        .map_or(0, |sample| sample.timestamp_ms),
                )
            })
            .collect(),
    };
    let expired = state.cache_now.is_none_or(|cached_at| {
        now < cached_at || now.saturating_duration_since(cached_at) >= Duration::from_secs(1)
    });
    if state.cache_key.as_ref() != Some(&key) || expired {
        let gap = Duration::from_secs(3);
        state.cached_segments = series
            .iter()
            .map(|item| {
                item.samples
                    .downsampled_segments(now, range, gap, canvas.width() as usize)
            })
            .collect();
        state.cache_key = Some(key);
        state.cache_now = Some(now);
    }
    let segments = &state.cached_segments;
    let observed_max = segments
        .iter()
        .flatten()
        .flatten()
        .fold(0.0f32, |max, (_, value)| max.max(*value));
    let ymax = if let Some(max) = fixed_max {
        max.max(1.0)
    } else {
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
    for (item, item_segments) in series.iter().zip(segments) {
        for segment in item_segments {
            let points: Vec<Pos2> = segment.iter().map(|(x, y)| to_point(*x, *y)).collect();
            if points.len() >= 2 && !item.dashed {
                for pair in points.windows(2) {
                    painter.add(Shape::convex_polygon(
                        vec![
                            pair[0],
                            pair[1],
                            Pos2::new(pair[1].x, canvas.bottom()),
                            Pos2::new(pair[0].x, canvas.bottom()),
                        ],
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
    if response.has_focus() {
        let cutoff = now.checked_sub(range.duration()).unwrap_or(now);
        let mut observed: Vec<f32> = series
            .iter()
            .flat_map(|item| item.samples.samples.iter())
            .filter(|sample| sample.value.is_some() && sample.time >= cutoff && sample.time <= now)
            .map(|sample| {
                (sample.time.saturating_duration_since(cutoff).as_secs_f32()
                    / range.duration().as_secs_f32())
                .clamp(0.0, 1.0)
            })
            .collect();
        observed.sort_by(f32::total_cmp);
        observed.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
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
                if let Some(target) = observed
                    .iter()
                    .rev()
                    .find(|&&x| x < state.cursor.unwrap_or(1.01) - f32::EPSILON)
                    .copied()
                {
                    state.cursor = Some(target);
                } else if state.cursor.is_none() {
                    state.cursor = observed.last().copied();
                }
                state.pinned = true;
            } else if input.key_pressed(egui::Key::ArrowRight) {
                if let Some(target) = observed
                    .iter()
                    .find(|&&x| x > state.cursor.unwrap_or(-0.01) + f32::EPSILON)
                    .copied()
                {
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
        let mut concise = Vec::new();
        let tolerance = Duration::from_secs_f32(
            (range.duration().as_secs_f32() / canvas.width() * 1.5).clamp(1.5, 2.5),
        );
        for item in series {
            if let Some((sample, age)) = item
                .samples
                .query_sample_near(now, range, cursor, tolerance)
            {
                let reading = sample.value.map_or_else(
                    || {
                        sample
                            .unavailable_reason
                            .as_ref()
                            .map_or_else(|| "Unavailable".to_owned(), ToString::to_string)
                    },
                    |v| format_plot_value(v, unit),
                );
                details.push((item.name, reading.clone(), sample, age));
                concise.push(format!("{}: {}", item.name, reading));
            }
        }
        if !details.is_empty() {
            response.on_hover_ui_at_pointer(|ui| {
                ui.set_max_width(280.0);
                for (name, reading, sample, age) in &details {
                    ui.horizontal(|ui| {
                        ui.strong(*name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.strong(reading);
                        });
                    });
                    ui.label(
                        egui::RichText::new(format!(
                            "{} ago · {}",
                            format_age(*age),
                            sample.source
                        ))
                        .small(),
                    );
                    if let (Some(min), Some(max)) = (sample.minimum, sample.maximum)
                        && (max - min).abs() >= 0.05
                    {
                        ui.label(
                            egui::RichText::new(format!(
                                "Observed range: {}–{}",
                                format_plot_value(min, unit),
                                format_plot_value(max, unit)
                            ))
                            .small(),
                        );
                    }
                    if let Some(reason) = &sample.unavailable_reason {
                        ui.label(egui::RichText::new(reason.to_string()).small());
                    }
                }
            });
            let readout_summary = if state.pinned {
                format!("Pinned · {}", concise.join(" · "))
            } else {
                concise.join(" · ")
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

fn format_age(age: Duration) -> String {
    if age.as_secs() < 60 {
        format!("{}s", age.as_secs())
    } else {
        format!("{}m {}s", age.as_secs() / 60, age.as_secs() % 60)
    }
}

fn format_plot_value(value: f32, unit: &str) -> String {
    match unit {
        "%" => format!("{value:.1}%"),
        "B/s" => format_rate(Some(value)),
        "bytes" => format_bytes(value.max(0.0) as u64),
        _ => format!("{value:.1} {unit}"),
    }
}

#[derive(Clone)]
struct ActivityBucket {
    sum: f64,
    count: u32,
    minimum: f32,
    maximum: f32,
    source: String,
    source_changed: bool,
    last_time: Instant,
}

impl ActivityBucket {
    fn new(sample: &Sample, value: f32) -> Self {
        Self {
            sum: sample.sum,
            count: sample.count.max(1),
            minimum: sample.minimum.unwrap_or(value),
            maximum: sample.maximum.unwrap_or(value),
            source: sample.source.clone(),
            source_changed: false,
            last_time: sample.time,
        }
    }

    fn include(&mut self, sample: &Sample, value: f32) {
        self.sum += sample.sum;
        self.count = self.count.saturating_add(sample.count.max(1));
        self.minimum = self.minimum.min(sample.minimum.unwrap_or(value));
        self.maximum = self.maximum.max(sample.maximum.unwrap_or(value));
        self.source_changed |= self.source != sample.source;
        self.last_time = self.last_time.max(sample.time);
    }

    fn mean(&self) -> f32 {
        (self.sum / self.count.max(1) as f64) as f32
    }
}

fn activity_bucket_span(range: HistoryRange) -> usize {
    match range {
        HistoryRange::Sec30 | HistoryRange::Sec60 => 1,
        HistoryRange::Min5 => 3,
        HistoryRange::Min15 => 9,
    }
}

fn activity_buckets(
    series: &TimeSeries,
    now: Instant,
    range: HistoryRange,
) -> Vec<Option<ActivityBucket>> {
    let span = activity_bucket_span(range);
    let len = range.duration().as_secs() as usize / span;
    let cutoff = now.checked_sub(range.duration()).unwrap_or(now);
    let mut buckets = vec![None::<ActivityBucket>; len];
    for sample in &series.samples {
        if sample.time < cutoff || sample.time > now {
            continue;
        }
        let Some(value) = sample.value.filter(|v| v.is_finite()) else {
            continue;
        };
        let index = (sample.time.duration_since(cutoff).as_secs() as usize / span).min(len - 1);
        match &mut buckets[index] {
            Some(bucket) => bucket.include(sample, value),
            slot @ None => *slot = Some(ActivityBucket::new(sample, value)),
        }
    }
    buckets
}

/// A time-aligned CPU activity column chart. Empty columns are unobserved
/// intervals; short spikes are retained as a thin maximum marker over the
/// observed mean, rather than connected into a sawtooth line.
pub fn activity_bars(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    series: &TimeSeries,
    range: HistoryRange,
    now: Instant,
    height: f32,
    state: &mut PlotState,
    color: Color32,
    p: Palette,
) {
    let chart_id = ui.make_persistent_id(id);
    let width = ui.available_width().max(140.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height.max(190.0)), Sense::hover());
    let response = ui.interact(rect, chart_id, Sense::click_and_drag());
    let canvas = Rect::from_min_max(
        Pos2::new(rect.left() + 48.0, rect.top() + 28.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 30.0),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.surface);
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );

    let key = PlotCacheKey {
        range,
        width: canvas.width().round() as u32,
        scale_bits: ui.ctx().pixels_per_point().to_bits(),
        series: vec![(
            series.samples.len(),
            series
                .samples
                .back()
                .map_or(0, |sample| sample.timestamp_ms),
        )],
    };
    let expired = state.bar_cache_now.is_none_or(|cached_at| {
        now < cached_at || now.saturating_duration_since(cached_at) >= Duration::from_secs(1)
    });
    if state.bar_cache_key.as_ref() != Some(&key) || expired {
        state.cached_bars = activity_buckets(series, now, range);
        state.bar_cache_key = Some(key);
        state.bar_cache_now = Some(now);
    }
    let buckets = &state.cached_bars;
    let bucket_width = canvas.width() / buckets.len().max(1) as f32;
    let span = activity_bucket_span(range);
    let observed = series.observed_summary(now, range);

    for tick in [0, 25, 50, 75, 100] {
        let y = canvas.bottom() - tick as f32 / 100.0 * canvas.height();
        painter.line_segment(
            [Pos2::new(canvas.left(), y), Pos2::new(canvas.right(), y)],
            Stroke::new(1.0, p.border),
        );
        painter.text(
            Pos2::new(canvas.left() - 7.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{tick}%"),
            egui::FontId::proportional(11.0),
            p.secondary,
        );
    }
    for fraction in [0.25, 0.5, 0.75] {
        let x = canvas.left() + canvas.width() * fraction;
        painter.line_segment(
            [Pos2::new(x, canvas.top()), Pos2::new(x, canvas.bottom())],
            Stroke::new(1.0, p.border.gamma_multiply(0.6)),
        );
    }
    let bar_width = (bucket_width - 2.0).max(1.0);
    for (index, bucket) in buckets.iter().enumerate() {
        let Some(bucket) = bucket else { continue };
        let x = canvas.left() + index as f32 * bucket_width + (bucket_width - bar_width) * 0.5;
        let bar_height = (bucket.mean().clamp(0.0, 100.0) / 100.0 * canvas.height()).max(2.0);
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(x, canvas.bottom() - bar_height),
                Vec2::new(bar_width, bar_height),
            ),
            2.0,
            color.gamma_multiply(0.78),
        );
        if bucket.maximum > bucket.mean() + 0.5 {
            let peak_y =
                canvas.bottom() - bucket.maximum.clamp(0.0, 100.0) / 100.0 * canvas.height();
            painter.line_segment(
                [Pos2::new(x, peak_y), Pos2::new(x + bar_width, peak_y)],
                Stroke::new(2.0, color),
            );
        }
    }

    painter.text(
        Pos2::new(canvas.left() + 4.0, rect.top() + 9.0),
        egui::Align2::LEFT_CENTER,
        "CPU activity · average per interval",
        egui::FontId::proportional(12.0),
        p.text,
    );
    painter.text(
        Pos2::new(canvas.right(), rect.top() + 9.0),
        egui::Align2::RIGHT_CENTER,
        format!(
            "{}s bars · {}/{}s observed",
            span,
            observed.map_or(0, |s| s.observed_seconds),
            range.duration().as_secs()
        ),
        egui::FontId::proportional(11.0),
        p.secondary,
    );
    painter.text(
        Pos2::new(canvas.left(), rect.bottom() - 14.0),
        egui::Align2::LEFT_CENTER,
        format!("{} ago", range.label()),
        egui::FontId::proportional(11.0),
        p.secondary,
    );
    painter.text(
        Pos2::new(canvas.right(), rect.bottom() - 14.0),
        egui::Align2::RIGHT_CENTER,
        "Now",
        egui::FontId::proportional(11.0),
        p.secondary,
    );
    if observed
        .is_none_or(|summary| summary.observed_seconds < range.duration().as_secs() as usize / 3)
    {
        painter.text(
            canvas.center() - Vec2::new(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            "Building history",
            egui::FontId::proportional(17.0),
            p.secondary,
        );
        painter.text(
            canvas.center() + Vec2::new(0.0, 12.0),
            egui::Align2::CENTER_CENTER,
            "New columns appear as measurements arrive",
            egui::FontId::proportional(11.0),
            p.secondary,
        );
    }

    let hovered_index = response
        .hover_pos()
        .and_then(|pos| {
            canvas
                .contains(pos)
                .then(|| ((pos.x - canvas.left()) / bucket_width).floor() as usize)
        })
        .filter(|index| *index < buckets.len());
    if response.clicked() {
        response.request_focus();
        state.pinned = !state.pinned && hovered_index.is_some_and(|index| buckets[index].is_some());
        state.cursor = if state.pinned {
            hovered_index.map(|index| (index as f32 + 0.5) / buckets.len() as f32)
        } else {
            None
        };
    } else if !state.pinned {
        state.cursor = hovered_index.map(|index| (index as f32 + 0.5) / buckets.len() as f32);
    }
    if response.has_focus() {
        let observed_indices: Vec<usize> = buckets
            .iter()
            .enumerate()
            .filter_map(|(index, bucket)| bucket.as_ref().map(|_| index))
            .collect();
        let cursor_index = state
            .cursor
            .map(|x| (x * buckets.len() as f32).floor() as usize);
        let next = ui.input(|input| {
            if input.key_pressed(egui::Key::Escape) {
                state.pinned = false;
                state.cursor = None;
                return None;
            }
            if input.key_pressed(egui::Key::Home) {
                return observed_indices.first().copied();
            }
            if input.key_pressed(egui::Key::End) {
                return observed_indices.last().copied();
            }
            if input.key_pressed(egui::Key::ArrowLeft) {
                return observed_indices
                    .iter()
                    .rev()
                    .find(|&&index| cursor_index.is_none_or(|current| index < current))
                    .copied();
            }
            if input.key_pressed(egui::Key::ArrowRight) {
                return observed_indices
                    .iter()
                    .find(|&&index| cursor_index.is_none_or(|current| index > current))
                    .copied();
            }
            None
        });
        if let Some(index) = next {
            state.pinned = true;
            state.cursor = Some((index as f32 + 0.5) / buckets.len() as f32);
        }
    }
    if let Some(index) = state
        .cursor
        .map(|x| (x * buckets.len() as f32).floor() as usize)
        .filter(|index| *index < buckets.len())
        && let Some(bucket) = &buckets[index]
    {
        let x = canvas.left() + (index as f32 + 0.5) * bucket_width;
        painter.line_segment(
            [Pos2::new(x, canvas.top()), Pos2::new(x, canvas.bottom())],
            Stroke::new(1.0, p.focus),
        );
        let age = now.saturating_duration_since(bucket.last_time);
        response.on_hover_ui_at_pointer(|ui| {
            ui.set_min_width(205.0);
            ui.strong(format!("CPU · {} ago", format_age(age)));
            ui.heading(format_plot_value(bucket.mean(), "%"));
            ui.separator();
            egui::Grid::new("cpu_activity_bucket_tooltip")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Observed range");
                    ui.label(format!("{:.1}–{:.1}%", bucket.minimum, bucket.maximum));
                    ui.end_row();
                    ui.label("Readings");
                    ui.label(bucket.count.to_string());
                    ui.end_row();
                    ui.label("Source");
                    ui.label(if bucket.source_changed {
                        "Multiple providers"
                    } else {
                        &bucket.source
                    });
                    ui.end_row();
                });
        });
    }
}

pub fn sparkline(ui: &mut egui::Ui, series: &TimeSeries, now: Instant, color: Color32, p: Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(68.0, 28.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, p.surface_raised);
    let segments =
        series.downsampled_segments(now, HistoryRange::Sec60, Duration::from_secs(3), 68);
    let max = segments
        .iter()
        .flatten()
        .fold(1.0f32, |acc, (_, y)| acc.max(*y));
    for segment in segments {
        let points: Vec<_> = segment
            .into_iter()
            .map(|(x, y)| {
                Pos2::new(
                    rect.left() + x * rect.width(),
                    rect.bottom() - (y / max).clamp(0.0, 1.0) * rect.height(),
                )
            })
            .collect();
        if points.len() >= 2 {
            ui.painter()
                .add(Shape::line(points, Stroke::new(1.4, color)));
        } else if let Some(point) = points.first() {
            ui.painter().circle_filled(*point, 2.0, color);
        }
    }
}

pub fn capacity_bar(
    ui: &mut egui::Ui,
    used: Option<u64>,
    total: Option<u64>,
    color: Color32,
    p: Palette,
) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(100.0), 14.0),
        Sense::hover(),
    );
    ui.painter().rect_filled(rect, 4.0, p.surface_raised);
    if let (Some(used), Some(total)) = (used, total)
        && total > 0
    {
        let fraction = (used as f64 / total as f64).clamp(0.0, 1.0) as f32;
        let fill = Rect::from_min_size(rect.min, Vec2::new(rect.width() * fraction, rect.height()));
        ui.painter().rect_filled(fill, 4.0, color);
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Unavailable",
            egui::FontId::proportional(10.0),
            p.secondary,
        );
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
    use super::{activity_buckets, format_plot_value, nice_ceiling};
    use crate::history::{HistoryRange, TimeSeries};
    use std::time::{Duration, Instant};
    #[test]
    fn axis_bounds_enclose_observed_positive_values() {
        for value in [0.0, 0.001, 1.0, 27.5, 100.0, 1_048_576.0] {
            assert!(nice_ceiling(value) >= value);
        }
    }

    #[test]
    fn activity_chart_keeps_unobserved_time_blank_and_preserves_zero_and_peak() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        series.push(now - Duration::from_secs(2), 1_000, Some(0.0));
        series.push(now - Duration::from_secs(1), 2_000, Some(10.0));
        series.push(now - Duration::from_millis(500), 3_000, Some(70.0));
        let buckets = activity_buckets(&series, now, HistoryRange::Sec60);
        assert_eq!(buckets.len(), 60);
        assert!(buckets[..58].iter().all(Option::is_none));
        assert_eq!(buckets[58].as_ref().unwrap().mean(), 0.0);
        assert_eq!(buckets[59].as_ref().unwrap().mean(), 40.0);
        assert_eq!(buckets[59].as_ref().unwrap().maximum, 70.0);
    }

    #[test]
    fn long_ranges_group_columns_without_filling_missing_intervals() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        series.push(now - Duration::from_secs(6), 1_000, Some(25.0));
        let buckets = activity_buckets(&series, now, HistoryRange::Min5);
        assert_eq!(buckets.len(), 100);
        assert_eq!(buckets.iter().flatten().count(), 1);
    }

    #[test]
    fn chart_readouts_use_human_units() {
        assert_eq!(format_plot_value(9.13, "%"), "9.1%");
        assert_eq!(format_plot_value(2048.0, "B/s"), "2.0 KiB/s");
    }
}
