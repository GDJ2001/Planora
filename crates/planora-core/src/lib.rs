//! Portable versioned contracts shared by the UI, helper, and collectors.
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 4;
pub const SUPPORTED_SAMPLE_INTERVALS_MS: [u64; 5] = [100, 250, 500, 1_000, 2_000];
pub const MAX_REQUEST_SIZE: usize = 64 * 1024;
pub const MAX_RESPONSE_SIZE: usize = 8 * 1024 * 1024;
pub const MAX_PROCESSES_LIMIT: usize = 65_536;

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
    pub physical_adapter: u32,
    pub name: String,
    pub busiest_engine_percent: Metric<f32>,
    pub dedicated_used_bytes: Metric<u64>,
    pub dedicated_total_bytes: Metric<u64>,
    pub shared_used_bytes: Metric<u64>,
    pub shared_total_bytes: Metric<u64>,
    #[serde(default)]
    pub budget_bytes: Option<Metric<u64>>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SensorType {
    GpuChipPowerWatts,
    GpuBoardPowerWatts,
    CpuPackagePowerWatts,
    ThermalZoneCelsius,
    GpuTemperatureCelsius,
    CpuTemperatureCelsius,
    FanRpm,
    ClockMhz,
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
            Self::FanRpm => write!(f, "Fan Speed"),
            Self::ClockMhz => write!(f, "Clock Frequency"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SensorReading {
    pub name: String,
    pub sensor_type: SensorType,
    pub scope: String,
    pub provider: String,
    pub value: Metric<f32>,
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
    #[error("memory used ({used}) exceeds total ({total})")]
    MemoryExceedsTotal { used: u64, total: u64 },
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
            }
            if let Some(queue) = &device.queue_depth {
                validate_metric(queue, "device.queue_depth")?;
            }
            if let Some(speed) = &device.link_speed_bps {
                validate_metric(speed, "device.link_speed_bps")?;
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
        for sensor in &self.sensors {
            validate_metric(&sensor.value, "sensor.value")?;
            if let Some(val) = sensor.value.value
                && !val.is_finite()
            {
                return Err(FrameValidationError::NonFiniteFloat {
                    field: "sensor.value",
                });
            }
        }

        if let (Some(used), Some(total)) =
            (self.memory_used_bytes.value, self.memory_total_bytes.value)
            && total > 0
            && used > total
        {
            return Err(FrameValidationError::MemoryExceedsTotal { used, total });
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
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProfileResult {
    pub target_desc: String,
    pub duration_seconds: u32,
    pub elapsed_ms: u64,
    pub events_traced: u64,
    pub events_lost: u64,
    pub attribution_coverage_percent: f32,
    pub disk_records: Vec<DiskIoProfileRecord>,
    pub pmu_records: Vec<PmuProfileRecord>,
    pub unattributed_read_bytes: u64,
    pub unattributed_write_bytes: u64,
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
}
