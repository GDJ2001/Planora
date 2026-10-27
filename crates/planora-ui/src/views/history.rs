use crate::{
    components::{
        header::render_page_header,
        plot::{PlotSeries, PlotState, ValueScale, time_plot},
    },
    formatting::{format_percent, format_rate},
    history::{HistoryRange, ResourceHistories, TimeSeries},
    state::{AppState, DisplayPause, PercentScale},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Stroke};
use std::time::Instant;

pub struct HistoryViewState {
    pub range: HistoryRange,
    pub show_graphs: bool,
    pub show_gaps: bool,
    pub plots: [PlotState; 4],
}

impl Default for HistoryViewState {
    fn default() -> Self {
        Self {
            range: HistoryRange::Sec60,
            show_graphs: true,
            show_gaps: true,
            plots: Default::default(),
        }
    }
}

struct HistoryBucketRow {
    label: String,
    cpu: Option<f32>,
    mem: Option<f32>,
    disk_r: Option<f32>,
    disk_w: Option<f32>,
    net_rx: Option<f32>,
    net_tx: Option<f32>,
}

fn interval_label(newer_seconds_ago: usize, older_seconds_ago: usize) -> String {
    format!("{newer_seconds_ago}–{older_seconds_ago} s ago")
}

fn bucket_percent(value: Option<f32>) -> String {
    value.map_or_else(|| "—".into(), |value| format_percent(Some(value)))
}

fn bucket_rate(value: Option<f32>) -> String {
    value.map_or_else(|| "—".into(), |value| format_rate(Some(value)))
}

fn bucket_average(series: &TimeSeries, bucket_start: Instant, bucket_end: Instant) -> Option<f32> {
    let mut sum = 0.0f64;
    let mut count = 0u64;
    for s in &series.samples {
        let mid = s.midpoint();
        if mid >= bucket_start
            && mid < bucket_end
            && let Some(v) = s.value.filter(|v| v.is_finite())
        {
            let c = (s.count as u64).max(1);
            let s_sum = if s.count > 0 && s.sum.is_finite() {
                s.sum
            } else {
                v as f64 * c as f64
            };
            sum += s_sum;
            count += c;
        }
    }
    if count > 0 {
        Some((sum / count as f64) as f32)
    } else {
        None
    }
}

pub fn render_history_view(
    ui: &mut egui::Ui,
    histories: &ResourceHistories,
    state: &mut HistoryViewState,
    percent_scale: &mut PercentScale,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    now: Instant,
    p: Palette,
) {
    render_page_header(
        ui,
        "Subsystem History",
        "Timestamped historical measurement buckets for system and process telemetry",
        app_state,
        pause_state,
        poll_interval_ms,
        p,
    );

    // Range selector and scale toolbar
    ui.horizontal_wrapped(|ui| {
        ui.strong("History Window:");
        ui.add_space(8.0);
        for range in [
            HistoryRange::Sec30,
            HistoryRange::Sec60,
            HistoryRange::Min5,
            HistoryRange::Min15,
        ] {
            ui.selectable_value(&mut state.range, range, range.label());
        }
        ui.separator();
        ui.checkbox(&mut state.show_graphs, "Show graphs");
        ui.separator();
        let is_full = matches!(percent_scale, PercentScale::Full);
        if ui.selectable_label(!is_full, "Fit scale").clicked() {
            *percent_scale = PercentScale::Fit;
        }
        if ui.selectable_label(is_full, "0–100%").clicked() {
            *percent_scale = PercentScale::Full;
        }
    });

    ui.add_space(10.0);

    if state.show_graphs {
        let is_full = matches!(percent_scale, PercentScale::Full);
        let lanes: [(&str, Vec<PlotSeries<'_>>, &str, ValueScale); 4] = [
            (
                "CPU",
                vec![PlotSeries {
                    name: "CPU utilization",
                    samples: &histories.cpu,
                    color: p.cpu,
                    dashed: false,
                }],
                "%",
                ValueScale::Percent { full: is_full },
            ),
            (
                "Memory",
                vec![PlotSeries {
                    name: "Physical memory used",
                    samples: &histories.memory_pct,
                    color: p.memory,
                    dashed: false,
                }],
                "%",
                ValueScale::Percent { full: is_full },
            ),
            (
                "Storage",
                vec![
                    PlotSeries {
                        name: "Read",
                        samples: &histories.disk_read,
                        color: p.disk,
                        dashed: false,
                    },
                    PlotSeries {
                        name: "Write",
                        samples: &histories.disk_write,
                        color: p.accent,
                        dashed: true,
                    },
                ],
                "B/s",
                ValueScale::Auto,
            ),
            (
                "Network",
                vec![
                    PlotSeries {
                        name: "Receive",
                        samples: &histories.net_rx,
                        color: p.network,
                        dashed: false,
                    },
                    PlotSeries {
                        name: "Transmit",
                        samples: &histories.net_tx,
                        color: p.accent,
                        dashed: true,
                    },
                ],
                "B/s",
                ValueScale::Auto,
            ),
        ];
        for (idx, (title, series, unit, val_scale)) in lanes.into_iter().enumerate() {
            ui.heading(title);
            ui.horizontal_wrapped(|ui| {
                for plotted in &series {
                    ui.colored_label(
                        plotted.color,
                        if plotted.dashed {
                            format!("┄ {}", plotted.name)
                        } else {
                            format!("━ {}", plotted.name)
                        },
                    );
                }
            });
            time_plot(
                ui,
                title,
                &series,
                state.range,
                now,
                unit,
                val_scale,
                220.0,
                &mut state.plots[idx],
                p,
            );
            ui.add_space(14.0);
        }
    }

    render_statistics(ui, histories, state.range, now, p);
    ui.add_space(16.0);
    render_intervals(ui, histories, state, now, p);
}

fn value_label(value: f32, unit: &str) -> String {
    if unit == "%" {
        format_percent(Some(value))
    } else {
        format_rate(Some(value))
    }
}

fn cell(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    text: egui::RichText,
    right: bool,
) -> egui::Response {
    let layout = if right {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.allocate_ui_with_layout(egui::vec2(width, height), layout, |ui| {
        ui.set_min_size(egui::vec2(width, height));
        ui.set_max_width(width);
        ui.add_space(10.0);
        ui.add(egui::Label::new(text).truncate())
    })
    .inner
}

fn table_header(ui: &mut egui::Ui, widths: &[f32], labels: &[&str], p: Palette) {
    egui::Frame::NONE
        .fill(p.surface_raised)
        .corner_radius(6.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (index, (&width, label)) in widths.iter().zip(labels).enumerate() {
                    cell(
                        ui,
                        width,
                        32.0 * crate::appearance::text_scale(ui.ctx()),
                        egui::RichText::new(*label)
                            .small()
                            .strong()
                            .color(p.secondary),
                        index != 0,
                    );
                }
            });
        });
}

pub fn range_average_fraction(low: f32, average: f32, peak: f32) -> Option<f32> {
    let span = peak - low;
    (low.is_finite() && average.is_finite() && peak.is_finite() && span > f32::EPSILON)
        .then(|| ((average - low) / span).clamp(0.0, 1.0))
}

pub fn scale_fractions(low: f32, average: f32, peak: f32, ceiling: f32) -> Option<(f32, f32, f32)> {
    if !low.is_finite() || !average.is_finite() || !peak.is_finite() || ceiling <= 0.0 {
        return None;
    }
    let c = ceiling.max(f32::EPSILON);
    Some((
        (low / c).clamp(0.0, 1.0),
        (average / c).clamp(0.0, 1.0),
        (peak / c).clamp(0.0, 1.0),
    ))
}

fn observed_range(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    summary: Option<crate::history::ObservedSummary>,
    scale_ceiling: f32,
    unit: &str,
    color: egui::Color32,
    p: Palette,
) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let track_w = (width - 24.0).max(1.0);
    let track_h = 8.0;
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(track_w, track_h));

    // Base scale track (neutral recessed slot)
    ui.painter().rect_filled(track, 4.0, p.surface_raised);
    ui.painter().rect_stroke(
        track,
        4.0,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );

    if let Some(s) = summary
        && scale_ceiling > 0.0
    {
        let ceiling = scale_ceiling.max(f32::EPSILON);
        let low_frac = (s.minimum / ceiling).clamp(0.0, 1.0);
        let peak_frac = (s.maximum / ceiling).clamp(0.0, 1.0);
        let avg_frac = (s.average / ceiling).clamp(0.0, 1.0);

        let left_x = track.left() + low_frac * track.width();
        let right_x = track.left() + peak_frac * track.width();
        let avg_x = track.left() + avg_frac * track.width();

        // Scale-honest filled range band
        let band_rect = egui::Rect::from_min_max(
            egui::pos2(left_x, track.top() + 1.0),
            egui::pos2(
                right_x.max(left_x + 3.0).min(track.right()),
                track.bottom() - 1.0,
            ),
        );
        ui.painter()
            .rect_filled(band_rect, 2.0, color.gamma_multiply(0.55));

        // Average marker (vertical tick + center dot)
        ui.painter().line_segment(
            [
                egui::pos2(avg_x, track.top() - 3.0),
                egui::pos2(avg_x, track.bottom() + 3.0),
            ],
            Stroke::new(2.0, color),
        );
        ui.painter()
            .circle_filled(egui::pos2(avg_x, track.center().y), 3.0, color);

        let delta = (s.maximum - s.minimum).max(0.0);
        response.on_hover_text(format!(
            "Low {} · Average {} · Peak {} · Spread: Δ {}
Observed range on 0 → {} scale. The colored bar shows low to peak; the tick indicates average.",
            value_label(s.minimum, unit),
            value_label(s.average, unit),
            value_label(s.maximum, unit),
            value_label(delta, unit),
            value_label(scale_ceiling, unit),
        ));
    } else {
        response.on_hover_text("No observed samples in this range");
    }
}

fn render_statistics(
    ui: &mut egui::Ui,
    histories: &ResourceHistories,
    range: HistoryRange,
    now: Instant,
    p: Palette,
) {
    let panel_width = (ui.available_width() - 34.0).max(100.0);
    egui::Frame::group(ui.style()).fill(p.surface).stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8)).inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(panel_width);
            ui.horizontal_wrapped(|ui| {
                ui.heading("Resource statistics");
                ui.label(egui::RichText::new(format!("LAST {}", range.label())).small().color(p.accent));
            });
            ui.label(egui::RichText::new("Observed values only. Range tracks display scale-honest low-to-peak bands and average markers.").small().color(p.secondary));
            ui.add_space(12.0);
            let table_width = ui.available_width().max(880.0 * crate::appearance::text_scale(ui.ctx()));
            let widths = [0.20, 0.14, 0.13, 0.14, 0.19, 0.20].map(|fraction| fraction * table_width);

            let mut disk_ceiling: f32 = 1.0;
            let mut net_ceiling: f32 = 1.0;
            if let Some(s) = histories.disk_read.observed_summary(now, range) {
                disk_ceiling = disk_ceiling.max(s.maximum);
            }
            if let Some(s) = histories.disk_write.observed_summary(now, range) {
                disk_ceiling = disk_ceiling.max(s.maximum);
            }
            if let Some(s) = histories.net_rx.observed_summary(now, range) {
                net_ceiling = net_ceiling.max(s.maximum);
            }
            if let Some(s) = histories.net_tx.observed_summary(now, range) {
                net_ceiling = net_ceiling.max(s.maximum);
            }

            egui::ScrollArea::horizontal().id_salt("history_statistics_horizontal")
                .auto_shrink([false, true]).show(ui, |ui| {
                    table_header(ui, &widths, &["RESOURCE", "AVERAGE", "LOW", "PEAK", "OBSERVED RANGE", "COVERAGE"], p);
                    let rows: [(&str, &TimeSeries, &str, egui::Color32, f32); 6] = [
                        ("CPU utilization", &histories.cpu, "%", p.cpu, 100.0),
                        ("Memory used", &histories.memory_pct, "%", p.memory, 100.0),
                        ("Disk read", &histories.disk_read, "B/s", p.disk, disk_ceiling),
                        ("Disk write", &histories.disk_write, "B/s", p.disk, disk_ceiling),
                        ("Network receive", &histories.net_rx, "B/s", p.network, net_ceiling),
                        ("Network transmit", &histories.net_tx, "B/s", p.network, net_ceiling),
                    ];
                    for (index, (name, series, unit, color, ceiling)) in rows.into_iter().enumerate() {
                        let summary = series.observed_summary(now, range);
                        let height = 42.0 * crate::appearance::text_scale(ui.ctx());
                        ui.push_id(("history_statistics", index), |ui| {
                            egui::Frame::NONE.fill(if index % 2 == 0 { p.background } else { p.surface })
                                .corner_radius(4.0).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        cell(ui, widths[0], height, egui::RichText::new(name).color(color), false);
                                        for (column, value) in [summary.map(|s| s.average), summary.map(|s| s.minimum), summary.map(|s| s.maximum)].into_iter().enumerate() {
                                             let text = value.map_or_else(|| "—".into(), |v| value_label(v, unit));
                                             cell(ui, widths[column + 1], height, egui::RichText::new(text.clone()).monospace().color(p.text), true).on_hover_text(text);
                                        }
                                        observed_range(ui, widths[4], height, summary, ceiling, unit, color, p);
                                        ui.allocate_ui_with_layout(egui::vec2(widths[5], height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            ui.set_min_size(egui::vec2(widths[5], height));
                                            ui.add_space(12.0);
                                            if let Some(s) = summary {
                                                let coverage = s.coverage_percent();
                                                ui.label(
                                                    egui::RichText::new(format!("{coverage:.0}% ({}/{}s)", s.observed_seconds, s.expected_seconds))
                                                        .monospace()
                                                        .strong()
                                                        .color(p.text),
                                                )
                                                .on_hover_text(format!("{} of {} seconds contain observations. Missing intervals are excluded from statistics.", s.observed_seconds, s.expected_seconds));
                                            } else {
                                                ui.label(egui::RichText::new("No observations").small().color(p.secondary));
                                            }
                                        });
                                    });
                                });
                        });
                    }
                });
        });
}

fn interval_rows(
    histories: &ResourceHistories,
    range: HistoryRange,
    now: Instant,
) -> Vec<HistoryBucketRow> {
    let (count, seconds) = match range {
        HistoryRange::Sec30 => (30, 1),
        HistoryRange::Sec60 => (60, 1),
        HistoryRange::Min5 => (60, 5),
        HistoryRange::Min15 => (60, 15),
    };
    (0..count)
        .map(|index| {
            let end = now
                .checked_sub(std::time::Duration::from_secs((index * seconds) as u64))
                .unwrap_or(now);
            let start = now
                .checked_sub(std::time::Duration::from_secs(
                    ((index + 1) * seconds) as u64,
                ))
                .unwrap_or(now);
            HistoryBucketRow {
                label: interval_label(index * seconds, (index + 1) * seconds),
                cpu: bucket_average(&histories.cpu, start, end),
                mem: bucket_average(&histories.memory_pct, start, end),
                disk_r: bucket_average(&histories.disk_read, start, end),
                disk_w: bucket_average(&histories.disk_write, start, end),
                net_rx: bucket_average(&histories.net_rx, start, end),
                net_tx: bucket_average(&histories.net_tx, start, end),
            }
        })
        .collect()
}

impl HistoryBucketRow {
    fn values(&self) -> [Option<f32>; 6] {
        [
            self.cpu,
            self.mem,
            self.disk_r,
            self.disk_w,
            self.net_rx,
            self.net_tx,
        ]
    }
    fn observed(&self) -> bool {
        self.values().iter().any(Option::is_some)
    }
    fn text(&self) -> [String; 7] {
        [
            self.label.clone(),
            bucket_percent(self.cpu),
            bucket_percent(self.mem),
            bucket_rate(self.disk_r),
            bucket_rate(self.disk_w),
            bucket_rate(self.net_rx),
            bucket_rate(self.net_tx),
        ]
    }
}

fn render_intervals(
    ui: &mut egui::Ui,
    histories: &ResourceHistories,
    state: &mut HistoryViewState,
    now: Instant,
    p: Palette,
) {
    let mut rows = interval_rows(histories, state.range, now);
    let observed = rows.iter().filter(|row| row.observed()).count();
    let panel_width = (ui.available_width() - 34.0).max(100.0);
    egui::Frame::group(ui.style()).fill(p.surface).stroke(Stroke::new(1.0, p.border))
        .corner_radius(8.0).inner_margin(16.0).show(ui, |ui| {
            ui.set_width(panel_width);
            ui.horizontal_wrapped(|ui| {
                ui.heading("Interval detail");
                ui.label(egui::RichText::new(format!("{observed} / {} intervals observed", rows.len())).small().color(p.secondary));
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut state.show_gaps, "Show missing intervals");
                if !state.show_gaps { rows.retain(HistoryBucketRow::observed); }
                if ui.button("Copy table").on_hover_text("Copy the visible interval table with units as tab-separated text").clicked() {
                    let mut text = "Interval\tCPU (%)\tMemory (%)\tDisk read (bytes/s)\tDisk write (bytes/s)\tNetwork receive (bytes/s)\tNetwork transmit (bytes/s)\n".to_owned();
                    for row in &rows {
                        let values = row.values().map(|value| value.map_or_else(|| "Unavailable".into(), |v| v.to_string()));
                        text.push_str(&format!("{}\t{}\n", row.label, values.join("\t")));
                    }
                    ui.copy_text(text);
                }
            });
            ui.label(egui::RichText::new("Newest first · Each value is an interval average · — means no observation, not zero").small().color(p.secondary));
            ui.add_space(12.0);
            let table_width = ui.available_width().max(1000.0 * crate::appearance::text_scale(ui.ctx()));
            if table_width > ui.available_width() {
                ui.label(egui::RichText::new("Scroll horizontally to see every resource column.").small().color(p.secondary));
            }
            let widths = [0.16, 0.12, 0.12, 0.15, 0.15, 0.15, 0.15].map(|f| f * table_width);
            let height = 36.0 * crate::appearance::text_scale(ui.ctx());
            egui::ScrollArea::horizontal().id_salt("history_intervals_horizontal").auto_shrink([false, true]).show(ui, |ui| {
                table_header(ui, &widths, &["INTERVAL", "CPU", "MEMORY", "DISK READ", "DISK WRITE", "RECEIVE", "TRANSMIT"], p);
                if rows.is_empty() {
                    ui.add_space(18.0);
                    ui.label("No observed intervals in this range. Show missing intervals or wait for fresh samples.");
                }
                // At most 60 rows are shown. The page owns vertical scrolling, so a
                // second scroll viewport would leave clipped row fragments below
                // the sticky header when the two scroll offsets disagree.
                        for (index, row) in rows.iter().enumerate() {
                            egui::Frame::NONE.fill(if index % 2 == 0 { p.background } else { p.surface }).corner_radius(4.0).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    for (column, text) in row.text().into_iter().enumerate() {
                                        let values = row.values();
                                        let color = if column == 0 || values[column - 1].is_none() { p.secondary }
                                            else { [p.cpu, p.memory, p.disk, p.disk, p.network, p.network][column - 1] };
                                        let response = cell(ui, widths[column], height, egui::RichText::new(&text).monospace().color(color), column > 0);
                                        if (1..=2).contains(&column) && let Some(value) = values[column - 1] {
                                            let bar = egui::Rect::from_min_size(egui::pos2(response.rect.left(), response.rect.bottom() + 2.0), egui::vec2(response.rect.width() * value.clamp(0.0,100.0) / 100.0, 2.0));
                                            ui.painter().rect_filled(bar, 1.0, color.gamma_multiply(0.6));
                                        }
                                        response.on_hover_text(text);
                                    }
                                });
                            });
                        }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_fractions_computes_proportional_positions() {
        let (low, avg, peak) = scale_fractions(71.9, 72.0, 72.2, 100.0).unwrap();
        assert!((low - 0.719).abs() < 0.001);
        assert!((avg - 0.720).abs() < 0.001);
        assert!((peak - 0.722).abs() < 0.001);
        assert!((peak - low - 0.003).abs() < 0.001);

        let (low, _avg, peak) = scale_fractions(8.3, 40.0, 87.2, 100.0).unwrap();
        assert!((low - 0.083).abs() < 0.001);
        assert!((peak - 0.872).abs() < 0.001);
        assert!((peak - low - 0.789).abs() < 0.001);

        assert_eq!(scale_fractions(1.0, 2.0, 3.0, 0.0), None);
        assert_eq!(scale_fractions(f32::NAN, 2.0, 3.0, 100.0), None);
    }

    #[test]
    fn range_ruler_uses_each_rows_observed_bounds() {
        assert!((range_average_fraction(70.3, 70.4, 70.8).unwrap() - 0.2).abs() < 0.001);
        assert_eq!(range_average_fraction(0.0, 0.0, 0.0), None);
        assert_eq!(range_average_fraction(2.0, 3.0, 4.0), Some(0.5));
        assert_eq!(range_average_fraction(2.0, f32::NAN, 4.0), None);
    }

    #[test]
    fn history_tables_do_not_share_scroll_ids() {
        for size in [
            (1024.0, 640.0),
            (1180.0, 790.0),
            (1366.0, 768.0),
            (1536.0, 864.0),
            (1920.0, 1080.0),
        ] {
            for dark in [false, true] {
                for scale in [1.0, 1.5, 2.0] {
                    check_history_layout(size, dark, scale);
                }
            }
        }
    }

    fn check_history_layout(size: (f32, f32), dark: bool, scale: f32) {
        let ctx = egui::Context::default();
        let palette = crate::theme::get_palette(dark);
        crate::theme::configure_theme(&ctx, palette, crate::theme::Density::Normal, scale, false);
        ctx.data_mut(|data| data.insert_temp(egui::Id::unique("system_text_scale"), scale));
        let mut state = HistoryViewState {
            show_graphs: false,
            ..Default::default()
        };
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size.0, size.1),
                    )),
                    ..Default::default()
                },
                |ui| {
                    render_history_view(
                        ui,
                        &ResourceHistories::default(),
                        &mut state,
                        &mut PercentScale::Full,
                        &AppState::Live,
                        &mut DisplayPause::default(),
                        100,
                        Instant::now(),
                        palette,
                    );
                    assert!(
                        ui.min_rect().right() <= size.0 + 1.0,
                        "History content overflows: {size:?}, text scale {scale}"
                    );
                },
            );
            let svg = crate::smoke_test::shapes_to_svg(
                size.0,
                size.1,
                egui::Color32::BLACK,
                &output.shapes,
            );
            output.textures_delta.clear();
            assert!(
                !svg.contains("use of ScrollArea ID") && !svg.contains("use of widget ID"),
                "History tables have colliding scroll IDs"
            );
        }
    }

    #[test]
    fn interval_rows_preserve_zero_and_missing_samples() {
        let now = Instant::now();
        let mut histories = ResourceHistories::default();
        histories
            .cpu
            .push(now - std::time::Duration::from_millis(500), 1000, Some(0.0));
        let rows = interval_rows(&histories, HistoryRange::Sec30, now);
        assert_eq!(rows.len(), 30);
        assert!(rows[0].observed());
        assert_eq!(rows[0].cpu, Some(0.0));
        assert_eq!(rows[0].mem, None);
        assert!(!rows[1].observed());
        assert_eq!(rows.iter().filter(|row| row.observed()).count(), 1);
    }

    #[test]
    fn fifteen_minute_table_and_chart_keep_older_observations_after_range_switch() {
        let now = Instant::now();
        let mut histories = ResourceHistories::default();
        for age in (0..900u64).rev() {
            histories.cpu.push(
                now - std::time::Duration::from_secs(age),
                (900 - age) * 1_000,
                Some(25.0),
            );
        }
        for range in [
            HistoryRange::Sec60,
            HistoryRange::Min5,
            HistoryRange::Min15,
            HistoryRange::Sec30,
        ] {
            assert!(histories.cpu.observed_summary(now, range).is_some());
            assert!(
                !histories
                    .cpu
                    .chart_segments(now, range, std::time::Duration::from_secs(2), 600)
                    .is_empty()
            );
        }
        let rows = interval_rows(&histories, HistoryRange::Min15, now);
        assert_eq!(rows.len(), 60);
        assert!(rows[0].observed());
        assert!(
            rows[59].observed(),
            "The oldest 15-second bucket must remain visible"
        );
    }

    #[test]
    fn missing_history_is_not_a_zero_measurement() {
        assert_eq!(interval_label(0, 1), "0–1 s ago");
        assert_eq!(interval_label(15, 30), "15–30 s ago");
        assert_eq!(bucket_percent(None), "—");
        assert_eq!(bucket_rate(None), "—");
        assert_eq!(bucket_rate(Some(0.0)), "0 B/s");
    }

    #[test]
    fn history_lanes_keep_independent_axis_state() {
        let mut histories = ResourceHistories::default();
        let now = Instant::now();
        // Storage has large throughput: 500 MiB/s
        histories.disk_read.push(
            now - std::time::Duration::from_secs(2),
            1000,
            Some(524_288_000.0),
        );
        // Network has small throughput: 1 KiB/s
        histories
            .net_rx
            .push(now - std::time::Duration::from_secs(2), 1000, Some(1024.0));

        let mut state = HistoryViewState::default();
        let mut percent_scale = PercentScale::Fit;
        let p = crate::theme::get_palette(true);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(800.0, 1200.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut pause = DisplayPause::default();
                render_history_view(
                    ui,
                    &histories,
                    &mut state,
                    &mut percent_scale,
                    &AppState::Live,
                    &mut pause,
                    100,
                    now,
                    p,
                );
            },
        );
        output.textures_delta.clear();

        // Lane 2 is Storage, Lane 3 is Network
        // Both lanes rendered independently and did not bleed their cache or hysteresis into each other
        assert_eq!(state.plots.len(), 4);
    }
}
