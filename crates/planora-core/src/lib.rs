//! Portable contracts shared by the UI, helper, and collectors.
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// OS creation identity, in the units stated by `source`.
    pub started: u64,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metric<T> {
    pub value: Option<T>,
    pub unit: String,
    pub source: String,
    pub sampled_at_ms: u64,
    pub unavailable_reason: Option<String>,
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
        reason: impl Into<String>,
    ) -> Self {
        Self {
            value: None,
            unit: unit.into(),
            source: source.into(),
            sampled_at_ms,
            unavailable_reason: Some(reason.into()),
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
    Cache { level: u8, instance: u16 },
    Gpu(u16),
    Storage(u16),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoreDescriptor {
    pub id: u16,
    pub package_id: Option<u16>,
    pub numa_node: Option<u16>,
    pub logical_cpus: Vec<u16>,
    pub class: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheDescriptor {
    pub level: u8,
    pub size_bytes: u64,
    pub line_bytes: u16,
    pub shared_by: Vec<u16>,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Topology {
    pub vendor: String,
    pub model: String,
    pub architecture: String,
    pub cores: Vec<CoreDescriptor>,
    pub caches: Vec<CacheDescriptor>,
    #[serde(default)]
    pub gpus: Vec<GpuDescriptor>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GpuDescriptor {
    pub id: u16,
    pub name: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessRecord {
    pub identity: ProcessIdentity,
    pub name: String,
    pub cpu_percent: Metric<f32>,
    pub memory_bytes: Metric<u64>,
    pub disk_read_bytes_per_sec: Metric<u64>,
    pub disk_write_bytes_per_sec: Metric<u64>,
    pub gpu_percent: Metric<f32>,
    pub affinity: Metric<Vec<u16>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryFrame {
    pub protocol_version: u32,
    pub sequence: u64,
    pub sampled_at_ms: u64,
    pub collection_duration_ms: f32,
    pub cpu_percent: Metric<f32>,
    pub memory_used_bytes: Metric<u64>,
    pub memory_total_bytes: Metric<u64>,
    pub disk_read_bytes_per_sec: Metric<u64>,
    pub disk_write_bytes_per_sec: Metric<u64>,
    pub network_rx_bytes_per_sec: Metric<u64>,
    pub network_tx_bytes_per_sec: Metric<u64>,
    pub gpu_percent: Metric<f32>,
    pub package_power_watts: Metric<f32>,
    pub processes: Vec<ProcessRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkPoint {
    pub working_set_bytes: u64,
    pub min_ns: f64,
    pub median_ns: f64,
    pub p95_ns: f64,
    pub p99_ns: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub mode: String,
    pub methodology: String,
    pub comparable_to_x86_cycles: bool,
    pub points: Vec<BenchmarkPoint>,
    #[serde(default)]
    pub core_pairs: Vec<CorePair>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CorePair {
    pub cpu_a: u16,
    pub cpu_b: u16,
    pub estimated_one_way_ns: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    Hello {
        version: u32,
    },
    Topology,
    Snapshot,
    Capabilities,
    RunCacheProbe {
        page_mode: bool,
    },
    RunCoreMatrix {
        logical_cpus: Vec<u16>,
    },
    CancelBenchmark,
    BenchmarkStatus,
    GetAffinity {
        process: ProcessIdentity,
    },
    SetAffinity {
        process: ProcessIdentity,
        logical_cpus: Vec<u16>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    Hello {
        version: u32,
        helper_authorized: bool,
    },
    Topology(Topology),
    Snapshot(Box<TelemetryFrame>),
    Capabilities(Vec<Capability>),
    Benchmark(BenchmarkResult),
    BenchmarkStatus {
        running: bool,
        result: Option<BenchmarkResult>,
        error: Option<String>,
    },
    Affinity {
        process: ProcessIdentity,
        logical_cpus: Vec<u16>,
    },
    AffinityChanged {
        process: ProcessIdentity,
        previous_cpus: Vec<u16>,
        logical_cpus: Vec<u16>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_is_distinct_from_zero() {
        let zero = Metric::available(0u64, "bytes", "test", 1);
        let absent = Metric::<u64>::unavailable("bytes", "test", 1, "unsupported");
        assert_ne!(zero.value, absent.value);
        assert_eq!(absent.unavailable_reason.as_deref(), Some("unsupported"));
        assert_eq!(zero.unit, "bytes");
        assert_eq!(absent.unit, "bytes");
    }
    #[test]
    fn process_identity_includes_creation() {
        let a = ProcessIdentity {
            pid: 12,
            started: 1,
            source: "test".into(),
        };
        let b = ProcessIdentity {
            started: 2,
            ..a.clone()
        };
        assert_ne!(a, b);
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
}
