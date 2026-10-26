use planora_core::{AvailabilityReason, ProcessIdentity, SensorReading, TelemetryFrame};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::{Duration, Instant},
};

pub const MAX_HISTORY_DURATION: Duration = Duration::from_secs(900); // 15 minutes

/// Convert a measurement's wall-clock capture time into the local monotonic timeline.
/// `received_at` and `wall_clock_at_receipt_ms` must be taken together.
/// Never position a delayed measurement at its delivery time.
pub fn measurement_instant(
    received_at: Instant,
    wall_clock_at_receipt_ms: u64,
    sampled_at_ms: u64,
) -> Instant {
    if sampled_at_ms == 0 || sampled_at_ms >= wall_clock_at_receipt_ms {
        return received_at;
    }
    received_at
        .checked_sub(Duration::from_millis(
            wall_clock_at_receipt_ms - sampled_at_ms,
        ))
        .unwrap_or(received_at)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HistoryRange {
    Sec30,
    #[default]
    Sec60,
    Min5,
    Min15,
}

impl HistoryRange {
    pub fn duration(&self) -> Duration {
        match self {
            Self::Sec30 => Duration::from_secs(30),
            Self::Sec60 => Duration::from_secs(60),
            Self::Min5 => Duration::from_secs(300),
            Self::Min15 => Duration::from_secs(900),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Sec30 => "30s",
            Self::Sec60 => "60s",
            Self::Min5 => "5m",
            Self::Min15 => "15m",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub time: Instant,
    pub timestamp_ms: u64,
    pub start: Instant,
    pub value: Option<f32>,
    pub minimum: Option<f32>,
    pub maximum: Option<f32>,
    pub first: Option<f32>,
    pub last: Option<f32>,
    pub sum: f64,
    pub count: u32,
    pub source: String,
    pub unavailable_reason: Option<AvailabilityReason>,
}

impl Sample {
    pub fn midpoint(&self) -> Instant {
        if self.time > self.start {
            self.start + self.time.saturating_duration_since(self.start) / 2
        } else {
            self.time
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartPoint {
    pub x: f32,
    pub mean: f32,
    pub min: f32,
    pub max: f32,
    pub weight: u32,
}

#[derive(Clone, Debug, Default)]
pub struct TimeSeries {
    pub samples: VecDeque<Sample>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObservedSummary {
    pub minimum: f32,
    pub maximum: f32,
    pub average: f32,
    pub observed_seconds: usize,
    pub expected_seconds: usize,
}

impl ObservedSummary {
    pub fn coverage_percent(self) -> f32 {
        100.0 * self.observed_seconds as f32 / self.expected_seconds.max(1) as f32
    }
}

impl TimeSeries {
    pub fn observed_summary(&self, now: Instant, range: HistoryRange) -> Option<ObservedSummary> {
        let duration = range.duration();
        let expected_seconds = duration.as_secs() as usize;
        let cutoff = now.checked_sub(duration).unwrap_or(now);
        let use_unix_seconds = self.samples.back().is_some_and(|s| s.timestamp_ms >= 1_000);
        let mut buckets: std::collections::BTreeMap<u64, (f64, u32, f32, f32)> =
            std::collections::BTreeMap::new();
        for sample in &self.samples {
            if sample.time < cutoff || sample.start > now {
                continue;
            }
            if let Some(value) = sample.value.filter(|v| v.is_finite()) {
                let second = if use_unix_seconds {
                    sample.timestamp_ms / 1_000
                } else {
                    sample
                        .midpoint()
                        .saturating_duration_since(cutoff)
                        .as_secs()
                        .min(duration.as_secs().saturating_sub(1))
                };
                let sample_count = sample.count.max(1);
                let sample_sum = if sample.count > 0 && sample.sum.is_finite() {
                    sample.sum
                } else {
                    value as f64 * sample_count as f64
                };
                let bucket =
                    buckets
                        .entry(second)
                        .or_insert((0.0, 0, f32::INFINITY, f32::NEG_INFINITY));
                bucket.0 += sample_sum;
                bucket.1 += sample_count;
                bucket.2 = bucket.2.min(sample.minimum.unwrap_or(value));
                bucket.3 = bucket.3.max(sample.maximum.unwrap_or(value));
            }
        }
        while buckets.len() > expected_seconds {
            buckets.pop_first();
        }
        if buckets.is_empty() {
            return None;
        }
        let values: Vec<f32> = buckets
            .values()
            .map(|(sum, count, _, _)| (sum / (*count).max(1) as f64) as f32)
            .collect();
        let total: f64 = values.iter().map(|v| *v as f64).sum();
        Some(ObservedSummary {
            minimum: buckets
                .values()
                .fold(f32::INFINITY, |current, bucket| current.min(bucket.2)),
            maximum: buckets
                .values()
                .fold(f32::NEG_INFINITY, |current, bucket| current.max(bucket.3)),
            average: (total / values.len() as f64) as f32,
            observed_seconds: values.len().min(expected_seconds),
            expected_seconds,
        })
    }
    pub fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(3600),
        }
    }

    pub fn push(&mut self, time: Instant, timestamp_ms: u64, value: Option<f32>) {
        self.push_with_metadata(time, timestamp_ms, value, "UI history", None);
    }

    pub fn push_with_metadata(
        &mut self,
        time: Instant,
        timestamp_ms: u64,
        value: Option<f32>,
        source: &str,
        unavailable_reason: Option<&AvailabilityReason>,
    ) {
        // Repeated reports of a cached measurement are not new observations.
        if self
            .samples
            .back()
            .is_some_and(|s| s.timestamp_ms >= timestamp_ms)
        {
            return;
        }
        let value = value.filter(|value| value.is_finite());
        if let (Some(value), Some(last)) = (value, self.samples.back_mut())
            && last.timestamp_ms / 1000 == timestamp_ms / 1000
            && time.saturating_duration_since(last.time) < Duration::from_secs(1)
            && last.source == source
            && last.value.is_some()
        {
            last.sum += value as f64;
            last.count += 1;
            last.value = Some((last.sum / last.count as f64) as f32);
            last.minimum = Some(last.minimum.unwrap_or(value).min(value));
            last.maximum = Some(last.maximum.unwrap_or(value).max(value));
            last.last = Some(value);
            last.time = time;
            last.timestamp_ms = timestamp_ms;
            return;
        }
        self.samples.push_back(Sample {
            time,
            timestamp_ms,
            start: time,
            value,
            minimum: value,
            maximum: value,
            first: value,
            last: value,
            sum: value.unwrap_or(0.0) as f64,
            count: u32::from(value.is_some()),
            source: source.to_owned(),
            unavailable_reason: unavailable_reason.cloned(),
        });
        self.prune_before(
            time.checked_sub(MAX_HISTORY_DURATION + Duration::from_secs(5))
                .unwrap_or(time),
        );
    }

    pub fn prune_before(&mut self, cutoff: Instant) {
        while let Some(front) = self.samples.front() {
            if front.time < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn latest(&self) -> Option<Option<f32>> {
        self.samples.back().map(|s| s.value)
    }

    /// Stable recent measurement (e.g. completed 1-second aggregate)
    /// to avoid flickering instantaneous rate headlines when bursts arrive.
    pub fn latest_stable_value(&self) -> Option<f32> {
        if self.samples.len() >= 2 {
            let back = self.samples.back()?;
            if back.count >= 5 {
                back.value
            } else if let Some(prev) = self.samples.iter().rev().nth(1) {
                prev.value.or(back.value)
            } else {
                back.value
            }
        } else {
            self.samples.back().and_then(|s| s.value)
        }
    }

    /// Extract chart points normalized to [0.0, 1.0] in time across the given range ending at `now`.
    /// Each aggregate emits exactly one point at its midpoint time, eliminating vertical needle artifacts.
    /// Segments are split when missing data (None), source changes, or time gaps exceed `gap_threshold`.
    /// If `pixel_columns > 0` and points exceed `pixel_columns`, points sharing a column are merged
    /// preserving weighted mean, min of mins, and max of maxes.
    pub fn chart_segments(
        &self,
        now: Instant,
        range: HistoryRange,
        gap_threshold: Duration,
        pixel_columns: usize,
    ) -> Vec<Vec<ChartPoint>> {
        let dur = range.duration().as_secs_f32();
        if dur <= 0.0 || self.samples.is_empty() {
            return Vec::new();
        }
        let cutoff = now.checked_sub(range.duration()).unwrap_or(now);

        let mut segments: Vec<Vec<ChartPoint>> = Vec::new();
        let mut current_segment: Vec<ChartPoint> = Vec::new();
        let mut prev_time: Option<Instant> = None;
        let mut prev_source: Option<&str> = None;

        for s in &self.samples {
            let mid = s.midpoint();
            // Paused views use an earlier `now` and must not see future data.
            if mid < cutoff || mid > now {
                continue;
            }
            if let Some(prev) = prev_time
                && mid.saturating_duration_since(prev) > gap_threshold
                && !current_segment.is_empty()
            {
                segments.push(std::mem::take(&mut current_segment));
            }
            if prev_source.is_some_and(|source| source != s.source) && !current_segment.is_empty() {
                segments.push(std::mem::take(&mut current_segment));
            }
            prev_time = Some(mid);
            prev_source = Some(&s.source);

            if let Some(val) = s.value {
                let elapsed = mid.saturating_duration_since(cutoff).as_secs_f32();
                let norm_x = (elapsed / dur).clamp(0.0, 1.0);
                let min_val = s.minimum.unwrap_or(val).min(val);
                let max_val = s.maximum.unwrap_or(val).max(val);
                current_segment.push(ChartPoint {
                    x: norm_x,
                    mean: val,
                    min: min_val,
                    max: max_val,
                    weight: s.count.max(1),
                });
            } else {
                // Unavailable sample breaks the segment
                if !current_segment.is_empty() {
                    segments.push(std::mem::take(&mut current_segment));
                }
            }
        }

        if !current_segment.is_empty() {
            segments.push(current_segment);
        }

        if pixel_columns == 0 {
            return segments;
        }

        let columns = pixel_columns.max(1);
        segments
            .into_iter()
            .map(|segment| {
                if segment.len() <= columns {
                    return segment;
                }
                let mut result = Vec::with_capacity(columns);
                let mut start = 0;
                while start < segment.len() {
                    let bucket =
                        ((segment[start].x * columns as f32).floor() as usize).min(columns - 1);
                    let mut end = start + 1;
                    while end < segment.len()
                        && (((segment[end].x * columns as f32).floor() as usize).min(columns - 1))
                            == bucket
                    {
                        end += 1;
                    }
                    let mut total_weight = 0u64;
                    let mut weighted_sum = 0.0f64;
                    let mut weighted_x = 0.0f64;
                    let mut col_min = f32::INFINITY;
                    let mut col_max = f32::NEG_INFINITY;
                    for p in &segment[start..end] {
                        let w = p.weight as u64;
                        total_weight += w;
                        weighted_sum += p.mean as f64 * w as f64;
                        weighted_x += p.x as f64 * w as f64;
                        col_min = col_min.min(p.min);
                        col_max = col_max.max(p.max);
                    }
                    let total_w = total_weight.max(1) as f32;
                    result.push(ChartPoint {
                        x: (weighted_x as f32 / total_w).clamp(0.0, 1.0),
                        mean: (weighted_sum as f32 / total_w),
                        min: col_min,
                        max: col_max,
                        weight: total_weight.min(u32::MAX as u64) as u32,
                    });
                    start = end;
                }
                result
            })
            .collect()
    }

    /// Extract points normalized to [0.0, 1.0] in time across the given range ending at `now`.
    /// Segments are split when missing data (None) or time gaps exceed `gap_threshold`.
    pub fn normalized_segments(
        &self,
        now: Instant,
        range: HistoryRange,
        gap_threshold: Duration,
    ) -> Vec<Vec<(f32, f32)>> {
        self.chart_segments(now, range, gap_threshold, 0)
            .into_iter()
            .map(|seg| seg.into_iter().map(|p| (p.x, p.mean)).collect())
            .collect()
    }

    /// Collapse dense samples to at most two extrema per horizontal pixel.
    /// Gaps are preserved because each contiguous segment is processed separately.
    pub fn downsampled_segments(
        &self,
        now: Instant,
        range: HistoryRange,
        gap_threshold: Duration,
        pixel_columns: usize,
    ) -> Vec<Vec<(f32, f32)>> {
        self.chart_segments(now, range, gap_threshold, pixel_columns)
            .into_iter()
            .map(|seg| seg.into_iter().map(|p| (p.x, p.mean)).collect())
            .collect()
    }

    /// Query exact sample closest to `norm_x` in the time window `[now - range, now]`.
    pub fn query_sample(
        &self,
        now: Instant,
        range: HistoryRange,
        norm_x: f32,
    ) -> Option<(&Sample, Duration)> {
        let dur = range.duration().as_secs_f32();
        let cutoff = now.checked_sub(range.duration()).unwrap_or(now);
        let target_time = cutoff + Duration::from_secs_f32((norm_x.clamp(0.0, 1.0) * dur).max(0.0));

        let mut best: Option<(&Sample, Duration)> = None;
        let mut min_diff = Duration::MAX;

        for s in &self.samples {
            let mid = s.midpoint();
            if mid < cutoff || mid > now {
                continue;
            }
            let diff = if mid > target_time {
                mid.saturating_duration_since(target_time)
            } else {
                target_time.saturating_duration_since(mid)
            };
            if diff < min_diff {
                min_diff = diff;
                let age = now.saturating_duration_since(mid);
                best = Some((s, age));
            }
        }

        best
    }

    /// Keep a chart cursor from displaying a distant reading over an
    /// unobserved interval. The caller chooses tolerance for its pixel scale.
    pub fn query_sample_near(
        &self,
        now: Instant,
        range: HistoryRange,
        norm_x: f32,
        tolerance: Duration,
    ) -> Option<(&Sample, Duration)> {
        let cutoff = now.checked_sub(range.duration()).unwrap_or(now);
        let target = cutoff
            + Duration::from_secs_f32(norm_x.clamp(0.0, 1.0) * range.duration().as_secs_f32());
        self.query_sample(now, range, norm_x).filter(|(sample, _)| {
            let mid = sample.midpoint();
            if mid >= target {
                mid.duration_since(target) <= tolerance
            } else {
                target.duration_since(mid) <= tolerance
            }
        })
    }
}

/// Collection of resource time-series histories
#[derive(Clone, Debug, Default)]
pub struct ResourceHistories {
    pub session_id: u64,
    pub last_sequence: u64,
    pub last_cpu_sampled_at_ms: u64,

    pub cpu: TimeSeries,
    pub memory_pct: TimeSeries,
    pub disk_read: TimeSeries,
    pub disk_write: TimeSeries,
    pub net_rx: TimeSeries,
    pub net_tx: TimeSeries,
    /// Scoped histories use stable hardware IDs rather than display labels.
    pub details: BTreeMap<String, TimeSeries>,
    pub not_retained: BTreeSet<String>,

    // Selected process history (only recorded while observed)
    pub selected_identity: Option<ProcessIdentity>,
    pub selected_cpu: TimeSeries,
    pub selected_memory: TimeSeries,
    pub selected_io_read: TimeSeries,
    pub selected_io_write: TimeSeries,
}

impl ResourceHistories {
    pub fn ingest_frame(
        &mut self,
        frame: &TelemetryFrame,
        sensors: &[SensorReading],
        selected: Option<&ProcessIdentity>,
        now: Instant,
        wall_clock_now_ms: u64,
    ) {
        self.reset_if_new_session(frame.session_id);
        self.set_selected_process(selected);
        if frame.sequence == self.last_sequence {
            return;
        }
        self.last_sequence = frame.sequence;
        if frame.cpu_percent.sampled_at_ms != self.last_cpu_sampled_at_ms {
            self.last_cpu_sampled_at_ms = frame.cpu_percent.sampled_at_ms;
            let metric = &frame.cpu_percent;
            self.cpu.push_with_metadata(
                measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms),
                metric.sampled_at_ms,
                metric.value,
                &metric.source,
                metric.unavailable_reason.as_ref(),
            );
        }
        let used = &frame.memory_used_bytes;
        let total = &frame.memory_total_bytes;
        let memory_pct = match (used.value, total.value) {
            (Some(u), Some(t)) if t > 0 => Some((u as f64 * 100.0 / t as f64) as f32),
            _ => None,
        };
        self.memory_pct.push_with_metadata(
            measurement_instant(now, wall_clock_now_ms, used.sampled_at_ms),
            used.sampled_at_ms,
            memory_pct,
            &used.source,
            used.unavailable_reason
                .as_ref()
                .or(total.unavailable_reason.as_ref()),
        );
        for (series, metric) in [
            (&mut self.disk_read, &frame.disk_read_bytes_per_sec),
            (&mut self.disk_write, &frame.disk_write_bytes_per_sec),
            (&mut self.net_rx, &frame.network_rx_bytes_per_sec),
            (&mut self.net_tx, &frame.network_tx_bytes_per_sec),
        ] {
            series.push_with_metadata(
                measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms),
                metric.sampled_at_ms,
                metric.value.map(|value| value as f32),
                &metric.source,
                metric.unavailable_reason.as_ref(),
            );
        }
        if let Some(identity) = selected
            && let Some(process) = frame
                .processes
                .iter()
                .find(|process| &process.identity == identity)
        {
            let cpu = &process.cpu_percent;
            self.selected_cpu.push_with_metadata(
                measurement_instant(now, wall_clock_now_ms, cpu.sampled_at_ms),
                cpu.sampled_at_ms,
                cpu.value,
                &cpu.source,
                cpu.unavailable_reason.as_ref(),
            );
            let memory = &process.memory_bytes;
            self.selected_memory.push_with_metadata(
                measurement_instant(now, wall_clock_now_ms, memory.sampled_at_ms),
                memory.sampled_at_ms,
                memory.value.map(|value| value as f32),
                &memory.source,
                memory.unavailable_reason.as_ref(),
            );
            for (series, metric) in [
                (&mut self.selected_io_read, &process.io_read_bytes_per_sec),
                (&mut self.selected_io_write, &process.io_write_bytes_per_sec),
            ] {
                series.push_with_metadata(
                    measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms),
                    metric.sampled_at_ms,
                    metric.value.map(|value| value as f32),
                    &metric.source,
                    metric.unavailable_reason.as_ref(),
                );
            }
        }
        self.ingest_scoped(frame, sensors, now, wall_clock_now_ms);
    }

    pub fn ingest_scoped(
        &mut self,
        frame: &TelemetryFrame,
        sensors: &[SensorReading],
        now: Instant,
        wall_clock_now_ms: u64,
    ) {
        for cpu in &frame.logical_cpus {
            let metric = &cpu.utilization_percent;
            self.push_detail(
                format!("cpu:{}:{}", cpu.group, cpu.cpu_index),
                measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms),
                metric.sampled_at_ms,
                metric.value,
                &metric.source,
                metric.unavailable_reason.as_ref(),
            );
        }
        for adapter in &frame.gpu_adapters {
            let metric = &adapter.busiest_engine_percent;
            self.push_detail(
                format!(
                    "gpu:{}:{:?}:busiest",
                    adapter.adapter_luid, adapter.physical_adapter
                ),
                measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms),
                metric.sampled_at_ms,
                metric.value,
                &metric.source,
                metric.unavailable_reason.as_ref(),
            );
        }
        for engine in &frame.gpu_engines {
            let metric = &engine.utilization_percent;
            let key = format!(
                "gpu-engine:{}:{:?}:{:?}:{}:{}",
                engine.adapter_luid.unwrap_or(0),
                engine.physical_adapter,
                engine.engine_id,
                engine.engine_type,
                engine.instance
            );
            self.push_detail(
                key,
                measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms),
                metric.sampled_at_ms,
                metric.value,
                &metric.source,
                metric.unavailable_reason.as_ref(),
            );
        }
        for device in &frame.disks {
            for (direction, metric) in [
                ("read", &device.read_bytes_per_sec),
                ("write", &device.write_bytes_per_sec),
            ] {
                self.push_detail(
                    format!("disk:{}:{direction}", device.id),
                    measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms),
                    metric.sampled_at_ms,
                    metric.value.map(|value| value as f32),
                    &metric.source,
                    metric.unavailable_reason.as_ref(),
                );
            }
            for (name, metric) in [
                ("latency", &device.latency_ms),
                ("queue", &device.queue_depth),
            ] {
                if let Some(metric) = metric {
                    self.push_detail(
                        format!("disk:{}:{name}", device.id),
                        measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms),
                        metric.sampled_at_ms,
                        metric.value,
                        &metric.source,
                        metric.unavailable_reason.as_ref(),
                    );
                }
            }
        }
        for device in &frame.network_interfaces {
            for (direction, metric) in [
                ("rx", &device.read_bytes_per_sec),
                ("tx", &device.write_bytes_per_sec),
            ] {
                self.push_detail(
                    format!("net:{}:{direction}", device.id),
                    measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms),
                    metric.sampled_at_ms,
                    metric.value.map(|value| value as f32),
                    &metric.source,
                    metric.unavailable_reason.as_ref(),
                );
            }
        }
        for sensor in sensors {
            let metric = &sensor.value;
            let instant = measurement_instant(now, wall_clock_now_ms, metric.sampled_at_ms);
            self.push_detail(
                sensor_series_key(sensor),
                instant,
                metric.sampled_at_ms,
                metric.value,
                &metric.source,
                metric.unavailable_reason.as_ref(),
            );
        }
    }
    pub fn reset_if_new_session(&mut self, current_session: u64) {
        if self.session_id != current_session {
            self.session_id = current_session;
            self.last_sequence = 0;
            self.last_cpu_sampled_at_ms = 0;
            self.cpu.clear();
            self.memory_pct.clear();
            self.disk_read.clear();
            self.disk_write.clear();
            self.net_rx.clear();
            self.net_tx.clear();
            self.details.clear();
            self.not_retained.clear();
            self.selected_identity = None;
            self.selected_cpu.clear();
            self.selected_memory.clear();
            self.selected_io_read.clear();
            self.selected_io_write.clear();
        }
    }

    pub fn set_selected_process(&mut self, identity: Option<&ProcessIdentity>) {
        if self.selected_identity.as_ref() != identity {
            self.selected_identity = identity.cloned();
            self.selected_cpu.clear();
            self.selected_memory.clear();
            self.selected_io_read.clear();
            self.selected_io_write.clear();
        }
    }

    pub fn detail(&self, key: &str) -> Option<&TimeSeries> {
        self.details.get(key)
    }

    pub fn enforce_budget(&mut self) -> usize {
        // Conservative cap leaves room for two immutable snapshots plus the
        // mutable worker copy under the 64 MiB history budget.
        self.enforce_budget_with_limit(100_000)
    }

    fn enforce_budget_with_limit(&mut self, max_detail_points: usize) -> usize {
        let mut points: usize = self
            .details
            .values()
            .map(|series| series.samples.len())
            .sum();
        if points <= max_detail_points {
            return 0;
        }
        let mut oldest: Vec<_> = self
            .details
            .iter()
            .map(|(key, series)| {
                (
                    key.clone(),
                    series
                        .samples
                        .back()
                        .map_or(0, |sample| sample.timestamp_ms),
                )
            })
            .collect();
        oldest.sort_by_key(|(_, timestamp)| *timestamp);
        let mut removed = 0;
        for (key, _) in oldest {
            if points <= max_detail_points {
                break;
            }
            if let Some(series) = self.details.remove(&key) {
                points = points.saturating_sub(series.samples.len());
                if self.not_retained.len() < 1024 {
                    self.not_retained.insert(key);
                }
                removed += 1;
            }
        }
        removed
    }

    pub fn push_detail(
        &mut self,
        key: String,
        time: Instant,
        timestamp_ms: u64,
        value: Option<f32>,
        source: &str,
        unavailable_reason: Option<&AvailabilityReason>,
    ) {
        if self.details.len() >= 1024 && !self.details.contains_key(&key) {
            if self.not_retained.len() < 1024 {
                self.not_retained.insert(key);
            }
            return;
        }
        self.not_retained.remove(&key);
        self.details.entry(key).or_default().push_with_metadata(
            time,
            timestamp_ms,
            value,
            source,
            unavailable_reason,
        );
    }
}

pub fn sensor_series_key(sensor: &SensorReading) -> String {
    if !sensor.id.is_empty() {
        format!("sensor:{}", sensor.id)
    } else {
        format!(
            "sensor:{}:{}:{}:{}:{:?}",
            sensor.device_id, sensor.scope, sensor.provider, sensor.name, sensor.sensor_type
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_measurements_use_capture_time() {
        let receipt = Instant::now();
        assert_eq!(measurement_instant(receipt, 50_000, 50_000), receipt);
        assert_eq!(
            measurement_instant(receipt, 50_000, 48_500),
            receipt - Duration::from_millis(1500)
        );
    }

    #[test]
    fn duplicate_timestamp_does_not_add_a_sample() {
        let now = Instant::now();
        let mut ts = TimeSeries::new();
        ts.push(now, 10, Some(12.0));
        ts.push(now + Duration::from_secs(1), 10, Some(12.0));
        ts.push(now + Duration::from_secs(2), 9, Some(50.0)); // late/out-of-order
        assert_eq!(ts.samples.len(), 1);
    }

    #[test]
    fn paused_window_excludes_future_samples() {
        let now = Instant::now();
        let mut ts = TimeSeries::new();
        ts.push(now - Duration::from_secs(1), 100, Some(1.0));
        ts.push(now + Duration::from_secs(1), 200, Some(99.0));
        let segs = ts.normalized_segments(now, HistoryRange::Sec30, Duration::from_secs(3));
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].len(), 1);
        assert_eq!(segs[0][0].1, 1.0);
        assert_eq!(
            ts.query_sample(now, HistoryRange::Sec30, 1.0)
                .unwrap()
                .0
                .value,
            Some(1.0)
        );
    }

    #[test]
    fn pid_reuse_resets_selected_process_history() {
        let now = Instant::now();
        let mut histories = ResourceHistories::default();
        let old = ProcessIdentity {
            pid: 42,
            started: 1,
            sequence_number: None,
            source: planora_core::IdentitySource::SysinfoEpochSeconds,
        };
        let reused = ProcessIdentity {
            pid: 42,
            started: 2,
            sequence_number: None,
            source: planora_core::IdentitySource::SysinfoEpochSeconds,
        };
        histories.set_selected_process(Some(&old));
        histories.selected_cpu.push(now, 1, Some(10.0));
        histories.set_selected_process(Some(&reused));
        assert!(histories.selected_cpu.samples.is_empty());
        assert_eq!(histories.selected_identity.as_ref(), Some(&reused));
    }

    #[test]
    fn band_retains_spikes_and_gaps() {
        let now = Instant::now();
        let mut ts = TimeSeries::new();
        for idx in 0..1_000 {
            let t = now - Duration::from_millis((999 - idx) * 10);
            let value = if idx == 375 {
                Some(99.0)
            } else if idx == 500 {
                None
            } else {
                Some(1.0)
            };
            ts.push(t, idx + 1, value);
        }
        let full = ts.chart_segments(now, HistoryRange::Sec30, Duration::from_secs(2), 0);
        let compact = ts.chart_segments(now, HistoryRange::Sec30, Duration::from_secs(2), 40);
        assert_eq!(full.len(), compact.len());
        assert!(compact.iter().flatten().any(|p| p.max == 99.0));
        assert!(compact.iter().map(Vec::len).sum::<usize>() <= 80);
    }

    #[test]
    fn chart_points_have_strictly_increasing_x() {
        let now = Instant::now();
        let mut ts = TimeSeries::new();
        for sec in 0..30u64 {
            // Push several sub-samples per second
            for sub in 0..5u64 {
                let t = now - Duration::from_millis((29 - sec) * 1000 + (4 - sub) * 100);
                ts.push(t, sec * 1000 + sub * 100, Some((sec * 2 + sub) as f32));
            }
        }
        let segments = ts.chart_segments(now, HistoryRange::Sec30, Duration::from_secs(2), 60);
        assert!(!segments.is_empty());
        for segment in segments {
            for window in segment.windows(2) {
                assert!(
                    window[1].x > window[0].x,
                    "x must be strictly increasing: {} vs {}",
                    window[0].x,
                    window[1].x
                );
            }
        }
    }

    #[test]
    fn column_merge_preserves_weighted_mean_and_extremes() {
        let now = Instant::now();
        let mut ts = TimeSeries::new();
        // Push 28 distinct 1-second aggregates across the 30s window
        for i in 0..28 {
            let t = now - Duration::from_secs(28 - i);
            ts.push_with_metadata(
                t,
                (i + 1) * 1000,
                Some(if i == 5 { 100.0 } else { 10.0 }),
                "test",
                None,
            );
        }
        // Downsample to only 2 columns across the 30s range
        let segments = ts.chart_segments(now, HistoryRange::Sec30, Duration::from_secs(3), 2);
        assert_eq!(segments.len(), 1);
        let merged = &segments[0];
        assert_eq!(merged.len(), 2);
        // The extreme 100.0 must be preserved in max of column 0
        assert!(merged[0].max == 100.0);
        assert!(merged[1].max == 10.0);
        assert!(merged[0].min == 10.0);
        assert!(merged[1].min == 10.0);
    }

    #[test]
    fn aggregate_is_positioned_at_its_midpoint() {
        let start = Instant::now();
        let end = start + Duration::from_millis(800);
        let mut ts = TimeSeries::new();
        ts.push_with_metadata(start, 1000, Some(10.0), "test", None);
        ts.push_with_metadata(end, 1800, Some(20.0), "test", None);
        let sample = ts.samples.back().unwrap();
        assert_eq!(sample.start, start);
        assert_eq!(sample.time, end);
        assert_eq!(sample.midpoint(), start + Duration::from_millis(400));
    }

    #[test]
    fn test_push_and_query() {
        let mut ts = TimeSeries::new();
        let now = Instant::now();
        ts.push(now - Duration::from_secs(10), 1000, Some(25.0));
        ts.push(now - Duration::from_secs(5), 2000, Some(50.0));
        ts.push(now, 3000, Some(75.0));

        let latest = ts.latest();
        assert_eq!(latest, Some(Some(75.0)));

        let query = ts.query_sample(now, HistoryRange::Sec30, 1.0);
        assert!(query.is_some());
        let (s, _age) = query.unwrap();
        assert_eq!(s.value, Some(75.0));
    }

    #[test]
    fn chart_cursor_does_not_report_a_reading_across_unobserved_time() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        series.push(now - Duration::from_secs(2), 1_000, Some(25.0));
        assert!(
            series
                .query_sample_near(now, HistoryRange::Sec60, 0.1, Duration::from_secs(2))
                .is_none()
        );
        assert!(
            series
                .query_sample_near(
                    now,
                    HistoryRange::Sec60,
                    58.0 / 60.0,
                    Duration::from_secs(2)
                )
                .is_some()
        );
    }

    #[test]
    fn test_gap_creates_segments() {
        let mut ts = TimeSeries::new();
        let now = Instant::now();
        // Point at -20s
        ts.push(now - Duration::from_secs(20), 1000, Some(10.0));
        // Point at -19s
        ts.push(now - Duration::from_secs(19), 2000, Some(20.0));
        // Gap of 8s (threshold 2.5s)
        // Point at -10s
        ts.push(now - Duration::from_secs(10), 3000, Some(30.0));
        ts.push(now - Duration::from_secs(9), 4000, Some(40.0));

        let segs = ts.normalized_segments(now, HistoryRange::Sec30, Duration::from_secs_f32(2.5));
        assert_eq!(segs.len(), 2, "Expected 2 segments split by time gap");
        assert_eq!(segs[0].len(), 2);
        assert_eq!(segs[1].len(), 2);
    }

    #[test]
    fn test_none_sample_creates_gap() {
        let mut ts = TimeSeries::new();
        let now = Instant::now();
        ts.push(now - Duration::from_secs(4), 1000, Some(10.0));
        ts.push(now - Duration::from_secs(3), 2000, None);
        ts.push(now - Duration::from_secs(2), 3000, Some(20.0));

        let segs = ts.normalized_segments(now, HistoryRange::Sec30, Duration::from_secs_f32(5.0));
        assert_eq!(segs.len(), 2, "Expected 2 segments split by None value");
    }

    #[test]
    fn summary_counts_observed_seconds_without_filling_gaps() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        series.push(now - Duration::from_secs(3), 1, Some(10.0));
        series.push(now - Duration::from_millis(2_900), 2, Some(30.0));
        series.push(now - Duration::from_secs(2), 3, None);
        series.push(now - Duration::from_secs(1), 4, Some(50.0));
        let summary = series.observed_summary(now, HistoryRange::Sec30).unwrap();
        assert_eq!(summary.observed_seconds, 2);
        assert_eq!(summary.expected_seconds, 30);
        assert_eq!(summary.average, 35.0);
        assert!(summary.coverage_percent() < 7.0);
    }

    #[test]
    fn zero_activity_does_not_invent_fake_bounds() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        for s in 0..10 {
            series.push(now - Duration::from_secs(10 - s), (s + 1) * 1000, Some(0.0));
        }
        let summary = series.observed_summary(now, HistoryRange::Sec30).unwrap();
        assert_eq!(summary.minimum, 0.0);
        assert_eq!(summary.maximum, 0.0);
        assert_eq!(summary.average, 0.0);
        assert_eq!(summary.observed_seconds, 10);
    }

    #[test]
    fn invalid_values_are_ignored() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        series.push(now - Duration::from_secs(2), 1000, Some(f32::NAN));
        series.push(now - Duration::from_secs(1), 2000, Some(f32::INFINITY));
        assert!(series.samples.is_empty() || series.samples.iter().all(|s| s.value.is_none()));
    }

    #[test]
    fn slow_cadence_is_not_packed_into_dense_seconds() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        series.push(now - Duration::from_secs(20), 1000, Some(10.0));
        series.push(now - Duration::from_secs(15), 6000, Some(12.0));
        series.push(now - Duration::from_secs(10), 11000, Some(14.0));
        series.push(now - Duration::from_secs(5), 16000, Some(16.0));
        let summary = series.observed_summary(now, HistoryRange::Sec30).unwrap();
        assert_eq!(summary.observed_seconds, 4);
        assert_eq!(summary.expected_seconds, 30);
    }

    #[test]
    fn source_change_breaks_segments() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        series.push_with_metadata(
            now - Duration::from_secs(4),
            1000,
            Some(10.0),
            "provider_a",
            None,
        );
        series.push_with_metadata(
            now - Duration::from_secs(3),
            2000,
            Some(12.0),
            "provider_a",
            None,
        );
        series.push_with_metadata(
            now - Duration::from_secs(2),
            3000,
            Some(14.0),
            "provider_b",
            None,
        );
        series.push_with_metadata(
            now - Duration::from_secs(1),
            4000,
            Some(16.0),
            "provider_b",
            None,
        );
        let segs =
            series.downsampled_segments(now, HistoryRange::Sec30, Duration::from_secs(3), 100);
        assert_eq!(segs.len(), 2, "Source change must break continuous segment");
    }

    #[test]
    fn history_budget_evicts_oldest_detail_series() {
        let now = Instant::now();
        let mut histories = ResourceHistories::default();
        for i in 0..10 {
            let key = format!("sensor:device_{i}");
            histories.push_detail(key, now, (i + 1) * 1000, Some(i as f32), "test", None);
        }
        assert_eq!(histories.details.len(), 10);
        assert_eq!(histories.enforce_budget_with_limit(5), 5);
        assert_eq!(histories.details.len(), 5);
        assert_eq!(histories.not_retained.len(), 5);
    }

    #[test]
    fn session_reset_clears_device_histories() {
        let mut histories = ResourceHistories::default();
        let now = Instant::now();
        histories.push_detail("disk:0".into(), now, 1000, Some(50.0), "test", None);
        assert!(histories.detail("disk:0").is_some());
        histories.reset_if_new_session(42);
        assert!(histories.detail("disk:0").is_none());
        assert_eq!(histories.session_id, 42);
    }

    #[test]
    fn continuous_ten_hz_stream_reports_full_sixty_second_coverage() {
        let base = Instant::now();
        let mut series = TimeSeries::new();
        // 65 seconds of 10 Hz samples with realistic scheduling jitter
        for tick in 0..650u64 {
            let jitter_ms = (tick * 7) % 19;
            let offset_ms = tick * 100 + jitter_ms;
            series.push(
                base + Duration::from_millis(offset_ms),
                100_000 + offset_ms,
                Some(42.0),
            );
        }
        // Check multiple sub-second query phases inside second 64
        for phase_ms in [64_020u64, 64_350, 64_950] {
            let query_now = base + Duration::from_millis(phase_ms);
            let summary = series
                .observed_summary(query_now, HistoryRange::Sec60)
                .unwrap();
            assert_eq!(
                summary.observed_seconds, 60,
                "phase {phase_ms}ms should report 60/60 observed seconds"
            );
            assert_eq!(summary.expected_seconds, 60);
            assert!((summary.coverage_percent() - 100.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn sensor_series_key_prevents_multi_drive_collision() {
        let drive0 = SensorReading {
            id: "/nvme/0/temperature/0".into(),
            provider: "LibreHardwareMonitor".into(),
            scope: "Drive".into(),
            device_id: "/nvme/0".into(),
            device_name: "Samsung SSD 990 PRO".into(),
            name: "Temperature".into(),
            sensor_type: planora_core::SensorType::StorageTemperatureCelsius,
            value: planora_core::Metric::available(41.0, "°C", "LHM", 1_000),
        };
        let drive1 = SensorReading {
            id: "/nvme/1/temperature/0".into(),
            device_id: "/nvme/1".into(),
            device_name: "WD Black SN850X".into(),
            value: planora_core::Metric::available(53.0, "°C", "LHM", 1_000),
            ..drive0.clone()
        };
        assert_ne!(sensor_series_key(&drive0), sensor_series_key(&drive1));
    }
}
