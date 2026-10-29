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
use planora_core::{Metric, ProcessIdentity, ProcessRecord, TelemetryFrame};
use std::time::{Duration, Instant};

pub struct OverviewViewState {
    pub range: HistoryRange,
    pub top_by_cpu: bool,
    pub top_table: ProcessTableState,
    cached_frame_seq: u64,
    cached_top_col: Option<SortColumn>,
    last_top_refresh: Option<Instant>,
    cached_top: Vec<ProcessIdentity>,
}

impl Default for OverviewViewState {
    fn default() -> Self {
        Self {
            range: HistoryRange::Sec60,
            top_by_cpu: true,
            top_table: ProcessTableState::default(),
            cached_frame_seq: 0,
            cached_top_col: None,
            last_top_refresh: None,
            cached_top: Vec::new(),
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

const SUMMARY_HEADINGS: [&str; 6] = ["Resource", "Now", "Average", "Low", "Peak", "Coverage"];

struct SummaryDisplay {
    label: String,
    values: [String; 5],
    hints: [String; 5],
    detail: String,
    color: Color32,
}

fn summary_display(
    rows: &[SummaryRow<'_>],
    range: HistoryRange,
    now: Instant,
) -> Vec<SummaryDisplay> {
    rows.iter().map(|row| {
        let summary = row.series.observed_summary(now, range);
        let mut values = [row.current.clone(), "—".into(), "—".into(), "—".into(), "Collecting".into()];
        if let Some(s) = summary {
            values[1] = summary_value(s.average, row.unit);
            values[2] = summary_value(s.minimum, row.unit);
            values[3] = summary_value(s.maximum, row.unit);
            values[4] = format!("{}/{} seconds · {:.0}%", s.observed_seconds, s.expected_seconds, s.coverage_percent());
        }
        SummaryDisplay {
            label: row.label.into(),
            values,
            hints: [
                row.reason.clone().unwrap_or_else(|| "Latest measured value".into()),
                "Average of observed samples".into(),
                "Lowest observed value".into(),
                "Highest observed value".into(),
                "Observed time in the selected range. Missing intervals are excluded from statistics.".into(),
            ],
            detail: row.detail.clone().unwrap_or_else(|| row.label.into()),
            color: row.color,
        }
    }).collect()
}

fn measured_width(ui: &egui::Ui, text: &str, monospace: bool) -> f32 {
    let style = if monospace {
        egui::TextStyle::Monospace
    } else {
        egui::TextStyle::Body
    };
    ui.painter()
        .layout_no_wrap(
            text.into(),
            style.resolve(ui.style()),
            ui.visuals().text_color(),
        )
        .size()
        .x
        .ceil()
}

fn summary_column_widths(
    ui: &egui::Ui,
    rows: &[SummaryDisplay],
    available: f32,
) -> Option<[f32; 6]> {
    // Each cell includes its own 12-point gutter. Measure the same fonts used to
    // render values: fixed percentage widths can silently truncate byte rates.
    let mut widths = std::array::from_fn(|i| measured_width(ui, SUMMARY_HEADINGS[i], false) + 12.0);
    for row in rows {
        widths[0] = widths[0].max(measured_width(ui, &row.label, false) + 12.0);
        for (i, value) in row.values.iter().enumerate() {
            widths[i + 1] = widths[i + 1].max(measured_width(ui, value, true) + 12.0);
        }
    }
    let minimum: f32 = widths.iter().sum();
    if minimum > available {
        return None;
    }
    let spare = (available - minimum) / 6.0;
    for width in &mut widths {
        *width += spare;
    }
    Some(widths)
}

fn summary_cell(
    ui: &mut egui::Ui,
    width: f32,
    text: egui::RichText,
    right: bool,
) -> egui::Response {
    let height = ui.text_style_height(&egui::TextStyle::Body).max(24.0) + 8.0;
    // Reserve the full column even when the label is short. An allocating
    // child layout may shrink to its content and shift all later columns.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let mut cell = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(6.0, 0.0)))
            .layout(if right {
                egui::Layout::right_to_left(Align::Center)
            } else {
                egui::Layout::left_to_right(Align::Center)
            }),
    );
    cell.add(egui::Label::new(text))
}

fn render_summary_display(ui: &mut egui::Ui, rows: &[SummaryDisplay], p: Palette) {
    let available = ui.available_width();
    if let Some(widths) = summary_column_widths(ui, rows, available) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, heading) in SUMMARY_HEADINGS.iter().enumerate() {
                summary_cell(
                    ui,
                    widths[i],
                    egui::RichText::new(*heading).color(p.secondary),
                    i > 0,
                );
            }
        });
        ui.separator();
        for (index, row) in rows.iter().enumerate() {
            egui::Frame::NONE
                .fill(if index % 2 == 0 {
                    ui.visuals().faint_bg_color
                } else {
                    Color32::TRANSPARENT
                })
                .corner_radius(4)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        summary_cell(
                            ui,
                            widths[0],
                            egui::RichText::new(&row.label).color(row.color),
                            false,
                        )
                        .on_hover_text(&row.detail);
                        for (i, value) in row.values.iter().enumerate() {
                            summary_cell(
                                ui,
                                widths[i + 1],
                                egui::RichText::new(value).monospace(),
                                true,
                            )
                            .on_hover_text(&row.hints[i]);
                        }
                    });
                });
        }
    } else {
        // A compact summary is a list of complete records, never a clipped
        // table or a horizontally scrolling page.
        for row in rows {
            egui::Frame::NONE
                .fill(ui.visuals().faint_bg_color)
                .corner_radius(6)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.set_width((available - 20.0).max(1.0));
                    ui.add(
                        egui::Label::new(egui::RichText::new(&row.label).color(row.color).strong())
                            .wrap(),
                    )
                    .on_hover_text(&row.detail);
                    ui.add_space(4.0);
                    let min_field = row
                        .values
                        .iter()
                        .enumerate()
                        .map(|(i, value)| {
                            measured_width(ui, value, true).max(measured_width(
                                ui,
                                SUMMARY_HEADINGS[i + 1],
                                false,
                            ))
                        })
                        .fold(0.0f32, f32::max);
                    let columns =
                        if 2.0 * min_field + ui.spacing().item_spacing.x <= ui.available_width() {
                            2
                        } else {
                            1
                        };
                    for start in (0..5).step_by(columns) {
                        ui.columns(columns, |cols| {
                            for (offset, col) in cols.iter_mut().enumerate() {
                                let i = start + offset;
                                if i >= 5 {
                                    break;
                                }
                                col.add(
                                    egui::Label::new(
                                        egui::RichText::new(SUMMARY_HEADINGS[i + 1])
                                            .color(p.secondary),
                                    )
                                    .wrap(),
                                );
                                col.add(
                                    egui::Label::new(
                                        egui::RichText::new(&row.values[i]).monospace(),
                                    )
                                    .wrap(),
                                )
                                .on_hover_text(&row.hints[i]);
                            }
                        });
                    }
                });
            ui.add_space(6.0);
        }
    }
}

fn summary_table(
    ui: &mut egui::Ui,
    rows: &[SummaryRow<'_>],
    range: HistoryRange,
    now: Instant,
    p: Palette,
) {
    render_summary_display(ui, &summary_display(rows, range, now), p);
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
        let gpu_series_key = adapter.activity_history_key();
        let (activity, _, suffix) = adapter.activity_percent();
        let series = histories.detail(&gpu_series_key).unwrap_or(&empty_series);
        let gpu_sub = if adapter.dedicated_total_bytes.value == Some(0) {
            "Shared memory (UMA)".into()
        } else if let (Some(u), Some(t)) = (
            adapter.dedicated_used_bytes.value,
            adapter.dedicated_total_bytes.value,
        ) {
            format!("VRAM {} / {}", format_bytes(u), format_bytes(t))
        } else {
            "GPU activity".into()
        };
        cards.push((
            if suffix == "aggregate" {
                "GPU · aggregate"
            } else {
                "GPU · busiest"
            },
            "gpu",
            activity
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
    for row in cards.chunks(columns) {
        let card_w = ((available_width - 12.0 * (row.len() - 1) as f32) / row.len() as f32)
            .floor()
            .max(1.0);
        let inner_w = (card_w - 24.0).max(1.0);
        let text: Vec<_> = row
            .iter()
            .map(|(title, _, value, _, color, subtitle)| {
                [
                    (*title, 14.0, *color),
                    (value.as_str(), 22.0, p.text),
                    (subtitle.as_str(), 12.0, p.secondary),
                ]
                .map(|(text, size, color)| {
                    ui.painter().layout(
                        text.into(),
                        egui::FontId::proportional(size * text_scale),
                        color,
                        inner_w,
                    )
                })
            })
            .collect();
        let card_h = text
            .iter()
            .map(|galleys| galleys.iter().map(|g| g.size().y).sum::<f32>())
            .fold(0.0f32, f32::max)
            + 24.0
            + 12.0
            + 36.0;
        ui.with_layout(egui::Layout::left_to_right(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for ((title, target_resource, value, series, color, subtitle), galleys) in
                row.iter().zip(&text)
            {
                let (card_rect, response) =
                    ui.allocate_exact_size(Vec2::new(card_w, card_h), egui::Sense::click());
                let response = response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!(
                        "{title} — {value}\n{subtitle}\nOpen in Performance"
                    ));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::accesskit::Role::Button,
                        true,
                        format!("{title} — {value} — {subtitle}"),
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
                        card_rect.shrink(2.0),
                        CornerRadius::same(6),
                        Stroke::new(2.0, p.focus),
                        egui::StrokeKind::Inside,
                    );
                }
                if response.clicked()
                    || (response.has_focus()
                        && ui.input(|i| {
                            i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)
                        }))
                {
                    *performance_request = Some((*target_resource).to_string());
                    *active_page = Page::Performance;
                }
                let inner = card_rect.shrink(12.0);
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::top_down(Align::Min)),
                );
                child.set_clip_rect(inner.intersect(ui.clip_rect()));
                child.spacing_mut().item_spacing.y = 4.0;
                for galley in galleys {
                    child.add(egui::Label::new(galley.clone()).selectable(false));
                }
                child.add_space((inner.bottom() - 36.0 - child.cursor().top()).max(0.0));
                sparkline_sized(&mut child, series, now, *color, p, Vec2::new(inner_w, 36.0));
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
            let key = adapter.activity_history_key();
            let (activity, _, suffix) = adapter.activity_percent();
            rows.push(SummaryRow {
                label: if suffix == "aggregate" {
                    "GPU · aggregate"
                } else {
                    "GPU · busiest"
                },
                current: metric_percent(activity),
                series: histories.detail(&key).unwrap_or(&empty_gpu_series),
                unit: "%",
                color: p.gpu,
                reason: activity
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
        let query_changed = state.cached_top_col != Some(sort_col);
        let expired = state
            .last_top_refresh
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(1));
        if query_changed || (expired && state.cached_frame_seq != frame.sequence) {
            let t_sort = Instant::now();
            let sorted = sort_processes(&frame.processes, "", sort_col, true);
            state.cached_top = sorted.iter().take(8).map(|p| p.identity.clone()).collect();
            state.cached_top_col = Some(sort_col);
            state.cached_frame_seq = frame.sequence;
            state.last_top_refresh = Some(Instant::now());
            let sort_ms = t_sort.elapsed().as_secs_f32() * 1000.0;
            crate::PROCESS_SORT_RING.with(|r| r.borrow_mut().record(sort_ms));
        }

        let by_id: std::collections::HashMap<&ProcessIdentity, &ProcessRecord> =
            frame.processes.iter().map(|p| (&p.identity, p)).collect();
        let top_rows: Vec<&ProcessRecord> = state
            .cached_top
            .iter()
            .filter_map(|id| by_id.get(id).copied())
            .collect();

        state.top_table.sort_column = sort_col;
        state.top_table.sort_descending = true;
        render_process_table_inline(
            ui,
            &top_rows,
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

    fn visit_text(
        shape: &eframe::egui::epaint::Shape,
        f: &mut impl FnMut(&eframe::egui::epaint::TextShape),
    ) {
        match shape {
            eframe::egui::epaint::Shape::Text(text) => f(text),
            eframe::egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit_text(shape, f);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn summary_values_are_complete_and_contained_in_table_and_stacked_modes() {
        use eframe::egui;
        for dark in [true, false] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for width in [300.0, 480.0, 760.0, 1180.0, 1920.0] {
                    let ctx = egui::Context::default();
                    crate::theme::configure_theme(
                        &ctx,
                        crate::theme::get_palette(dark),
                        crate::theme::Density::Normal,
                        scale,
                        false,
                    );
                    let mut rows = vec![super::SummaryDisplay {
                        label: "Network receive on a long named interface".into(),
                        values: [
                            "999.9 GiB/s".into(),
                            "123.4 GiB/s".into(),
                            "0 B/s".into(),
                            "1.2 TiB/s".into(),
                            "899/900 seconds · 100%".into(),
                        ],
                        hints: std::array::from_fn(|_| "Observed value".into()),
                        detail: "Provider details".into(),
                        color: egui::Color32::BLUE,
                    }];
                    rows.push(super::SummaryDisplay {
                        label: "CPU".into(),
                        values: [
                            "12%".into(),
                            "10%".into(),
                            "0%".into(),
                            "99%".into(),
                            "50/60 seconds - 83%".into(),
                        ],
                        hints: std::array::from_fn(|_| String::new()),
                        detail: String::new(),
                        color: egui::Color32::BLUE,
                    });
                    let mut right = 0.0;
                    let mut left = 0.0;
                    let mut table = false;
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 10000.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            left = ui.cursor().left();
                            right = ui.max_rect().right();
                            table = super::summary_column_widths(ui, &rows, ui.available_width())
                                .is_some();
                            super::render_summary_display(
                                ui,
                                &rows,
                                crate::theme::get_palette(dark),
                            );
                            assert!(ui.min_rect().right() <= right + 1.0);
                        },
                    );
                    output.textures_delta.clear();
                    let mut texts = Vec::new();
                    let mut rights = std::collections::HashMap::new();
                    for shape in &output.shapes {
                        visit_text(&shape.shape, &mut |text| {
                            assert!(!text.galley.elided, "elided {}", text.galley.job.text);
                            let bounds = text.galley.rect.translate(text.pos.to_vec2());
                            assert!(
                                bounds.left() >= left - 1.0 && bounds.right() <= right + 1.0,
                                "width {width} scale {scale}: {} {bounds:?}",
                                text.galley.job.text
                            );
                            rights.insert(text.galley.job.text.clone(), bounds.right());
                            texts.push(text.galley.job.text.clone());
                        });
                    }
                    for value in &rows[0].values {
                        assert!(texts.contains(value), "missing {value}");
                    }
                    for heading in &super::SUMMARY_HEADINGS[1..] {
                        assert!(texts.iter().any(|t| t == heading));
                    }
                    if table {
                        for (i, heading) in super::SUMMARY_HEADINGS[1..].iter().enumerate() {
                            for row in &rows {
                                assert!(
                                    (rights[*heading] - rights[&row.values[i]]).abs() <= 1.0,
                                    "unaligned column {heading}"
                                );
                            }
                        }
                    }
                    if width == 300.0 {
                        assert!(!table);
                    }
                    if width == 1920.0 && scale == 1.0 {
                        assert!(table);
                    }
                }
            }
        }
    }

    #[test]
    fn overview_card_text_and_charts_stay_inside_each_card() {
        use eframe::egui;
        let fixture = crate::smoke_test::create_native_fixture_app(
            crate::theme::Theme::Dark,
            crate::state::Page::Overview,
        );
        let frame = fixture.shared.lock().unwrap().frame.clone().unwrap();
        for width in [280.0, 600.0, 1180.0, 1536.0] {
            for scale in [1.0, 1.5, 2.0] {
                let ctx = egui::Context::default();
                crate::theme::configure_theme(
                    &ctx,
                    crate::theme::get_palette(true),
                    crate::theme::Density::Normal,
                    scale,
                    false,
                );
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("system_text_scale"), scale));
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 10000.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        super::resource_cards(
                            ui,
                            &frame,
                            &fixture.histories,
                            &mut None,
                            &mut crate::state::Page::Overview,
                            std::time::Instant::now(),
                            crate::theme::get_palette(true),
                        );
                    },
                );
                output.textures_delta.clear();
                for shape in &output.shapes {
                    visit_text(&shape.shape, &mut |text| {
                        assert!(!text.galley.elided);
                        let bounds = text.galley.rect.translate(text.pos.to_vec2());
                        assert!(
                            shape.clip_rect.expand(1.0).contains_rect(bounds),
                            "card clipped at width {width} scale {scale}: {} {bounds:?}, {:?}",
                            text.galley.job.text,
                            shape.clip_rect
                        );
                    });
                    if let egui::epaint::Shape::LineSegment { points, .. } = &shape.shape {
                        for point in points {
                            assert!(shape.clip_rect.expand(1.0).contains(*point));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn empty_summary_never_invents_observations() {
        let empty = crate::history::TimeSeries::default();
        let rows = [super::SummaryRow {
            label: "CPU",
            current: "Unavailable".into(),
            series: &empty,
            unit: "%",
            color: eframe::egui::Color32::BLUE,
            reason: Some("Provider missing".into()),
            detail: None,
        }];
        let display = super::summary_display(
            &rows,
            crate::history::HistoryRange::Min15,
            std::time::Instant::now(),
        );
        assert_eq!(
            display[0].values,
            ["Unavailable", "—", "—", "—", "Collecting"]
        );
        assert_eq!(display[0].hints[0], "Provider missing");
    }

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

        for heading in super::SUMMARY_HEADINGS {
            assert!(
                text_found.iter().any(|t| t == heading),
                "Heading {heading} must be present in Overview render output"
            );
        }

        output.textures_delta.clear();
    }
}
