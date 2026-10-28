//! Portable versioned contracts shared by the UI, helper, and collectors.
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 6;
pub const SUPPORTED_SAMPLE_INTERVALS_MS: [u64; 5] = [100, 250, 500, 1_000, 2_000];
pub const MAX_REQUEST_SIZE: usize = 64 * 1024;
pub const MAX_RESPONSE_SIZE: usize = 8 * 1024 * 1024;
pub const MAX_PROCESSES_LIMIT: usize = 65_536;

pub const UNIT_PERCENT: &str = "%";
pub const UNIT_BYTES: &str = "bytes";
pub const UNIT_BYTES_PER_SEC: &str = "B/s";
pub const UNIT_BITS_PER_SEC: &str = "bps";
pub const UNIT_WATTS: &str = "W";
pub const UNIT_CELSIUS: &str = "°C";
pub const UNIT_MHZ: &str = "MHz";
pub const UNIT_VOLTS: &str = "V";
pub const UNIT_AMPERES: &str = "A";
pub const UNIT_RPM: &str = "RPM";
pub const UNIT_JOULES: &str = "J";
pub const UNIT_MILLISECONDS: &str = "ms";
pub const UNIT_QUEUE_DEPTH: &str = "requests";

pub fn canonicalize_unit(unit: &str) -> &str {
    match unit {
        "bytes/s" | "B/s" => UNIT_BYTES_PER_SEC,
        "%" => UNIT_PERCENT,
        "bytes" | "B" => UNIT_BYTES,
        "bps" | "bits/s" => UNIT_BITS_PER_SEC,
        "W" | "watts" | "Watts" => UNIT_WATTS,
        "°C" | "C" | "degC" => UNIT_CELSIUS,
        "MHz" | "mhz" => UNIT_MHZ,
        "V" | "volts" => UNIT_VOLTS,
        "A" | "amperes" => UNIT_AMPERES,
        "RPM" | "rpm" => UNIT_RPM,
        "J" | "joules" => UNIT_JOULES,
        "ms" => UNIT_MILLISECONDS,
        _ => unit,
    }
}

/// Typed classification of why a metric cannot be provided.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AvailabilityReason {
    WarmingUp,
    Unsupported(String),
    PermissionDenied(String),
    TemporarilyFailed(String),
    Stale(u64),
}

impl std::fmt::Display for AvailabilityReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WarmingUp => write!(f, "Warming up"),
            Self::Unsupported(r) => write!(f, "Unsupported ({r})"),
            Self::PermissionDenied(r) => write!(f, "Permission denied ({r})"),
            Self::TemporarilyFailed(r) => write!(f, "Temporarily unavailable ({r})"),
            Self::Stale(elapsed_ms) => write!(f, "Stale data ({elapsed_ms} ms old)"),
        }
    }
}

/// Provenance and mechanism used to track process creation identity.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum IdentitySource {
    WindowsBasicProcessSequence,
    WindowsProcessFiletime,
    LinuxProcStartTicks,
    DarwinStartTime,
    SysinfoEpochSeconds,
}

impl std::fmt::Display for IdentitySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowsBasicProcessSequence => {
                write!(f, "Windows sequence (SystemBasicProcessInformation)")
            }
            Self::WindowsProcessFiletime => write!(f, "Windows FILETIME"),
            Self::LinuxProcStartTicks => write!(f, "Linux /proc/[pid]/stat start ticks"),
            Self::DarwinStartTime => write!(f, "macOS start time"),
            Self::SysinfoEpochSeconds => write!(f, "sysinfo epoch seconds"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// OS creation identity / start timestamp or ticks.
    pub started: u64,
    /// Monotonic sequence number (e.g. from Windows 11 SystemBasicProcessInformation) if supported.
    pub sequence_number: Option<u64>,
    pub source: IdentitySource,
}

impl ProcessIdentity {
    /// A Windows process mutation can only use a creation FILETIME that the
    /// helper can compare with the opened process handle.
    pub fn valid_for_windows_action(&self) -> bool {
        self.pid != 0
            && self.started != 0
            && self.source == IdentitySource::WindowsProcessFiletime
            && self.sequence_number != Some(0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Metric<T> {
    pub value: Option<T>,
    pub unit: String,
    pub source: String,
    pub sampled_at_ms: u64,
    pub unavailable_reason: Option<AvailabilityReason>,
}

impl<T> Default for Metric<T> {
    fn default() -> Self {
        Self {
            value: None,
            unit: String::new(),
            source: String::new(),
            sampled_at_ms: 0,
            unavailable_reason: Some(AvailabilityReason::WarmingUp),
        }
    }
}

impl<T> Metric<T> {
    pub fn available(
        value: T,
        unit: impl Into<String>,
        source: impl Into<String>,
        sampled_at_ms: u64,
    ) -> Self {
        Self {
            value: Some(value),
            unit: unit.into(),
            source: source.into(),
            sampled_at_ms,
            unavailable_reason: None,
        }
    }

    pub fn unavailable(
        unit: impl Into<String>,
        source: impl Into<String>,
        sampled_at_ms: u64,
        reason: AvailabilityReason,
    ) -> Self {
        Self {
            value: None,
            unit: unit.into(),
            source: source.into(),
            sampled_at_ms,
            unavailable_reason: Some(reason),
        }
    }

    pub fn is_available(&self) -> bool {
        self.value.is_some()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ComponentId {
    Package(u16),
    NumaNode(u16),
    PhysicalCore(u16),
    LogicalCpu(u16),
    Cache(u32),
    Gpu(u16),
    Storage(u16),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CoreDescriptor {
    pub id: u16,
    pub package_id: Option<u16>,
    pub numa_node: Option<u16>,
    pub logical_cpus: Vec<u16>,
    pub class: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CacheType {
    Unified,
    Data,
    Instruction,
    Unknown,
}

impl std::fmt::Display for CacheType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unified => write!(f, "Unified"),
            Self::Data => write!(f, "Data"),
            Self::Instruction => write!(f, "Instruction"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CacheDescriptor {
    pub id: u32,
    pub level: u8,
    pub cache_type: CacheType,
    pub size_bytes: u64,
    pub line_bytes: Option<u16>,
    pub shared_by: Vec<u16>,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GpuDescriptor {
    pub id: u16,
    pub name: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Topology {
    pub generation: u64,
    pub vendor: String,
    pub model: String,
    pub architecture: String,
    pub cores: Vec<CoreDescriptor>,
    pub caches: Vec<CacheDescriptor>,
    #[serde(default)]
    pub gpus: Vec<GpuDescriptor>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProcessRecord {
    pub identity: ProcessIdentity,
    pub name: String,
    pub cpu_percent: Metric<f32>,
    pub memory_bytes: Metric<u64>,
    pub disk_read_bytes_per_sec: Metric<u64>,
    pub disk_write_bytes_per_sec: Metric<u64>,
    pub io_read_bytes_per_sec: Metric<u64>,
    pub io_write_bytes_per_sec: Metric<u64>,
    pub gpu_percent: Metric<f32>,
    pub affinity: Metric<Vec<u16>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GpuEngineSample {
    /// Windows performance counter instance, including its adapter LUID and engine identity.
    pub instance: String,
    pub engine_type: String,
    #[serde(default)]
    pub adapter_luid: Option<u64>,
    #[serde(default)]
    pub physical_adapter: Option<u32>,
    #[serde(default)]
    pub engine_id: Option<u32>,
    #[serde(default)]
    pub process_pid: Option<u32>,
    pub utilization_percent: Metric<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LogicalCpuSample {
    pub cpu_index: u16,
    pub group: u16,
    pub utilization_percent: Metric<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GpuAdapterSample {
    pub adapter_luid: u64,
    #[serde(default)]
    pub physical_adapter: Option<u32>,
    pub name: String,
    pub busiest_engine_percent: Metric<f32>,
    pub dedicated_used_bytes: Metric<u64>,
    pub dedicated_total_bytes: Metric<u64>,
    pub shared_used_bytes: Metric<u64>,
    pub shared_total_bytes: Metric<u64>,
    #[serde(default)]
    pub budget_bytes: Option<Metric<u64>>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SensorType {
    GpuChipPowerWatts,
    GpuBoardPowerWatts,
    CpuPackagePowerWatts,
    ThermalZoneCelsius,
    GpuTemperatureCelsius,
    CpuTemperatureCelsius,
    StorageTemperatureCelsius,
    FanRpm,
    ClockMhz,
    VoltageVolts,
    CurrentAmperes,
    LoadPercent,
    EnergyJoules,
}

impl std::fmt::Display for SensorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GpuChipPowerWatts => write!(f, "GPU Chip Power"),
            Self::GpuBoardPowerWatts => write!(f, "GPU Board Power"),
            Self::CpuPackagePowerWatts => write!(f, "CPU Package Power"),
            Self::ThermalZoneCelsius => write!(f, "Thermal Zone"),
            Self::GpuTemperatureCelsius => write!(f, "GPU Temperature"),
            Self::CpuTemperatureCelsius => write!(f, "CPU Temperature"),
            Self::StorageTemperatureCelsius => write!(f, "Storage Temperature"),
            Self::FanRpm => write!(f, "Fan Speed"),
            Self::ClockMhz => write!(f, "Clock Frequency"),
            Self::VoltageVolts => write!(f, "Voltage"),
            Self::CurrentAmperes => write!(f, "Current"),
            Self::LoadPercent => write!(f, "Load"),
            Self::EnergyJoules => write!(f, "Energy"),
        }
    }
}

impl SensorType {
    pub fn is_plausible_value(&self, val: f32) -> bool {
        if !val.is_finite() {
            return false;
        }
        match self {
            Self::GpuTemperatureCelsius
            | Self::CpuTemperatureCelsius
            | Self::ThermalZoneCelsius
            | Self::StorageTemperatureCelsius => {
                (-40.0..=150.0).contains(&val)
                    && (val - 127.0).abs() > 0.01
                    && (val - 128.0).abs() > 0.01
                    && (val - 255.0).abs() > 0.01
            }
            Self::GpuChipPowerWatts | Self::GpuBoardPowerWatts | Self::CpuPackagePowerWatts => {
                (0.0..=2_500.0).contains(&val)
            }
            Self::FanRpm => (0.0..=50_000.0).contains(&val) && (val - 65535.0).abs() > 0.1,
            Self::ClockMhz => (0.0..=15_000.0).contains(&val),
            Self::VoltageVolts => (0.0..=30.0).contains(&val),
            Self::CurrentAmperes => (0.0..=1_500.0).contains(&val),
            Self::LoadPercent => (0.0..=100.0).contains(&val),
            Self::EnergyJoules => val >= 0.0,
        }
    }

    pub fn expected_unit(&self) -> &'static str {
        match self {
            Self::GpuTemperatureCelsius
            | Self::CpuTemperatureCelsius
            | Self::ThermalZoneCelsius
            | Self::StorageTemperatureCelsius => UNIT_CELSIUS,
            Self::GpuChipPowerWatts | Self::GpuBoardPowerWatts | Self::CpuPackagePowerWatts => {
                UNIT_WATTS
            }
            Self::FanRpm => UNIT_RPM,
            Self::ClockMhz => UNIT_MHZ,
            Self::VoltageVolts => UNIT_VOLTS,
            Self::CurrentAmperes => UNIT_AMPERES,
            Self::LoadPercent => UNIT_PERCENT,
            Self::EnergyJoules => UNIT_JOULES,
        }
    }

    pub fn validate_reading(&self, reading: &SensorReading) -> Result<(), FrameValidationError> {
        let name = "sensor.value";
        validate_metric(&reading.value, name)?;
        if canonicalize_unit(&reading.value.unit) != canonicalize_unit(self.expected_unit()) {
            return Err(FrameValidationError::UnitMismatch {
                field: name,
                expected: self.expected_unit(),
                actual: reading.value.unit.clone(),
            });
        }
        if let Some(val) = reading.value.value
            && !self.is_plausible_value(val)
        {
            return Err(FrameValidationError::OutOfPhysicalBounds {
                field: name,
                value: val,
                min: match self {
                    Self::GpuTemperatureCelsius
                    | Self::CpuTemperatureCelsius
                    | Self::ThermalZoneCelsius
                    | Self::StorageTemperatureCelsius => -40.0,
                    _ => 0.0,
                },
                max: match self {
                    Self::GpuTemperatureCelsius
                    | Self::CpuTemperatureCelsius
                    | Self::ThermalZoneCelsius
                    | Self::StorageTemperatureCelsius => 150.0,
                    Self::GpuChipPowerWatts
                    | Self::GpuBoardPowerWatts
                    | Self::CpuPackagePowerWatts => 2500.0,
                    Self::FanRpm => 50000.0,
                    Self::ClockMhz => 15000.0,
                    Self::VoltageVolts => 30.0,
                    Self::CurrentAmperes => 1500.0,
                    Self::LoadPercent => 100.0,
                    Self::EnergyJoules => f32::MAX,
                },
            });
        }
        Ok(())
    }
}

/// Hardware owner of a sensor; independent of its measured unit or display name.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum HardwareCategory {
    Cpu,
    Gpu,
    Memory,
    Storage,
    Network,
    MotherboardCooling,
    Other,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SensorReading {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
    pub name: String,
    #[serde(default)]
    pub hardware_category: HardwareCategory,
    pub sensor_type: SensorType,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub provider: String,
    pub value: Metric<f32>,
}

impl SensorReading {
    pub fn is_plausible(&self) -> bool {
        if let Some(val) = self.value.value {
            self.sensor_type.is_plausible_value(val)
        } else {
            true
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ResourceDeviceSample {
    /// Stable provider identity within a session; the display name may change.
    pub id: String,
    pub label: String,
    pub read_bytes_per_sec: Metric<u64>,
    pub write_bytes_per_sec: Metric<u64>,
    #[serde(default)]
    pub latency_ms: Option<Metric<f32>>,
    #[serde(default)]
    pub queue_depth: Option<Metric<f32>>,
    #[serde(default)]
    pub link_speed_bps: Option<Metric<u64>>,
    #[serde(default)]
    pub link_state: Option<String>,
    /// OS-reported hardware interface flag; absent for older recordings/providers.
    #[serde(default)]
    pub is_hardware_interface: Option<bool>,
    /// OS interface type, retained for details without inferring identity from names.
    #[serde(default)]
    pub interface_type: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TelemetryFrame {
    pub protocol_version: u32,
    pub session_id: u64,
    pub topology_generation: u64,
    pub sequence: u64,
    pub sampled_at_ms: u64,
    pub collection_duration_ms: f32,
    pub cpu_percent: Metric<f32>,
    #[serde(default)]
    pub logical_cpus: Vec<LogicalCpuSample>,
    pub memory_used_bytes: Metric<u64>,
    #[serde(default)]
    pub memory_available_bytes: Metric<u64>,
    pub memory_total_bytes: Metric<u64>,
    #[serde(default)]
    pub commit_used_bytes: Metric<u64>,
    #[serde(default)]
    pub commit_total_bytes: Metric<u64>,
    pub disk_read_bytes_per_sec: Metric<u64>,
    pub disk_write_bytes_per_sec: Metric<u64>,
    pub network_rx_bytes_per_sec: Metric<u64>,
    pub network_tx_bytes_per_sec: Metric<u64>,
    pub gpu_percent: Metric<f32>,
    #[serde(default)]
    pub gpu_adapters: Vec<GpuAdapterSample>,
    pub gpu_engines: Vec<GpuEngineSample>,
    pub gpu_engine_unavailable_reason: Option<AvailabilityReason>,
    pub disks: Vec<ResourceDeviceSample>,
    pub network_interfaces: Vec<ResourceDeviceSample>,
    pub package_power_watts: Metric<f32>,
    #[serde(default)]
    pub sensors: Vec<SensorReading>,
    pub processes: Vec<ProcessRecord>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FrameValidationError {
    #[error("protocol version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u32, actual: u32 },
    #[error("non-finite float in field '{field}'")]
    NonFiniteFloat { field: &'static str },
    #[error("percentage in '{field}' out of bounds: {value}")]
    PercentageOutOfBounds { field: &'static str, value: f32 },
    #[error("metric in '{field}' out of physical bounds: {value} (min: {min}, max: {max})")]
    OutOfPhysicalBounds {
        field: &'static str,
        value: f32,
        min: f32,
        max: f32,
    },
    #[error("unit mismatch in '{field}': expected '{expected}', got '{actual}'")]
    UnitMismatch {
        field: &'static str,
        expected: &'static str,
        actual: String,
    },
    #[error("memory used ({used}) exceeds total ({total})")]
    MemoryExceedsTotal { used: u64, total: u64 },
    #[error("memory available ({available}) exceeds total ({total})")]
    AvailableExceedsTotal { available: u64, total: u64 },
    #[error("commit memory used ({used}) exceeds total ({total})")]
    CommitExceedsTotal { used: u64, total: u64 },
    #[error("GPU dedicated memory used ({used}) exceeds total ({total})")]
    DedicatedExceedsTotal { used: u64, total: u64 },
    #[error(
        "metric '{field}' has contradictory state (has_value={has_value}, has_reason={has_reason})"
    )]
    ContradictoryMetricState {
        field: &'static str,
        has_value: bool,
        has_reason: bool,
    },
    #[error("process count ({count}) exceeds maximum allowed ({max})")]
    ExcessiveProcesses { count: usize, max: usize },
    #[error("device count ({count}) exceeds maximum allowed ({max})")]
    ExcessiveDevices { count: usize, max: usize },
}

fn validate_metric<T>(m: &Metric<T>, name: &'static str) -> Result<(), FrameValidationError> {
    let has_value = m.value.is_some();
    let has_reason = m.unavailable_reason.is_some();
    if has_value == has_reason {
        return Err(FrameValidationError::ContradictoryMetricState {
            field: name,
            has_value,
            has_reason,
        });
    }
    Ok(())
}

fn validate_pct(m: &Metric<f32>, name: &'static str) -> Result<(), FrameValidationError> {
    validate_metric(m, name)?;
    if let Some(val) = m.value {
        if !val.is_finite() {
            return Err(FrameValidationError::NonFiniteFloat { field: name });
        }
        if !(0.0..=100.0).contains(&val) {
            return Err(FrameValidationError::PercentageOutOfBounds {
                field: name,
                value: val,
            });
        }
    }
    Ok(())
}

impl TelemetryFrame {
    pub fn validate(&self) -> Result<(), FrameValidationError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(FrameValidationError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                actual: self.protocol_version,
            });
        }
        if !self.collection_duration_ms.is_finite() {
            return Err(FrameValidationError::NonFiniteFloat {
                field: "collection_duration_ms",
            });
        }
        validate_pct(&self.cpu_percent, "cpu_percent")?;
        for cpu in &self.logical_cpus {
            validate_pct(&cpu.utilization_percent, "logical_cpu.utilization_percent")?;
        }
        validate_pct(&self.gpu_percent, "gpu_percent")?;
        for adapter in &self.gpu_adapters {
            validate_pct(
                &adapter.busiest_engine_percent,
                "gpu_adapter.busiest_engine_percent",
            )?;
            validate_metric(
                &adapter.dedicated_used_bytes,
                "gpu_adapter.dedicated_used_bytes",
            )?;
            validate_metric(
                &adapter.dedicated_total_bytes,
                "gpu_adapter.dedicated_total_bytes",
            )?;
            validate_metric(&adapter.shared_used_bytes, "gpu_adapter.shared_used_bytes")?;
            validate_metric(
                &adapter.shared_total_bytes,
                "gpu_adapter.shared_total_bytes",
            )?;
            if let Some(budget) = &adapter.budget_bytes {
                validate_metric(budget, "gpu_adapter.budget_bytes")?;
            }
            if let (Some(used), Some(total)) = (
                adapter.dedicated_used_bytes.value,
                adapter.dedicated_total_bytes.value,
            ) && (used > total || total == 0)
            {
                return Err(FrameValidationError::DedicatedExceedsTotal { used, total });
            }
            if let (Some(used), Some(total)) = (
                adapter.shared_used_bytes.value,
                adapter.shared_total_bytes.value,
            ) && (used > total || total == 0)
            {
                return Err(FrameValidationError::DedicatedExceedsTotal { used, total });
            }
        }
        for engine in &self.gpu_engines {
            validate_pct(
                &engine.utilization_percent,
                "gpu_engine.utilization_percent",
            )?;
        }
        if !self.gpu_engines.is_empty() && self.gpu_engine_unavailable_reason.is_some() {
            return Err(FrameValidationError::ContradictoryMetricState {
                field: "gpu_engines",
                has_value: true,
                has_reason: true,
            });
        }
        let device_count = self.disks.len() + self.network_interfaces.len();
        if device_count > 4096 {
            return Err(FrameValidationError::ExcessiveDevices {
                count: device_count,
                max: 4096,
            });
        }
        for device in self.disks.iter().chain(&self.network_interfaces) {
            validate_metric(&device.read_bytes_per_sec, "device.read_bytes_per_sec")?;
            validate_metric(&device.write_bytes_per_sec, "device.write_bytes_per_sec")?;
            if let Some(latency) = &device.latency_ms {
                validate_metric(latency, "device.latency_ms")?;
                if let Some(val) = latency.value
                    && (!val.is_finite() || !(0.0..=60_000.0).contains(&val))
                {
                    return Err(FrameValidationError::OutOfPhysicalBounds {
                        field: "device.latency_ms",
                        value: val,
                        min: 0.0,
                        max: 60_000.0,
                    });
                }
            }
            if let Some(queue) = &device.queue_depth {
                validate_metric(queue, "device.queue_depth")?;
                if let Some(val) = queue.value
                    && (!val.is_finite() || !(0.0..=65_536.0).contains(&val))
                {
                    return Err(FrameValidationError::OutOfPhysicalBounds {
                        field: "device.queue_depth",
                        value: val,
                        min: 0.0,
                        max: 65_536.0,
                    });
                }
            }
            if let Some(speed) = &device.link_speed_bps {
                validate_metric(speed, "device.link_speed_bps")?;
                if let Some(spd) = speed.value
                    && (spd == 0 || spd == u64::MAX || spd > 10_000_000_000_000)
                {
                    return Err(FrameValidationError::OutOfPhysicalBounds {
                        field: "device.link_speed_bps",
                        value: spd as f32,
                        min: 1.0,
                        max: 10_000_000_000_000.0,
                    });
                }
            }
        }
        validate_metric(&self.memory_used_bytes, "memory_used_bytes")?;
        validate_metric(&self.memory_available_bytes, "memory_available_bytes")?;
        validate_metric(&self.memory_total_bytes, "memory_total_bytes")?;
        validate_metric(&self.commit_used_bytes, "commit_used_bytes")?;
        validate_metric(&self.commit_total_bytes, "commit_total_bytes")?;
        validate_metric(&self.disk_read_bytes_per_sec, "disk_read_bytes_per_sec")?;
        validate_metric(&self.disk_write_bytes_per_sec, "disk_write_bytes_per_sec")?;
        validate_metric(&self.network_rx_bytes_per_sec, "network_rx_bytes_per_sec")?;
        validate_metric(&self.network_tx_bytes_per_sec, "network_tx_bytes_per_sec")?;
        validate_metric(&self.package_power_watts, "package_power_watts")?;
        if let Some(val) = self.package_power_watts.value {
            if !val.is_finite() || !(0.0..=2_500.0).contains(&val) {
                return Err(FrameValidationError::OutOfPhysicalBounds {
                    field: "package_power_watts",
                    value: val,
                    min: 0.0,
                    max: 2_500.0,
                });
            }
            if canonicalize_unit(&self.package_power_watts.unit) != canonicalize_unit(UNIT_WATTS) {
                return Err(FrameValidationError::UnitMismatch {
                    field: "package_power_watts",
                    expected: UNIT_WATTS,
                    actual: self.package_power_watts.unit.clone(),
                });
            }
        }
        for sensor in &self.sensors {
            sensor.sensor_type.validate_reading(sensor)?;
        }

        if let (Some(used), Some(total)) =
            (self.memory_used_bytes.value, self.memory_total_bytes.value)
            && total > 0
            && used > total
        {
            return Err(FrameValidationError::MemoryExceedsTotal { used, total });
        }

        if let (Some(available), Some(total)) = (
            self.memory_available_bytes.value,
            self.memory_total_bytes.value,
        ) && total > 0
            && available > total
        {
            return Err(FrameValidationError::AvailableExceedsTotal { available, total });
        }

        if let (Some(used), Some(total)) =
            (self.commit_used_bytes.value, self.commit_total_bytes.value)
            && total > 0
            && used > total
        {
            return Err(FrameValidationError::CommitExceedsTotal { used, total });
        }

        if self.processes.len() > MAX_PROCESSES_LIMIT {
            return Err(FrameValidationError::ExcessiveProcesses {
                count: self.processes.len(),
                max: MAX_PROCESSES_LIMIT,
            });
        }

        for p in &self.processes {
            validate_pct(&p.cpu_percent, "process.cpu_percent")?;
            validate_pct(&p.gpu_percent, "process.gpu_percent")?;
            validate_metric(&p.memory_bytes, "process.memory_bytes")?;
            validate_metric(
                &p.disk_read_bytes_per_sec,
                "process.disk_read_bytes_per_sec",
            )?;
            validate_metric(
                &p.disk_write_bytes_per_sec,
                "process.disk_write_bytes_per_sec",
            )?;
            validate_metric(&p.io_read_bytes_per_sec, "process.io_read_bytes_per_sec")?;
            validate_metric(&p.io_write_bytes_per_sec, "process.io_write_bytes_per_sec")?;
            validate_metric(&p.affinity, "process.affinity")?;
        }

        Ok(())
    }

    /// Sanitizes known hardware sensor/counter provider anomalies (such as disconnected
    /// fan tachometers, unpopulated diodes, or driver rate jitter) into unavailable metrics
    /// so an isolated anomalous sensor never fails validation of the whole frame.
    pub fn sanitize_provider_anomalies(&mut self) {
        fn sanitize_pct_metric(metric: &mut Metric<f32>, label: &str) {
            let canonical = canonicalize_unit(&metric.unit);
            if metric.unit != canonical {
                metric.unit = canonical.to_string();
            }
            if let Some(val) = metric.value {
                if val > 100.0 && val <= 100.5 {
                    metric.value = Some(100.0);
                } else if !val.is_finite() || !(0.0..=100.0).contains(&val) {
                    metric.value = None;
                    metric.unavailable_reason = Some(AvailabilityReason::TemporarilyFailed(
                        format!("{label} percentage out of bounds: {val}"),
                    ));
                }
            }
        }

        fn canonicalize_metric_unit<T>(metric: &mut Metric<T>) {
            let canonical = canonicalize_unit(&metric.unit);
            if metric.unit != canonical {
                metric.unit = canonical.to_string();
            }
        }

        fn sanitize_used_against_total(
            used_metric: &mut Metric<u64>,
            total: Option<u64>,
            zero_total_reason: Option<&str>,
            label: &str,
        ) {
            if let Some(total_val) = total
                && let Some(used_val) = used_metric.value
            {
                if total_val == 0 {
                    used_metric.value = None;
                    used_metric.unavailable_reason = Some(AvailabilityReason::Unsupported(
                        zero_total_reason
                            .unwrap_or("Capacity reported as zero")
                            .into(),
                    ));
                } else if used_val > total_val {
                    if used_val <= total_val.saturating_mul(2) {
                        used_metric.value = Some(total_val);
                    } else {
                        used_metric.value = None;
                        used_metric.unavailable_reason =
                            Some(AvailabilityReason::TemporarilyFailed(format!(
                                "{label} usage ({used_val}) exceeded total capacity ({total_val})"
                            )));
                    }
                }
            }
        }

        // Canonicalize top-level units & sanitize percentages
        sanitize_pct_metric(&mut self.cpu_percent, "CPU");
        sanitize_pct_metric(&mut self.gpu_percent, "GPU");
        for cpu in &mut self.logical_cpus {
            sanitize_pct_metric(&mut cpu.utilization_percent, "Logical CPU");
        }
        for m in [
            &mut self.memory_used_bytes,
            &mut self.memory_available_bytes,
            &mut self.memory_total_bytes,
            &mut self.commit_used_bytes,
            &mut self.commit_total_bytes,
            &mut self.disk_read_bytes_per_sec,
            &mut self.disk_write_bytes_per_sec,
            &mut self.network_rx_bytes_per_sec,
            &mut self.network_tx_bytes_per_sec,
        ] {
            canonicalize_metric_unit(m);
        }
        canonicalize_metric_unit(&mut self.package_power_watts);

        let mem_total = self.memory_total_bytes.value;
        sanitize_used_against_total(&mut self.memory_used_bytes, mem_total, None, "Memory");
        sanitize_used_against_total(
            &mut self.memory_available_bytes,
            mem_total,
            None,
            "Available memory",
        );
        let commit_total = self.commit_total_bytes.value;
        sanitize_used_against_total(&mut self.commit_used_bytes, commit_total, None, "Commit");

        // Sanitize individual sensor readings
        for sensor in &mut self.sensors {
            canonicalize_metric_unit(&mut sensor.value);
            if let Some(val) = sensor.value.value
                && (!sensor.sensor_type.is_plausible_value(val)
                    || canonicalize_unit(&sensor.value.unit)
                        != canonicalize_unit(sensor.sensor_type.expected_unit()))
            {
                let bad_unit = sensor.value.unit.clone();
                sensor.value.unit = sensor.sensor_type.expected_unit().to_string();
                sensor.value.value = None;
                sensor.value.unavailable_reason =
                    Some(AvailabilityReason::TemporarilyFailed(format!(
                        "Filtered implausible {} reading: {:.1} {}",
                        sensor.sensor_type, val, bad_unit
                    )));
            }
        }

        // Sanitize package_power_watts if out of bounds or wrong unit
        if let Some(val) = self.package_power_watts.value
            && (!val.is_finite()
                || !(0.0..=2_500.0).contains(&val)
                || self.package_power_watts.unit != UNIT_WATTS)
        {
            self.package_power_watts.unit = UNIT_WATTS.to_string();
            self.package_power_watts.value = None;
            self.package_power_watts.unavailable_reason =
                Some(AvailabilityReason::TemporarilyFailed(format!(
                    "Filtered implausible package power: {val:.1} W"
                )));
        }

        // Sanitize device link speeds, latencies, queue depths, and units
        for dev in self.disks.iter_mut().chain(&mut self.network_interfaces) {
            canonicalize_metric_unit(&mut dev.read_bytes_per_sec);
            canonicalize_metric_unit(&mut dev.write_bytes_per_sec);
            if let Some(spd) = &mut dev.link_speed_bps {
                canonicalize_metric_unit(spd);
                if let Some(val) = spd.value
                    && (val == 0 || val == u64::MAX || val > 10_000_000_000_000)
                {
                    spd.value = None;
                    spd.unavailable_reason = Some(AvailabilityReason::Unsupported(
                        "Link speed unknown or invalid".into(),
                    ));
                }
            }
            if let Some(lat) = &mut dev.latency_ms {
                canonicalize_metric_unit(lat);
                if let Some(val) = lat.value
                    && (!val.is_finite() || !(0.0..=60_000.0).contains(&val))
                {
                    lat.value = None;
                    lat.unavailable_reason = Some(AvailabilityReason::TemporarilyFailed(
                        "Disk latency counter out of bounds".into(),
                    ));
                }
            }
            if let Some(qd) = &mut dev.queue_depth {
                canonicalize_metric_unit(qd);
                if let Some(val) = qd.value
                    && (!val.is_finite() || !(0.0..=65_536.0).contains(&val))
                {
                    qd.value = None;
                    qd.unavailable_reason = Some(AvailabilityReason::TemporarilyFailed(
                        "Disk queue counter out of bounds".into(),
                    ));
                }
            }
        }

        // Sanitize GPU engine & adapter metrics
        for engine in &mut self.gpu_engines {
            sanitize_pct_metric(&mut engine.utilization_percent, "GPU engine");
        }
        for adapter in &mut self.gpu_adapters {
            sanitize_pct_metric(&mut adapter.busiest_engine_percent, "GPU adapter");
            canonicalize_metric_unit(&mut adapter.dedicated_used_bytes);
            canonicalize_metric_unit(&mut adapter.dedicated_total_bytes);
            canonicalize_metric_unit(&mut adapter.shared_used_bytes);
            canonicalize_metric_unit(&mut adapter.shared_total_bytes);
            if let Some(budget) = &mut adapter.budget_bytes {
                canonicalize_metric_unit(budget);
            }
            let ded_total = adapter.dedicated_total_bytes.value;
            sanitize_used_against_total(
                &mut adapter.dedicated_used_bytes,
                ded_total,
                Some("Dedicated VRAM not applicable on unified memory architecture"),
                "Dedicated VRAM",
            );
            let shr_total = adapter.shared_total_bytes.value;
            sanitize_used_against_total(
                &mut adapter.shared_used_bytes,
                shr_total,
                Some("Shared GPU memory capacity reported as zero"),
                "Shared GPU memory",
            );
        }

        // Sanitize per-process percentages & canonicalize units
        for proc in &mut self.processes {
            sanitize_pct_metric(&mut proc.cpu_percent, "Process CPU");
            sanitize_pct_metric(&mut proc.gpu_percent, "Process GPU");
            canonicalize_metric_unit(&mut proc.memory_bytes);
            canonicalize_metric_unit(&mut proc.disk_read_bytes_per_sec);
            canonicalize_metric_unit(&mut proc.disk_write_bytes_per_sec);
            canonicalize_metric_unit(&mut proc.io_read_bytes_per_sec);
            canonicalize_metric_unit(&mut proc.io_write_bytes_per_sec);
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CapabilityState {
    Available,
    WarmingUp,
    RequiresAction,
    MissingRuntime,
    PermissionDenied,
    Unsupported,
    TemporarilyFailed,
    DeviceRemoved,
    NotApplicable,
    NotImplemented,
}

impl CapabilityState {
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Capability {
    pub name: String,
    pub scope: String,
    pub provider: String,
    pub state: CapabilityState,
    pub last_probed_at_ms: u64,
    pub reason: Option<String>,
    pub recovery_action: Option<String>,
}

impl Capability {
    pub fn available(&self) -> bool {
        self.state.is_available()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkPoint {
    pub working_set_bytes: u64,
    pub min_ns: f64,
    pub median_ns: f64,
    pub p95_ns: f64,
    pub p99_ns: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CorePair {
    pub cpu_a: u16,
    pub cpu_b: u16,
    pub estimated_one_way_ns: f64,
    pub min_ns: f64,
    pub p95_ns: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkResult {
    pub mode: String,
    pub methodology: String,
    pub comparable_to_x86_cycles: bool,
    pub is_partial: bool,
    pub points: Vec<BenchmarkPoint>,
    #[serde(default)]
    pub core_pairs: Vec<CorePair>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCode {
    VersionMismatch,
    Unauthorized,
    PermissionDenied,
    NotFound,
    Busy,
    Cancelled,
    InvalidInput,
    Timeout,
    PidReused,
    Unsupported,
    Internal,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum BenchmarkState {
    Idle,
    Running {
        job_id: u64,
        elapsed_ms: u64,
    },
    Cancelling {
        job_id: u64,
    },
    Completed {
        job_id: u64,
        result: BenchmarkResult,
    },
    Failed {
        job_id: u64,
        code: ErrorCode,
        message: String,
    },
    Cancelled {
        job_id: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProfileTarget {
    DiskIo {
        target_process: Option<ProcessIdentity>,
        duration_seconds: u32,
    },
    PmuSampling {
        target_process: Option<ProcessIdentity>,
        duration_seconds: u32,
        sources: Vec<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DiskIoProfileRecord {
    pub pid: u32,
    #[serde(default)]
    pub identity: Option<ProcessIdentity>,
    pub process_name: String,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_operations: u64,
    pub write_operations: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PmuProfileRecord {
    pub source: String,
    pub sample_count: u64,
    pub target_pid: Option<u32>,
    #[serde(default)]
    pub target_identity: Option<ProcessIdentity>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PmuSourceInfo {
    pub id: u32,
    pub name: String,
    pub min_interval: u32,
    pub max_interval: u32,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sampling_unit: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProfileResult {
    pub target_desc: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub sampling_semantics: Option<String>,
    #[serde(default)]
    pub effective_sample_hz: Option<u32>,
    #[serde(default)]
    pub complete: Option<bool>,
    pub duration_seconds: u32,
    pub elapsed_ms: u64,
    pub events_traced: u64,
    pub events_lost: u64,
    pub attribution_coverage_percent: f32,
    pub disk_records: Vec<DiskIoProfileRecord>,
    pub pmu_records: Vec<PmuProfileRecord>,
    pub unattributed_read_bytes: u64,
    pub unattributed_write_bytes: u64,
    #[serde(default)]
    pub unattributed_operations: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ProfileState {
    Idle,
    Running {
        job_id: u64,
        target_desc: String,
        elapsed_ms: u64,
        duration_seconds: u32,
    },
    Cancelling {
        job_id: u64,
    },
    Completed {
        job_id: u64,
        result: ProfileResult,
    },
    Failed {
        job_id: u64,
        code: ErrorCode,
        message: String,
    },
    Cancelled {
        job_id: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticEntry {
    pub timestamp_ms: u64,
    pub operation_id: u64,
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub request_id: u64,
    pub command: Command,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub request_id: u64,
    pub response: Response,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Command {
    Hello {
        version: u32,
    },
    Topology,
    Snapshot,
    SetSamplingInterval {
        interval_ms: u64,
    },
    Capabilities,
    RefreshCapabilities,
    RunCacheProbe {
        job_id: u64,
        page_mode: bool,
    },
    RunCoreMatrix {
        job_id: u64,
        logical_cpus: Vec<u16>,
    },
    CancelBenchmark {
        job_id: u64,
    },
    BenchmarkStatus,
    StartProfile {
        job_id: u64,
        target: ProfileTarget,
    },
    CancelProfile {
        job_id: u64,
    },
    ProfileStatus,
    PmuSources,
    GetAffinity {
        process: ProcessIdentity,
    },
    SetAffinity {
        process: ProcessIdentity,
        logical_cpus: Vec<u16>,
    },
    DiagnosticLogs,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Response {
    Hello {
        version: u32,
        session_id: u64,
        helper_authorized: bool,
    },
    Topology(Topology),
    Snapshot(Box<TelemetryFrame>),
    SamplingStatus {
        requested_ms: u64,
        effective_ms: u64,
    },
    Capabilities(Vec<Capability>),
    BenchmarkStatus(BenchmarkState),
    ProfileStatus(ProfileState),
    PmuSources(Vec<PmuSourceInfo>),
    Affinity {
        process: ProcessIdentity,
        logical_cpus: Vec<u16>,
    },
    AffinityChanged {
        process: ProcessIdentity,
        previous_cpus: Vec<u16>,
        logical_cpus: Vec<u16>,
    },
    DiagnosticLogs(Vec<DiagnosticEntry>),
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privileged_windows_action_requires_comparable_creation_identity() {
        let mut identity = ProcessIdentity {
            pid: 42,
            started: 123,
            sequence_number: None,
            source: IdentitySource::WindowsProcessFiletime,
        };
        assert!(identity.valid_for_windows_action());
        identity.started = 0;
        assert!(!identity.valid_for_windows_action());
        identity.started = 123;
        identity.pid = 0;
        assert!(!identity.valid_for_windows_action());
        identity.pid = 42;
        identity.source = IdentitySource::WindowsBasicProcessSequence;
        assert!(!identity.valid_for_windows_action());
        identity.source = IdentitySource::WindowsProcessFiletime;
        identity.sequence_number = Some(0);
        assert!(!identity.valid_for_windows_action());
    }

    #[test]
    fn unavailable_is_distinct_from_zero() {
        let zero = Metric::available(0u64, "bytes", "test", 1);
        let absent = Metric::<u64>::unavailable(
            "bytes",
            "test",
            1,
            AvailabilityReason::Unsupported("unsupported".into()),
        );
        assert_ne!(zero.value, absent.value);
        assert!(absent.unavailable_reason.is_some());
        assert_eq!(zero.unit, "bytes");
        assert_eq!(absent.unit, "bytes");
    }

    #[test]
    fn process_identity_includes_creation_and_source() {
        let a = ProcessIdentity {
            pid: 12,
            started: 1,
            sequence_number: Some(100),
            source: IdentitySource::WindowsBasicProcessSequence,
        };
        let b = ProcessIdentity {
            started: 2,
            ..a.clone()
        };
        assert_ne!(a, b);
        assert_eq!(
            a.source.to_string(),
            "Windows sequence (SystemBasicProcessInformation)"
        );
    }

    #[test]
    fn metric_serialization_round_trip() {
        let m = Metric::available(42.5f32, "%", "test_source", 100);
        let s = serde_json::to_string(&m).unwrap();
        let d: Metric<f32> = serde_json::from_str(&s).unwrap();
        assert_eq!(d.value, Some(42.5));
        assert_eq!(d.unit, "%");
        assert_eq!(d.source, "test_source");
        assert_eq!(d.sampled_at_ms, 100);
        assert!(d.unavailable_reason.is_none());
    }

    #[test]
    fn availability_reason_display_formatting() {
        assert_eq!(AvailabilityReason::WarmingUp.to_string(), "Warming up");
        assert_eq!(
            AvailabilityReason::Unsupported("no PMU".into()).to_string(),
            "Unsupported (no PMU)"
        );
        assert_eq!(
            AvailabilityReason::PermissionDenied("need root".into()).to_string(),
            "Permission denied (need root)"
        );
        assert_eq!(
            AvailabilityReason::Stale(1500).to_string(),
            "Stale data (1500 ms old)"
        );
    }

    #[test]
    fn frame_validation_catches_invalid_percentages_and_contradictions() {
        let valid_frame = TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            session_id: 1,
            topology_generation: 1,
            sequence: 1,
            sampled_at_ms: 1000,
            collection_duration_ms: 12.5,
            cpu_percent: Metric::available(45.0, "%", "sys", 1000),
            logical_cpus: Vec::new(),
            memory_used_bytes: Metric::available(4_000_000, "bytes", "sys", 1000),
            memory_available_bytes: Metric::available(4_000_000, "bytes", "sys", 1000),
            memory_total_bytes: Metric::available(8_000_000, "bytes", "sys", 1000),
            commit_used_bytes: Metric::available(5_000_000, "bytes", "sys", 1000),
            commit_total_bytes: Metric::available(10_000_000, "bytes", "sys", 1000),
            disk_read_bytes_per_sec: Metric::available(1024, "bytes/s", "sys", 1000),
            disk_write_bytes_per_sec: Metric::available(2048, "bytes/s", "sys", 1000),
            network_rx_bytes_per_sec: Metric::available(512, "bytes/s", "sys", 1000),
            network_tx_bytes_per_sec: Metric::available(256, "bytes/s", "sys", 1000),
            gpu_percent: Metric::unavailable(
                "%",
                "sys",
                1000,
                AvailabilityReason::Unsupported("no gpu".into()),
            ),
            gpu_adapters: Vec::new(),
            gpu_engines: Vec::new(),
            gpu_engine_unavailable_reason: Some(AvailabilityReason::WarmingUp),
            disks: Vec::new(),
            network_interfaces: Vec::new(),
            package_power_watts: Metric::unavailable(
                "W",
                "sys",
                1000,
                AvailabilityReason::Unsupported("no rapl".into()),
            ),
            sensors: Vec::new(),
            processes: vec![],
        };

        assert!(valid_frame.validate().is_ok());

        let mut contradictory_gpu = valid_frame.clone();
        contradictory_gpu.gpu_engines.push(GpuEngineSample {
            instance: "engine-0".into(),
            engine_type: "3D".into(),
            adapter_luid: None,
            physical_adapter: None,
            engine_id: None,
            process_pid: None,
            utilization_percent: Metric::available(4.0, "%", "pdh", 1000),
        });
        assert!(matches!(
            contradictory_gpu.validate(),
            Err(FrameValidationError::ContradictoryMetricState {
                field: "gpu_engines",
                ..
            })
        ));

        let mut invalid_device = valid_frame.clone();
        invalid_device.disks.push(ResourceDeviceSample {
            id: "disk0".into(),
            label: "Disk 0".into(),
            read_bytes_per_sec: Metric::available(100, "bytes/s", "test", 1000),
            write_bytes_per_sec: Metric::unavailable(
                "bytes/s",
                "test",
                1000,
                AvailabilityReason::WarmingUp,
            ),
            latency_ms: None,
            queue_depth: None,
            link_speed_bps: None,
            is_hardware_interface: None,
            interface_type: None,
            link_state: None,
        });
        assert!(invalid_device.validate().is_ok());
        invalid_device.disks[0]
            .read_bytes_per_sec
            .unavailable_reason = Some(AvailabilityReason::WarmingUp);
        assert!(matches!(
            invalid_device.validate(),
            Err(FrameValidationError::ContradictoryMetricState {
                field: "device.read_bytes_per_sec",
                ..
            })
        ));

        // Test contradictory metric (has value AND reason)
        let mut contradictory = valid_frame.clone();
        contradictory.cpu_percent.unavailable_reason = Some(AvailabilityReason::WarmingUp);
        assert!(matches!(
            contradictory.validate(),
            Err(FrameValidationError::ContradictoryMetricState { .. })
        ));

        // Test out of bounds percentage (>100%)
        let mut out_of_bounds = valid_frame.clone();
        out_of_bounds.cpu_percent.value = Some(150.0);
        assert!(matches!(
            out_of_bounds.validate(),
            Err(FrameValidationError::PercentageOutOfBounds { .. })
        ));

        // Test non-finite float
        let mut non_finite = valid_frame.clone();
        non_finite.cpu_percent.value = Some(f32::NAN);
        assert!(matches!(
            non_finite.validate(),
            Err(FrameValidationError::NonFiniteFloat { .. })
        ));

        // Test memory used > memory total
        let mut memory_exceeds = valid_frame.clone();
        memory_exceeds.memory_used_bytes.value = Some(9_000_000);
        assert!(matches!(
            memory_exceeds.validate(),
            Err(FrameValidationError::MemoryExceedsTotal { .. })
        ));

        // Test version mismatch
        let mut bad_version = valid_frame.clone();
        bad_version.protocol_version = 999;
        assert!(matches!(
            bad_version.validate(),
            Err(FrameValidationError::VersionMismatch { .. })
        ));
    }

    #[test]
    fn canonicalize_unit_handles_standard_variations() {
        assert_eq!(canonicalize_unit("bytes/s"), UNIT_BYTES_PER_SEC);
        assert_eq!(canonicalize_unit("B/s"), UNIT_BYTES_PER_SEC);
        assert_eq!(canonicalize_unit("%"), UNIT_PERCENT);
        assert_eq!(canonicalize_unit("W"), UNIT_WATTS);
        assert_eq!(canonicalize_unit("Watts"), UNIT_WATTS);
        assert_eq!(canonicalize_unit("°C"), UNIT_CELSIUS);
        assert_eq!(canonicalize_unit("C"), UNIT_CELSIUS);
        assert_eq!(canonicalize_unit("RPM"), UNIT_RPM);
        assert_eq!(canonicalize_unit("MHz"), UNIT_MHZ);
    }

    #[test]
    fn sensor_reading_physical_bounds_and_unit_validation() {
        let reading = SensorReading {
            id: "s1".into(),
            device_id: "cpu0".into(),
            device_name: "CPU".into(),
            name: "Core 0 Temp".into(),
            hardware_category: HardwareCategory::Unknown,
            sensor_type: SensorType::CpuTemperatureCelsius,
            scope: "Core".into(),
            provider: "test".into(),
            value: Metric::available(65.0, "°C", "test", 1000),
        };
        assert!(reading.sensor_type.validate_reading(&reading).is_ok());

        // Sentinel temperature 255°C rejected
        let mut bad_temp = reading.clone();
        bad_temp.value.value = Some(255.0);
        assert!(matches!(
            bad_temp.sensor_type.validate_reading(&bad_temp),
            Err(FrameValidationError::OutOfPhysicalBounds { .. })
        ));

        // Negative temperature < -40°C rejected
        bad_temp.value.value = Some(-50.0);
        assert!(matches!(
            bad_temp.sensor_type.validate_reading(&bad_temp),
            Err(FrameValidationError::OutOfPhysicalBounds { .. })
        ));

        // Fan RPM cannot have unit "%"
        let bad_fan_unit = SensorReading {
            id: "fan1".into(),
            device_id: "gpu0".into(),
            device_name: "GPU".into(),
            name: "Fan".into(),
            hardware_category: HardwareCategory::Unknown,
            sensor_type: SensorType::FanRpm,
            scope: "Cooling".into(),
            provider: "test".into(),
            value: Metric::available(50.0, "%", "test", 1000),
        };
        assert!(matches!(
            bad_fan_unit.sensor_type.validate_reading(&bad_fan_unit),
            Err(FrameValidationError::UnitMismatch { .. })
        ));

        // Disconnected fan tachometer sentinel (65535 RPM) rejected
        let mut disconnected_fan = bad_fan_unit.clone();
        disconnected_fan.value.unit = "RPM".into();
        disconnected_fan.value.value = Some(65535.0);
        assert!(matches!(
            disconnected_fan
                .sensor_type
                .validate_reading(&disconnected_fan),
            Err(FrameValidationError::OutOfPhysicalBounds { .. })
        ));
    }

    #[test]
    fn sanitize_provider_anomalies_isolates_faulty_hardware_sensor() {
        let mut frame = TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            session_id: 1,
            topology_generation: 1,
            sequence: 1,
            sampled_at_ms: 1000,
            collection_duration_ms: 10.0,
            cpu_percent: Metric::available(10.0, "%", "sys", 1000),
            logical_cpus: Vec::new(),
            memory_used_bytes: Metric::available(1000, "bytes", "sys", 1000),
            memory_available_bytes: Metric::available(2000, "bytes", "sys", 1000),
            memory_total_bytes: Metric::available(3000, "bytes", "sys", 1000),
            commit_used_bytes: Metric::available(1000, "bytes", "sys", 1000),
            commit_total_bytes: Metric::available(3000, "bytes", "sys", 1000),
            disk_read_bytes_per_sec: Metric::available(0, "B/s", "sys", 1000),
            disk_write_bytes_per_sec: Metric::available(0, "B/s", "sys", 1000),
            network_rx_bytes_per_sec: Metric::available(0, "B/s", "sys", 1000),
            network_tx_bytes_per_sec: Metric::available(0, "B/s", "sys", 1000),
            gpu_percent: Metric::unavailable(
                "%",
                "sys",
                1000,
                AvailabilityReason::Unsupported("none".into()),
            ),
            gpu_adapters: Vec::new(),
            gpu_engines: Vec::new(),
            gpu_engine_unavailable_reason: None,
            disks: vec![ResourceDeviceSample {
                id: "d0".into(),
                label: "Disk 0".into(),
                read_bytes_per_sec: Metric::available(0, "B/s", "sys", 1000),
                write_bytes_per_sec: Metric::available(0, "B/s", "sys", 1000),
                latency_ms: Some(Metric::available(100_000.0, "ms", "sys", 1000)), // corrupted latency
                queue_depth: None,
                link_speed_bps: Some(Metric::available(u64::MAX, "bps", "sys", 1000)), // NDIS unknown sentinel
                is_hardware_interface: None,
                interface_type: None,
                link_state: None,
            }],
            network_interfaces: Vec::new(),
            package_power_watts: Metric::available(5000.0, "W", "sys", 1000), // out of bounds power
            sensors: vec![
                SensorReading {
                    id: "good_temp".into(),
                    device_id: "cpu0".into(),
                    device_name: "CPU".into(),
                    name: "CPU Package".into(),
                    hardware_category: HardwareCategory::Unknown,
                    sensor_type: SensorType::CpuTemperatureCelsius,
                    scope: "Package".into(),
                    provider: "test".into(),
                    value: Metric::available(55.0, "°C", "test", 1000),
                },
                SensorReading {
                    id: "bad_fan".into(),
                    device_id: "fan0".into(),
                    device_name: "Chassis".into(),
                    name: "Fan 1".into(),
                    hardware_category: HardwareCategory::Unknown,
                    sensor_type: SensorType::FanRpm,
                    scope: "Cooling".into(),
                    provider: "test".into(),
                    value: Metric::available(65535.0, "RPM", "test", 1000), // sentinel disconnected tach
                },
            ],
            processes: Vec::new(),
        };

        // Before sanitization, frame validation fails on bad_fan / corrupted latency / bad link speed
        assert!(frame.validate().is_err());

        // After sanitization, anomalies are converted to Unavailable and the frame is valid!
        frame.sanitize_provider_anomalies();
        assert!(frame.validate().is_ok());

        assert_eq!(frame.sensors[0].value.value, Some(55.0));
        assert!(frame.sensors[1].value.value.is_none());
        assert!(frame.sensors[1].value.unavailable_reason.is_some());
        assert!(frame.disks[0].latency_ms.as_ref().unwrap().value.is_none());
        assert!(
            frame.disks[0]
                .link_speed_bps
                .as_ref()
                .unwrap()
                .value
                .is_none()
        );
        assert!(frame.package_power_watts.value.is_none());
    }

    #[test]
    fn optional_hardware_metadata_preserves_legacy_v5_frames() {
        let sensor = SensorReading {
            id: "cpu/temp/0".into(),
            device_id: "cpu:0".into(),
            device_name: "CPU".into(),
            name: "Package".into(),
            hardware_category: HardwareCategory::Cpu,
            sensor_type: SensorType::CpuTemperatureCelsius,
            scope: "Package".into(),
            provider: "broker".into(),
            value: Metric::available(55.0, "°C", "broker", 1_000),
        };
        let mut old_sensor = serde_json::to_value(&sensor).unwrap();
        old_sensor
            .as_object_mut()
            .unwrap()
            .remove("hardware_category");
        let imported: SensorReading = serde_json::from_value(old_sensor).unwrap();
        assert_eq!(imported.hardware_category, HardwareCategory::Unknown);
        assert_eq!(imported.id, sensor.id);

        let iface = ResourceDeviceSample {
            id: "luid:1".into(),
            label: "Ethernet".into(),
            read_bytes_per_sec: Metric::available(0, "B/s", "OS", 1_000),
            write_bytes_per_sec: Metric::available(0, "B/s", "OS", 1_000),
            latency_ms: None,
            queue_depth: None,
            link_speed_bps: None,
            link_state: Some("Connected".into()),
            is_hardware_interface: Some(true),
            interface_type: Some(6),
        };
        let mut old_iface = serde_json::to_value(&iface).unwrap();
        old_iface
            .as_object_mut()
            .unwrap()
            .remove("is_hardware_interface");
        old_iface.as_object_mut().unwrap().remove("interface_type");
        let imported: ResourceDeviceSample = serde_json::from_value(old_iface).unwrap();
        assert_eq!(imported.is_hardware_interface, None);
        assert_eq!(imported.interface_type, None);
        assert_eq!(imported.id, iface.id);
    }

    #[test]
    fn sanitize_uma_and_vram_overshoot_preserves_contract() {
        let mut frame = TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            session_id: 1,
            topology_generation: 1,
            sequence: 2,
            sampled_at_ms: 1000,
            collection_duration_ms: 2.0,
            cpu_percent: Metric::available(12.0, "%", "sys", 1000),
            logical_cpus: Vec::new(),
            memory_used_bytes: Metric::available(1000, "bytes", "sys", 1000),
            memory_available_bytes: Metric::available(2000, "bytes", "sys", 1000),
            memory_total_bytes: Metric::available(3000, "bytes", "sys", 1000),
            commit_used_bytes: Metric::available(1000, "bytes", "sys", 1000),
            commit_total_bytes: Metric::available(3000, "bytes", "sys", 1000),
            disk_read_bytes_per_sec: Metric::available(10, "bytes/s", "legacy", 1000),
            disk_write_bytes_per_sec: Metric::available(20, "bytes/s", "legacy", 1000),
            network_rx_bytes_per_sec: Metric::available(30, "bytes/s", "legacy", 1000),
            network_tx_bytes_per_sec: Metric::available(40, "bytes/s", "legacy", 1000),
            gpu_percent: Metric::available(100.3, "%", "pdh", 1000),
            gpu_adapters: vec![
                GpuAdapterSample {
                    adapter_luid: 1,
                    physical_adapter: Some(0),
                    name: "Integrated UMA GPU".into(),
                    busiest_engine_percent: Metric::available(100.2, "%", "pdh", 1000),
                    dedicated_used_bytes: Metric::available(0, "bytes", "pdh", 1000),
                    dedicated_total_bytes: Metric::available(0, "bytes", "dxgi", 1000),
                    shared_used_bytes: Metric::available(512, "bytes", "pdh", 1000),
                    shared_total_bytes: Metric::available(4096, "bytes", "dxgi", 1000),
                    budget_bytes: None,
                },
                GpuAdapterSample {
                    adapter_luid: 2,
                    physical_adapter: Some(0),
                    name: "Discrete GPU".into(),
                    busiest_engine_percent: Metric::available(99.0, "%", "pdh", 1000),
                    dedicated_used_bytes: Metric::available(8_050_000_000, "bytes", "pdh", 1000),
                    dedicated_total_bytes: Metric::available(8_000_000_000, "bytes", "dxgi", 1000),
                    shared_used_bytes: Metric::available(128, "bytes", "pdh", 1000),
                    shared_total_bytes: Metric::available(4096, "bytes", "dxgi", 1000),
                    budget_bytes: None,
                },
            ],
            gpu_engines: Vec::new(),
            gpu_engine_unavailable_reason: Some(AvailabilityReason::WarmingUp),
            disks: Vec::new(),
            network_interfaces: Vec::new(),
            package_power_watts: Metric::unavailable(
                "W",
                "sys",
                1000,
                AvailabilityReason::Unsupported("none".into()),
            ),
            sensors: Vec::new(),
            processes: Vec::new(),
        };

        assert!(matches!(
            frame.validate(),
            Err(FrameValidationError::PercentageOutOfBounds { .. }
                | FrameValidationError::DedicatedExceedsTotal { .. })
        ));

        frame.sanitize_provider_anomalies();
        assert!(frame.validate().is_ok());
        assert_eq!(frame.gpu_percent.value, Some(100.0));
        assert_eq!(frame.disk_read_bytes_per_sec.unit, UNIT_BYTES_PER_SEC);
        assert!(frame.gpu_adapters[0].dedicated_used_bytes.value.is_none());
        assert!(matches!(
            frame.gpu_adapters[0]
                .dedicated_used_bytes
                .unavailable_reason,
            Some(AvailabilityReason::Unsupported(_))
        ));
        assert_eq!(
            frame.gpu_adapters[1].dedicated_used_bytes.value,
            Some(8_000_000_000)
        );
    }
}
