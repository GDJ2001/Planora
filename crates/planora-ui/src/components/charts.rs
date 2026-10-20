use crate::{
    formatting::{format_duration, format_rate},
    history::{HistoryRange, TimeSeries},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Stroke, Vec2};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChartScale {
    Fixed0To100,
    Throughput,
}

pub struct ChartSeriesConfig<'a> {
    pub name: &'static str,
    pub color: Color32,
    pub series: &'a TimeSeries,
    pub unit: &'static str,
}

pub struct ChartPanelState {
    pub range: HistoryRange,
    pub show_table: bool,
    pub series1_enabled: bool,
    pub series2_enabled: bool,
    pub pointer_inside: bool,
    pub keyboard_cursor_x: Option<f32>,
}

impl Default for ChartPanelState {
    fn default() -> Self {
        Self {
            range: HistoryRange::Sec60,
            show_table: false,
            series1_enabled: true,
            series2_enabled: true,
            pointer_inside: false,
            keyboard_cursor_x: None,
        }
    }
}

pub fn render_chart_panel(
    ui: &mut egui::Ui,
    title: &str,
    state: &mut ChartPanelState,
    scale: ChartScale,
    mut series_configs: Vec<ChartSeriesConfig>,
    now: Instant,
    shared_hover_norm_x: &mut Option<f32>,
    p: Palette,
) {
    let panel_w = ui.available_width();
    state.pointer_inside = false;
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_max_width((panel_w - 26.0).max(100.0));
            ui.set_width((panel_w - 26.0).max(100.0));

            // Header: Title, Range buttons, and Tabular View toggle
            ui.horizontal(|ui| {
                ui.strong(title);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Tabular alternative button
                    let tbl_text = if state.show_table { "Chart" } else { "Table" };
                    if ui.small_button(tbl_text).clicked() {
                        state.show_table = !state.show_table;
                    }

                    ui.add_space(8.0);

                    // Range selection: 30s | 60s | 5m
                    for r in [
                        HistoryRange::Min15,
                        HistoryRange::Min5,
                        HistoryRange::Sec60,
                        HistoryRange::Sec30,
                    ] {
                        let is_active = state.range == r;
                        if ui.selectable_label(is_active, r.label()).clicked() {
                            state.range = r;
                        }
                    }
                });
            });

            ui.add_space(8.0);

            if state.show_table {
                // The alternate view has no chart focus or active cursor.
                state.keyboard_cursor_x = None;
                // Tabular view of points
                ui.label(
                    egui::RichText::new("Tabular Sample History")
                        .size(11.0)
                        .color(p.secondary),
                );
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        egui::Grid::new(format!("table_{title}"))
                            .striped(true)
                            .num_columns(3)
                            .spacing([20.0, 4.0])
                            .show(ui, |ui| {
                                ui.strong("Sample Age");
                                for c in &series_configs {
                                    ui.strong(c.name);
                                }
                                ui.end_row();

                                if let Some(primary) = series_configs.first() {
                                    let cutoff =
                                        now.checked_sub(state.range.duration()).unwrap_or(now);
                                    for s in primary.series.samples.iter().rev() {
                                        if s.time < cutoff {
                                            break;
                                        }
                                        let age = now.saturating_duration_since(s.time);
                                        ui.label(format_duration(age));
                                        match s.value {
                                            Some(v) => match scale {
                                                ChartScale::Fixed0To100 => {
                                                    ui.label(format!("{v:.1}%"))
                                                }
                                                ChartScale::Throughput => {
                                                    ui.label(format_rate(Some(v)))
                                                }
                                            },
                                            None => ui.label("Unavailable"),
                                        };
                                        ui.end_row();
                                    }
                                }
                            });
                    });
            } else {
                // Interactive Chart Canvas
                let y_axis_w = 46.0;
                let canvas_h = 140.0;
                let spacing_x = ui.spacing().item_spacing.x;
                let canvas_w = (ui.available_width() - y_axis_w - spacing_x - 2.0)
                    .floor()
                    .max(80.0);

                let gap_thresh = Duration::from_millis(2500);

                // Determine scale limits
                let (max_val, y_ticks): (f32, Vec<(f32, String)>) = match scale {
                    ChartScale::Fixed0To100 => (
                        100.0,
                        vec![
                            (1.0, "100%".into()),
                            (0.75, "75%".into()),
                            (0.5, "50%".into()),
                            (0.25, "25%".into()),
                            (0.0, "0%".into()),
                        ],
                    ),
                    ChartScale::Throughput => {
                        let mut peak = 1024.0f32; // min baseline
                        for (idx, c) in series_configs.iter().enumerate() {
                            let enabled = if idx == 0 {
                                state.series1_enabled
                            } else {
                                state.series2_enabled
                            };
                            if !enabled {
                                continue;
                            }
                            for seg in c.series.normalized_segments(now, state.range, gap_thresh) {
                                for (_, v) in seg {
                                    if v > peak {
                                        peak = v;
                                    }
                                }
                            }
                        }
                        // Round peak to nice headroom
                        let max = peak * 1.15;
                        let ticks = vec![
                            (1.0, format_rate(Some(max))),
                            (0.66, format_rate(Some(max * 0.66))),
                            (0.33, format_rate(Some(max * 0.33))),
                            (0.0, "0 B/s".into()),
                        ];
                        (max, ticks)
                    }
                };

                let mut keyboard_readout: Vec<String> = Vec::new();
                ui.horizontal(|ui| {
                    // Y-axis labels
                    let (y_rect, _) =
                        ui.allocate_exact_size(Vec2::new(y_axis_w, canvas_h), egui::Sense::hover());
                    let p_y = ui.painter();
                    for (ratio, label) in &y_ticks {
                        let y = y_rect.max.y - ratio * (y_rect.height() - 16.0) - 8.0;
                        p_y.text(
                            Pos2::new(y_rect.max.x - 4.0, y),
                            egui::Align2::RIGHT_CENTER,
                            label,
                            egui::FontId::monospace(9.5),
                            p.secondary,
                        );
                    }

                    // Chart area
                    let (canvas_rect, response) =
                        ui.allocate_exact_size(Vec2::new(canvas_w, canvas_h), egui::Sense::click());
                    if !response.has_focus() {
                        state.keyboard_cursor_x = None;
                    }
                    if response.clicked() {
                        response.request_focus();
                        state.keyboard_cursor_x = Some(
                            response
                                .interact_pointer_pos()
                                .map(|pos| {
                                    ((pos.x - canvas_rect.min.x) / canvas_rect.width())
                                        .clamp(0.0, 1.0)
                                })
                                .unwrap_or(0.5),
                        );
                    }
                    let painter = ui.painter_at(canvas_rect);

                    painter.rect_filled(canvas_rect, CornerRadius::same(4), p.surface_raised);
                    painter.rect_stroke(
                        canvas_rect,
                        CornerRadius::same(4),
                        Stroke::new(1.0, p.border),
                        egui::StrokeKind::Inside,
                    );

                    // Horizontal grid lines
                    for (ratio, _) in &y_ticks {
                        let y = canvas_rect.max.y - ratio * (canvas_rect.height() - 16.0) - 8.0;
                        painter.line_segment(
                            [
                                Pos2::new(canvas_rect.min.x, y),
                                Pos2::new(canvas_rect.max.x, y),
                            ],
                            Stroke::new(0.5, p.border),
                        );
                    }

                    // Time axis labels across bottom
                    let time_labels = match state.range {
                        HistoryRange::Sec30 => ["-30s", "-20s", "-10s", "0s"],
                        HistoryRange::Sec60 => ["-60s", "-40s", "-20s", "0s"],
                        HistoryRange::Min5 => ["-5m", "-3m", "-1m", "0s"],
                        HistoryRange::Min15 => ["-15m", "-10m", "-5m", "0s"],
                    };
                    for (i, t_lbl) in time_labels.iter().enumerate() {
                        let x = canvas_rect.min.x
                            + (i as f32 / (time_labels.len() - 1) as f32)
                                * (canvas_rect.width() - 20.0)
                            + 10.0;
                        painter.text(
                            Pos2::new(x, canvas_rect.max.y - 4.0),
                            egui::Align2::CENTER_BOTTOM,
                            *t_lbl,
                            egui::FontId::monospace(9.0),
                            p.secondary,
                        );
                    }

                    // Click to focus; Left/Right/Home/End inspect without a mouse.
                    let mut active_crosshair_x = *shared_hover_norm_x;
                    let mut dismissed = false;
                    if response.has_focus() {
                        let (left, right, home, end, escape) = ui.input(|i| {
                            (
                                i.key_pressed(egui::Key::ArrowLeft),
                                i.key_pressed(egui::Key::ArrowRight),
                                i.key_pressed(egui::Key::Home),
                                i.key_pressed(egui::Key::End),
                                i.key_pressed(egui::Key::Escape),
                            )
                        });
                        if escape {
                            response.surrender_focus();
                            dismissed = true;
                            state.keyboard_cursor_x = None;
                            *shared_hover_norm_x = None;
                            active_crosshair_x = None;
                        } else {
                            let cursor = state.keyboard_cursor_x.unwrap_or(0.5);
                            let next = if home {
                                0.0
                            } else if end {
                                1.0
                            } else if left {
                                (cursor - 0.02).max(0.0)
                            } else if right {
                                (cursor + 0.02).min(1.0)
                            } else {
                                cursor
                            };
                            state.keyboard_cursor_x = Some(next);
                            active_crosshair_x = Some(next);
                            *shared_hover_norm_x = Some(next);
                        }
                    }
                    if let Some(hover_pos) = response.hover_pos()
                        && canvas_rect.contains(hover_pos)
                        && !dismissed
                    {
                        let norm = ((hover_pos.x - canvas_rect.min.x) / canvas_rect.width())
                            .clamp(0.0, 1.0);
                        *shared_hover_norm_x = Some(norm);
                        active_crosshair_x = Some(norm);
                        state.pointer_inside = true;
                    }

                    // Plot each active series
                    let mut points_plotted = 0;
                    for (idx, c) in series_configs.iter().enumerate() {
                        let enabled = if idx == 0 {
                            state.series1_enabled
                        } else {
                            state.series2_enabled
                        };
                        if !enabled {
                            continue;
                        }
                        let segs = c.series.downsampled_segments(
                            now,
                            state.range,
                            gap_thresh,
                            canvas_w as usize,
                        );
                        for seg in &segs {
                            points_plotted += seg.len();
                            if seg.is_empty() {
                                continue;
                            }
                            let pts: Vec<Pos2> = seg
                                .iter()
                                .map(|(norm_x, val)| {
                                    let x = canvas_rect.min.x + norm_x * canvas_rect.width();
                                    let norm_y = (val / max_val).clamp(0.0, 1.0);
                                    let y = canvas_rect.max.y
                                        - 16.0
                                        - norm_y * (canvas_rect.height() - 24.0);
                                    Pos2::new(x, y)
                                })
                                .collect();

                            if pts.len() >= 2 {
                                // Draw area fill for single series
                                if series_configs.len() == 1 {
                                    // A fluctuating trace is not a convex polygon.
                                    // Fill each adjacent pair as a convex trapezoid.
                                    for pair in pts.windows(2) {
                                        painter.add(egui::Shape::convex_polygon(
                                            vec![
                                                pair[0],
                                                pair[1],
                                                Pos2::new(pair[1].x, canvas_rect.max.y - 16.0),
                                                Pos2::new(pair[0].x, canvas_rect.max.y - 16.0),
                                            ],
                                            c.color.linear_multiply(0.15),
                                            Stroke::NONE,
                                        ));
                                    }
                                }
                                painter.add(egui::Shape::line(pts, Stroke::new(1.8, c.color)));
                            }
                        }
                    }

                    if points_plotted == 0 {
                        let has_any_recorded =
                            series_configs.iter().any(|c| !c.series.samples.is_empty());
                        let msg = if has_any_recorded {
                            "No samples in current time window"
                        } else {
                            "Awaiting telemetry samples..."
                        };
                        painter.text(
                            canvas_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            msg,
                            egui::FontId::proportional(13.0),
                            p.secondary,
                        );
                    } else {
                        // Check if the latest sample is stale (> 3s old)
                        let is_series_stale = series_configs.iter().all(|c| {
                            c.series
                                .samples
                                .back()
                                .map(|s| {
                                    now.saturating_duration_since(s.time)
                                        > Duration::from_millis(3000)
                                })
                                .unwrap_or(false)
                        });
                        if is_series_stale {
                            let stale_rect = egui::Rect::from_min_size(
                                Pos2::new(canvas_rect.max.x - 88.0, canvas_rect.min.y + 6.0),
                                Vec2::new(82.0, 18.0),
                            );
                            painter.rect_filled(
                                stale_rect,
                                CornerRadius::same(4),
                                p.surface_raised,
                            );
                            painter.rect_stroke(
                                stale_rect,
                                CornerRadius::same(4),
                                Stroke::new(1.0, p.warning),
                                egui::StrokeKind::Inside,
                            );
                            painter.text(
                                stale_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "Data Frozen",
                                egui::FontId::proportional(11.0),
                                p.warning,
                            );
                        }
                    }

                    // Draw Crosshair guideline and query tooltip if active
                    if let Some(norm_x) = active_crosshair_x {
                        let cross_x = canvas_rect.min.x + norm_x * canvas_rect.width();
                        painter.line_segment(
                            [
                                Pos2::new(cross_x, canvas_rect.min.y),
                                Pos2::new(cross_x, canvas_rect.max.y - 16.0),
                            ],
                            Stroke::new(1.0, p.focus),
                        );

                        // Find point on series
                        let mut tooltip_lines = Vec::new();
                        for (idx, c) in series_configs.iter().enumerate() {
                            let enabled = if idx == 0 {
                                state.series1_enabled
                            } else {
                                state.series2_enabled
                            };
                            if !enabled {
                                continue;
                            }
                            if let Some((sample, age)) =
                                c.series.query_sample(now, state.range, norm_x)
                                && let Some(val) = sample.value
                            {
                                let norm_y = (val / max_val).clamp(0.0, 1.0);
                                let cross_y = canvas_rect.max.y
                                    - 16.0
                                    - norm_y * (canvas_rect.height() - 24.0);
                                painter.circle_filled(Pos2::new(cross_x, cross_y), 3.0, c.color);
                                painter.circle_stroke(
                                    Pos2::new(cross_x, cross_y),
                                    4.5,
                                    Stroke::new(1.0, p.surface),
                                );

                                let formatted_val = match scale {
                                    ChartScale::Fixed0To100 => format!("{val:.1}%"),
                                    ChartScale::Throughput => format_rate(Some(val)),
                                };
                                tooltip_lines.push(format!(
                                    "{}: {} (age: {})",
                                    c.name,
                                    formatted_val,
                                    format_duration(age)
                                ));
                            }
                        }

                        if response.has_focus() {
                            keyboard_readout = tooltip_lines.clone();
                        }
                        if !tooltip_lines.is_empty() && response.hovered() {
                            response.on_hover_ui(|ui| {
                                for line in tooltip_lines {
                                    ui.label(egui::RichText::new(line).monospace().size(11.0));
                                }
                            });
                        }
                    }
                });
                if !keyboard_readout.is_empty() {
                    ui.label(
                        egui::RichText::new(keyboard_readout.join(" | "))
                            .monospace()
                            .size(11.0)
                            .color(p.secondary),
                    );
                }
            }

            // Visible Series Legend
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if let Some(c) = series_configs.get_mut(0) {
                    ui.checkbox(&mut state.series1_enabled, "");
                    let (s_rect, _) =
                        ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(s_rect.center(), 4.0, c.color);
                    ui.label(egui::RichText::new(c.name).size(11.5));
                }
                if let Some(c) = series_configs.get_mut(1) {
                    ui.add_space(14.0);
                    ui.checkbox(&mut state.series2_enabled, "");
                    let (s_rect, _) =
                        ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(s_rect.center(), 4.0, c.color);
                    ui.label(egui::RichText::new(c.name).size(11.5));
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_releases_chart_focus_and_stays_dismissed() {
        let ctx = egui::Context::default();
        let mut state = ChartPanelState::default();
        let mut shared_cursor = None;
        let now = Instant::now();
        let mut series = TimeSeries::new();
        series.push(now - Duration::from_secs(1), 1, Some(20.0));
        series.push(now, 2, Some(40.0));
        let p = crate::theme::get_palette(true);
        let mut render = |events| {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(700.0, 500.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    render_chart_panel(
                        ui,
                        "CPU",
                        &mut state,
                        ChartScale::Fixed0To100,
                        vec![ChartSeriesConfig {
                            name: "CPU",
                            color: p.cpu,
                            series: &series,
                            unit: "%",
                        }],
                        now,
                        &mut shared_cursor,
                        p,
                    );
                },
            );
            let shapes = output.shapes.clone();
            output.drop_without_applying_deltas();
            shapes
        };
        let output = render(vec![]);
        let canvas = output
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Rect(rect) = &shape.shape
                    && rect.fill == p.surface_raised
                    && rect.rect.width() > 200.0
                {
                    Some(rect.rect)
                } else {
                    None
                }
            })
            .expect("chart canvas must be painted");
        let pos = canvas.center();
        render(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        render(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert!(ctx.memory(|m| m.focused().is_some()));
        render(vec![
            egui::Event::PointerGone,
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            },
        ]);
        render(vec![]);
        assert!(ctx.memory(|m| m.focused().is_none()));
        assert!(state.keyboard_cursor_x.is_none());
    }
}
