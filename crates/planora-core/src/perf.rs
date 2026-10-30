//! Bounded latency and performance tracking structures for Planora.
//!
//! Provides zero-allocation circular latency rings for hot path profiling
//! and structured, exportable performance reports across Windows and Linux.

use crate::allocation_perf::AllocationSnapshot;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Summarized latency statistics derived from a circular sample window.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LatencyStats {
    pub count: u64,
    pub min_ms: f32,
    pub max_ms: f32,
    pub mean_ms: f32,
    pub p50_ms: f32,
    pub p95_ms: f32,
    pub p99_ms: f32,
}

impl LatencyStats {
    /// Compute summary statistics from a slice of sample durations in milliseconds.
    pub fn compute(samples: &[f32], total_count: u64) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let len = sorted.len();
        let min_ms = sorted[0];
        let max_ms = sorted[len - 1];
        let sum: f64 = sorted.iter().map(|&v| v as f64).sum();
        let mean_ms = (sum / len as f64) as f32;

        let percentile = |p: f32| -> f32 {
            if len == 1 {
                return sorted[0];
            }
            let rank = (p * (len - 1) as f32).round() as usize;
            sorted[rank.min(len - 1)]
        };

        Self {
            count: total_count,
            min_ms,
            max_ms,
            mean_ms,
            p50_ms: percentile(0.50),
            p95_ms: percentile(0.95),
            p99_ms: percentile(0.99),
        }
    }
}

/// Fixed-capacity circular buffer storing recent timing durations in milliseconds.
///
/// Recording into the ring is allocation-free and $O(1)$.
#[derive(Clone, Debug)]
pub struct LatencyRing<const N: usize = 128> {
    samples: [f32; N],
    index: usize,
    count: u64,
}

impl<const N: usize> Default for LatencyRing<N> {
    fn default() -> Self {
        Self {
            samples: [0.0; N],
            index: 0,
            count: 0,
        }
    }
}

impl<const N: usize> LatencyRing<N> {
    /// Create a new empty latency ring with fixed capacity `N`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a single latency observation in milliseconds.
    pub fn record(&mut self, ms: f32) {
        if ms.is_finite() && ms >= 0.0 {
            self.samples[self.index] = ms;
            self.index = (self.index + 1) % N;
            self.count = self.count.saturating_add(1);
        }
    }

    /// Total number of observations recorded since creation.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Extract a slice of populated observations from the ring.
    pub fn active_samples(&self) -> Vec<f32> {
        let valid_len = (self.count as usize).min(N);
        self.samples[..valid_len].to_vec()
    }

    /// Compute summary statistics over the current ring window.
    pub fn stats(&self) -> LatencyStats {
        let active = self.active_samples();
        LatencyStats::compute(&active, self.count)
    }

    /// Reset all counters and clear recorded samples.
    pub fn reset(&mut self) {
        self.samples = [0.0; N];
        self.index = 0;
        self.count = 0;
    }
}

/// Redacted, machine-verifiable performance report covering UI and collection hot paths.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PerformanceReport {
    pub format_version: u32,
    pub application_version: String,
    pub source_commit: String,
    pub target: String,
    pub timestamp_ms: u64,
    pub process_resident_memory_bytes: Option<u64>,
    pub process_virtual_memory_bytes: Option<u64>,
    #[serde(default)]
    pub allocations: Option<AllocationSnapshot>,
    pub first_paint_ms: Option<f32>,
    pub ui_frame: LatencyStats,
    pub helper_collection_total: LatencyStats,
    pub helper_collection_by_provider: BTreeMap<String, LatencyStats>,
    pub ipc_serialization: LatencyStats,
    pub history_publication: LatencyStats,
    pub chart_preparation: LatencyStats,
    pub process_filtering_sorting: LatencyStats,
    pub recording_write: LatencyStats,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_ring_computes_accurate_percentiles() {
        let mut ring = LatencyRing::<10>::new();
        for i in 1..=10 {
            ring.record(i as f32);
        }
        assert_eq!(ring.count(), 10);
        let stats = ring.stats();
        assert_eq!(stats.min_ms, 1.0);
        assert_eq!(stats.max_ms, 10.0);
        assert_eq!(stats.mean_ms, 5.5);
        assert_eq!(stats.p50_ms, 6.0); // 0.5 * 9 = 4.5 -> round 5 -> value 6.0
        assert_eq!(stats.p95_ms, 10.0);
        assert_eq!(stats.p99_ms, 10.0);
    }

    #[test]
    fn latency_ring_wraparound_maintains_capacity() {
        let mut ring = LatencyRing::<4>::new();
        ring.record(10.0);
        ring.record(20.0);
        ring.record(30.0);
        ring.record(40.0);
        ring.record(50.0); // overwrites 10.0
        assert_eq!(ring.count(), 5);
        let stats = ring.stats();
        assert_eq!(stats.min_ms, 20.0);
        assert_eq!(stats.max_ms, 50.0);
    }

    #[test]
    fn performance_report_serialization_round_trip() {
        let report = PerformanceReport {
            format_version: 1,
            application_version: "0.3.0-beta.43".into(),
            source_commit: "aaa5fce".into(),
            target: "x86_64-pc-windows-msvc".into(),
            timestamp_ms: 1700000000000,
            process_resident_memory_bytes: Some(64 * 1024 * 1024),
            process_virtual_memory_bytes: Some(128 * 1024 * 1024),
            allocations: None,
            first_paint_ms: Some(12.4),
            ui_frame: LatencyStats::default(),
            helper_collection_total: LatencyStats::default(),
            helper_collection_by_provider: BTreeMap::new(),
            ipc_serialization: LatencyStats::default(),
            history_publication: LatencyStats::default(),
            chart_preparation: LatencyStats::default(),
            process_filtering_sorting: LatencyStats::default(),
            recording_write: LatencyStats::default(),
        };
        let serialized = serde_json::to_string(&report).expect("serialize report");
        let deserialized: PerformanceReport =
            serde_json::from_str(&serialized).expect("deserialize report");
        assert_eq!(report, deserialized);
    }
}
