use crate::{
    components::{
        header::render_page_header,
        plot::{PlotSeries, PlotState, capacity_bar, sparkline, time_plot},
    },
    export::ExportManager,
    formatting::{format_bytes, format_percent, format_rate},
    history::{HistoryRange, ResourceHistories, TimeSeries},
    state::{AppState, DisplayPause},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Rect, Stroke, Vec2};
use planora_core::{SensorReading, TelemetryFrame, Topology};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SensorTrendKind {
    #[default]
    Power,
    Temperature,
    Clock,
    Fan,
    Load,
}

impl SensorTrendKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Power => "Power (W)",
            Self::Temperature => "Temperatures (°C)",
            Self::Clock => "Clocks (MHz)",
            Self::Fan => "Fans (RPM)",
            Self::Load => "Load (%)",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Self::Power => "W",
            Self::Temperature => "°C",
            Self::Clock => "MHz",
            Self::Fan => "RPM",
            Self::Load => "%",
        }
    }

    pub fn matches_sensor(self, reading: &SensorReading) -> bool {
        match self {
            Self::Power => {
                matches!(
                    reading.sensor_type,
                    planora_core::SensorType::CpuPackagePowerWatts
                        | planora_core::SensorType::GpuChipPowerWatts
                        | planora_core::SensorType::GpuBoardPowerWatts
                ) || reading.value.unit == "W"
            }
            Self::Temperature => {
                matches!(
                    reading.sensor_type,
                    planora_core::SensorType::CpuTemperatureCelsius
                        | planora_core::SensorType::GpuTemperatureCelsius
                        | planora_core::SensorType::ThermalZoneCelsius
                        | planora_core::SensorType::StorageTemperatureCelsius
                ) || reading.value.unit == "°C"
            }
            Self::Clock => {
                matches!(reading.sensor_type, planora_core::SensorType::ClockMhz)
                    || reading.value.unit == "MHz"
            }
            Self::Fan => {
                matches!(reading.sensor_type, planora_core::SensorType::FanRpm)
                    || reading.value.unit == "RPM"
            }
            Self::Load => {
                matches!(reading.sensor_type, planora_core::SensorType::LoadPercent)
                    || reading.value.unit == "%"
            }
        }
    }
}

pub struct ResourcesViewState {
    pub active_tab: usize, // 0: CPU, 1: Memory, 2: GPU, 3: Storage, 4: Network, 5: Sensors
    pub search_query: String,
    pub selected_resource: Option<String>,
    pub range: HistoryRange,
    pub selected_cpu: Option<(u16, u16)>,
    pub selected_engines: BTreeSet<String>,
    pub small_multiples: bool,
    pub show_provider_details: bool,
    pub export_message: Option<(Instant, String)>,
    pub selected_sensor_trend: SensorTrendKind,
    plots: BTreeMap<String, PlotState>,
}

impl Default for ResourcesViewState {
    fn default() -> Self {
        Self {
            active_tab: 0,
            search_query: String::new(),
            selected_resource: None,
            range: HistoryRange::Sec60,
            selected_cpu: None,
            selected_engines: BTreeSet::new(),
            small_multiples: false,
            show_provider_details: false,
            export_message: None,
            selected_sensor_trend: SensorTrendKind::default(),
            plots: BTreeMap::new(),
        }
    }
}

pub struct Choice {
    pub id: String,
    pub label: String,
    pub value: String,
    pub status_text: String,
    pub status_symbol: &'static str,
    pub status_color: egui::Color32,
    pub category: usize,
    pub color: egui::Color32,
    pub series_key: String,
    pub source: String,
    pub unavailable_reason: Option<String>,
}

pub fn choices(frame: &TelemetryFrame, sensors: &[SensorReading], p: Palette) -> Vec<Choice> {
    let mut rows = vec![
        Choice {
            id: "cpu".into(),
            label: "CPU".into(),
            value: format_percent(frame.cpu_percent.value),
            status_text: if frame.cpu_percent.value.is_some() {
                "Active".into()
            } else {
                frame
                    .cpu_percent
                    .unavailable_reason
                    .as_ref()
                    .map_or_else(|| "Unavailable".into(), ToString::to_string)
            },
            status_symbol: if frame.cpu_percent.value.is_some() {
                "●"
            } else {
                "—"
            },
            status_color: if frame.cpu_percent.value.is_some() {
                p.success
            } else {
                p.warning
            },
            category: 0,
            color: p.cpu,
            series_key: "cpu".into(),
            source: frame.cpu_percent.source.clone(),
            unavailable_reason: frame
                .cpu_percent
                .unavailable_reason
                .as_ref()
                .map(ToString::to_string),
        },
        Choice {
            id: "memory".into(),
            label: "Memory".into(),
            value: frame
                .memory_used_bytes
                .value
                .map_or_else(|| "Unavailable".into(), format_bytes),
            status_text: if frame.memory_used_bytes.value.is_some() {
                "Active".into()
            } else {
                frame
                    .memory_used_bytes
                    .unavailable_reason
                    .as_ref()
                    .map_or_else(|| "Unavailable".into(), ToString::to_string)
            },
            status_symbol: if frame.memory_used_bytes.value.is_some() {
                "●"
            } else {
                "—"
            },
            status_color: if frame.memory_used_bytes.value.is_some() {
                p.success
            } else {
                p.warning
            },
            category: 1,
            color: p.memory,
            series_key: "memory".into(),
            source: frame.memory_used_bytes.source.clone(),
            unavailable_reason: frame
                .memory_used_bytes
                .unavailable_reason
                .as_ref()
                .map(ToString::to_string),
        },
    ];

    let mut adapters: Vec<_> = frame.gpu_adapters.iter().collect();
    adapters.sort_by_key(|adapter| (adapter.adapter_luid, adapter.physical_adapter));
    for adapter in adapters {
        let is_ok = adapter.busiest_engine_percent.value.is_some();
        rows.push(Choice {
            id: format!(
                "gpu:{}:{:?}",
                adapter.adapter_luid, adapter.physical_adapter
            ),
            label: adapter.name.clone(),
            value: format_percent(adapter.busiest_engine_percent.value),
            status_text: if is_ok {
                "Active".into()
            } else {
                "Unavailable".into()
            },
            status_symbol: if is_ok { "●" } else { "—" },
            status_color: if is_ok { p.success } else { p.warning },
            category: 2,
            color: p.gpu,
            series_key: format!(
                "gpu:{}:{:?}:busiest",
                adapter.adapter_luid, adapter.physical_adapter
            ),
            source: adapter.busiest_engine_percent.source.clone(),
            unavailable_reason: adapter
                .busiest_engine_percent
                .unavailable_reason
                .as_ref()
                .map(ToString::to_string),
        });
    }
    if frame.gpu_adapters.is_empty() {
        rows.push(Choice {
            id: "gpu:none".into(),
            label: "GPU".into(),
            value: "Unavailable".into(),
            status_text: "Not discovered".into(),
            status_symbol: "—",
            status_color: p.secondary,
            category: 2,
            color: p.gpu,
            series_key: "gpu:none".into(),
            source: "DXGI".into(),
            unavailable_reason: Some("No discrete or integrated GPU adapters discovered".into()),
        });
    }

    let mut disks: Vec<_> = frame.disks.iter().collect();
    disks.sort_by(|a, b| a.id.cmp(&b.id));
    for disk in disks {
        let is_ok = disk.read_bytes_per_sec.value.is_some();
        rows.push(Choice {
            id: format!("disk:{}", disk.id),
            label: disk.label.clone(),
            value: format_rate(disk.read_bytes_per_sec.value.map(|v| v as f32)),
            status_text: if is_ok {
                "Active".into()
            } else {
                "Unavailable".into()
            },
            status_symbol: if is_ok { "●" } else { "—" },
            status_color: if is_ok { p.success } else { p.warning },
            category: 3,
            color: p.disk,
            series_key: format!("disk:{}:read", disk.id),
            source: disk.read_bytes_per_sec.source.clone(),
            unavailable_reason: disk
                .read_bytes_per_sec
                .unavailable_reason
                .as_ref()
                .map(ToString::to_string),
        });
    }
    if frame.disks.is_empty() {
        rows.push(Choice {
            id: "disk:none".into(),
            label: "Storage".into(),
            value: "Unavailable".into(),
            status_text: "Not discovered".into(),
            status_symbol: "—",
            status_color: p.secondary,
            category: 3,
            color: p.disk,
            series_key: "disk:none".into(),
            source: "Storage".into(),
            unavailable_reason: Some("No physical disk devices discovered".into()),
        });
    }

    let mut interfaces: Vec<_> = frame.network_interfaces.iter().collect();
    interfaces.sort_by(|a, b| a.id.cmp(&b.id));
    for iface in interfaces {
        let is_connected = iface.link_state.as_deref() != Some("Disconnected");
        rows.push(Choice {
            id: format!("net:{}", iface.id),
            label: iface.label.clone(),
            value: format_rate(iface.read_bytes_per_sec.value.map(|v| v as f32)),
            status_text: iface.link_state.as_deref().unwrap_or("Connected").into(),
            status_symbol: if is_connected { "●" } else { "○" },
            status_color: if is_connected { p.success } else { p.error },
            category: 4,
            color: p.network,
            series_key: format!("net:{}:rx", iface.id),
            source: iface.read_bytes_per_sec.source.clone(),
            unavailable_reason: iface
                .read_bytes_per_sec
                .unavailable_reason
                .as_ref()
                .map(ToString::to_string),
        });
    }
    if frame.network_interfaces.is_empty() {
        rows.push(Choice {
            id: "net:none".into(),
            label: "Network".into(),
            value: "Unavailable".into(),
            status_text: "Not discovered".into(),
            status_symbol: "—",
            status_color: p.secondary,
            category: 4,
            color: p.network,
            series_key: "net:none".into(),
            source: "Network".into(),
            unavailable_reason: Some("No network adapters reported".into()),
        });
    }

    let mut sensor_rows: Vec<_> = sensors.iter().collect();
    sensor_rows
        .sort_by(|a, b| (&a.scope, &a.provider, &a.name).cmp(&(&b.scope, &b.provider, &b.name)));
    for sensor in sensor_rows {
        let age_ms = planora_telemetry::unix_ms().saturating_sub(sensor.value.sampled_at_ms);
        let is_stale = age_ms > 5_000;
        let is_ok = sensor.value.value.is_some();
        let (status_text, status_symbol, status_color) = if is_stale {
            (format!("Stale ({}s)", age_ms / 1000), "⚠", p.warning)
        } else if is_ok {
            ("Active".into(), "●", p.success)
        } else {
            ("Unavailable".into(), "—", p.secondary)
        };
        let id = format!(
            "sensor:{}:{}:{}:{:?}",
            sensor.scope, sensor.provider, sensor.name, sensor.sensor_type
        );
        rows.push(Choice {
            id: id.clone(),
            label: format!("{} ({})", sensor.name, sensor.scope),
            value: sensor.value.value.map_or_else(
                || "Unavailable".into(),
                |v| format!("{v:.1} {}", sensor.value.unit),
            ),
            status_text,
            status_symbol,
            status_color,
            category: 5,
            color: p.accent,
            series_key: id,
            source: sensor.provider.clone(),
            unavailable_reason: sensor
                .value
                .unavailable_reason
                .as_ref()
                .map(ToString::to_string),
        });
    }
    if sensors.is_empty() {
        rows.push(Choice {
            id: "sensor:none".into(),
            label: "Sensors".into(),
            value: "Unavailable".into(),
            status_text: "No sensors".into(),
            status_symbol: "—",
            status_color: p.secondary,
            category: 5,
            color: p.accent,
            series_key: "sensor:none".into(),
            source: "Sensors".into(),
            unavailable_reason: Some("No active vendor hardware sensors reported".into()),
        });
    }
    rows
}

pub fn render_resources_view(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    sensors: &[SensorReading],
    topology: Option<&Topology>,
    mut exporter: Option<&mut ExportManager>,
    state: &mut ResourcesViewState,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    graph_modes: &mut [bool; 6],
    legends: &mut [bool; 6],
    now: Instant,
    p: Palette,
) {
    render_page_header(
        ui,
        "Performance",
        "Live resource trends, exact values, and device details",
        app_state,
        pause_state,
        poll_interval_ms,
        p,
    );

    let mut all = choices(frame, sensors, p);
    for item in &mut all {
        if item.unavailable_reason.is_some() {
            continue;
        }
        let state_label = if pause_state.is_paused {
            Some("Display paused")
        } else {
            match app_state {
                AppState::Replay => Some("Replay"),
                AppState::Stale(_) => Some("Stale"),
                AppState::Reconnecting => Some("Reconnecting"),
                AppState::Live => None,
                _ => Some("Disconnected"),
            }
        };
        if let Some(label) = state_label {
            item.status_text = label.into();
            item.status_color = if *app_state == AppState::Replay {
                p.accent
            } else {
                p.warning
            };
            item.status_symbol = "●";
        }
    }
    if state
        .selected_resource
        .as_ref()
        .is_none_or(|id| !all.iter().any(|item| &item.id == id))
    {
        // The native fixture CLI accepts category names while device IDs remain
        // stable and specific (for example, gpu:<LUID>:<physical adapter>).
        let requested_prefix = match state.selected_resource.as_deref() {
            Some("gpu") => Some("gpu:"),
            Some("disk" | "storage") => Some("disk:"),
            Some("network" | "net") => Some("net:"),
            Some("sensor" | "sensors") => Some("sensor:"),
            _ => None,
        };
        state.selected_resource = requested_prefix
            .and_then(|prefix| all.iter().find(|item| item.id.starts_with(prefix)))
            .or_else(|| all.first())
            .map(|item| item.id.clone());
    }

    // Dismiss export message after 5 seconds
    if state
        .export_message
        .as_ref()
        .is_some_and(|(when, _)| when.elapsed() > Duration::from_secs(5))
    {
        state.export_message = None;
    }

    let wide = ui.available_width() >= 1080.0;
    if wide {
        let content_width = ui.available_width() - 256.0;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(240.0);
                ui.heading("Resources");
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut state.search_query)
                        .hint_text("Filter resources...")
                        .desired_width(230.0),
                );
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt("performance_resource_selector")
                    .max_height(720.0)
                    .show(ui, |ui| {
                        for item in &all {
                            if !item
                                .label
                                .to_lowercase()
                                .contains(&state.search_query.to_lowercase())
                            {
                                continue;
                            }
                            let selected = state.selected_resource.as_ref() == Some(&item.id);
                            egui::Frame::group(ui.style())
                                .fill(if selected { p.selected } else { p.surface })
                                .stroke(Stroke::new(
                                    if selected { 1.5 } else { 1.0 },
                                    if selected { item.color } else { p.border },
                                ))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.set_width(216.0);
                                    ui.horizontal(|ui| {
                                        ui.colored_label(item.color, "▌");
                                        let resp = ui.selectable_label(selected, &item.label);
                                        if resp.clicked() {
                                            state.selected_resource = Some(item.id.clone());
                                        }
                                        resp.on_hover_text(&item.label);
                                    });
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label(
                                                egui::RichText::new(&item.value)
                                                    .monospace()
                                                    .size(15.0)
                                                    .strong()
                                                    .color(item.color),
                                            );
                                            ui.horizontal(|ui| {
                                                ui.colored_label(
                                                    item.status_color,
                                                    item.status_symbol,
                                                );
                                                ui.label(
                                                    egui::RichText::new(&item.status_text)
                                                        .size(11.0)
                                                        .color(p.secondary),
                                                );
                                            });
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if let Some(series) = choice_series(item, histories)
                                                {
                                                    sparkline(ui, series, now, item.color, p);
                                                }
                                            },
                                        );
                                    });
                                });
                            ui.add_space(6.0);
                        }
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(content_width.max(260.0));
                if let Some(item) = all
                    .iter()
                    .find(|item| state.selected_resource.as_ref() == Some(&item.id))
                {
                    render_selected(
                        ui,
                        frame,
                        histories,
                        sensors,
                        topology,
                        exporter.as_deref_mut(),
                        state,
                        app_state,
                        pause_state,
                        poll_interval_ms,
                        item,
                        graph_modes,
                        legends,
                        now,
                        p,
                    );
                }
            });
        });
    } else {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Selected resource:").strong());
            egui::ComboBox::from_id_salt("performance_resource_picker")
                .selected_text(
                    all.iter()
                        .find(|item| state.selected_resource.as_ref() == Some(&item.id))
                        .map_or("CPU", |item| item.label.as_str()),
                )
                .show_ui(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.search_query)
                            .hint_text("Find a resource"),
                    );
                    for item in &all {
                        if !item
                            .label
                            .to_lowercase()
                            .contains(&state.search_query.to_lowercase())
                        {
                            continue;
                        }
                        if ui
                            .selectable_label(
                                state.selected_resource.as_ref() == Some(&item.id),
                                format!("{} · {}", item.label, item.value),
                            )
                            .clicked()
                        {
                            state.selected_resource = Some(item.id.clone());
                        }
                    }
                });
        });
        ui.add_space(8.0);
        if let Some(item) = all
            .iter()
            .find(|item| state.selected_resource.as_ref() == Some(&item.id))
        {
            render_selected(
                ui,
                frame,
                histories,
                sensors,
                topology,
                exporter,
                state,
                app_state,
                pause_state,
                poll_interval_ms,
                item,
                graph_modes,
                legends,
                now,
                p,
            );
        }
    }
}

fn choice_series<'a>(item: &Choice, histories: &'a ResourceHistories) -> Option<&'a TimeSeries> {
    match item.id.as_str() {
        "cpu" => Some(&histories.cpu),
        "memory" => Some(&histories.memory_pct),
        _ => histories.detail(&item.series_key),
    }
}

fn format_summary_value(value: f32, unit: &str) -> String {
    match unit {
        "B/s" => format_rate(Some(value)),
        "bytes" => format_bytes(value.max(0.0) as u64),
        _ => format!("{value:.1} {unit}"),
    }
}

fn cpu_heatmap(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    topology: Option<&Topology>,
    state: &mut ResourcesViewState,
    p: Palette,
) {
    if frame.logical_cpus.is_empty() {
        return;
    }
    ui.heading("Logical processor heatmap");
    ui.label(
        egui::RichText::new(
            "Select a logical processor to inspect its observed trend. Windows processor-group addressing is preserved.",
        )
        .color(p.secondary)
        .size(12.0),
    );
    let mut cpus: Vec<_> = frame.logical_cpus.iter().collect();
    cpus.sort_by_key(|cpu| (cpu.group, cpu.cpu_index));
    egui::ScrollArea::horizontal()
        .id_salt("performance_cpu_heatmap")
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(ui.available_width().max(280.0));
                for cpu in cpus {
                    let identity = (cpu.group, cpu.cpu_index);
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(48.0, 48.0), egui::Sense::click());
                    let level = cpu
                        .utilization_percent
                        .value
                        .unwrap_or(0.0)
                        .clamp(0.0, 100.0)
                        / 100.0;
                    let fill = if cpu.utilization_percent.value.is_some() {
                        p.cpu.gamma_multiply(0.20 + 0.80 * level)
                    } else {
                        p.surface_raised
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(6),
                        Stroke::new(
                            if state.selected_cpu == Some(identity) {
                                2.0
                            } else {
                                1.0
                            },
                            if state.selected_cpu == Some(identity) {
                                p.focus
                            } else {
                                p.border
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center() - Vec2::new(0.0, 6.0),
                        egui::Align2::CENTER_CENTER,
                        cpu.cpu_index.to_string(),
                        egui::FontId::monospace(12.0),
                        p.text,
                    );
                    let pct_str = cpu
                        .utilization_percent
                        .value
                        .map_or_else(|| "—".into(), |v| format!("{v:.0}%"));
                    ui.painter().text(
                        rect.center() + Vec2::new(0.0, 8.0),
                        egui::Align2::CENTER_CENTER,
                        pct_str,
                        egui::FontId::monospace(10.0),
                        p.secondary,
                    );
                    if response.clicked() {
                        state.selected_cpu = Some(identity);
                    }
                    let core_class = topology.and_then(|t| {
                        t.cores
                            .iter()
                            .find(|c| c.logical_cpus.contains(&cpu.cpu_index))
                            .and_then(|c| c.class.as_deref())
                    });
                    let hover_info = format!(
                        "Group {} · Logical CPU {}{}\nUtilization: {}\nSource: {}",
                        cpu.group,
                        cpu.cpu_index,
                        core_class.map_or_else(String::new, |cls| format!(" ({cls} core)")),
                        format_percent(cpu.utilization_percent.value),
                        cpu.utilization_percent.source
                    );
                    response.on_hover_text(hover_info);
                }
            });
        });
}

fn render_selected(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    sensors: &[SensorReading],
    topology: Option<&Topology>,
    mut exporter: Option<&mut ExportManager>,
    state: &mut ResourcesViewState,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    item: &Choice,
    graph_modes: &mut [bool; 6],
    legends: &mut [bool; 6],
    now: Instant,
    p: Palette,
) {
    state.active_tab = item.category;

    // 1. Tinted Resource Header with Prominent Value and Status Badges
    egui::Frame::group(ui.style())
        .fill(p.surface_raised)
        .stroke(Stroke::new(1.0, item.color))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(&item.label)
                            .size(22.0)
                            .strong()
                            .color(p.text),
                    );
                    ui.label(
                        egui::RichText::new(&item.value)
                            .size(38.0)
                            .strong()
                            .monospace()
                            .color(item.color),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (badge_text, badge_color) = if pause_state.is_paused {
                        ("Paused", p.warning)
                    } else if *app_state == AppState::Replay {
                        ("Replay", p.accent)
                    } else if matches!(app_state, AppState::Stale(_)) {
                        ("Stale", p.warning)
                    } else if *app_state == AppState::Reconnecting {
                        ("Reconnecting", p.warning)
                    } else if *app_state != AppState::Live {
                        ("Disconnected", p.warning)
                    } else {
                        ("● Live", p.success)
                    };
                    egui::Frame::new()
                        .fill(badge_color.gamma_multiply(0.15))
                        .stroke(Stroke::new(1.0, badge_color))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(badge_text)
                                    .size(12.0)
                                    .strong()
                                    .color(badge_color),
                            );
                        });
                });
            });
        });

    ui.add_space(10.0);

    // 2. Control Toolbar: Mode selector, Time range, Legend, Small-multiples, Details toggle, CSV export
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut graph_modes[item.category], true, "Graphs + details");
        ui.selectable_value(&mut graph_modes[item.category], false, "Tables only");
        ui.separator();

        for range in [
            HistoryRange::Sec30,
            HistoryRange::Sec60,
            HistoryRange::Min5,
            HistoryRange::Min15,
        ] {
            ui.selectable_value(&mut state.range, range, range.label());
        }

        if graph_modes[item.category] {
            ui.separator();
            ui.checkbox(&mut legends[item.category], "Legend");
        }

        if item.category == 0 {
            ui.separator();
            ui.checkbox(&mut state.small_multiples, "Small-multiple core charts");
        }

        ui.separator();
        ui.toggle_value(&mut state.show_provider_details, "Details");

        let empty = TimeSeries::default();
        let primary = choice_series(item, histories).unwrap_or(&empty);
        let secondary_key = match item.category {
            3 => Some(item.series_key.replace(":read", ":write")),
            4 => Some(item.series_key.replace(":rx", ":tx")),
            _ => None,
        };
        let secondary = secondary_key
            .as_deref()
            .and_then(|key| histories.detail(key));

        let (unit, _fixed_max) = match item.category {
            0..=2 => ("%", Some(100.0)),
            3 | 4 => ("B/s", None),
            _ => (
                sensors
                    .iter()
                    .find(|s| {
                        format!(
                            "sensor:{}:{}:{}:{:?}",
                            s.scope, s.provider, s.name, s.sensor_type
                        ) == item.id
                    })
                    .map_or("Reading", |s| s.value.unit.as_str()),
                None,
            ),
        };
        let primary_name = if item.category == 3 {
            "Read"
        } else if item.category == 4 {
            "Receive"
        } else {
            item.label.as_str()
        };

        if let Some(exp) = exporter.as_deref_mut() {
            ui.separator();
            if ui
                .button("Export CSV")
                .on_hover_text("Export this resource's visible samples to CSV")
                .clicked()
            {
                let mut export_series =
                    vec![(primary_name.to_string(), primary.clone(), unit.to_string())];
                if let Some(sec) = secondary {
                    let sec_name = if item.category == 3 {
                        "Write"
                    } else {
                        "Transmit"
                    };
                    export_series.push((sec_name.to_string(), sec.clone(), unit.to_string()));
                }
                if item.category == 0
                    && let Some((group, index)) = state.selected_cpu
                    && let Some(series) = histories.detail(&format!("cpu:{group}:{index}"))
                {
                    export_series.push((
                        format!("Group {group} CPU {index}"),
                        series.clone(),
                        "%".into(),
                    ));
                }
                if item.category == 2 {
                    for key in state.selected_engines.iter().take(4) {
                        if let Some(series) = histories.detail(key) {
                            export_series.push((key.clone(), series.clone(), "%".into()));
                        }
                    }
                }
                if item.category == 3 {
                    for (name, unit) in [("latency", "ms"), ("queue", "depth")] {
                        if let Some(series) = histories.detail(&format!(
                            "disk:{}:{name}",
                            item.id.trim_start_matches("disk:")
                        )) {
                            export_series.push((name.into(), series.clone(), unit.into()));
                        }
                    }
                }
                match exp.resource_series(item.label.clone(), export_series, state.range, now) {
                    Ok(()) => {
                        state.export_message =
                            Some((Instant::now(), format!("Export started for {}", item.label)));
                    }
                    Err(e) => {
                        state.export_message =
                            Some((Instant::now(), format!("Export failed: {e}")));
                    }
                }
            }
        }
    });

    if let Some((_, msg)) = &state.export_message {
        ui.add_space(4.0);
        ui.colored_label(p.focus, msg);
    }

    if state.show_provider_details {
        ui.add_space(6.0);
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.focus))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.strong("Provider & Telemetry Details");
                ui.label(format!("• Source Provider: {}", item.source));
                ui.label(format!("• Collection Interval: {} ms", poll_interval_ms));
                if let Some(reason) = &item.unavailable_reason {
                    ui.colored_label(p.warning, format!("• Availability Note: {reason}"));
                } else {
                    ui.label("• Availability: Normal telemetry stream active");
                }
                ui.label(format!(
                    "• History Aggregation: 1-second buckets up to 15 minutes (Visible window: {})",
                    state.range.label()
                ));
            });
    }

    ui.add_space(8.0);

    let empty = TimeSeries::default();
    let primary = choice_series(item, histories).unwrap_or(&empty);
    let secondary_key = match item.category {
        3 => Some(item.series_key.replace(":read", ":write")),
        4 => Some(item.series_key.replace(":rx", ":tx")),
        _ => None,
    };
    let secondary = secondary_key
        .as_deref()
        .and_then(|key| histories.detail(key));
    let (unit, fixed_max) = match item.category {
        0..=2 => ("%", Some(100.0)),
        3 | 4 => ("B/s", None),
        _ => (
            sensors
                .iter()
                .find(|s| {
                    format!(
                        "sensor:{}:{}:{}:{:?}",
                        s.scope, s.provider, s.name, s.sensor_type
                    ) == item.id
                })
                .map_or("Reading", |s| s.value.unit.as_str()),
            None,
        ),
    };
    let primary_name = if item.category == 3 {
        "Read"
    } else if item.category == 4 {
        "Receive"
    } else {
        item.label.as_str()
    };

    // 3. Summary Metrics Bar (Prominent in both modes)
    if let Some(summary) = primary.observed_summary(now, state.range) {
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [
                ("Average", summary.average),
                ("Minimum", summary.minimum),
                ("Maximum", summary.maximum),
            ] {
                egui::Frame::new()
                    .fill(p.surface_raised)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(10, 5))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{label}: {}",
                                format_summary_value(value, unit)
                            ))
                            .strong()
                            .color(p.text),
                        );
                    });
            }
            egui::Frame::new()
                .fill(p.surface_raised)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Coverage: {}/{} s ({:.0}%)",
                            summary.observed_seconds,
                            summary.expected_seconds,
                            summary.coverage_percent()
                        ))
                        .color(p.secondary),
                    );
                });
        });
    } else {
        let status = if histories.not_retained.contains(&item.series_key) {
            "History not retained — the bounded history budget was reached. Current readings remain available."
        } else {
            "Collecting history — unobserved time remains blank."
        };
        ui.label(egui::RichText::new(status).color(p.secondary));
    }

    // 4. Large Interactive Chart (in Graphs + details mode)
    if graph_modes[item.category] {
        ui.add_space(8.0);
        let mut plot_series = vec![PlotSeries {
            name: primary_name,
            samples: primary,
            color: item.color,
            dashed: false,
        }];
        if let Some(sec) = secondary {
            plot_series.push(PlotSeries {
                name: if item.category == 3 {
                    "Write"
                } else {
                    "Transmit"
                },
                samples: sec,
                color: p.accent,
                dashed: true,
            });
        }
        if legends[item.category] {
            ui.horizontal(|ui| {
                for plotted in &plot_series {
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
        }
        let plot = state.plots.entry(item.id.clone()).or_default();
        let chart_h = (ui.available_height() * 0.42).clamp(240.0, 360.0);
        time_plot(
            ui,
            &item.id,
            &plot_series,
            state.range,
            now,
            unit,
            fixed_max,
            chart_h,
            plot,
            p,
        );

        if item.category == 0 {
            ui.add_space(10.0);
            if state.small_multiples {
                ui.heading("Logical processor core charts");
                ui.label(
                    egui::RichText::new(
                        "Compact 60-second utilization sparklines for all logical processors.",
                    )
                    .color(p.secondary)
                    .size(12.0),
                );
                let mut cpus: Vec<_> = frame.logical_cpus.iter().collect();
                cpus.sort_by_key(|c| (c.group, c.cpu_index));
                let cols = (ui.available_width() / 150.0).floor().max(1.0) as usize;
                for chunk in cpus.chunks(cols) {
                    ui.horizontal(|ui| {
                        for cpu in chunk {
                            let key = format!("cpu:{}:{}", cpu.group, cpu.cpu_index);
                            let series_opt = histories.detail(&key);
                            egui::Frame::group(ui.style())
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0, p.border))
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(egui::Margin::same(8))
                                .show(ui, |ui| {
                                    ui.set_width(130.0);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("CPU {}", cpu.cpu_index))
                                                .strong()
                                                .size(11.0),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(format_percent(
                                                        cpu.utilization_percent.value,
                                                    ))
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(p.cpu),
                                                );
                                            },
                                        );
                                    });
                                    if let Some(series) = series_opt {
                                        sparkline(ui, series, now, p.cpu, p);
                                    } else {
                                        let (rect, _) = ui.allocate_exact_size(
                                            Vec2::new(130.0, 28.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(rect, 3.0, p.surface_raised);
                                    }
                                });
                        }
                    });
                    ui.add_space(4.0);
                }
            } else {
                cpu_heatmap(ui, frame, topology, state, p);
                if let Some((group, index)) = state.selected_cpu {
                    let key = format!("cpu:{group}:{index}");
                    if let Some(series) = histories.detail(&key) {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(format!("Group {group} · Logical CPU {index}"))
                                .strong(),
                        );
                        let plot = state.plots.entry(key.clone()).or_default();
                        time_plot(
                            ui,
                            &key,
                            &[PlotSeries {
                                name: "Logical processor",
                                samples: series,
                                color: p.cpu,
                                dashed: false,
                            }],
                            state.range,
                            now,
                            "%",
                            Some(100.0),
                            180.0,
                            plot,
                            p,
                        );
                    }
                }
            }
        }

        if item.category == 1 {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Physical memory allocation:").strong());
            capacity_bar(
                ui,
                frame.memory_used_bytes.value,
                frame.memory_total_bytes.value,
                p.memory,
                p,
            );
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Windows commit charge budget:").strong());
            capacity_bar(
                ui,
                frame.commit_used_bytes.value,
                frame.commit_total_bytes.value,
                p.primary,
                p,
            );
        }

        if item.category == 2
            && let Some(adapter) = frame.gpu_adapters.iter().find(|adapter| {
                item.id
                    == format!(
                        "gpu:{}:{:?}",
                        adapter.adapter_luid, adapter.physical_adapter
                    )
            })
        {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Dedicated GPU memory:").strong());
            capacity_bar(
                ui,
                adapter.dedicated_used_bytes.value,
                adapter.dedicated_total_bytes.value,
                p.gpu,
                p,
            );
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Shared GPU memory:").strong());
            capacity_bar(
                ui,
                adapter.shared_used_bytes.value,
                adapter.shared_total_bytes.value,
                p.accent,
                p,
            );

            let mut engines: Vec<_> = frame
                .gpu_engines
                .iter()
                .filter(|engine| {
                    engine.adapter_luid == Some(adapter.adapter_luid)
                        && engine.physical_adapter == adapter.physical_adapter
                })
                .collect();
            engines.sort_by(|a, b| a.instance.cmp(&b.instance));
            if !engines.is_empty() {
                ui.add_space(10.0);
                ui.strong("Engine instances (up to 4 selected)");
                ui.label(
                    egui::RichText::new(
                        "Each trace is one reported counter instance; percentages are never summed across engines.",
                    )
                    .color(p.secondary)
                    .size(11.5),
                );
                ui.horizontal_wrapped(|ui| {
                    for engine in engines.into_iter().take(16) {
                        let key = format!(
                            "gpu-engine:{}:{:?}:{:?}:{}:{}",
                            engine.adapter_luid.unwrap_or(0),
                            engine.physical_adapter,
                            engine.engine_id,
                            engine.engine_type,
                            engine.instance
                        );
                        let mut enabled = state.selected_engines.contains(&key);
                        if ui
                            .checkbox(
                                &mut enabled,
                                format!("{} #{:?}", engine.engine_type, engine.engine_id),
                            )
                            .on_hover_text(&engine.instance)
                            .changed()
                        {
                            if enabled && state.selected_engines.len() < 4 {
                                state.selected_engines.insert(key);
                            } else if !enabled {
                                state.selected_engines.remove(&key);
                            }
                        }
                    }
                });
                let curves: Vec<_> = state
                    .selected_engines
                    .iter()
                    .filter_map(|key| {
                        histories.detail(key).map(|series| PlotSeries {
                            name: "Engine instance",
                            samples: series,
                            color: p.gpu,
                            dashed: false,
                        })
                    })
                    .take(4)
                    .collect();
                if !curves.is_empty() {
                    let plot = state
                        .plots
                        .entry(format!("{}:engines", item.id))
                        .or_default();
                    time_plot(
                        ui,
                        ("gpu_engines", &item.id),
                        &curves,
                        state.range,
                        now,
                        "%",
                        Some(100.0),
                        210.0,
                        plot,
                        p,
                    );
                }
            }
        }

        if item.category == 3 {
            for (name, unit) in [("latency", "ms"), ("queue", "depth")] {
                let key = format!("disk:{}:{name}", item.id.trim_start_matches("disk:"));
                if let Some(series) = histories.detail(&key) {
                    ui.add_space(8.0);
                    ui.strong(if name == "latency" {
                        "Disk latency"
                    } else {
                        "Queue depth"
                    });
                    let plot = state.plots.entry(key.clone()).or_default();
                    time_plot(
                        ui,
                        &key,
                        &[PlotSeries {
                            name,
                            samples: series,
                            color: p.disk,
                            dashed: false,
                        }],
                        state.range,
                        now,
                        unit,
                        None,
                        180.0,
                        plot,
                        p,
                    );
                }
            }
        }
        ui.add_space(14.0);
    }

    // 5. Detailed Tables (Prominently rendered in both modes)
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| match item.category {
            0 => render_cpu_tab(ui, frame, topology, histories, state.range, now, p),
            1 => render_memory_tab(ui, frame, p),
            2 => render_gpu_tab(ui, frame, p),
            3 => render_storage_tab(ui, frame, p),
            4 => render_network_tab(ui, frame, p),
            5 => render_sensors_tab(
                ui,
                sensors,
                histories,
                state,
                graph_modes[5],
                exporter,
                now,
                p,
            ),
            _ => render_cpu_tab(ui, frame, topology, histories, state.range, now, p),
        });
}

fn render_cpu_tab(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    topology: Option<&Topology>,
    histories: &ResourceHistories,
    range: HistoryRange,
    now: Instant,
    p: Palette,
) {
    ui.horizontal(|ui| {
        ui.heading("Logical Processors");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Total System CPU: {}",
                    format_percent(frame.cpu_percent.value)
                ))
                .strong()
                .color(p.cpu),
            );
        });
    });

    ui.label(
        egui::RichText::new(
            "Individual logical processor loads preserving Windows processor-group addressing.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if frame.logical_cpus.is_empty() {
        ui.label("No individual logical core metrics were reported in this frame.");
        return;
    }

    egui::ScrollArea::horizontal()
        .id_salt("performance_cpu_table")
        .show(ui, |ui| {
            egui::Grid::new("resources_cpu_grid")
                .striped(true)
                .num_columns(7)
                .spacing([20.0, 8.0])
                .min_row_height(32.0)
                .show(ui, |ui| {
                    ui.strong("CPU #");
                    ui.strong("Group");
                    ui.strong("Core Type");
                    ui.strong("Utilization");
                    ui.strong("Observed Range");
                    ui.strong("Load Bar");
                    ui.strong("Source");
                    ui.end_row();

                    for core in &frame.logical_cpus {
                        ui.monospace(format!("CPU {}", core.cpu_index));
                        ui.monospace(format!("Group {}", core.group));

                        let class_str = topology.and_then(|t| {
                            t.cores
                                .iter()
                                .find(|c| c.logical_cpus.contains(&core.cpu_index))
                                .and_then(|c| c.class.as_deref())
                        });
                        ui.label(class_str.unwrap_or("Standard"));

                        ui.monospace(format_percent(core.utilization_percent.value));

                        let key = format!("cpu:{}:{}", core.group, core.cpu_index);
                        let range_str = if let Some(series) = histories.detail(&key)
                            && let Some(summary) = series.observed_summary(now, range)
                        {
                            format!(
                                "{:.0}–{:.0}% (avg {:.0}%)",
                                summary.minimum, summary.maximum, summary.average
                            )
                        } else {
                            "—".into()
                        };
                        ui.monospace(range_str);

                        // Compact load bar
                        let (bar_rect, _) =
                            ui.allocate_exact_size(Vec2::new(120.0, 16.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(bar_rect, CornerRadius::same(2), p.surface_raised);
                        if let Some(pct) = core.utilization_percent.value {
                            let fill_w = bar_rect.width() * (pct.clamp(0.0, 100.0) / 100.0);
                            if fill_w > 0.5 {
                                let fill_rect = Rect::from_min_size(
                                    bar_rect.min,
                                    Vec2::new(fill_w, bar_rect.height()),
                                );
                                ui.painter()
                                    .rect_filled(fill_rect, CornerRadius::same(2), p.cpu);
                            }
                        }

                        ui.label(&core.utilization_percent.source);
                        ui.end_row();
                    }
                });
        });
}

fn render_memory_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.heading("System Memory & Commit Budget");
    ui.label(
        egui::RichText::new(
            "Physical memory allocation and Windows commit limit / page file backing.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    egui::ScrollArea::horizontal()
        .id_salt("performance_memory_table")
        .show(ui, |ui| {
            egui::Grid::new("resources_memory_grid")
                .striped(true)
                .num_columns(6)
                .spacing([20.0, 10.0])
                .min_row_height(32.0)
                .show(ui, |ui| {
                    ui.strong("Metric");
                    ui.strong("Used");
                    ui.strong("Available");
                    ui.strong("Total / Limit");
                    ui.strong("Utilization");
                    ui.strong("Source");
                    ui.end_row();

                    // Physical RAM
                    let phys_used = frame.memory_used_bytes.value;
                    let phys_avail = frame.memory_available_bytes.value;
                    let phys_total = frame.memory_total_bytes.value;
                    let phys_pct = match (phys_used, phys_total) {
                        (Some(u), Some(t)) if t > 0 => Some((u as f64 * 100.0 / t as f64) as f32),
                        _ => None,
                    };

                    ui.colored_label(p.memory, "Physical RAM");
                    ui.monospace(phys_used.map_or_else(|| "Unavailable".into(), format_bytes));
                    ui.monospace(phys_avail.map_or_else(|| "Unavailable".into(), format_bytes));
                    ui.monospace(phys_total.map_or_else(|| "Unavailable".into(), format_bytes));
                    ui.monospace(format_percent(phys_pct));
                    ui.label(&frame.memory_used_bytes.source);
                    ui.end_row();

                    // Commit Limit
                    let com_used = frame.commit_used_bytes.value;
                    let com_total = frame.commit_total_bytes.value;
                    let com_avail = match (com_used, com_total) {
                        (Some(u), Some(t)) if t >= u => Some(t - u),
                        _ => None,
                    };
                    let com_pct = match (com_used, com_total) {
                        (Some(u), Some(t)) if t > 0 => Some((u as f64 * 100.0 / t as f64) as f32),
                        _ => None,
                    };

                    ui.colored_label(p.primary, "Commit Charge");
                    ui.monospace(com_used.map_or_else(|| "Unavailable".into(), format_bytes));
                    ui.monospace(com_avail.map_or_else(|| "Unavailable".into(), format_bytes));
                    ui.monospace(com_total.map_or_else(|| "Unavailable".into(), format_bytes));
                    ui.monospace(format_percent(com_pct));
                    ui.label(&frame.commit_used_bytes.source);
                    ui.end_row();
                });
        });
}

fn render_gpu_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.heading("GPU Adapters & Engines");
    ui.label(
        egui::RichText::new(
            "Physical graphics adapters discovered via DXGI and engine load from Windows PDH.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if frame.gpu_adapters.is_empty() {
        ui.label("No discrete or integrated GPU adapters discovered.");
    } else {
        ui.strong("Adapters");
        egui::ScrollArea::horizontal()
            .id_salt("performance_gpu_adapters_table")
            .show(ui, |ui| {
                egui::Grid::new("resources_gpu_adapters_grid")
                    .striped(true)
                    .num_columns(6)
                    .spacing([18.0, 8.0])
                    .min_row_height(32.0)
                    .show(ui, |ui| {
                        ui.strong("Adapter");
                        ui.strong("LUID");
                        ui.strong("Busiest Engine");
                        ui.strong("Dedicated VRAM (Used / Capacity)");
                        ui.strong("Shared Memory (Used / Capacity)");
                        ui.strong("Budget");
                        ui.end_row();

                        for adapter in &frame.gpu_adapters {
                            ui.label(&adapter.name);
                            ui.monospace(format!("0x{:x}", adapter.adapter_luid));
                            ui.monospace(format_percent(adapter.busiest_engine_percent.value));

                            let ded_str = match (
                                adapter.dedicated_used_bytes.value,
                                adapter.dedicated_total_bytes.value,
                            ) {
                                (Some(u), Some(t)) if t > 0 => {
                                    format!("{} / {}", format_bytes(u), format_bytes(t))
                                }
                                (Some(u), _) => format_bytes(u),
                                _ => "—".into(),
                            };
                            ui.monospace(ded_str);

                            let shr_str = match (
                                adapter.shared_used_bytes.value,
                                adapter.shared_total_bytes.value,
                            ) {
                                (Some(u), Some(t)) if t > 0 => {
                                    format!("{} / {}", format_bytes(u), format_bytes(t))
                                }
                                (Some(u), _) => format_bytes(u),
                                _ => "—".into(),
                            };
                            ui.monospace(shr_str);

                            let budget_str = adapter
                                .budget_bytes
                                .as_ref()
                                .and_then(|b| b.value)
                                .map_or_else(|| "—".into(), format_bytes);
                            ui.monospace(budget_str);
                            ui.end_row();
                        }
                    });
            });
    }

    ui.add_space(14.0);

    if !frame.gpu_engines.is_empty() {
        ui.strong(format!("Active GPU Engines ({})", frame.gpu_engines.len()));
        ui.label(
            egui::RichText::new(
                "Each row is one reported physical counter instance; percentages are never summed across engines.",
            )
            .color(p.secondary)
            .size(11.0),
        );
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("performance_gpu_engine_table")
            .max_height(260.0)
            .show(ui, |ui| {
                egui::Grid::new("resources_gpu_engines_grid")
                    .striped(true)
                    .num_columns(5)
                    .spacing([18.0, 6.0])
                    .min_row_height(30.0)
                    .show(ui, |ui| {
                        ui.strong("Adapter LUID");
                        ui.strong("Engine Type");
                        ui.strong("Index");
                        ui.strong("Process PID");
                        ui.strong("Utilization");
                        ui.end_row();

                        for eng in &frame.gpu_engines {
                            let luid_str = eng
                                .adapter_luid
                                .map(|l| format!("0x{l:x}"))
                                .unwrap_or_else(|| "Adapter".into());
                            ui.label(luid_str);
                            ui.label(&eng.engine_type);
                            ui.monospace(
                                eng.engine_id
                                    .map_or_else(|| "—".into(), |idx| idx.to_string()),
                            );
                            ui.monospace(
                                eng.process_pid
                                    .map_or_else(|| "System".into(), |pid| format!("PID {pid}")),
                            );
                            ui.monospace(format_percent(eng.utilization_percent.value));
                            ui.end_row();
                        }
                    });
            });
    }
}

fn render_storage_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.heading("Physical Disks & Block Storage");
    ui.label(
        egui::RichText::new(
            "Throughput, response time latency, and queue depth per physical disk.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if frame.disks.is_empty() {
        ui.label("No physical disk devices discovered in current telemetry frame.");
        return;
    }

    egui::ScrollArea::horizontal()
        .id_salt("performance_storage_table")
        .show(ui, |ui| {
            egui::Grid::new("resources_storage_grid")
                .striped(true)
                .num_columns(5)
                .spacing([20.0, 8.0])
                .min_row_height(32.0)
                .show(ui, |ui| {
                    ui.strong("Device Label");
                    ui.strong("Read Rate");
                    ui.strong("Write Rate");
                    ui.strong("Avg Latency");
                    ui.strong("Queue Depth");
                    ui.end_row();

                    for disk in &frame.disks {
                        ui.label(&disk.label);
                        ui.monospace(format_rate(disk.read_bytes_per_sec.value.map(|v| v as f32)));
                        ui.monospace(format_rate(
                            disk.write_bytes_per_sec.value.map(|v| v as f32),
                        ));

                        let lat_str = disk
                            .latency_ms
                            .as_ref()
                            .and_then(|m| m.value)
                            .map_or_else(|| "—".into(), |v| format!("{v:.2} ms"));
                        ui.monospace(lat_str);

                        let queue_str = disk
                            .queue_depth
                            .as_ref()
                            .and_then(|m| m.value)
                            .map_or_else(|| "—".into(), |v| format!("{v:.1}"));
                        ui.monospace(queue_str);
                        ui.end_row();
                    }
                });
        });
}

fn render_network_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.heading("Network Adapters & Interfaces");
    ui.label(
        egui::RichText::new(
            "Network interface bandwidth, link connection status, and theoretical link speed.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if frame.network_interfaces.is_empty() {
        ui.label("No network adapters reported in current telemetry frame.");
        return;
    }

    egui::ScrollArea::horizontal()
        .id_salt("performance_network_table")
        .show(ui, |ui| {
            egui::Grid::new("resources_network_grid")
                .striped(true)
                .num_columns(5)
                .spacing([20.0, 8.0])
                .min_row_height(32.0)
                .show(ui, |ui| {
                    ui.strong("Interface");
                    ui.strong("Receive Rate");
                    ui.strong("Transmit Rate");
                    ui.strong("Link State");
                    ui.strong("Link Speed");
                    ui.end_row();

                    for iface in &frame.network_interfaces {
                        ui.label(&iface.label);
                        ui.monospace(format_rate(
                            iface.read_bytes_per_sec.value.map(|v| v as f32),
                        ));
                        ui.monospace(format_rate(
                            iface.write_bytes_per_sec.value.map(|v| v as f32),
                        ));

                        let is_conn = iface.link_state.as_deref() != Some("Disconnected");
                        let state_str = iface.link_state.as_deref().unwrap_or("Connected");
                        ui.colored_label(if is_conn { p.success } else { p.error }, state_str);

                        let speed_str = iface
                            .link_speed_bps
                            .as_ref()
                            .and_then(|m| m.value)
                            .map_or_else(
                                || "—".into(),
                                |bps| {
                                    if bps >= 1_000_000_000 {
                                        format!("{:.1} Gbps", bps as f64 / 1_000_000_000.0)
                                    } else if bps >= 1_000_000 {
                                        format!("{:.0} Mbps", bps as f64 / 1_000_000.0)
                                    } else {
                                        format!("{bps} bps")
                                    }
                                },
                            );
                        ui.monospace(speed_str);
                        ui.end_row();
                    }
                });
        });
}

fn render_sensors_tab(
    ui: &mut egui::Ui,
    sensors: &[SensorReading],
    histories: &ResourceHistories,
    state: &mut ResourcesViewState,
    show_graphs: bool,
    exporter: Option<&mut ExportManager>,
    now: Instant,
    p: Palette,
) {
    ui.horizontal(|ui| {
        ui.heading("Hardware Sensors & Telemetry Providers");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(exp) = exporter
                && ui
                    .button("Export Sensors CSV")
                    .on_hover_text("Export current sensor snapshot to CSV")
                    .clicked()
            {
                match exp.sensors(sensors.to_vec()) {
                    Ok(()) => {
                        state.export_message =
                            Some((Instant::now(), "Sensors exported to CSV".into()));
                    }
                    Err(e) => {
                        state.export_message =
                            Some((Instant::now(), format!("Export failed: {e}")));
                    }
                }
            }
        });
    });

    ui.label(
        egui::RichText::new(
            "Substantiated hardware metrics from isolated workers: Intel IGCL, NVIDIA NVML, AMD ADLX, and Advanced Sensors (LibreHardwareMonitor / PawnIO).",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if sensors.is_empty() {
        ui.label(
            "No active vendor sensors reported. Sensors require supported GPU/CPU hardware and driver runtimes.",
        );
        ui.label(
            egui::RichText::new(
                "Check the Capabilities tab to see provider runtime discovery status.",
            )
            .size(11.5)
            .color(p.secondary),
        );
        return;
    }

    // 1. Selectable Trend Charts by Compatible Unit
    if show_graphs {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Trend Group:").strong());
            for kind in [
                SensorTrendKind::Power,
                SensorTrendKind::Temperature,
                SensorTrendKind::Clock,
                SensorTrendKind::Fan,
                SensorTrendKind::Load,
            ] {
                ui.selectable_value(&mut state.selected_sensor_trend, kind, kind.label());
            }
        });

        let current_kind = state.selected_sensor_trend;
        let matching_sensors: Vec<&SensorReading> = sensors
            .iter()
            .filter(|s| current_kind.matches_sensor(s))
            .collect();

        if !matching_sensors.is_empty() {
            ui.add_space(6.0);
            let colors = [
                p.accent, p.cpu, p.gpu, p.memory, p.disk, p.network, p.warning, p.success,
            ];
            let mut plot_series: Vec<PlotSeries<'_>> = Vec::new();
            for (idx, sensor) in matching_sensors.iter().enumerate() {
                let id = format!(
                    "sensor:{}:{}:{}:{:?}",
                    sensor.scope, sensor.provider, sensor.name, sensor.sensor_type
                );
                let series = histories.detail(&id).or_else(|| {
                    if !sensor.id.is_empty() {
                        histories.detail(&format!("sensor:{}", sensor.id))
                    } else {
                        None
                    }
                });
                if let Some(s) = series {
                    let color = colors[idx % colors.len()];
                    plot_series.push(PlotSeries {
                        name: &sensor.name,
                        samples: s,
                        color,
                        dashed: idx >= colors.len(),
                    });
                }
            }

            if !plot_series.is_empty() {
                let key = format!("sensor_trend_{:?}", current_kind);
                let plot = state.plots.entry(key.clone()).or_default();
                let chart_h = 220.0;
                time_plot(
                    ui,
                    &key,
                    &plot_series,
                    state.range,
                    now,
                    current_kind.unit(),
                    if current_kind == SensorTrendKind::Load {
                        Some(100.0)
                    } else {
                        None
                    },
                    chart_h,
                    plot,
                    p,
                );

                ui.horizontal_wrapped(|ui| {
                    for series in &plot_series {
                        ui.colored_label(series.color, format!("━ {}", series.name));
                    }
                });
            } else {
                ui.label(
                    egui::RichText::new("Collecting history for selected trend group...")
                        .color(p.secondary),
                );
            }
        } else {
            ui.label(
                egui::RichText::new(format!(
                    "No active sensors matching {} on this system.",
                    current_kind.label()
                ))
                .color(p.secondary),
            );
        }
        ui.add_space(12.0);
    }

    // 2. Device-Grouped Sensor Tables
    let mut groups: BTreeMap<String, Vec<&SensorReading>> = BTreeMap::new();
    for sensor in sensors {
        let device_name = if !sensor.device_name.is_empty() {
            sensor.device_name.clone()
        } else if !sensor.device_id.is_empty() {
            sensor.device_id.clone()
        } else {
            format!("{} Device", sensor.scope)
        };
        groups.entry(device_name).or_default().push(sensor);
    }

    for (device_name, device_sensors) in groups {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(&device_name)
                .size(14.0)
                .strong()
                .color(p.text),
        );
        ui.add_space(2.0);

        egui::ScrollArea::horizontal()
            .id_salt(format!("performance_sensors_{}", device_name))
            .show(ui, |ui| {
                egui::Grid::new(format!("grid_sensors_{}", device_name))
                    .striped(true)
                    .num_columns(9)
                    .spacing([18.0, 6.0])
                    .min_row_height(28.0)
                    .show(ui, |ui| {
                        ui.strong("Sensor");
                        ui.strong("Current");
                        ui.strong("Minimum");
                        ui.strong("Maximum");
                        ui.strong("Average");
                        ui.strong("Unit");
                        ui.strong("Source");
                        ui.strong("Age");
                        ui.strong("Status");
                        ui.end_row();

                        for sensor in device_sensors {
                            // 1. Sensor Name
                            ui.label(&sensor.name);

                            // Lookup summary for min/max/avg
                            let key = format!(
                                "sensor:{}:{}:{}:{:?}",
                                sensor.scope, sensor.provider, sensor.name, sensor.sensor_type
                            );
                            let summary = histories
                                .detail(&key)
                                .or_else(|| {
                                    if !sensor.id.is_empty() {
                                        histories.detail(&format!("sensor:{}", sensor.id))
                                    } else {
                                        None
                                    }
                                })
                                .and_then(|s| s.observed_summary(now, state.range));

                            // 2. Current
                            let current_str = sensor.value.value.map_or_else(
                                || "—".into(),
                                |v| format!("{v:.1}"),
                            );
                            ui.monospace(current_str);

                            // 3. Minimum
                            let min_str = summary
                                .as_ref()
                                .map_or_else(|| "—".into(), |s| format!("{:.1}", s.minimum));
                            ui.monospace(min_str);

                            // 4. Maximum
                            let max_str = summary
                                .as_ref()
                                .map_or_else(|| "—".into(), |s| format!("{:.1}", s.maximum));
                            ui.monospace(max_str);

                            // 5. Average
                            let avg_str = summary
                                .as_ref()
                                .map_or_else(|| "—".into(), |s| format!("{:.1}", s.average));
                            ui.monospace(avg_str);

                            // 6. Unit
                            ui.label(&sensor.value.unit);

                            // 7. Source
                            let src = if !sensor.provider.is_empty() {
                                &sensor.provider
                            } else {
                                &sensor.value.source
                            };
                            ui.label(src);

                            // 8. Age
                            let age_ms = planora_telemetry::unix_ms()
                                .saturating_sub(sensor.value.sampled_at_ms);
                            let age_str = if age_ms < 1_000 {
                                "< 1s".into()
                            } else if age_ms < 60_000 {
                                format!("{}s", age_ms / 1_000)
                            } else {
                                format!("{}m", age_ms / 60_000)
                            };
                            ui.monospace(age_str);

                            // 9. Status
                            let (status_text, status_color) =
                                if let Some(r) = &sensor.value.unavailable_reason {
                                    (r.to_string(), p.secondary)
                                } else if age_ms > 5_000 {
                                    (format!("Stale ({}s)", age_ms / 1_000), p.warning)
                                } else if sensor.value.value.is_some() {
                                    ("Active".into(), p.success)
                                } else {
                                    ("Unavailable".into(), p.secondary)
                                };

                            let status_resp = ui.colored_label(status_color, status_text);
                            status_resp.on_hover_text(format!(
                                "Sensor: {}\nScope: {}\nProvider: {}\nDevice: {}\nSampled: {} ms ago\nReason: {}",
                                sensor.name,
                                sensor.scope,
                                sensor.provider,
                                if sensor.device_name.is_empty() { "—" } else { &sensor.device_name },
                                age_ms,
                                sensor
                                    .value
                                    .unavailable_reason
                                    .as_ref()
                                    .map_or_else(|| "None".into(), ToString::to_string)
                            ));

                            ui.end_row();
                        }
                    });
            });
    }
}
