use crate::{
    components::{
        header::render_page_header,
        plot::{capacity_bar, sparkline},
        table::{ProcessTableState, SortColumn, render_process_table, sort_processes},
    },
    formatting::format_rate,
    history::{HistoryRange, ResourceHistories, TimeSeries},
    state::{AppState, DisplayPause, Page},
    theme::Palette,
};
use eframe::egui::{self, CornerRadius, Stroke, Vec2};
use planora_core::{Metric, ProcessIdentity, TelemetryFrame};
use std::time::Instant;

pub struct OverviewViewState {
    pub range: HistoryRange,
    pub top_by_cpu: bool,
    pub top_table: ProcessTableState,
}

impl Default for OverviewViewState {
    fn default() -> Self {
        Self {
            range: HistoryRange::Sec60,
            top_by_cpu: true,
            top_table: ProcessTableState::default(),
        }
    }
}

struct SummaryRow<'a> {
    label: &'static str,
    current: String,
    series: &'a TimeSeries,
    unit: &'static str,
    color: egui::Color32,
    reason: Option<String>,
}

fn metric_percent(metric: &Metric<f32>) -> String {
    metric
        .value
        .map_or_else(|| "Unavailable".into(), |value| format!("{value:.1}"))
}

fn metric_rate(metric: &Metric<u64>) -> String {
    metric
        .value
        .map_or_else(|| "Unavailable".into(), |value| format!("{value}"))
}

fn summary_table(ui: &mut egui::Ui, rows: &[SummaryRow<'_>], range: HistoryRange, now: Instant) {
    egui::ScrollArea::horizontal().show(ui, |ui| {
        egui::Grid::new("overview_resource_summary")
            .striped(true)
            .min_col_width(76.0)
            .spacing([22.0, 9.0])
            .show(ui, |ui| {
                for title in ["Resource", "Current", "Average", "Minimum", "Maximum", "Unit", "Coverage"] {
                    ui.strong(title);
                }
                ui.end_row();
                for row in rows {
                    ui.colored_label(row.color, row.label);
                    ui.label(&row.current).on_hover_text(row.reason.as_deref().unwrap_or("Current reading"));
                    if let Some(summary) = row.series.observed_summary(now, range) {
                        for value in [summary.average, summary.minimum, summary.maximum] {
                            ui.monospace(format!("{value:.1}"));
                        }
                        ui.label(row.unit);
                        ui.label(format!("{}/{} s ({:.0}%)", summary.observed_seconds, summary.expected_seconds, summary.coverage_percent()))
                            .on_hover_text("One observed value per second. Missing or unavailable seconds are excluded; the average uses observed seconds only.");
                    } else {
                        for _ in 0..3 { ui.label("—"); }
                        ui.label(row.unit);
                        ui.label("Collecting history");
                    }
                    ui.end_row();
                }
            });
    });
}

type CardItem<'a> = (&'a str, String, &'a TimeSeries, egui::Color32, Option<(Option<u64>, Option<u64>)>);

fn resource_cards(ui: &mut egui::Ui, frame: &TelemetryFrame, histories: &ResourceHistories, now: Instant, p: Palette) {
    let memory_pct = frame.memory_used_bytes.value.zip(frame.memory_total_bytes.value)
        .filter(|(_, total)| *total > 0)
        .map(|(used, total)| used as f64 * 100.0 / total as f64);
    let empty_series = TimeSeries::default();
    let mut cards: Vec<CardItem> = vec![
        ("CPU", frame.cpu_percent.value.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "Unavailable".into()), &histories.cpu, p.cpu, None),
        ("Memory", memory_pct.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "Unavailable".into()), &histories.memory_pct, p.memory, Some((frame.memory_used_bytes.value, frame.memory_total_bytes.value))),
    ];
    if let Some(adapter) = frame.gpu_adapters.first() {
        let gpu_series_key = format!("gpu:{}:{:?}:busiest", adapter.adapter_luid, adapter.physical_adapter);
        let series = histories.detail(&gpu_series_key).unwrap_or(&empty_series);
        cards.push((
            "GPU · busiest",
            adapter.busiest_engine_percent.value.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "Unavailable".into()),
            series,
            p.gpu,
            Some((adapter.dedicated_used_bytes.value, adapter.dedicated_total_bytes.value)),
        ));
    }
    cards.push(("Storage · read", format_rate(frame.disk_read_bytes_per_sec.value.map(|v| v as f32)), &histories.disk_read, p.disk, None));
    cards.push(("Network · rx", format_rate(frame.network_rx_bytes_per_sec.value.map(|v| v as f32)), &histories.net_rx, p.network, None));

    let columns = if ui.available_width() >= 960.0 { cards.len().min(5) } else if ui.available_width() >= 480.0 { 2 } else { 1 };
    let width = ((ui.available_width() - 12.0 * (columns - 1) as f32) / columns as f32).max(160.0);
    for row in cards.chunks(columns) {
        ui.horizontal(|ui| {
            for (title, value, series, color, cap) in row {
                ui.allocate_ui(Vec2::new(width, 118.0), |ui| {
                    egui::Frame::group(ui.style()).fill(p.surface).stroke(Stroke::new(1.0, p.border))
                        .corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                            ui.set_width(width - 24.0);
                            ui.colored_label(*color, *title);
                            ui.label(egui::RichText::new(value).size(22.0).strong().color(p.text));
                            sparkline(ui, series, now, *color, p);
                            if let Some((used, total)) = cap {
                                capacity_bar(ui, *used, *total, *color, p);
                            }
                        });
                });
            }
        });
        ui.add_space(10.0);
    }
}

pub fn render_overview_view(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    state: &mut OverviewViewState,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    selected_identity: &mut Option<ProcessIdentity>,
    active_page: &mut Page,
    now: Instant,
    p: Palette,
) {
    render_page_header(
        ui,
        "System Overview",
        "Current readings and observed history by resource",
        app_state,
        pause_state,
        poll_interval_ms,
        p,
    );
    resource_cards(ui, frame, histories, now, p);
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Resource summary");
                ui.add_space(12.0);
                for range in [
                    HistoryRange::Sec30,
                    HistoryRange::Sec60,
                    HistoryRange::Min5,
                    HistoryRange::Min15,
                ] {
                    ui.selectable_value(&mut state.range, range, range.label());
                }
            });
            ui.label(
                egui::RichText::new(
                    "All rates are bytes per second; history averages exclude missing intervals.",
                )
                .small()
                .color(p.secondary),
            );
            ui.add_space(8.0);
            let rows = [
                SummaryRow {
                    label: "CPU",
                    current: metric_percent(&frame.cpu_percent),
                    series: &histories.cpu,
                    unit: "%",
                    color: p.cpu,
                    reason: frame
                        .cpu_percent
                        .unavailable_reason
                        .as_ref()
                        .map(ToString::to_string),
                },
                SummaryRow {
                    label: "Memory",
                    current: frame
                        .memory_used_bytes
                        .value
                        .zip(frame.memory_total_bytes.value)
                        .filter(|(_, total)| *total > 0)
                        .map_or_else(
                            || "Unavailable".into(),
                            |(used, total)| format!("{:.1}", used as f64 * 100.0 / total as f64),
                        ),
                    series: &histories.memory_pct,
                    unit: "%",
                    color: p.memory,
                    reason: frame
                        .memory_used_bytes
                        .unavailable_reason
                        .as_ref()
                        .map(ToString::to_string),
                },
                SummaryRow {
                    label: "Disk read",
                    current: metric_rate(&frame.disk_read_bytes_per_sec),
                    series: &histories.disk_read,
                    unit: "B/s",
                    color: p.disk,
                    reason: frame
                        .disk_read_bytes_per_sec
                        .unavailable_reason
                        .as_ref()
                        .map(ToString::to_string),
                },
                SummaryRow {
                    label: "Disk write",
                    current: metric_rate(&frame.disk_write_bytes_per_sec),
                    series: &histories.disk_write,
                    unit: "B/s",
                    color: p.disk,
                    reason: frame
                        .disk_write_bytes_per_sec
                        .unavailable_reason
                        .as_ref()
                        .map(ToString::to_string),
                },
                SummaryRow {
                    label: "Network receive",
                    current: metric_rate(&frame.network_rx_bytes_per_sec),
                    series: &histories.net_rx,
                    unit: "B/s",
                    color: p.network,
                    reason: frame
                        .network_rx_bytes_per_sec
                        .unavailable_reason
                        .as_ref()
                        .map(ToString::to_string),
                },
                SummaryRow {
                    label: "Network transmit",
                    current: metric_rate(&frame.network_tx_bytes_per_sec),
                    series: &histories.net_tx,
                    unit: "B/s",
                    color: p.network,
                    reason: frame
                        .network_tx_bytes_per_sec
                        .unavailable_reason
                        .as_ref()
                        .map(ToString::to_string),
                },
            ];
            summary_table(ui, &rows, state.range, now);
        });
    ui.add_space(12.0);

    if !frame.gpu_engines.is_empty() {
        egui::CollapsingHeader::new(format!("GPU engines ({})", frame.gpu_engines.len())).show(
            ui,
            |ui| {
                let mut engines: Vec<_> = frame.gpu_engines.iter().collect();
                engines.sort_by(|a, b| {
                    b.utilization_percent
                        .value
                        .unwrap_or(0.0)
                        .total_cmp(&a.utilization_percent.value.unwrap_or(0.0))
                });
                egui::Grid::new("gpu_engine_table")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.strong("Engine");
                        ui.strong("Usage");
                        ui.strong("Counter instance");
                        ui.end_row();
                        for engine in engines.into_iter().take(32) {
                            ui.label(&engine.engine_type);
                            ui.label(
                                engine
                                    .utilization_percent
                                    .value
                                    .map_or_else(|| "Unavailable".into(), |v| format!("{v:.1}%")),
                            );
                            ui.label(&engine.instance);
                            ui.end_row();
                        }
                    });
            },
        );
    }
    for (title, devices, read, write) in [
        ("Disk devices", &frame.disks, "Read", "Write"),
        (
            "Network interfaces",
            &frame.network_interfaces,
            "Receive",
            "Transmit",
        ),
    ] {
        if devices.is_empty() {
            continue;
        }
        egui::CollapsingHeader::new(format!("{title} ({})", devices.len())).show(ui, |ui| {
            egui::Grid::new(title)
                .striped(true)
                .num_columns(3)
                .show(ui, |ui| {
                    ui.strong("Device");
                    ui.strong(read);
                    ui.strong(write);
                    ui.end_row();
                    for device in devices.iter().take(64) {
                        ui.label(&device.label).on_hover_text(&device.id);
                        ui.label(format_rate(
                            device.read_bytes_per_sec.value.map(|v| v as f32),
                        ));
                        ui.label(format_rate(
                            device.write_bytes_per_sec.value.map(|v| v as f32),
                        ));
                        ui.end_row();
                    }
                });
        });
    }
    ui.add_space(12.0);
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Top active processes");
                ui.selectable_value(&mut state.top_by_cpu, true, "CPU");
                ui.selectable_value(&mut state.top_by_cpu, false, "Memory");
                if ui.button("View all processes").clicked() {
                    *active_page = Page::Processes;
                }
            });
            let sort_col = if state.top_by_cpu {
                SortColumn::Cpu
            } else {
                SortColumn::Memory
            };
            let sorted = sort_processes(&frame.processes, "", sort_col, true);
            state.top_table.sort_column = sort_col;
            state.top_table.sort_descending = true;
            render_process_table(
                ui,
                &sorted[..sorted.len().min(8)],
                selected_identity,
                &mut state.top_table,
                32.0,
                p,
            );
        });
}
