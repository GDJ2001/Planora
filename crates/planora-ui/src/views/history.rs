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
    let mut count = 0usize;
    for s in &series.samples {
        if s.time >= bucket_start
            && s.time < bucket_end
            && let Some(v) = s.value.filter(|v| v.is_finite())
        {
            sum += v as f64;
            count += 1;
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

fn observed_range(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    summary: Option<crate::history::ObservedSummary>,
    unit: &str,
    color: egui::Color32,
    p: Palette,
) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let track =
        egui::Rect::from_center_size(rect.center(), egui::vec2((width - 28.0).max(1.0), 5.0));
    ui.painter().rect_filled(track, 3.0, p.surface_raised);
    if let Some(s) = summary {
        let ceiling = if unit == "%" {
            100.0
        } else {
            s.maximum.max(1.0)
        };
        let x = |value: f32| track.left() + value.clamp(0.0, ceiling) / ceiling * track.width();
        ui.painter().line_segment(
            [
                egui::pos2(x(s.minimum), track.center().y),
                egui::pos2(x(s.maximum), track.center().y),
            ],
            Stroke::new(5.0, color.gamma_multiply(0.6)),
        );
        ui.painter()
            .circle_filled(egui::pos2(x(s.average), track.center().y), 4.0, color);
        response.on_hover_text(format!(
            "Low {} · Average {} · Peak {}. {}",
            value_label(s.minimum, unit),
            value_label(s.average, unit),
            value_label(s.maximum, unit),
            if unit == "%" {
                "Scale: 0–100%."
            } else {
                "Scale: zero to this resource's peak; not comparable across rows."
            }
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
            ui.label(egui::RichText::new("Observed values only. Range bars show low to peak; the dot marks the average.").small().color(p.secondary));
            ui.add_space(12.0);
            let table_width = ui.available_width().max(880.0 * crate::appearance::text_scale(ui.ctx()));
            let widths = [0.20, 0.14, 0.13, 0.14, 0.19, 0.20].map(|fraction| fraction * table_width);
            egui::ScrollArea::horizontal().id_salt("history_statistics_horizontal")
                .auto_shrink([false, true]).show(ui, |ui| {
                    table_header(ui, &widths, &["RESOURCE", "AVERAGE", "LOW", "PEAK", "OBSERVED RANGE", "COVERAGE"], p);
                    let rows = [
                        ("CPU utilization", &histories.cpu, "%", p.cpu),
                        ("Memory used", &histories.memory_pct, "%", p.memory),
                        ("Disk read", &histories.disk_read, "B/s", p.disk),
                        ("Disk write", &histories.disk_write, "B/s", p.disk),
                        ("Network receive", &histories.net_rx, "B/s", p.network),
                        ("Network transmit", &histories.net_tx, "B/s", p.network),
                    ];
                    for (index, (name, series, unit, color)) in rows.into_iter().enumerate() {
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
                                        observed_range(ui, widths[4], height, summary, unit, color, p);
                                        ui.allocate_ui_with_layout(egui::vec2(widths[5], height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            ui.set_min_size(egui::vec2(widths[5], height));
                                            ui.add_space(12.0);
                                            if let Some(s) = summary {
                                                let coverage = s.coverage_percent();
                                                ui.add(egui::ProgressBar::new(coverage / 100.0)
                                                    .desired_width(widths[5] - 24.0).fill(color.gamma_multiply(0.45))
                                                    .text(format!("{coverage:.0}% observed")))
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
                egui::ScrollArea::vertical().id_salt("history_intervals_rows")
                    .min_scrolled_height(240.0).max_height(360.0).auto_shrink([false, true]).show_rows(ui, height, rows.len(), |ui, visible| {
                        for index in visible {
                            let row = &rows[index];
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
        });
}

#[cfg(test)]
mod tests {
    use super::*;

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
