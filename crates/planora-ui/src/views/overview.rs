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
use eframe::egui::{self, Align, Color32, CornerRadius, Stroke, Vec2};
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
    &'static str,
    String,
    &'a TimeSeries,
    egui::Color32,
    Option<(Option<u64>, Option<u64>)>,
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

fn resource_card_columns(width: f32, card_count: usize) -> usize {
    let columns = if width >= 1148.0 {
        5
    } else if width >= 744.0 {
        3
    } else if width >= 492.0 {
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
            None,
        ),
        (
            "Memory",
            "memory",
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
            "gpu",
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
        "disk",
        metric_rate(&frame.disk_read_bytes_per_sec, &histories.disk_read),
        &histories.disk_read,
        p.disk,
        None,
    ));
    cards.push((
        "Network · rx",
        "net",
        metric_rate(&frame.network_rx_bytes_per_sec, &histories.net_rx),
        &histories.net_rx,
        p.network,
        None,
    ));

    let available_width = ui.available_width();
    let columns = resource_card_columns(available_width, cards.len());
    for row in cards.chunks(columns) {
        // A partial final row fills the available width instead of leaving an
        // isolated card beside a large empty area.
        let width =
            ((available_width - 12.0 * (row.len() - 1) as f32) / row.len() as f32).max(160.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for (title, target_resource, value, series, color, cap) in row {
                let card_w = width;
                let mut prepared = egui::Frame::group(ui.style())
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(12))
                    .begin(ui);

                {
                    let ui = &mut prepared.content_ui;
                    ui.set_width(card_w - 24.0);
                    ui.set_min_height(136.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(*title).color(*color).strong())
                            .selectable(false),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(value).size(22.0).strong().color(p.text),
                        )
                        .selectable(false),
                    );
                    sparkline_sized(
                        ui,
                        series,
                        now,
                        *color,
                        p,
                        Vec2::new((card_w - 24.0).max(68.0), 36.0),
                    );
                    if let Some((used, total)) = cap {
                        capacity_bar(ui, *used, *total, *color, p);
                    }
                }

                let alloc_resp = prepared.allocate_space(ui);
                let response = ui
                    .interact(alloc_resp.rect, alloc_resp.id, egui::Sense::click())
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
                prepared.frame.fill = fill;
                prepared.frame.stroke = stroke;
                prepared.paint(ui);

                if response.has_focus() {
                    ui.painter().rect_stroke(
                        alloc_resp.rect.expand(1.0),
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

#[cfg(test)]
mod layout_tests {
    use super::resource_card_columns;

    #[test]
    fn five_resource_cards_use_the_wide_row_and_reflow_on_smaller_windows() {
        assert_eq!(resource_card_columns(1310.0, 5), 5);
        assert_eq!(resource_card_columns(970.0, 5), 3);
        assert_eq!(resource_card_columns(600.0, 5), 2);
        assert_eq!(resource_card_columns(420.0, 5), 1);
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
}
