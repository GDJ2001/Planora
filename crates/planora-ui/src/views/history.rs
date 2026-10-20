use crate::{
    components::header::render_page_header,
    formatting::{format_percent, format_rate},
    history::{HistoryRange, ResourceHistories, TimeSeries},
    state::{AppState, DisplayPause},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Stroke};
use std::time::Instant;

pub struct HistoryViewState {
    pub range: HistoryRange,
}

impl Default for HistoryViewState {
    fn default() -> Self {
        Self {
            range: HistoryRange::Sec60,
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

    // Range selector toolbar
    ui.horizontal(|ui| {
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
    });

    ui.add_space(10.0);

    // 1. Summary Statistics Table
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.strong(format!("Aggregate Statistics ({})", state.range.label()));
            ui.add_space(6.0);

            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("history_summary_grid")
                    .striped(true)
                    .min_col_width(80.0)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("Resource");
                        ui.strong("Average");
                        ui.strong("Minimum");
                        ui.strong("Maximum");
                        ui.strong("Unit");
                        ui.strong("Coverage");
                        ui.end_row();

                        let rows = [
                            ("CPU Utilization", &histories.cpu, "%", p.cpu),
                            ("Memory Used", &histories.memory_pct, "%", p.memory),
                            ("Disk Read Rate", &histories.disk_read, "B/s", p.disk),
                            ("Disk Write Rate", &histories.disk_write, "B/s", p.disk),
                            ("Network Receive", &histories.net_rx, "B/s", p.network),
                            ("Network Transmit", &histories.net_tx, "B/s", p.network),
                        ];

                        for (label, series, unit, color) in rows {
                            ui.colored_label(color, label);
                            if let Some(s) = series.observed_summary(now, state.range) {
                                if unit == "%" {
                                    ui.monospace(format!("{:.1}", s.average));
                                    ui.monospace(format!("{:.1}", s.minimum));
                                    ui.monospace(format!("{:.1}", s.maximum));
                                } else {
                                    ui.monospace(format_rate(Some(s.average)));
                                    ui.monospace(format_rate(Some(s.minimum)));
                                    ui.monospace(format_rate(Some(s.maximum)));
                                }
                                ui.label(unit);
                                ui.label(format!(
                                    "{}/{} s ({:.0}%)",
                                    s.observed_seconds,
                                    s.expected_seconds,
                                    s.coverage_percent()
                                ));
                            } else {
                                ui.label("—");
                                ui.label("—");
                                ui.label("—");
                                ui.label(unit);
                                ui.label("Collecting data");
                            }
                            ui.end_row();
                        }
                    });
            });
        });

    ui.add_space(14.0);

    // 2. Timestamped Buckets Table
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.strong("Timestamped Interval Buckets");
            ui.label(
                egui::RichText::new(
                    "Chronological interval samples. Most recent intervals appear first.",
                )
                .size(11.5)
                .color(p.secondary),
            );
            ui.add_space(8.0);

            // Compute bucket step size
            let (num_buckets, step_secs) = match state.range {
                HistoryRange::Sec30 => (30, 1),
                HistoryRange::Sec60 => (60, 1),
                HistoryRange::Min5 => (60, 5),
                HistoryRange::Min15 => (60, 15),
            };

            let mut bucket_rows = Vec::with_capacity(num_buckets);
            for i in 0..num_buckets {
                let end_offset = i * step_secs;
                let start_offset = (i + 1) * step_secs;
                let b_end = now
                    .checked_sub(std::time::Duration::from_secs(end_offset as u64))
                    .unwrap_or(now);
                let b_start = now
                    .checked_sub(std::time::Duration::from_secs(start_offset as u64))
                    .unwrap_or(now);

                let label = if end_offset == 0 {
                    format!("0s - -{}s", step_secs)
                } else {
                    format!("-{}s - -{}s", end_offset, start_offset)
                };

                bucket_rows.push(HistoryBucketRow {
                    label,
                    cpu: bucket_average(&histories.cpu, b_start, b_end),
                    mem: bucket_average(&histories.memory_pct, b_start, b_end),
                    disk_r: bucket_average(&histories.disk_read, b_start, b_end),
                    disk_w: bucket_average(&histories.disk_write, b_start, b_end),
                    net_rx: bucket_average(&histories.net_rx, b_start, b_end),
                    net_tx: bucket_average(&histories.net_tx, b_start, b_end),
                });
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("history_buckets_grid")
                    .striped(true)
                    .min_col_width(70.0)
                    .spacing([20.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("Interval");
                        ui.strong("CPU %");
                        ui.strong("Memory %");
                        ui.strong("Disk Read");
                        ui.strong("Disk Write");
                        ui.strong("Net Rx");
                        ui.strong("Net Tx");
                        ui.end_row();

                        for row in bucket_rows {
                            ui.monospace(&row.label);
                            ui.monospace(format_percent(row.cpu));
                            ui.monospace(format_percent(row.mem));
                            ui.monospace(format_rate(row.disk_r));
                            ui.monospace(format_rate(row.disk_w));
                            ui.monospace(format_rate(row.net_rx));
                            ui.monospace(format_rate(row.net_tx));
                            ui.end_row();
                        }
                    });
            });
        });
}
