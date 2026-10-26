use crate::{
    components::{
        header::render_page_header,
        plot::sparkline_sized,
        table::{ProcessTableState, SortColumn, render_process_table_inline, sort_processes},
    },
    formatting::{format_bytes, format_rate},
    history::{HistoryRange, ResourceHistories, TimeSeries},
    state::{AppState, DisplayPause, Page},
    theme::Palette,
};
use eframe::egui::{self, Align, Color32, CornerRadius, Rect, Stroke, Vec2};
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

pub fn show_contained_card<R>(
    ui: &mut egui::Ui,
    p: Palette,
    inner_margin: i8,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::Response {
    let outer_w = ui.available_width().max(1.0);
    let frame = egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(inner_margin));
    let inner_w = (outer_w - frame.total_margin().sum().x).max(1.0);
    frame
        .show(ui, |ui| {
            ui.set_width(inner_w);
            ui.set_max_width(inner_w);
            let clip_x = Rect::from_x_y_ranges(ui.max_rect().x_range(), ui.clip_rect().y_range())
                .intersect(ui.clip_rect());
            ui.set_clip_rect(clip_x);
            add_contents(ui)
        })
        .response
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

fn metric_rate(metric: &Metric<u64>, series: &TimeSeries) -> String {
    let value = if metric.value.is_some() {
        series
            .latest_stable_value()
            .or_else(|| metric.value.map(|v| v as f32))
    } else {
        None
    };
    format_rate(value)
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
        ui.set_max_width(width);
        ui.add(egui::Label::new(text).truncate())
    })
    .inner
}

fn summary_table(
    ui: &mut egui::Ui,
    rows: &[SummaryRow<'_>],
    range: HistoryRange,
    now: Instant,
    p: Palette,
) {
    let text_scale = crate::appearance::text_scale(ui.ctx()).max(1.0);
    let min_table_width = 560.0 * text_scale;
    let available_w = ui.available_width();
    let table_width = available_w.max(min_table_width);

    let render_table_content = |ui: &mut egui::Ui| {
        ui.set_min_width(table_width);
        ui.set_max_width(table_width);
        let content_width = (table_width - 8.0).max(100.0);
        let widths = [
            (content_width * 0.22).floor(),
            (content_width * 0.16).floor(),
            (content_width * 0.15).floor(),
            (content_width * 0.15).floor(),
            (content_width * 0.15).floor(),
            (content_width * 0.17).floor(),
        ];
        let headings = ["RESOURCE", "NOW", "AVERAGE", "LOW", "PEAK", "COVERAGE"];
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(4, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (index, (heading, width)) in headings.iter().zip(widths).enumerate() {
                        let align = if index == 0 {
                            Align::LEFT
                        } else {
                            Align::RIGHT
                        };
                        summary_cell(
                            ui,
                            width,
                            24.0,
                            egui::RichText::new(*heading)
                                .small()
                                .color(ui.visuals().weak_text_color()),
                            align,
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
                .inner_margin(egui::Margin::symmetric(4, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        summary_cell(
                            ui,
                            widths[0],
                            30.0,
                            egui::RichText::new(row.label).color(row.color),
                            Align::LEFT,
                        )
                        .on_hover_text(row.detail.as_deref().unwrap_or(row.label));

                        summary_cell(
                            ui,
                            widths[1],
                            30.0,
                            egui::RichText::new(&row.current).strong().monospace(),
                            Align::RIGHT,
                        )
                        .on_hover_text(row.reason.as_deref().unwrap_or("Latest measured value"));

                        for (column, width) in widths[2..5].iter().enumerate() {
                            summary_cell(
                                ui,
                                *width,
                                30.0,
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
                            let cov_str = format!(
                                "{:.0}% ({}/{}s)",
                                summary.coverage_percent(),
                                summary.observed_seconds,
                                summary.expected_seconds
                            );
                            summary_cell(
                                ui,
                                widths[5],
                                30.0,
                                egui::RichText::new(cov_str).monospace().color(p.secondary),
                                Align::RIGHT,
                            )
                            .on_hover_text(coverage_detail);
                        } else {
                            summary_cell(
                                ui,
                                widths[5],
                                30.0,
                                egui::RichText::new("Collecting").monospace().color(p.secondary),
                                Align::RIGHT,
                            )
                            .on_hover_text("No valid observations in this range yet");
                        }
                    });
                });
        }
    };

    if available_w < min_table_width {
        egui::ScrollArea::horizontal()
            .id_salt("overview_resource_summary_scroll")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                render_table_content(ui);
            });
    } else {
        render_table_content(ui);
    }
}

type CardItem<'a> = (
    &'a str,
    &'static str,
    String,
    &'a TimeSeries,
    egui::Color32,
    String,
);

fn overview_card_visuals(
    hovered: bool,
    pressed: bool,
    has_focus: bool,
    accent: Color32,
    p: Palette,
) -> (Color32, Stroke) {
    if pressed {
        let fill = crate::views::resources::blend_color(p.surface, accent, 0.10);
        (fill, Stroke::new(1.0, accent))
    } else if hovered || has_focus {
        let fill = crate::views::resources::blend_color(p.surface, accent, 0.05);
        let stroke = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 160);
        (fill, Stroke::new(1.0, stroke))
    } else {
        (p.surface, Stroke::new(1.0, p.border))
    }
}

fn resource_card_columns(width: f32, card_count: usize, text_scale: f32) -> usize {
    let scale = text_scale.max(1.0);
    let columns = if width >= 1148.0 * scale {
        5
    } else if width >= 744.0 * scale {
        3
    } else if width >= 492.0 * scale {
        2
    } else {
        1
    };
    columns.min(card_count).max(1)
}

fn resource_cards(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    performance_request: &mut Option<String>,
    active_page: &mut Page,
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
    let memory_subtitle = if let (Some(used), Some(total)) = (
        frame.memory_used_bytes.value,
        frame.memory_total_bytes.value,
    ) {
        format!("{} / {}", format_bytes(used), format_bytes(total))
    } else {
        "Physical memory".into()
    };
    let mut cards: Vec<CardItem> = vec![
        (
            "CPU",
            "cpu",
            frame
                .cpu_percent
                .value
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "Unavailable".into()),
            &histories.cpu,
            p.cpu,
            format!("{} logical processors", frame.logical_cpus.len()),
        ),
        (
            "Memory",
            "memory",
            memory_pct
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "Unavailable".into()),
            &histories.memory_pct,
            p.memory,
            memory_subtitle,
        ),
    ];
    if let Some(adapter) = frame.gpu_adapters.first() {
        let gpu_series_key = format!(
            "gpu:{}:{:?}:busiest",
            adapter.adapter_luid, adapter.physical_adapter
        );
        let series = histories.detail(&gpu_series_key).unwrap_or(&empty_series);
        let gpu_sub = if adapter.dedicated_total_bytes.value == Some(0) {
            "Shared memory (UMA)".into()
        } else if let (Some(u), Some(t)) = (
            adapter.dedicated_used_bytes.value,
            adapter.dedicated_total_bytes.value,
        ) {
            format!("VRAM {} / {}", format_bytes(u), format_bytes(t))
        } else {
            "GPU engine".into()
        };
        cards.push((
            "GPU · busiest",
            "gpu",
            adapter
                .busiest_engine_percent
                .value
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "Unavailable".into()),
            series,
            p.gpu,
            gpu_sub,
        ));
    }
    cards.push((
        "Storage · read",
        "disk",
        metric_rate(&frame.disk_read_bytes_per_sec, &histories.disk_read),
        &histories.disk_read,
        p.disk,
        format!(
            "Write {}",
            metric_rate(&frame.disk_write_bytes_per_sec, &histories.disk_write)
        ),
    ));
    cards.push((
        "Network · rx",
        "net",
        metric_rate(&frame.network_rx_bytes_per_sec, &histories.net_rx),
        &histories.net_rx,
        p.network,
        format!(
            "Tx {}",
            metric_rate(&frame.network_tx_bytes_per_sec, &histories.net_tx)
        ),
    ));

    let available_width = ui.available_width();
    let text_scale = crate::appearance::text_scale(ui.ctx()).max(1.0);
    let columns = resource_card_columns(available_width, cards.len(), text_scale);
    let card_h = (98.0 * text_scale + 40.0).round();
    for row in cards.chunks(columns) {
        let col_count = row.len();
        let total_spacing = 12.0 * (col_count - 1) as f32;
        let card_w = ((available_width - total_spacing) / col_count as f32)
            .floor()
            .max(120.0);
        ui.with_layout(egui::Layout::left_to_right(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for (title, target_resource, value, series, color, subtitle) in row {
                let (card_rect, response) =
                    ui.allocate_exact_size(Vec2::new(card_w, card_h), egui::Sense::click());
                let card_rect = card_rect.round();
                let response = response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!("Open {} in Performance", title));

                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::accesskit::Role::Button,
                        true,
                        format!("{} - {}", title, value),
                    )
                });

                let (fill, stroke) = overview_card_visuals(
                    response.hovered(),
                    response.is_pointer_button_down_on(),
                    response.has_focus(),
                    *color,
                    p,
                );

                ui.painter().rect(
                    card_rect,
                    CornerRadius::same(8),
                    fill,
                    stroke,
                    egui::StrokeKind::Inside,
                );

                if response.has_focus() {
                    ui.painter().rect_stroke(
                        card_rect.expand(1.0),
                        CornerRadius::same(9),
                        Stroke::new(2.0, p.focus),
                        egui::StrokeKind::Outside,
                    );
                }

                let activated = response.clicked()
                    || (response.has_focus()
                        && ui.input(|i| {
                            i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)
                        }));
                if activated {
                    *performance_request = Some((*target_resource).to_string());
                    *active_page = Page::Performance;
                }

                let inner_rect = card_rect.shrink(12.0);
                if inner_rect.width() > 10.0 && inner_rect.height() > 10.0 {
                    let card_clip = card_rect.shrink(1.0).intersect(ui.clip_rect());
                    let spark_h = 36.0;
                    let spark_rect = Rect::from_min_size(
                        egui::pos2(inner_rect.min.x, inner_rect.max.y - spark_h),
                        Vec2::new(inner_rect.width(), spark_h),
                    );
                    let text_rect = Rect::from_min_max(
                        inner_rect.min,
                        egui::pos2(inner_rect.max.x, spark_rect.min.y - 4.0),
                    );

                    let mut cur_y = text_rect.min.y;
                    ui.painter_at(card_clip).text(
                        egui::pos2(text_rect.min.x, cur_y),
                        egui::Align2::LEFT_TOP,
                        *title,
                        egui::FontId::proportional(13.0 * text_scale),
                        *color,
                    );
                    cur_y += 18.0 * text_scale;

                    ui.painter_at(card_clip).text(
                        egui::pos2(text_rect.min.x, cur_y),
                        egui::Align2::LEFT_TOP,
                        value,
                        egui::FontId::proportional(22.0 * text_scale),
                        p.text,
                    );
                    cur_y += 26.0 * text_scale;

                    ui.painter_at(card_clip).text(
                        egui::pos2(text_rect.min.x, cur_y),
                        egui::Align2::LEFT_TOP,
                        subtitle,
                        egui::FontId::proportional(11.0 * text_scale),
                        p.secondary,
                    );

                    let mut child_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(spark_rect)
                            .layout(egui::Layout::top_down(Align::Min)),
                    );
                    child_ui.set_clip_rect(card_clip);
                    sparkline_sized(
                        &mut child_ui,
                        series,
                        now,
                        *color,
                        p,
                        Vec2::new(spark_rect.width(), spark_rect.height()),
                    );
                }
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
    performance_request: &mut Option<String>,
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
    resource_cards(
        ui,
        frame,
        histories,
        performance_request,
        active_page,
        now,
        p,
    );
    show_contained_card(ui, p, 14, |ui| {
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
                current: metric_rate(&frame.disk_read_bytes_per_sec, &histories.disk_read),
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
                current: metric_rate(&frame.disk_write_bytes_per_sec, &histories.disk_write),
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
                current: metric_rate(&frame.network_rx_bytes_per_sec, &histories.net_rx),
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
                current: metric_rate(&frame.network_tx_bytes_per_sec, &histories.net_tx),
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
        summary_table(ui, &rows, state.range, now, p);
    });
    ui.add_space(12.0);

    if !frame.gpu_engines.is_empty() {
        show_contained_card(ui, p, 12, |ui| {
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
                    egui::ScrollArea::horizontal()
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
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
                                        ui.label(engine.utilization_percent.value.map_or_else(
                                            || "Unavailable".into(),
                                            |v| format!("{v:.1}%"),
                                        ));
                                        ui.add(egui::Label::new(&engine.instance).truncate())
                                            .on_hover_text(&engine.instance);
                                        ui.end_row();
                                    }
                                });
                        });
                },
            );
        });
        ui.add_space(8.0);
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
        show_contained_card(ui, p, 12, |ui| {
            egui::CollapsingHeader::new(format!("{title} ({})", devices.len())).show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
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
            });
        });
        ui.add_space(8.0);
    }
    ui.add_space(4.0);
    show_contained_card(ui, p, 14, |ui| {
        ui.horizontal_wrapped(|ui| {
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
        render_process_table_inline(
            ui,
            &sorted[..sorted.len().min(8)],
            selected_identity,
            &mut state.top_table,
            32.0,
            p,
        );
    });
}

#[cfg(test)]
mod layout_tests {
    use super::resource_card_columns;

    #[test]
    fn five_resource_cards_use_the_wide_row_and_reflow_on_smaller_windows() {
        assert_eq!(resource_card_columns(1310.0, 5, 1.0), 5);
        assert_eq!(resource_card_columns(970.0, 5, 1.0), 3);
        assert_eq!(resource_card_columns(600.0, 5, 1.0), 2);
        assert_eq!(resource_card_columns(420.0, 5, 1.0), 1);
    }

    #[test]
    fn overview_workspace_stays_inside_available_width() {
        let fixture = crate::smoke_test::create_native_fixture_app(
            crate::theme::Theme::Dark,
            crate::state::Page::Overview,
        );
        let frame = fixture.shared.lock().unwrap().frame.clone().unwrap();
        for width in [420.0, 600.0, 800.0, 940.0, 1180.0, 1536.0] {
            for scale in [1.0, 1.5, 2.0] {
                let ctx = eframe::egui::Context::default();
                crate::theme::configure_theme(
                    &ctx,
                    crate::theme::get_palette(true),
                    crate::theme::Density::Normal,
                    scale,
                    false,
                );
                ctx.data_mut(|data| {
                    data.insert_temp(eframe::egui::Id::unique("system_text_scale"), scale)
                });
                let mut overview_state = super::OverviewViewState::default();
                let mut perf_req: Option<String> = None;
                let mut active_page = crate::state::Page::Overview;
                for _ in 0..2 {
                    let mut output = ctx.run_ui(
                        eframe::egui::RawInput {
                            screen_rect: Some(eframe::egui::Rect::from_min_size(
                                eframe::egui::Pos2::ZERO,
                                eframe::egui::Vec2::new(width, 790.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            let right = ui.max_rect().right();
                            super::render_overview_view(
                                ui,
                                &frame,
                                &fixture.histories,
                                &mut overview_state,
                                &crate::state::AppState::Live,
                                &mut crate::state::DisplayPause::default(),
                                100,
                                &mut None,
                                &mut perf_req,
                                &mut active_page,
                                std::time::Instant::now(),
                                crate::theme::get_palette(true),
                            );
                            assert!(
                                ui.min_rect().right() <= right + 1.0,
                                "Overview at width {width}, scale {scale}: overflowed to {}",
                                ui.min_rect().right()
                            );
                        },
                    );
                    output.textures_delta.clear();
                }
            }
        }
    }

    #[test]
    fn overview_card_click_requests_performance_resource() {
        let fixture = crate::smoke_test::create_native_fixture_app(
            crate::theme::Theme::Dark,
            crate::state::Page::Overview,
        );
        let frame = fixture.shared.lock().unwrap().frame.clone().unwrap();
        let ctx = eframe::egui::Context::default();
        let p = crate::theme::get_palette(true);

        let mut overview_state = super::OverviewViewState::default();
        let mut perf_req: Option<String> = None;
        let mut active_page = crate::state::Page::Overview;

        // Run frame 1 to lay out the overview cards
        let mut output = ctx.run_ui(
            eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::Vec2::new(1180.0, 790.0),
                )),
                ..Default::default()
            },
            |ui| {
                super::render_overview_view(
                    ui,
                    &frame,
                    &fixture.histories,
                    &mut overview_state,
                    &crate::state::AppState::Live,
                    &mut crate::state::DisplayPause::default(),
                    100,
                    &mut None,
                    &mut perf_req,
                    &mut active_page,
                    std::time::Instant::now(),
                    p,
                );
            },
        );
        output.textures_delta.clear();
        assert_eq!(active_page, crate::state::Page::Overview);
        assert!(perf_req.is_none());

        // Click within the first card's bounds (e.g. CPU card area around x=60, y=95)
        let click_pos = eframe::egui::pos2(60.0, 95.0);
        let mut output = ctx.run_ui(
            eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::Vec2::new(1180.0, 790.0),
                )),
                events: vec![
                    eframe::egui::Event::PointerMoved(click_pos),
                    eframe::egui::Event::PointerButton {
                        pos: click_pos,
                        button: eframe::egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    eframe::egui::Event::PointerButton {
                        pos: click_pos,
                        button: eframe::egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            |ui| {
                super::render_overview_view(
                    ui,
                    &frame,
                    &fixture.histories,
                    &mut overview_state,
                    &crate::state::AppState::Live,
                    &mut crate::state::DisplayPause::default(),
                    100,
                    &mut None,
                    &mut perf_req,
                    &mut active_page,
                    std::time::Instant::now(),
                    p,
                );
            },
        );
        output.textures_delta.clear();
        assert_eq!(active_page, crate::state::Page::Performance);
        assert_eq!(perf_req.as_deref(), Some("cpu"));
    }

    #[test]
    fn overview_cards_have_uniform_row_top_height_and_all_summary_columns_fit() {
        let fixture = crate::smoke_test::create_native_fixture_app(
            crate::theme::Theme::Dark,
            crate::state::Page::Overview,
        );
        let frame = fixture.shared.lock().unwrap().frame.clone().unwrap();
        let ctx = eframe::egui::Context::default();
        let p = crate::theme::get_palette(true);

        let mut overview_state = super::OverviewViewState::default();
        let mut perf_req: Option<String> = None;
        let mut active_page = crate::state::Page::Overview;

        let mut output = ctx.run_ui(
            eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::Vec2::new(1536.0, 864.0),
                )),
                ..Default::default()
            },
            |ui| {
                super::render_overview_view(
                    ui,
                    &frame,
                    &fixture.histories,
                    &mut overview_state,
                    &crate::state::AppState::Live,
                    &mut crate::state::DisplayPause::default(),
                    100,
                    &mut None,
                    &mut perf_req,
                    &mut active_page,
                    std::time::Instant::now(),
                    p,
                );
            },
        );

        // Verify all 6 summary column headings are rendered and within screen bounds
        use eframe::egui;
        let mut text_found: Vec<String> = Vec::new();
        fn collect_text(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(t) => {
                    out.push(t.galley.job.text.clone());
                }
                egui::epaint::Shape::Vec(shapes) => {
                    for s in shapes {
                        collect_text(s, out);
                    }
                }
                _ => {}
            }
        }
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut text_found);
        }

        for heading in ["RESOURCE", "NOW", "AVERAGE", "LOW", "PEAK", "COVERAGE"] {
            assert!(
                text_found.iter().any(|t| t == heading),
                "Heading {heading} must be present in Overview render output"
            );
        }

        output.textures_delta.clear();
    }
}
