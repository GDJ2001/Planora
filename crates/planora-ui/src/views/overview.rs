use crate::{
    components::{
        header::render_page_header,
        plot::{capacity_bar, sparkline_sized},
        table::{ProcessTableState, SortColumn, render_process_table, sort_processes},
    },
    formatting::format_rate,
    history::{HistoryRange, ResourceHistories, TimeSeries},
    state::{AppState, DisplayPause, Page},
    theme::Palette,
};
use eframe::egui::{self, Align, CornerRadius, Stroke, Vec2};
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
    detail: Option<String>,
}

fn metric_percent(metric: &Metric<f32>) -> String {
    metric
        .value
        .map_or_else(|| "Unavailable".into(), |value| format!("{value:.1}%"))
}

fn metric_rate(metric: &Metric<u64>) -> String {
    format_rate(metric.value.map(|value| value as f32))
}

fn summary_value(value: f32, unit: &str) -> String {
    if unit == "%" {
        format!("{value:.1}%")
    } else {
        format_rate(Some(value))
    }
}

fn summary_cell(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    text: egui::RichText,
    align: Align,
) -> egui::Response {
    let layout = if align == Align::RIGHT {
        egui::Layout::right_to_left(Align::Center)
    } else {
        egui::Layout::left_to_right(Align::Center)
    };
    ui.allocate_ui_with_layout(Vec2::new(width, height), layout, |ui| {
        if align == Align::RIGHT {
            ui.add_space(14.0);
        }
        ui.add(egui::Label::new(text).truncate())
    })
    .inner
}

fn summary_table(ui: &mut egui::Ui, rows: &[SummaryRow<'_>], range: HistoryRange, now: Instant) {
    let table_width = ui.available_width().max(830.0);
    egui::ScrollArea::horizontal()
        .id_salt("overview_resource_summary_scroll")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_min_width(table_width);
            let content_width = table_width - 20.0;
            let widths = [
                content_width * 0.19,
                content_width * 0.16,
                content_width * 0.16,
                content_width * 0.15,
                content_width * 0.15,
                content_width * 0.19,
            ];
            let headings = ["RESOURCE", "NOW", "AVERAGE", "LOW", "PEAK", "COVERAGE"];
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (index, (heading, width)) in headings.iter().zip(widths).enumerate() {
                            if index == 5 {
                                ui.add_space(14.0);
                            }
                            summary_cell(
                                ui,
                                if index == 5 { width - 14.0 } else { width },
                                24.0,
                                egui::RichText::new(*heading)
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                                if index == 0 || index == 5 {
                                    Align::LEFT
                                } else {
                                    Align::RIGHT
                                },
                            );
                        }
                    });
                });
            ui.separator();

            for (index, row) in rows.iter().enumerate() {
                let summary = row.series.observed_summary(now, range);
                let values = summary.map(|summary| {
                    [
                        summary_value(summary.average, row.unit),
                        summary_value(summary.minimum, row.unit),
                        summary_value(summary.maximum, row.unit),
                    ]
                });
                egui::Frame::NONE
                    .fill(if index % 2 == 0 {
                        ui.visuals().faint_bg_color
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(10, 3))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            summary_cell(
                                ui,
                                widths[0],
                                32.0,
                                egui::RichText::new(row.label).color(row.color),
                                Align::LEFT,
                            )
                            .on_hover_text(row.detail.as_deref().unwrap_or(row.label));
                            summary_cell(
                                ui,
                                widths[1],
                                32.0,
                                egui::RichText::new(&row.current).strong().monospace(),
                                Align::RIGHT,
                            )
                            .on_hover_text(row.reason.as_deref().unwrap_or("Latest measured value"));
                            for (column, width) in widths[2..5].iter().enumerate() {
                                summary_cell(
                                    ui,
                                    *width,
                                    32.0,
                                    egui::RichText::new(
                                        values.as_ref().map_or("—", |values| values[column].as_str()),
                                    )
                                    .monospace(),
                                    Align::RIGHT,
                                );
                            }
                            if let Some(summary) = summary {
                                let coverage_detail = format!(
                                    "{} of {} seconds observed. Missing intervals are excluded from the statistics.",
                                    summary.observed_seconds, summary.expected_seconds
                                );
                                ui.allocate_ui_with_layout(
                                    Vec2::new(widths[5], 32.0),
                                    egui::Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.set_min_width(widths[5]);
                                        ui.add_space(14.0);
                                        let bar_width = (widths[5] - 82.0).max(36.0);
                                        ui.add_sized(
                                            [bar_width, 10.0],
                                            egui::ProgressBar::new(summary.coverage_percent() / 100.0)
                                                .fill(row.color),
                                        )
                                        .on_hover_text(&coverage_detail);
                                        ui.add_space(4.0);
                                        ui.label(
                                            egui::RichText::new(format!("{:.0}%", summary.coverage_percent()))
                                                .monospace(),
                                        )
                                        .on_hover_text(coverage_detail);
                                    },
                                );
                            } else {
                                ui.add_space(14.0);
                                ui.add_sized(
                                    [widths[5] - 14.0, 32.0],
                                    egui::Label::new("Collecting"),
                                )
                                .on_hover_text("No valid observations in this range yet");
                            }
                        });
                    });
            }
        });
}

type CardItem<'a> = (
    &'a str,
    String,
    &'a TimeSeries,
    egui::Color32,
    Option<(Option<u64>, Option<u64>)>,
);

fn resource_cards(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    now: Instant,
    p: Palette,
) {
    let memory_pct = frame
        .memory_used_bytes
        .value
        .zip(frame.memory_total_bytes.value)
        .filter(|(_, total)| *total > 0)
        .map(|(used, total)| used as f64 * 100.0 / total as f64);
    let empty_series = TimeSeries::default();
    let mut cards: Vec<CardItem> = vec![
        (
            "CPU",
            frame
                .cpu_percent
                .value
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "Unavailable".into()),
            &histories.cpu,
            p.cpu,
            None,
        ),
        (
            "Memory",
            memory_pct
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "Unavailable".into()),
            &histories.memory_pct,
            p.memory,
            Some((
                frame.memory_used_bytes.value,
                frame.memory_total_bytes.value,
            )),
        ),
    ];
    if let Some(adapter) = frame.gpu_adapters.first() {
        let gpu_series_key = format!(
            "gpu:{}:{:?}:busiest",
            adapter.adapter_luid, adapter.physical_adapter
        );
        let series = histories.detail(&gpu_series_key).unwrap_or(&empty_series);
        cards.push((
            "GPU · busiest",
            adapter
                .busiest_engine_percent
                .value
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "Unavailable".into()),
            series,
            p.gpu,
            Some((
                adapter.dedicated_used_bytes.value,
                adapter.dedicated_total_bytes.value,
            )),
        ));
    }
    cards.push((
        "Storage · read",
        format_rate(frame.disk_read_bytes_per_sec.value.map(|v| v as f32)),
        &histories.disk_read,
        p.disk,
        None,
    ));
    cards.push((
        "Network · rx",
        format_rate(frame.network_rx_bytes_per_sec.value.map(|v| v as f32)),
        &histories.net_rx,
        p.network,
        None,
    ));

    let columns = if ui.available_width() >= 1280.0 {
        cards.len().min(4)
    } else if ui.available_width() >= 900.0 {
        cards.len().min(3)
    } else if ui.available_width() >= 480.0 {
        2
    } else {
        1
    };
    let width = ((ui.available_width() - 12.0 * (columns - 1) as f32) / columns as f32).max(160.0);
    for row in cards.chunks(columns) {
        ui.horizontal(|ui| {
            for (title, value, series, color, cap) in row {
                ui.allocate_ui_with_layout(
                    Vec2::new(width, 160.0),
                    egui::Layout::top_down(Align::Min),
                    |ui| {
                        egui::Frame::group(ui.style())
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0, p.border))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::same(12))
                            .show(ui, |ui| {
                                ui.set_width(width - 24.0);
                                ui.set_min_height(136.0);
                                ui.colored_label(*color, *title);
                                ui.label(
                                    egui::RichText::new(value).size(22.0).strong().color(p.text),
                                );
                                sparkline_sized(
                                    ui,
                                    series,
                                    now,
                                    *color,
                                    p,
                                    Vec2::new((width - 24.0).max(68.0), 36.0),
                                );
                                if let Some((used, total)) = cap {
                                    capacity_bar(ui, *used, *total, *color, p);
                                }
                            });
                    },
                );
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
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.heading("Resource summary");
                ui.add_space(20.0);
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
                    "Readable byte rates · missing samples excluded from history · hover for details",
                )
                .small()
                .color(p.secondary),
            );
            ui.add_space(8.0);
            let empty_gpu_series = TimeSeries::default();
            let mut rows = vec![
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
                    detail: None,
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
                            |(used, total)| format!("{:.1}%", used as f64 * 100.0 / total as f64),
                        ),
                    series: &histories.memory_pct,
                    unit: "%",
                    color: p.memory,
                    reason: frame
                        .memory_used_bytes
                        .unavailable_reason
                        .as_ref()
                        .map(ToString::to_string),
                    detail: None,
                },
            ];
            if let Some(adapter) = frame.gpu_adapters.first() {
                let key = format!(
                    "gpu:{}:{:?}:busiest",
                    adapter.adapter_luid, adapter.physical_adapter
                );
                rows.push(SummaryRow {
                    label: "GPU · busiest",
                    current: metric_percent(&adapter.busiest_engine_percent),
                    series: histories.detail(&key).unwrap_or(&empty_gpu_series),
                    unit: "%",
                    color: p.gpu,
                    reason: adapter
                        .busiest_engine_percent
                        .unavailable_reason
                        .as_ref()
                        .map(ToString::to_string),
                    detail: Some(adapter.name.clone()),
                });
            }
            rows.extend([
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
                    detail: None,
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
                    detail: None,
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
                    detail: None,
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
                    detail: None,
                },
            ]);
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
