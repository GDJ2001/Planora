use crate::{
    components::{
        header::render_page_header,
        plot::{
            PlotSeries, PlotState, ValueScale, capacity_bar, cpu_utilization_plot, sparkline,
            time_plot,
        },
    },
    export::ExportManager,
    formatting::{format_bytes, format_percent, format_rate},
    history::{HistoryRange, ResourceHistories, TimeSeries},
    state::{AppState, DisplayPause, PercentScale},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, Color32, CornerRadius, Rect, Stroke, Vec2};
use planora_core::{SensorReading, TelemetryFrame, Topology};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

#[cfg(test)]
mod workspace_tests {
    use super::*;

    #[test]
    fn performance_workspace_stays_inside_available_width() {
        let fixture = crate::smoke_test::create_native_fixture_app(
            crate::theme::Theme::Dark,
            crate::state::Page::Performance,
        );
        let frame = fixture.shared.lock().unwrap().frame.clone().unwrap();
        for width in [800.0, 940.0, 1120.0, 1328.0, 1712.0] {
            for scale in [1.0, 1.5, 2.0] {
                for resource in ["cpu", "memory", "gpu", "network"] {
                    let ctx = egui::Context::default();
                    crate::theme::configure_theme(
                        &ctx,
                        crate::theme::get_palette(true),
                        crate::theme::Density::Normal,
                        scale,
                        false,
                    );
                    ctx.data_mut(|data| {
                        data.insert_temp(egui::Id::unique("system_text_scale"), scale)
                    });
                    let mut state = ResourcesViewState {
                        selected_resource: Some(resource.into()),
                        ..Default::default()
                    };
                    for _ in 0..2 {
                        let mut output = ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(width, 790.0),
                                )),
                                ..Default::default()
                            },
                            |ui| {
                                let right = ui.max_rect().right();
                                render_resources_view(
                                    ui,
                                    &frame,
                                    &fixture.histories,
                                    &[],
                                    None,
                                    None,
                                    &mut state,
                                    &mut PercentScale::Fit,
                                    &AppState::Live,
                                    &mut DisplayPause::default(),
                                    100,
                                    &mut [true; 6],
                                    &mut [true; 6],
                                    Instant::now(),
                                    crate::theme::get_palette(true),
                                );
                                assert!(
                                    ui.min_rect().right() <= right + 1.0,
                                    "{resource} at {width}, text scale {scale}: expanded to {}",
                                    ui.min_rect().right()
                                );
                            },
                        );
                        output.textures_delta.clear();
                    }
                }
            }
        }
    }

    #[test]
    fn selector_card_click_anywhere_selects_resource() {
        let fixture = crate::smoke_test::create_native_fixture_app(
            crate::theme::Theme::Dark,
            crate::state::Page::Performance,
        );
        let frame = fixture.shared.lock().unwrap().frame.clone().unwrap();
        let ctx = egui::Context::default();
        let p = crate::theme::get_palette(true);
        let all_choices = choices(&frame, Some(&fixture.histories), &[], p);
        let mem_choice = all_choices.iter().find(|c| c.id == "memory").unwrap();

        // 1. Initial frame to lay out the card and capture its rect
        let mut card_rect = Rect::NOTHING;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1180.0, 790.0),
                )),
                ..Default::default()
            },
            |ui| {
                let resp = resource_selector_card(
                    ui,
                    mem_choice,
                    false,
                    220.0,
                    &fixture.histories,
                    Instant::now(),
                    p,
                );
                card_rect = resp.rect;
            },
        );
        output.textures_delta.clear();
        assert!(!card_rect.is_negative() && card_rect.width() > 50.0 && card_rect.height() > 40.0);

        // 2. Click in the lower right (where the sparkline is)
        let click_pos = egui::Pos2::new(card_rect.right() - 20.0, card_rect.bottom() - 15.0);
        let mut clicked = false;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1180.0, 790.0),
                )),
                events: vec![
                    egui::Event::PointerMoved(click_pos),
                    egui::Event::PointerButton {
                        pos: click_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    egui::Event::PointerButton {
                        pos: click_pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            |ui| {
                let resp = resource_selector_card(
                    ui,
                    mem_choice,
                    false,
                    220.0,
                    &fixture.histories,
                    Instant::now(),
                    p,
                );
                if resp.clicked() {
                    clicked = true;
                }
            },
        );
        output.textures_delta.clear();
        assert!(
            clicked,
            "clicking the sparkline region of the card must activate it"
        );
    }

    #[test]
    fn selector_card_exposes_selected_accessibility_state() {
        let fixture = crate::smoke_test::create_native_fixture_app(
            crate::theme::Theme::Dark,
            crate::state::Page::Performance,
        );
        let frame = fixture.shared.lock().unwrap().frame.clone().unwrap();
        let ctx = egui::Context::default();
        let p = crate::theme::get_palette(true);
        let all_choices = choices(&frame, Some(&fixture.histories), &[], p);
        let cpu_choice = all_choices.iter().find(|c| c.id == "cpu").unwrap();

        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1180.0, 790.0),
                )),
                ..Default::default()
            },
            |ui| {
                let resp = resource_selector_card(
                    ui,
                    cpu_choice,
                    true,
                    220.0,
                    &fixture.histories,
                    Instant::now(),
                    p,
                );
                assert!(resp.sense.senses_click(), "card must sense clicks");
                assert!(!resp.rect.is_negative(), "card rect must be valid");
            },
        );
        output.textures_delta.clear();
    }
}

#[cfg(test)]
mod history_selection_tests {
    use super::*;
    use planora_core::{Metric, SensorType};

    #[test]
    fn selected_temperature_sensor_retains_matching_group_and_tracks_selection() {
        let sensor = SensorReading {
            id: "cpu/core-1/tjmax".into(),
            device_id: "cpu:0".into(),
            device_name: "CPU".into(),
            name: "Core #1 Distance to TjMax".into(),
            hardware_category: planora_core::HardwareCategory::Unknown,
            sensor_type: SensorType::CpuTemperatureCelsius,
            scope: "Core #1".into(),
            provider: "Advanced sensors".into(),
            value: Metric::available(42.0, "°C", "Advanced sensors", 1_000),
        };
        let key = crate::history::sensor_series_key(&sensor);
        let mut state = ResourcesViewState {
            selected_resource: Some(key.clone()),
            ..Default::default()
        };
        state.sync_sensor_trend_with_selection(std::slice::from_ref(&sensor));
        assert_eq!(state.selected_sensor_trend, SensorTrendKind::Temperature);
        assert!(state.priority_history_keys(&[sensor]).contains(&key));
    }

    #[test]
    fn hardware_owner_keeps_generic_clock_and_fan_with_its_device() {
        let mut sensor = SensorReading {
            id: "fan/0".into(),
            device_id: "gpu:luid-a".into(),
            device_name: "Adapter A".into(),
            name: "Fan".into(),
            hardware_category: planora_core::HardwareCategory::Gpu,
            sensor_type: SensorType::FanRpm,
            scope: "GPU".into(),
            provider: "Vendor".into(),
            value: Metric::available(1_800.0, "RPM", "Vendor", 1_000),
        };
        assert_eq!(sensor_section(&sensor), ResourceSection::GpuSensors);
        sensor.sensor_type = SensorType::ClockMhz;
        assert_eq!(sensor_section(&sensor), ResourceSection::GpuSensors);
        sensor.hardware_category = planora_core::HardwareCategory::Unknown;
        assert_eq!(sensor_section(&sensor), ResourceSection::OtherHardware);
        sensor.sensor_type = SensorType::CpuTemperatureCelsius;
        assert_eq!(sensor_section(&sensor), ResourceSection::CpuSensors);
    }

    #[test]
    fn same_named_gpu_sensors_keep_distinct_device_groups_and_search_by_provider() {
        let sensor = SensorReading {
            id: "temperature".into(),
            device_id: "gpu:luid:1".into(),
            device_name: "Iris Xe".into(),
            name: "Temperature".into(),
            hardware_category: planora_core::HardwareCategory::Gpu,
            sensor_type: SensorType::GpuTemperatureCelsius,
            scope: "GPU".into(),
            provider: "IGCL".into(),
            value: Metric::available(52.0, "°C", "IGCL", 1_000),
        };
        let mut other = sensor.clone();
        other.device_id = "gpu:luid:2".into();
        let frame = crate::smoke_test::create_native_fixture_app(
            crate::theme::Theme::Dark,
            crate::state::Page::Performance,
        )
        .shared
        .lock()
        .unwrap()
        .frame
        .clone()
        .unwrap();
        let rows = choices(
            &frame,
            None,
            &[sensor, other],
            crate::theme::get_palette(true),
        );
        let gpu_rows: Vec<_> = rows
            .iter()
            .filter(|row| row.section == ResourceSection::GpuSensors)
            .collect();
        assert_eq!(gpu_rows.len(), 2);
        assert_ne!(gpu_rows[0].device, gpu_rows[1].device);
        assert!(gpu_rows.iter().all(|row| matches_choice(row, "igcl")));
    }

    #[test]
    fn network_prominence_requires_physical_interface_and_known_connected_state() {
        let fixture = crate::smoke_test::create_native_fixture_app(
            crate::theme::Theme::Dark,
            crate::state::Page::Performance,
        );
        let mut frame = fixture
            .shared
            .lock()
            .unwrap()
            .frame
            .clone()
            .unwrap()
            .as_ref()
            .clone();
        let base = frame.network_interfaces[0].clone();
        let mut virtual_iface = base.clone();
        virtual_iface.id = "virtual".into();
        virtual_iface.is_hardware_interface = Some(false);
        let mut unknown_iface = base.clone();
        unknown_iface.id = "unknown".into();
        unknown_iface.link_state = None;
        let mut disconnected = base;
        disconnected.id = "disconnected".into();
        disconnected.link_state = Some("Disconnected".into());
        frame
            .network_interfaces
            .extend([virtual_iface, unknown_iface, disconnected]);
        let rows = choices(&frame, None, &[], crate::theme::get_palette(true));
        let section = |id: &str| rows.iter().find(|row| row.id == id).unwrap().section;
        assert_eq!(section("net:fixture-ethernet"), ResourceSection::Main);
        assert_eq!(section("net:virtual"), ResourceSection::NetworkDetails);
        assert_eq!(section("net:unknown"), ResourceSection::NetworkDetails);
        assert_eq!(section("net:disconnected"), ResourceSection::NetworkDetails);
        assert_eq!(
            rows.iter()
                .find(|row| row.id == "net:unknown")
                .unwrap()
                .status_text,
            "Unknown"
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SensorTrendKind {
    #[default]
    Power,
    Temperature,
    Clock,
    Fan,
    Load,
    Voltage,
    Current,
    Throughput,
}

impl SensorTrendKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Power => "Power (W)",
            Self::Temperature => "Temperatures (°C)",
            Self::Clock => "Clocks (MHz)",
            Self::Fan => "Fans (RPM)",
            Self::Load => "Load (%)",
            Self::Voltage => "Voltage (V)",
            Self::Current => "Current (A)",
            Self::Throughput => "Throughput (B/s)",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Self::Power => "W",
            Self::Temperature => "°C",
            Self::Clock => "MHz",
            Self::Fan => "RPM",
            Self::Load => "%",
            Self::Voltage => "V",
            Self::Current => "A",
            Self::Throughput => "B/s",
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
            Self::Voltage => {
                matches!(reading.sensor_type, planora_core::SensorType::VoltageVolts)
                    || reading.value.unit == "V"
            }
            Self::Current => {
                matches!(
                    reading.sensor_type,
                    planora_core::SensorType::CurrentAmperes
                ) || reading.value.unit == "A"
            }
            Self::Throughput => reading.value.unit == "B/s",
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum ResourceSection {
    Main,
    CpuSensors,
    GpuSensors,
    MemorySensors,
    StorageDetails,
    NetworkDetails,
    MotherboardCooling,
    OtherHardware,
}

impl ResourceSection {
    fn label(self) -> &'static str {
        match self {
            Self::Main => "Resources",
            Self::CpuSensors => "CPU sensors",
            Self::GpuSensors => "GPU sensors",
            Self::MemorySensors => "Memory sensors",
            Self::StorageDetails => "Storage details",
            Self::NetworkDetails => "Network details",
            Self::MotherboardCooling => "Motherboard & cooling",
            Self::OtherHardware => "Other hardware",
        }
    }
}

const DETAIL_SECTIONS: [ResourceSection; 7] = [
    ResourceSection::CpuSensors,
    ResourceSection::GpuSensors,
    ResourceSection::MemorySensors,
    ResourceSection::StorageDetails,
    ResourceSection::NetworkDetails,
    ResourceSection::MotherboardCooling,
    ResourceSection::OtherHardware,
];

fn visible_section(item: &Choice, pinned_interfaces: &BTreeSet<String>) -> ResourceSection {
    if item.id.starts_with("net:") && pinned_interfaces.contains(&item.id) {
        ResourceSection::Main
    } else {
        item.section
    }
}

fn matches_choice(item: &Choice, query: &str) -> bool {
    query.is_empty()
        || [
            item.label.as_str(),
            item.section.label(),
            item.device.as_str(),
            item.family.as_str(),
            item.source.as_str(),
        ]
        .iter()
        .any(|text| text.to_lowercase().contains(query))
}

fn sensor_section(sensor: &SensorReading) -> ResourceSection {
    use planora_core::{HardwareCategory as H, SensorType as T};
    match sensor.hardware_category {
        H::Cpu => ResourceSection::CpuSensors,
        H::Gpu => ResourceSection::GpuSensors,
        H::Memory => ResourceSection::MemorySensors,
        H::Storage => ResourceSection::StorageDetails,
        H::Network => ResourceSection::NetworkDetails,
        H::MotherboardCooling => ResourceSection::MotherboardCooling,
        H::Other | H::Unknown => match sensor.sensor_type {
            T::CpuPackagePowerWatts | T::CpuTemperatureCelsius => ResourceSection::CpuSensors,
            T::GpuChipPowerWatts | T::GpuBoardPowerWatts | T::GpuTemperatureCelsius => {
                ResourceSection::GpuSensors
            }
            T::StorageTemperatureCelsius => ResourceSection::StorageDetails,
            _ => ResourceSection::OtherHardware,
        },
    }
}

fn sensor_family(sensor: &SensorReading) -> &'static str {
    use planora_core::SensorType as T;
    match sensor.sensor_type {
        T::GpuTemperatureCelsius
        | T::CpuTemperatureCelsius
        | T::StorageTemperatureCelsius
        | T::ThermalZoneCelsius => "Temperature",
        T::GpuChipPowerWatts
        | T::GpuBoardPowerWatts
        | T::CpuPackagePowerWatts
        | T::EnergyJoules => "Power",
        T::ClockMhz => "Clocks",
        T::FanRpm => "Cooling",
        T::VoltageVolts | T::CurrentAmperes => "Electrical",
        T::LoadPercent => "Load",
        T::StorageHealthPercent => "Health",
        T::DataGigabytes | T::ThroughputBytesPerSec => "Data & IO",
        T::TimeHours => "Lifetime",
        T::GenericNumeric => "Generic",
    }
}

pub struct ResourcesViewState {
    pub search_query: String,
    pub engine_search: String,
    detail_height: f32,
    pub selected_resource: Option<String>,
    pub summary_section: Option<ResourceSection>,
    /// Reveal the selected card once after startup or a compact picker change.
    /// Egui can restore an old list offset that hides the persisted selection.
    pub reveal_selected_resource: bool,
    pub expanded_sections: BTreeSet<ResourceSection>,
    pub pinned_interfaces: BTreeSet<String>,
    pub range: HistoryRange,
    pub selected_cpu: Option<(u16, u16)>,
    pub selected_engines: BTreeSet<String>,
    pub small_multiples: bool,
    pub show_provider_details: bool,
    pub export_message: Option<(Instant, String)>,
    pub selected_sensor_trend: SensorTrendKind,
    pub compare_sensors: bool,
    last_sensor_selection: Option<String>,
    plots: BTreeMap<String, PlotState>,
}

impl Default for ResourcesViewState {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            engine_search: String::new(),
            detail_height: 790.0,
            selected_resource: None,
            summary_section: None,
            reveal_selected_resource: true,
            expanded_sections: BTreeSet::new(),
            pinned_interfaces: BTreeSet::new(),
            range: HistoryRange::Sec60,
            selected_cpu: None,
            selected_engines: BTreeSet::new(),
            small_multiples: false,
            show_provider_details: false,
            export_message: None,
            selected_sensor_trend: SensorTrendKind::default(),
            compare_sensors: false,
            last_sensor_selection: None,
            plots: BTreeMap::new(),
        }
    }
}

impl ResourcesViewState {
    pub fn priority_history_keys(&self, sensors: &[SensorReading]) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        // Retain 60s history for all active hardware sensors so that left-rail
        // selector sparklines and instant selection switching are always available.
        keys.extend(sensors.iter().map(crate::history::sensor_series_key));
        if let Some(resource) = self.selected_resource.as_deref() {
            if resource.starts_with("sensor:") {
                keys.insert(resource.to_owned());
            } else if resource.starts_with("gpu:") {
                keys.insert(format!("{resource}:busiest"));
                keys.extend(self.selected_engines.iter().take(4).cloned());
            } else if resource.starts_with("disk:") {
                for suffix in ["read", "write", "latency", "queue"] {
                    keys.insert(format!("{resource}:{suffix}"));
                }
            } else if resource.starts_with("net:") {
                for suffix in ["rx", "tx"] {
                    keys.insert(format!("{resource}:{suffix}"));
                }
            }
        }
        keys
    }

    fn sync_sensor_trend_with_selection(&mut self, sensors: &[SensorReading]) {
        let selected = self.selected_resource.as_deref();
        if selected == self.last_sensor_selection.as_deref() {
            return;
        }
        self.last_sensor_selection = self.selected_resource.clone();
        let Some(sensor) = sensors.iter().find(|sensor| {
            selected.is_some_and(|id| id == crate::history::sensor_series_key(sensor))
        }) else {
            return;
        };
        if let Some(kind) = [
            SensorTrendKind::Power,
            SensorTrendKind::Temperature,
            SensorTrendKind::Clock,
            SensorTrendKind::Fan,
            SensorTrendKind::Load,
            SensorTrendKind::Voltage,
            SensorTrendKind::Current,
            SensorTrendKind::Throughput,
        ]
        .into_iter()
        .find(|kind| kind.matches_sensor(sensor))
        {
            self.selected_sensor_trend = kind;
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
    pub section: ResourceSection,
    pub device: String,
    pub family: String,
    pub color: egui::Color32,
    pub series_key: String,
    pub source: String,
    pub unavailable_reason: Option<String>,
}

pub fn choices(
    frame: &TelemetryFrame,
    histories: Option<&ResourceHistories>,
    sensors: &[SensorReading],
    p: Palette,
) -> Vec<Choice> {
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
            section: ResourceSection::Main,
            device: String::new(),
            family: String::new(),
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
            section: ResourceSection::Main,
            device: String::new(),
            family: String::new(),
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
        let (activity, _, _) = adapter.activity_percent();
        let is_ok = activity.value.is_some();
        rows.push(Choice {
            id: format!(
                "gpu:{}:{:?}",
                adapter.adapter_luid, adapter.physical_adapter
            ),
            label: adapter.name.clone(),
            value: format_percent(activity.value),
            status_text: if is_ok {
                "Active".into()
            } else {
                "Unavailable".into()
            },
            status_symbol: if is_ok { "●" } else { "—" },
            status_color: if is_ok { p.success } else { p.warning },
            category: 2,
            section: ResourceSection::Main,
            device: String::new(),
            family: String::new(),
            color: p.gpu,
            series_key: adapter.activity_history_key(),
            source: activity.source.clone(),
            unavailable_reason: activity
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
            section: ResourceSection::Main,
            device: String::new(),
            family: String::new(),
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
        let stable_rate = histories
            .and_then(|h| h.details.get(&format!("disk:{}:read", disk.id)))
            .and_then(|ts| ts.latest_stable_value())
            .or_else(|| disk.read_bytes_per_sec.value.map(|v| v as f32));
        rows.push(Choice {
            id: format!("disk:{}", disk.id),
            label: disk.label.clone(),
            value: format_rate(stable_rate),
            status_text: if is_ok {
                "Active".into()
            } else {
                "Unavailable".into()
            },
            status_symbol: if is_ok { "●" } else { "—" },
            status_color: if is_ok { p.success } else { p.warning },
            category: 3,
            section: ResourceSection::Main,
            device: String::new(),
            family: String::new(),
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
            section: ResourceSection::Main,
            device: String::new(),
            family: String::new(),
            color: p.disk,
            series_key: "disk:none".into(),
            source: "Storage".into(),
            unavailable_reason: Some("No physical disk devices discovered".into()),
        });
    }

    let mut interfaces: Vec<_> = frame.network_interfaces.iter().collect();
    interfaces.sort_by(|a, b| a.id.cmp(&b.id));
    for iface in interfaces {
        let is_connected = iface.link_state.as_deref() == Some("Connected")
            || iface.link_state.as_deref() == Some("Up");
        let stable_rx = histories
            .and_then(|h| h.details.get(&format!("net:{}:rx", iface.id)))
            .and_then(|ts| ts.latest_stable_value())
            .or_else(|| iface.read_bytes_per_sec.value.map(|v| v as f32));
        rows.push(Choice {
            id: format!("net:{}", iface.id),
            label: iface.label.clone(),
            value: format_rate(stable_rx),
            status_text: iface.link_state.as_deref().unwrap_or("Unknown").into(),
            status_symbol: if is_connected { "●" } else { "○" },
            status_color: if is_connected { p.success } else { p.secondary },
            category: 4,
            section: if is_connected && iface.is_hardware_interface == Some(true) {
                ResourceSection::Main
            } else {
                ResourceSection::NetworkDetails
            },
            device: iface.id.clone(),
            family: "Activity".into(),
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
            section: ResourceSection::NetworkDetails,
            device: String::new(),
            family: String::new(),
            color: p.network,
            series_key: "net:none".into(),
            source: "Network".into(),
            unavailable_reason: Some("No network adapters reported".into()),
        });
    }

    let mut sensor_rows: Vec<_> = sensors.iter().collect();
    sensor_rows.sort_by(|a, b| {
        (
            &a.scope,
            &a.device_name,
            &a.device_id,
            &a.provider,
            &a.name,
            &a.id,
        )
            .cmp(&(
                &b.scope,
                &b.device_name,
                &b.device_id,
                &b.provider,
                &b.name,
                &b.id,
            ))
    });
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
        let id = crate::history::sensor_series_key(sensor);
        let label = if !sensor.device_name.is_empty() && !sensor.name.contains(&sensor.device_name)
        {
            format!("{} · {}", sensor.device_name, sensor.name)
        } else {
            format!("{} ({})", sensor.name, sensor.scope)
        };
        rows.push(Choice {
            id: id.clone(),
            label,
            value: sensor.value.value.map_or_else(
                || "Unavailable".into(),
                |v| format!("{v:.1} {}", sensor.value.unit),
            ),
            status_text,
            status_symbol,
            status_color,
            category: 5,
            section: sensor_section(sensor),
            device: if sensor.device_name.is_empty() {
                sensor.device_id.clone()
            } else if sensor.device_id.is_empty() {
                sensor.device_name.clone()
            } else {
                format!("{} ({})", sensor.device_name, sensor.device_id)
            },
            family: sensor_family(sensor).into(),
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
            section: ResourceSection::OtherHardware,
            device: String::new(),
            family: String::new(),
            color: p.accent,
            series_key: "sensor:none".into(),
            source: "Sensors".into(),
            unavailable_reason: Some("No active vendor hardware sensors reported".into()),
        });
    }
    rows
}

pub fn blend_color(base: Color32, tint: Color32, factor: f32) -> Color32 {
    let factor = factor.clamp(0.0, 1.0);
    let inv = 1.0 - factor;
    Color32::from_rgba_premultiplied(
        (base.r() as f32 * inv + tint.r() as f32 * factor).round() as u8,
        (base.g() as f32 * inv + tint.g() as f32 * factor).round() as u8,
        (base.b() as f32 * inv + tint.b() as f32 * factor).round() as u8,
        (base.a() as f32 * inv + tint.a() as f32 * factor).round() as u8,
    )
}

fn render_resource_selector(
    ui: &mut egui::Ui,
    all: &[Choice],
    histories: &ResourceHistories,
    state: &mut ResourcesViewState,
    now: Instant,
    p: Palette,
) {
    let query = state.search_query.trim().to_lowercase();
    let main_items: Vec<_> = all
        .iter()
        .filter(|item| {
            visible_section(item, &state.pinned_interfaces) == ResourceSection::Main
                && matches_choice(item, &query)
        })
        .collect();
    for item in main_items {
        let selected = state.selected_resource.as_ref() == Some(&item.id);
        let card_width = (ui.available_width() - 22.0).max(100.0);
        let response = resource_selector_card(ui, item, selected, card_width, histories, now, p);
        let activated = response.clicked()
            || (response.has_focus()
                && ui
                    .input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)));
        if activated {
            state.selected_resource = Some(item.id.clone());
            state.summary_section = None;
        }
        if selected && state.reveal_selected_resource {
            response.scroll_to_me_animation(
                Some(egui::Align::Center),
                egui::style::ScrollAnimation::none(),
            );
            state.reveal_selected_resource = false;
        }
        ui.add_space(6.0);
    }

    for section in DETAIL_SECTIONS {
        let mut entries: Vec<_> = all
            .iter()
            .filter(|item| {
                visible_section(item, &state.pinned_interfaces) == section
                    && matches_choice(item, &query)
            })
            .collect();
        entries.sort_by(|a, b| {
            (&a.device, &a.family, &a.label, &a.id).cmp(&(&b.device, &b.family, &b.label, &b.id))
        });
        if entries.is_empty() {
            continue;
        }
        ui.add_space(8.0);
        let expanded = state.expanded_sections.contains(&section) || !query.is_empty();
        let arrow = if expanded { "▾" } else { "▸" };
        ui.horizontal(|ui| {
            if ui
                .small_button(arrow)
                .on_hover_text("Expand or collapse")
                .clicked()
                && !state.expanded_sections.remove(&section)
            {
                state.expanded_sections.insert(section);
            }
            if ui
                .button(format!("{} ({})", section.label(), entries.len()))
                .on_hover_text("Open category summary table")
                .clicked()
            {
                state.summary_section = Some(section);
            }
        });
        if !expanded {
            continue;
        }
        if section == ResourceSection::CpuSensors
            && !entries.iter().any(|item| item.family == "Cooling")
        {
            ui.label(
                egui::RichText::new("Fan RPM not reported by the current read-only providers")
                    .small()
                    .color(p.secondary),
            )
            .on_hover_text("A physical fan may be present, but its tachometer reading is not exposed to Planora. A requested fan speed is not an actual RPM reading.");
        }
        let mut previous_device = String::new();
        let mut previous_family = String::new();
        for item in entries {
            if item.device != previous_device {
                previous_device.clone_from(&item.device);
                previous_family.clear();
                if !item.device.is_empty() {
                    ui.add_space(5.0);
                    if ui
                        .add_sized(
                            [ui.available_width().max(100.0), 28.0],
                            egui::Button::new(&item.device).truncate(),
                        )
                        .on_hover_text(format!("Open device measurements\n{}", item.device))
                        .clicked()
                    {
                        state.selected_resource = Some(item.id.clone());
                        state.summary_section = None;
                    }
                }
            }
            if item.family != previous_family {
                previous_family.clone_from(&item.family);
                if !item.family.is_empty() {
                    ui.label(egui::RichText::new(&item.family).small().color(p.secondary));
                }
            }
            ui.push_id(&item.id, |ui| {
                ui.horizontal(|ui| {
                    let selected = state.selected_resource.as_ref() == Some(&item.id);
                    let short_label = if item.category == 5 {
                        item.label
                            .split_once(" · ")
                            .map_or(item.label.as_str(), |(_, name)| name)
                    } else {
                        item.label.as_str()
                    };
                    let pin_available =
                        section == ResourceSection::NetworkDetails && item.id != "net:none";
                    let row_width =
                        (ui.available_width() - if pin_available { 48.0 } else { 0.0 }).max(90.0);
                    let response = ui.add_sized(
                        [row_width, 28.0],
                        egui::Button::new(format!("{}  {}", short_label, item.value))
                            .selected(selected)
                            .truncate(),
                    );
                    response.clone().on_hover_text(format!(
                        "{}\n{}\nSource: {}\nStatus: {}",
                        item.label, item.value, item.source, item.status_text
                    ));
                    if response.clicked() {
                        state.selected_resource = Some(item.id.clone());
                        state.summary_section = None;
                    }
                    if selected && state.reveal_selected_resource {
                        response.scroll_to_me_animation(
                            Some(egui::Align::Center),
                            egui::style::ScrollAnimation::none(),
                        );
                        state.reveal_selected_resource = false;
                    }
                    if pin_available
                        && ui
                            .small_button("Pin")
                            .on_hover_text("Pin to main resources")
                            .clicked()
                    {
                        state.pinned_interfaces.insert(item.id.clone());
                    }
                });
            });
        }
    }
}

fn selector_card_visuals(
    selected: bool,
    hovered: bool,
    pressed: bool,
    has_focus: bool,
    accent: Color32,
    p: Palette,
) -> (Color32, Stroke) {
    if selected {
        (p.selected, Stroke::new(1.5, accent))
    } else if pressed {
        let fill = blend_color(p.surface, accent, 0.12);
        (fill, Stroke::new(1.0, accent))
    } else if hovered || has_focus {
        let fill = blend_color(p.surface, accent, 0.06);
        let stroke = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 160);
        (fill, Stroke::new(1.0, stroke))
    } else {
        (p.surface, Stroke::new(1.0, p.border))
    }
}

pub fn resource_selector_card(
    ui: &mut egui::Ui,
    item: &Choice,
    selected: bool,
    card_width: f32,
    histories: &ResourceHistories,
    now: Instant,
    p: Palette,
) -> egui::Response {
    let mut prepared = egui::Frame::group(ui.style())
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(10))
        .begin(ui);

    {
        let ui = &mut prepared.content_ui;
        ui.set_width(card_width);
        ui.horizontal(|ui| {
            ui.colored_label(item.color, "▌");
            ui.add(
                egui::Label::new(egui::RichText::new(&item.label).strong().color(p.text))
                    .truncate()
                    .selectable(false),
            );
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&item.value)
                            .monospace()
                            .size(15.0 * crate::appearance::text_scale(ui.ctx()))
                            .strong()
                            .color(item.color),
                    )
                    .selectable(false),
                );
                ui.horizontal(|ui| {
                    ui.colored_label(item.status_color, item.status_symbol);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&item.status_text)
                                .size(11.0 * crate::appearance::text_scale(ui.ctx()))
                                .color(p.secondary),
                        )
                        .selectable(false),
                    );
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(series) = choice_series(item, histories) {
                    sparkline(ui, series, now, item.color, p);
                }
            });
        });
    }

    let alloc_resp = prepared.allocate_space(ui);
    let response = ui
        .interact(alloc_resp.rect, alloc_resp.id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(&item.label);

    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::accesskit::Role::Button, true, selected, &item.label)
    });

    let (fill, stroke) = selector_card_visuals(
        selected,
        response.hovered(),
        response.is_pointer_button_down_on(),
        response.has_focus(),
        item.color,
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

    response
}

pub fn render_resources_view(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    sensors: &[SensorReading],
    topology: Option<&Topology>,
    mut exporter: Option<&mut ExportManager>,
    state: &mut ResourcesViewState,
    percent_scale: &mut PercentScale,
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

    let mut all = choices(frame, Some(histories), sensors, p);
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
    if let Some(missing_id) = state.selected_resource.as_deref()
        && missing_id.starts_with("net:")
        && !all.iter().any(|item| item.id == missing_id)
    {
        all.push(Choice {
            id: missing_id.to_owned(),
            label: "Unavailable network interface".into(),
            value: "Unavailable".into(),
            status_text: "Device removed".into(),
            status_symbol: "○",
            status_color: p.warning,
            category: 4,
            section: ResourceSection::NetworkDetails,
            device: missing_id.to_owned(),
            family: "Activity".into(),
            color: p.network,
            series_key: format!("{missing_id}:rx"),
            source: "Windows interface inventory".into(),
            unavailable_reason: Some("Interface is no longer reported by the OS".into()),
        });
    }
    if state
        .selected_resource
        .as_ref()
        .is_none_or(|id| !all.iter().any(|item| &item.id == id))
    {
        // The native fixture CLI accepts category names while device IDs remain
        // stable and specific (for example, gpu:<LUID>:<physical adapter>).
        let requested_prefix = match state.selected_resource.as_deref() {
            Some("gpu" | "gpu:none") => Some("gpu:"),
            Some("disk" | "storage") => Some("disk:"),
            Some("network" | "net") => Some("net:"),
            Some("sensor" | "sensors") => Some("sensor:"),
            _ => None,
        };
        state.selected_resource = requested_prefix
            .and_then(|prefix| all.iter().find(|item| item.id.starts_with(prefix)))
            .or_else(|| all.first())
            .map(|item| item.id.clone());
        state.reveal_selected_resource = true;
    }
    state.sync_sensor_trend_with_selection(sensors);
    if state.reveal_selected_resource
        && let Some(selected) = all
            .iter()
            .find(|item| state.selected_resource.as_ref() == Some(&item.id))
        && selected.section != ResourceSection::Main
    {
        state.expanded_sections.insert(selected.section);
    }

    // Dismiss export message after 5 seconds
    if state
        .export_message
        .as_ref()
        .is_some_and(|(when, _)| when.elapsed() > Duration::from_secs(5))
    {
        state.export_message = None;
    }

    #[cfg(test)]
    assert!(
        ui.min_rect().right() <= ui.max_rect().right() + 1.0,
        "page header overflow"
    );
    let text_scale = crate::appearance::text_scale(ui.ctx());
    let selector_width = (240.0 * text_scale.max(1.0)).min(320.0);
    let area = ui.available_rect_before_wrap();
    state.detail_height = area.height();
    let wide = area.width() >= selector_width + 16.0 + 760.0;
    if wide {
        let selector = Rect::from_min_size(area.min, Vec2::new(selector_width, area.height()));
        let detail = Rect::from_min_max(egui::pos2(selector.right() + 16.0, area.top()), area.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(selector), |ui| {
            ui.set_width(selector.width());
            ui.set_clip_rect(selector.intersect(ui.clip_rect()));
            ui.heading("Resources");
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut state.search_query)
                    .hint_text("Filter resources...")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt("performance_resource_selector")
                .max_height(ui.available_height())
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    render_resource_selector(ui, &all, histories, state, now, p);
                });
        });
        ui.scope_builder(egui::UiBuilder::new().max_rect(detail), |ui| {
            ui.set_width(detail.width());
            ui.set_clip_rect(detail.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical()
                .id_salt("performance_details_wide")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(section) = state.summary_section {
                        render_section_summary(
                            ui,
                            section,
                            frame,
                            sensors,
                            histories,
                            &mut state.range,
                            now,
                            p,
                        );
                    } else if let Some(item) = all
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
                            percent_scale,
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
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Selected resource:").strong());
            egui::ComboBox::from_id_salt("performance_resource_picker")
                .selected_text(
                    state
                        .summary_section
                        .map(ResourceSection::label)
                        .unwrap_or_else(|| {
                            all.iter()
                                .find(|item| state.selected_resource.as_ref() == Some(&item.id))
                                .map_or("CPU", |item| item.label.as_str())
                        }),
                )
                .show_ui(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.search_query)
                            .hint_text("Find a resource"),
                    );
                    for section in std::iter::once(ResourceSection::Main).chain(DETAIL_SECTIONS) {
                        let entries: Vec<_> = all
                            .iter()
                            .filter(|item| {
                                visible_section(item, &state.pinned_interfaces) == section
                                    && matches_choice(item, &state.search_query.to_lowercase())
                            })
                            .collect();
                        if entries.is_empty() {
                            continue;
                        }
                        if section != ResourceSection::Main {
                            ui.separator();
                            if ui.button(format!("{} summary", section.label())).clicked() {
                                state.summary_section = Some(section);
                            }
                        }
                        for item in entries {
                            if ui
                                .selectable_label(
                                    state.selected_resource.as_ref() == Some(&item.id),
                                    format!("{} · {}", item.label, item.value),
                                )
                                .on_hover_text(format!("{}\nSource: {}", item.device, item.source))
                                .clicked()
                            {
                                state.selected_resource = Some(item.id.clone());
                                state.summary_section = None;
                                state.reveal_selected_resource = true;
                            }
                        }
                    }
                });
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .id_salt("performance_details_compact")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(section) = state.summary_section {
                    render_section_summary(
                        ui,
                        section,
                        frame,
                        sensors,
                        histories,
                        &mut state.range,
                        now,
                        p,
                    );
                } else if let Some(item) = all
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
                        percent_scale,
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
    }
}

fn capacity_pair(
    ui: &mut egui::Ui,
    items: [(&str, Option<u64>, Option<u64>, egui::Color32); 2],
    p: Palette,
) {
    ui.add_space(8.0);
    let draw =
        |ui: &mut egui::Ui,
         (label, used, total, color): (&str, Option<u64>, Option<u64>, egui::Color32)| {
            egui::Frame::group(ui.style())
                .inner_margin(12.0)
                .fill(p.surface)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.strong(label);
                    let value = |bytes: Option<u64>| {
                        bytes.map_or_else(|| "Unavailable".into(), format_bytes)
                    };
                    if total == Some(0) {
                        ui.label("Shared memory (UMA)");
                    } else {
                        ui.label(format!("{} / {}", value(used), value(total)));
                    }
                    capacity_bar(ui, used, total, color, p);
                });
        };
    if ui.available_width() >= 656.0 {
        ui.columns(2, |columns| {
            draw(&mut columns[0], items[0]);
            draw(&mut columns[1], items[1]);
        });
    } else {
        for item in items {
            draw(ui, item);
            ui.add_space(8.0);
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

fn summary_badge(ui: &mut egui::Ui, text: &str, text_color: egui::Color32, p: Palette) {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let max_text_width = (ui.max_rect().width() - 20.0).max(10.0);
    let galley = ui
        .painter()
        .layout(text.to_owned(), font_id, text_color, max_text_width);
    let padding = egui::vec2(20.0, 10.0);
    let mut desired_size = galley.size() + padding;
    desired_size.x = desired_size.x.min(ui.max_rect().width());
    let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), p.surface_raised);
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
        let text_pos = rect.min + egui::vec2(10.0, 5.0);
        ui.painter().galley(text_pos, galley, text_color);
    }
}

fn cpu_utilization_ring(ui: &mut egui::Ui, value: Option<f32>, p: Palette) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(72.0), egui::Sense::hover());
    let center = rect.center();
    let radius = 29.0;
    ui.painter()
        .circle_stroke(center, radius, Stroke::new(7.0, p.border_control));
    if let Some(percent) = value.filter(|value| value.is_finite()) {
        let sweep = percent.clamp(0.0, 100.0) / 100.0 * std::f32::consts::TAU;
        if sweep > 0.0 {
            let steps = ((sweep / std::f32::consts::TAU * 64.0).ceil() as usize).max(2);
            let points = (0..=steps)
                .map(|step| {
                    let angle = -std::f32::consts::FRAC_PI_2 + sweep * step as f32 / steps as f32;
                    center + Vec2::new(angle.cos(), angle.sin()) * radius
                })
                .collect();
            ui.painter()
                .add(egui::Shape::line(points, Stroke::new(7.0, p.cpu)));
        }
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            format!("{percent:.0}%"),
            egui::FontId::monospace(13.0 * crate::appearance::text_scale(ui.ctx())),
            p.text,
        );
    } else {
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            "—",
            egui::FontId::proportional(18.0 * crate::appearance::text_scale(ui.ctx())),
            p.secondary,
        );
    }
    response.on_hover_text("Current overall CPU utilization");
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
        egui::RichText::new("Select a processor for its history.")
            .color(p.secondary)
            .size(12.0 * crate::appearance::text_scale(ui.ctx())),
    );
    let mut cpus: Vec<_> = frame.logical_cpus.iter().collect();
    cpus.sort_by_key(|cpu| (cpu.group, cpu.cpu_index));
    egui::ScrollArea::horizontal()
        .id_salt("performance_cpu_heatmap")
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(ui.available_width().max(280.0));
                for (flat_idx, cpu) in cpus.iter().enumerate() {
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
                        egui::FontId::monospace(12.0 * crate::appearance::text_scale(ui.ctx())),
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
                        egui::FontId::monospace(10.0 * crate::appearance::text_scale(ui.ctx())),
                        p.secondary,
                    );
                    if response.clicked() {
                        state.selected_cpu = Some(identity);
                    }
                    let flat_cpu = u16::try_from(flat_idx).unwrap_or(cpu.cpu_index);
                    let core_class = topology.and_then(|t| {
                        t.cores
                            .iter()
                            .find(|c| c.logical_cpus.contains(&flat_cpu))
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

fn render_section_summary(
    ui: &mut egui::Ui,
    section: ResourceSection,
    frame: &TelemetryFrame,
    sensors: &[SensorReading],
    histories: &ResourceHistories,
    range: &mut HistoryRange,
    now: Instant,
    p: Palette,
) {
    ui.heading(section.label());
    ui.horizontal_wrapped(|ui| {
        ui.label("Observed range:");
        for value in [
            HistoryRange::Sec30,
            HistoryRange::Sec60,
            HistoryRange::Min5,
            HistoryRange::Min15,
        ] {
            ui.selectable_value(range, value, value.label());
        }
    });
    ui.label(egui::RichText::new("Observed values for this hardware category. Select a measurement for its trend and device table.").color(p.secondary));
    ui.add_space(8.0);
    let mut rows: Vec<_> = sensors
        .iter()
        .filter(|sensor| sensor_section(sensor) == section)
        .collect();
    rows.sort_by(|a, b| {
        (&a.device_id, &a.provider, sensor_family(a), &a.name, &a.id).cmp(&(
            &b.device_id,
            &b.provider,
            sensor_family(b),
            &b.name,
            &b.id,
        ))
    });
    if rows.is_empty() && section != ResourceSection::NetworkDetails {
        ui.label("No readings in this category.");
        return;
    }
    egui::ScrollArea::horizontal()
        .id_salt(format!("section_summary_{section:?}"))
        .show(ui, |ui| {
            egui::Grid::new(format!("section_summary_grid_{section:?}"))
                .striped(true)
                .spacing([18.0, 7.0])
                .min_row_height(30.0)
                .show(ui, |ui| {
                    for heading in [
                        "Device / measurement",
                        "Current",
                        "Minimum",
                        "Maximum",
                        "Average",
                        "Unit",
                        "Age",
                        "Status",
                    ] {
                        ui.strong(heading);
                    }
                    ui.end_row();
                    for sensor in rows {
                        let key = crate::history::sensor_series_key(sensor);
                        let summary = histories
                            .detail(&key)
                            .and_then(|series| series.observed_summary(now, *range));
                        let label = format!(
                            "{} · {} · {}",
                            sensor.device_name,
                            sensor_family(sensor),
                            sensor.name
                        );
                        ui.label(label).on_hover_text(format!(
                            "Provider: {}\nSource: {}\nDevice ID: {}\nScope: {}",
                            sensor.provider, sensor.value.source, sensor.device_id, sensor.scope
                        ));
                        ui.monospace(
                            sensor
                                .value
                                .value
                                .map_or_else(|| "—".into(), |v| format!("{v:.1}")),
                        );
                        ui.monospace(
                            summary.map_or_else(|| "—".into(), |s| format!("{:.1}", s.minimum)),
                        );
                        ui.monospace(
                            summary.map_or_else(|| "—".into(), |s| format!("{:.1}", s.maximum)),
                        );
                        ui.monospace(
                            summary.map_or_else(|| "—".into(), |s| format!("{:.1}", s.average)),
                        );
                        ui.label(&sensor.value.unit);
                        let age_ms =
                            planora_telemetry::unix_ms().saturating_sub(sensor.value.sampled_at_ms);
                        ui.monospace(format!("{}s", age_ms / 1_000));
                        let status = sensor.value.unavailable_reason.as_ref().map_or_else(
                            || {
                                if age_ms > 5_000 {
                                    "Stale".into()
                                } else {
                                    "Available".into()
                                }
                            },
                            ToString::to_string,
                        );
                        ui.label(status).on_hover_text(format!(
                            "Provider: {}\nSample time: {} ms Unix",
                            sensor.provider, sensor.value.sampled_at_ms
                        ));
                        ui.end_row();
                    }
                    if section == ResourceSection::NetworkDetails {
                        for iface in &frame.network_interfaces {
                            let status = iface.link_state.as_deref().unwrap_or("Unknown");
                            for (direction, metric, suffix) in [
                                ("Receive", &iface.read_bytes_per_sec, "rx"),
                                ("Transmit", &iface.write_bytes_per_sec, "tx"),
                            ] {
                                let key = format!("net:{}:{suffix}", iface.id);
                                let summary = histories
                                    .detail(&key)
                                    .and_then(|series| series.observed_summary(now, *range));
                                ui.label(format!("{} · {direction}", iface.label))
                                    .on_hover_text(format!(
                                        "Interface ID: {}\nType: {:?}\nHardware: {:?}\nSource: {}",
                                        iface.id,
                                        iface.interface_type,
                                        iface.is_hardware_interface,
                                        metric.source
                                    ));
                                ui.monospace(
                                    metric.value.map_or_else(
                                        || "—".into(),
                                        |v| format_rate(Some(v as f32)),
                                    ),
                                );
                                ui.monospace(
                                    summary.map_or_else(
                                        || "—".into(),
                                        |s| format_rate(Some(s.minimum)),
                                    ),
                                );
                                ui.monospace(
                                    summary.map_or_else(
                                        || "—".into(),
                                        |s| format_rate(Some(s.maximum)),
                                    ),
                                );
                                ui.monospace(
                                    summary.map_or_else(
                                        || "—".into(),
                                        |s| format_rate(Some(s.average)),
                                    ),
                                );
                                ui.label("B/s");
                                let age_ms = planora_telemetry::unix_ms()
                                    .saturating_sub(metric.sampled_at_ms);
                                ui.monospace(format!("{}s", age_ms / 1_000));
                                ui.label(status);
                                ui.end_row();
                            }
                        }
                    }
                });
        });
    if section == ResourceSection::CpuSensors
        && !sensors.iter().any(|sensor| {
            sensor_section(sensor) == section
                && sensor.sensor_type == planora_core::SensorType::FanRpm
        })
    {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("CPU fan RPM unavailable: no tachometer reading is exposed by the enabled read-only providers on this machine.").color(p.secondary));
    }
}

fn render_selected(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    sensors: &[SensorReading],
    topology: Option<&Topology>,
    mut exporter: Option<&mut ExportManager>,
    state: &mut ResourcesViewState,
    percent_scale: &mut PercentScale,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    item: &Choice,
    graph_modes: &mut [bool; 6],
    legends: &mut [bool; 6],
    now: Instant,
    p: Palette,
) {
    #[cfg(test)]
    let original_right = ui.max_rect().right();

    // 1. Tinted Resource Header with Prominent Value and Status Badges
    egui::Frame::group(ui.style())
        .fill(p.surface_raised)
        .stroke(Stroke::new(1.0, item.color))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let title_width = (ui.available_width() - 110.0).max(100.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(title_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(title_width);
                        ui.label(
                            egui::RichText::new(&item.label)
                                .size(22.0 * crate::appearance::text_scale(ui.ctx()))
                                .strong()
                                .color(p.text),
                        );
                        if item.category == 2 {
                            ui.label(
                                egui::RichText::new(if item.series_key.ends_with(":aggregate") {
                                    "Aggregate device busy"
                                } else {
                                    "Busiest engine"
                                })
                                .small()
                                .color(p.secondary),
                            );
                        }
                        ui.label(
                            egui::RichText::new(&item.value)
                                .size(38.0 * crate::appearance::text_scale(ui.ctx()))
                                .strong()
                                .monospace()
                                .color(item.color),
                        );
                        if item.category == 3 || item.category == 4 {
                            ui.label(
                                egui::RichText::new("1 s avg")
                                    .size(11.0 * crate::appearance::text_scale(ui.ctx()))
                                    .color(p.secondary),
                            );
                        }
                    },
                );

                ui.vertical(|ui| {
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
                    ui.label(
                        egui::RichText::new(badge_text)
                            .size(12.0 * crate::appearance::text_scale(ui.ctx()))
                            .strong()
                            .color(badge_color),
                    );
                    if item.category == 0 {
                        cpu_utilization_ring(ui, frame.cpu_percent.value, p);
                    }
                });
            });
        });

    ui.add_space(10.0);

    #[cfg(test)]
    assert!(
        ui.min_rect().right() <= original_right + 1.0,
        "overflow before // 2. Control: {} > {}",
        ui.min_rect().right(),
        original_right
    );
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

        if graph_modes[item.category] && item.category <= 2 {
            ui.separator();
            let is_full = matches!(percent_scale, PercentScale::Full);
            if ui.selectable_label(!is_full, "Fit scale").clicked() {
                *percent_scale = PercentScale::Fit;
            }
            if ui.selectable_label(is_full, "0–100%").clicked() {
                *percent_scale = PercentScale::Full;
            }
        }

        ui.separator();
        ui.toggle_value(&mut state.show_provider_details, "Details");
        if item.id.starts_with("net:") && item.id != "net:none" {
            let pinned = state.pinned_interfaces.contains(&item.id);
            if ui
                .button(if pinned {
                    "Unpin interface"
                } else {
                    "Pin to main resources"
                })
                .clicked()
            {
                if pinned {
                    state.pinned_interfaces.remove(&item.id);
                } else {
                    state.pinned_interfaces.insert(item.id.clone());
                }
            }
        }

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
                    .find(|s| crate::history::sensor_series_key(s) == item.id)
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
    let is_full = matches!(percent_scale, PercentScale::Full);
    let (unit, val_scale) = match item.category {
        0..=2 => ("%", ValueScale::Percent { full: is_full }),
        3 | 4 => ("B/s", ValueScale::Auto),
        _ => {
            let u = sensors
                .iter()
                .find(|s| crate::history::sensor_series_key(s) == item.id)
                .map_or("Reading", |s| s.value.unit.as_str());
            let sc = if u == "%" {
                ValueScale::Percent { full: is_full }
            } else {
                ValueScale::Auto
            };
            (u, sc)
        }
    };
    let primary_name = if item.category == 3 {
        "Read"
    } else if item.category == 4 {
        "Receive"
    } else {
        item.label.as_str()
    };

    #[cfg(test)]
    assert!(
        ui.min_rect().right() <= original_right + 1.0,
        "overflow before // 3. Summary: {} > {}",
        ui.min_rect().right(),
        original_right
    );
    // 3. Summary Metrics Bar (Prominent in both modes)
    if let Some(summary) = primary.observed_summary(now, state.range) {
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [
                ("Average", summary.average),
                ("Minimum", summary.minimum),
                ("Maximum", summary.maximum),
            ] {
                let text = format!("{label}: {}", format_summary_value(value, unit));
                summary_badge(ui, &text, p.text, p);
            }
            let cov_text = format!(
                "Coverage: {}/{} s ({:.0}%)",
                summary.observed_seconds,
                summary.expected_seconds,
                summary.coverage_percent()
            );
            summary_badge(ui, &cov_text, p.secondary, p);
        });
    } else {
        let status = if let Some(reason) = &item.unavailable_reason {
            format!("Sensor unavailable — {reason}")
        } else if histories.not_retained.contains(&item.series_key) {
            "History not retained — the bounded history budget was reached. Current readings remain available.".into()
        } else {
            "Collecting history — unobserved time remains blank.".into()
        };
        ui.label(egui::RichText::new(status).color(p.secondary));
    }

    #[cfg(test)]
    assert!(
        ui.min_rect().right() <= original_right + 1.0,
        "overflow before // 4. Large: {} > {}",
        ui.min_rect().right(),
        original_right
    );
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
                if item.category == 0 {
                    ui.colored_label(item.color, "━ CPU utilization · 1 s averages");
                } else {
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
                }
            });
        }
        let plot = state.plots.entry(item.id.clone()).or_default();
        let chart_h = (state.detail_height * 0.40).clamp(240.0, 420.0);
        if item.category == 0 {
            cpu_utilization_plot(
                ui,
                &item.id,
                &plot_series,
                state.range,
                now,
                val_scale,
                chart_h,
                plot,
                p,
            );
        } else {
            time_plot(
                ui,
                &item.id,
                &plot_series,
                state.range,
                now,
                unit,
                val_scale,
                chart_h,
                plot,
                p,
            );
        }

        if item.category == 0 {
            ui.add_space(10.0);
            ui.checkbox(&mut state.small_multiples, "Show logical processor trends")
                .on_hover_text("Show a compact 60-second trend for each logical processor");
            if state.small_multiples {
                ui.heading("Logical processor core charts");
                ui.label(
                    egui::RichText::new(
                        "Compact 60-second utilization sparklines for all logical processors.",
                    )
                    .color(p.secondary)
                    .size(12.0 * crate::appearance::text_scale(ui.ctx())),
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
                                                .size(
                                                    11.0 * crate::appearance::text_scale(ui.ctx()),
                                                ),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(format_percent(
                                                        cpu.utilization_percent.value,
                                                    ))
                                                    .monospace()
                                                    .size(
                                                        11.0 * crate::appearance::text_scale(
                                                            ui.ctx(),
                                                        ),
                                                    )
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
                            ValueScale::Percent { full: is_full },
                            180.0,
                            plot,
                            p,
                        );
                    }
                }
            }
        }

        if item.category == 1 {
            capacity_pair(
                ui,
                [
                    (
                        "Physical memory",
                        frame.memory_used_bytes.value,
                        frame.memory_total_bytes.value,
                        p.memory,
                    ),
                    (
                        "Commit usage / limit",
                        frame.commit_used_bytes.value,
                        frame.commit_total_bytes.value,
                        p.primary,
                    ),
                ],
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
            capacity_pair(
                ui,
                [
                    (
                        "Dedicated memory usage / capacity",
                        adapter.dedicated_used_bytes.value,
                        adapter.dedicated_total_bytes.value,
                        p.gpu,
                    ),
                    (
                        "Shared memory usage / capacity",
                        adapter.shared_used_bytes.value,
                        adapter.shared_total_bytes.value,
                        p.accent,
                    ),
                ],
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
                    .size(11.5 * crate::appearance::text_scale(ui.ctx())),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut state.engine_search)
                        .hint_text("Filter engines or process IDs")
                        .desired_width(f32::INFINITY),
                );
                let query = state.engine_search.to_lowercase();
                engines.retain(|engine| {
                    format!("{} {}", engine.engine_type, engine.instance)
                        .to_lowercase()
                        .contains(&query)
                });
                ui.columns(4, |columns| {
                    for (column, title) in columns.iter_mut().zip([
                        "Engine",
                        "Physical adapter",
                        "Process PID",
                        "Utilization",
                    ]) {
                        column.strong(title);
                    }
                });
                egui::ScrollArea::vertical()
                    .id_salt(("engine_picker", &item.id))
                    .max_height(192.0)
                    .auto_shrink([false, true])
                    .show_rows(
                        ui,
                        32.0 * crate::appearance::text_scale(ui.ctx()),
                        engines.len(),
                        |ui, rows| {
                            for row in rows {
                                let engine = engines[row];
                                let key = format!(
                                    "gpu-engine:{}:{:?}:{:?}:{}:{}",
                                    engine.adapter_luid.unwrap_or(0),
                                    engine.physical_adapter,
                                    engine.engine_id,
                                    engine.engine_type,
                                    engine.instance
                                );
                                ui.push_id(&key, |ui| {
                                    ui.columns(4, |columns| {
                                        let mut enabled = state.selected_engines.contains(&key);
                                        let label = engine.engine_id.map_or_else(
                                            || engine.engine_type.clone(),
                                            |id| format!("{} #{id}", engine.engine_type),
                                        );
                                        let can_select =
                                            enabled || state.selected_engines.len() < 4;
                                        if columns[0]
                                            .add_enabled(
                                                can_select,
                                                egui::Checkbox::new(&mut enabled, label),
                                            )
                                            .on_hover_text(&engine.instance)
                                            .changed()
                                        {
                                            if enabled {
                                                state.selected_engines.insert(key.clone());
                                            } else {
                                                state.selected_engines.remove(&key);
                                            }
                                        }
                                        columns[1].label(engine.physical_adapter.map_or_else(
                                            || "Not reported".into(),
                                            |id| id.to_string(),
                                        ));
                                        columns[2].label(engine.process_pid.map_or_else(
                                            || "Unattributed".into(),
                                            |id| id.to_string(),
                                        ));
                                        columns[3].label(format_percent(
                                            engine.utilization_percent.value,
                                        ));
                                    });
                                });
                            }
                        },
                    );
                if engines.is_empty() {
                    ui.label("No engines match this filter.");
                }
                let curve_names: Vec<String> = state
                    .selected_engines
                    .iter()
                    .take(4)
                    .map(|key| {
                        engines
                            .iter()
                            .find(|e| {
                                let k = format!(
                                    "gpu-engine:{}:{:?}:{:?}:{}:{}",
                                    e.adapter_luid.unwrap_or(0),
                                    e.physical_adapter,
                                    e.engine_id,
                                    e.engine_type,
                                    e.instance
                                );
                                &k == key
                            })
                            .map(|e| {
                                e.engine_id.map_or_else(
                                    || e.engine_type.clone(),
                                    |id| format!("{} #{id}", e.engine_type),
                                )
                            })
                            .unwrap_or_else(|| "Engine instance".into())
                    })
                    .collect();

                let curves: Vec<_> = state
                    .selected_engines
                    .iter()
                    .take(4)
                    .zip(&curve_names)
                    .filter_map(|(key, name)| {
                        histories.detail(key).map(|series| PlotSeries {
                            name: name.as_str(),
                            samples: series,
                            color: p.gpu,
                            dashed: false,
                        })
                    })
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
                        ValueScale::Percent { full: is_full },
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
                        ValueScale::Auto,
                        180.0,
                        plot,
                        p,
                    );
                }
            }
        }
        ui.add_space(14.0);
    }

    #[cfg(test)]
    assert!(
        ui.min_rect().right() <= original_right + 1.0,
        "overflow before // 5. Detailed: {} > {}",
        ui.min_rect().right(),
        original_right
    );
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
            3 => render_storage_tab(ui, frame, histories, p),
            4 => render_network_tab(ui, frame, histories, p),
            5 => render_sensors_tab(
                ui,
                sensors,
                histories,
                state,
                item,
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
    ui.horizontal_wrapped(|ui| {
        ui.heading("Logical Processors");
        ui.label(
            egui::RichText::new(format!(
                "Total System CPU: {}",
                format_percent(frame.cpu_percent.value)
            ))
            .strong()
            .color(p.cpu),
        );
    });

    ui.label(
        egui::RichText::new(
            "Individual logical processor loads preserving Windows processor-group addressing.",
        )
        .size(11.5 * crate::appearance::text_scale(ui.ctx()))
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

                    for (flat_idx, core) in frame.logical_cpus.iter().enumerate() {
                        ui.monospace(format!("CPU {}", core.cpu_index));
                        ui.monospace(format!("Group {}", core.group));

                        let flat_cpu = u16::try_from(flat_idx).unwrap_or(core.cpu_index);
                        let class_str = topology.and_then(|t| {
                            t.cores
                                .iter()
                                .find(|c| c.logical_cpus.contains(&flat_cpu))
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
        .size(11.5 * crate::appearance::text_scale(ui.ctx()))
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
            if cfg!(target_os = "linux") {
                "Graphics adapters discovered via DRM. AMD aggregate busy is device-wide and is not physical-engine utilization."
            } else {
                "Physical graphics adapters discovered via DXGI and engine load from Windows PDH."
            },
        )
        .size(11.5 * crate::appearance::text_scale(ui.ctx()))
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
                        ui.strong(if cfg!(target_os = "linux") {
                            "Adapter ID"
                        } else {
                            "LUID"
                        });
                        ui.strong("Activity");
                        ui.strong("Dedicated VRAM (Used / Capacity)");
                        ui.strong("Shared Memory (Used / Capacity)");
                        ui.strong("Budget");
                        ui.end_row();

                        for adapter in &frame.gpu_adapters {
                            ui.label(&adapter.name);
                            ui.monospace(format!("0x{:x}", adapter.adapter_luid));
                            let (activity, activity_name, _) = adapter.activity_percent();
                            ui.monospace(format_percent(activity.value))
                                .on_hover_text(activity_name);

                            let ded_str = match (
                                adapter.dedicated_used_bytes.value,
                                adapter.dedicated_total_bytes.value,
                            ) {
                                (Some(u), Some(t)) if t > 0 => {
                                    format!("{} / {}", format_bytes(u), format_bytes(t))
                                }
                                (_, Some(0)) => "Shared memory (UMA)".into(),
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
            .size(11.0 * crate::appearance::text_scale(ui.ctx())),
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

fn render_storage_tab(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    p: Palette,
) {
    ui.heading("Physical Disks & Block Storage");
    ui.label(
        egui::RichText::new(
            "Throughput (1-second rolling window), response time latency, and queue depth per physical disk.",
        )
        .size(11.5 * crate::appearance::text_scale(ui.ctx()))
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
                    ui.strong("Read Rate (1s)");
                    ui.strong("Write Rate (1s)");
                    ui.strong("Avg Latency");
                    ui.strong("Queue Depth");
                    ui.end_row();

                    for disk in &frame.disks {
                        ui.label(&disk.label);
                        let read_rate = if disk.read_bytes_per_sec.value.is_some() {
                            histories
                                .detail(&format!("disk:{}:read", disk.id))
                                .and_then(|ts| ts.latest_stable_value())
                                .or_else(|| disk.read_bytes_per_sec.value.map(|v| v as f32))
                        } else {
                            None
                        };
                        let write_rate = if disk.write_bytes_per_sec.value.is_some() {
                            histories
                                .detail(&format!("disk:{}:write", disk.id))
                                .and_then(|ts| ts.latest_stable_value())
                                .or_else(|| disk.write_bytes_per_sec.value.map(|v| v as f32))
                        } else {
                            None
                        };
                        ui.monospace(format_rate(read_rate));
                        ui.monospace(format_rate(write_rate));

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

fn render_network_tab(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    p: Palette,
) {
    ui.heading("Network Adapters & Interfaces");
    ui.label(
        egui::RichText::new(
            "Network interface bandwidth (1-second rolling window), link connection status, and theoretical link speed.",
        )
        .size(11.5 * crate::appearance::text_scale(ui.ctx()))
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
                    ui.strong("Receive Rate (1s)");
                    ui.strong("Transmit Rate (1s)");
                    ui.strong("Link State");
                    ui.strong("Link Speed");
                    ui.end_row();

                    for iface in &frame.network_interfaces {
                        ui.label(&iface.label);
                        let rx_rate = if iface.read_bytes_per_sec.value.is_some() {
                            histories
                                .detail(&format!("net:{}:rx", iface.id))
                                .and_then(|ts| ts.latest_stable_value())
                                .or_else(|| iface.read_bytes_per_sec.value.map(|v| v as f32))
                        } else {
                            None
                        };
                        let tx_rate = if iface.write_bytes_per_sec.value.is_some() {
                            histories
                                .detail(&format!("net:{}:tx", iface.id))
                                .and_then(|ts| ts.latest_stable_value())
                                .or_else(|| iface.write_bytes_per_sec.value.map(|v| v as f32))
                        } else {
                            None
                        };
                        ui.monospace(format_rate(rx_rate));
                        ui.monospace(format_rate(tx_rate));

                        let state_str = iface.link_state.as_deref().unwrap_or("Unknown");
                        let state_color = match state_str {
                            "Connected" | "Up" => p.success,
                            "Disconnected" => p.error,
                            _ => p.secondary,
                        };
                        ui.colored_label(state_color, state_str);

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
    item: &Choice,
    show_graphs: bool,
    exporter: Option<&mut ExportManager>,
    now: Instant,
    p: Palette,
) {
    let selected = sensors
        .iter()
        .find(|sensor| crate::history::sensor_series_key(sensor) == item.id);
    let sensors: Vec<SensorReading> = if let Some(selected) = selected {
        sensors
            .iter()
            .filter(|sensor| {
                sensor_section(sensor) == sensor_section(selected)
                    && sensor.device_id == selected.device_id
                    && sensor.provider == selected.provider
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    ui.horizontal(|ui| {
        ui.heading("Device measurements");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(exp) = exporter
                && ui
                    .button("Export Sensors CSV")
                    .on_hover_text("Export current sensor snapshot to CSV")
                    .clicked()
            {
                match exp.sensors(sensors.clone()) {
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
            "Exact readings and observed range. Provider and device identity remain available in the table.",
        )
        .size(11.5 * crate::appearance::text_scale(ui.ctx()))
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
            .size(11.5 * crate::appearance::text_scale(ui.ctx()))
            .color(p.secondary),
        );
        return;
    }

    if show_graphs {
        ui.checkbox(
            &mut state.compare_sensors,
            "Compare up to four compatible readings from this device",
        );
    }
    // The selected reading already has a large chart above. Comparison is opt-in.
    if show_graphs && state.compare_sensors {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Trend Group:").strong());
            let mut trend_kinds = vec![
                SensorTrendKind::Power,
                SensorTrendKind::Temperature,
                SensorTrendKind::Clock,
                SensorTrendKind::Fan,
                SensorTrendKind::Load,
            ];
            for extra in [
                SensorTrendKind::Voltage,
                SensorTrendKind::Current,
                SensorTrendKind::Throughput,
            ] {
                if sensors.iter().any(|s| extra.matches_sensor(s)) {
                    trend_kinds.push(extra);
                }
            }
            for kind in trend_kinds {
                ui.selectable_value(&mut state.selected_sensor_trend, kind, kind.label());
            }
        });

        let current_kind = state.selected_sensor_trend;
        let mut matching_sensors: Vec<&SensorReading> = sensors
            .iter()
            .filter(|s| current_kind.matches_sensor(s))
            .collect();
        matching_sensors.sort_by_key(|sensor| {
            (
                crate::history::sensor_series_key(sensor) != item.id,
                sensor.name.clone(),
            )
        });
        matching_sensors.truncate(4);

        if !matching_sensors.is_empty() {
            ui.add_space(6.0);
            let colors = [
                p.accent, p.cpu, p.gpu, p.memory, p.disk, p.network, p.warning, p.success,
            ];
            let mut name_counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for sensor in &matching_sensors {
                *name_counts.entry(sensor.name.as_str()).or_insert(0) += 1;
            }
            let series_labels: Vec<String> = matching_sensors
                .iter()
                .map(|sensor| {
                    if name_counts.get(sensor.name.as_str()).copied().unwrap_or(0) > 1 {
                        let dev = if !sensor.device_name.is_empty() {
                            sensor.device_name.as_str()
                        } else if !sensor.device_id.is_empty() {
                            sensor.device_id.as_str()
                        } else {
                            sensor.scope.as_str()
                        };
                        format!("{dev} · {}", sensor.name)
                    } else {
                        sensor.name.clone()
                    }
                })
                .collect();
            let mut plot_series: Vec<PlotSeries<'_>> = Vec::new();
            for (idx, sensor) in matching_sensors.iter().enumerate() {
                let key = crate::history::sensor_series_key(sensor);
                if let Some(s) = histories.detail(&key) {
                    let color = colors[idx % colors.len()];
                    plot_series.push(PlotSeries {
                        name: &series_labels[idx],
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
                        ValueScale::Percent { full: false }
                    } else {
                        ValueScale::Auto
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
            let message = if current_kind == SensorTrendKind::Fan {
                "No fan RPM reading is exposed by the current read-only providers. A physical fan may still be present; Planora will not substitute requested speed or fan duty for measured RPM.".to_owned()
            } else {
                format!(
                    "No active sensors matching {} on this device.",
                    current_kind.label()
                )
            };
            ui.label(egui::RichText::new(message).color(p.secondary));
        }
        ui.add_space(12.0);
    }

    // 2. Device-Grouped Sensor Tables
    let mut groups: BTreeMap<(String, String), Vec<&SensorReading>> = BTreeMap::new();
    for sensor in &sensors {
        let device_name = if !sensor.device_name.is_empty() {
            sensor.device_name.clone()
        } else if !sensor.device_id.is_empty() {
            sensor.device_id.clone()
        } else {
            format!("{} Device", sensor.scope)
        };
        let group_id = if !sensor.device_id.is_empty() {
            sensor.device_id.clone()
        } else {
            device_name.clone()
        };
        groups
            .entry((device_name, group_id))
            .or_default()
            .push(sensor);
    }

    let mut device_name_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for (device_name, _) in groups.keys() {
        *device_name_counts.entry(device_name.as_str()).or_insert(0) += 1;
    }

    for ((device_name, group_id), device_sensors) in &groups {
        let heading_text = if device_name_counts
            .get(device_name.as_str())
            .copied()
            .unwrap_or(0)
            > 1
        {
            format!("{device_name} ({group_id})")
        } else {
            device_name.clone()
        };
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(&heading_text)
                .size(14.0 * crate::appearance::text_scale(ui.ctx()))
                .strong()
                .color(p.text),
        );
        ui.add_space(2.0);

        egui::ScrollArea::horizontal()
            .id_salt(format!("performance_sensors_{group_id}"))
            .show(ui, |ui| {
                egui::Grid::new(format!("grid_sensors_{group_id}"))
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
                            let key = crate::history::sensor_series_key(sensor);
                            let summary = histories
                                .detail(&key)
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
