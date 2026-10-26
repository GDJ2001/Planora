use planora_core::{AvailabilityReason, ProcessIdentity};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
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
    pub value: Option<f32>,
    pub source: String,
    pub unavailable_reason: Option<AvailabilityReason>,
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
        let cutoff = now.checked_sub(duration).unwrap_or(now);
        let mut buckets: std::collections::BTreeMap<u64, (f64, usize)> =
            std::collections::BTreeMap::new();
        for sample in &self.samples {
            if sample.time < cutoff || sample.time > now {
                continue;
            }
            if let Some(value) = sample.value.filter(|v| v.is_finite()) {
                let second = sample.time.duration_since(cutoff).as_secs();
                let bucket = buckets.entry(second).or_default();
                bucket.0 += value as f64;
                bucket.1 += 1;
            }
        }
        if buckets.is_empty() {
            return None;
        }
        let values: Vec<f32> = buckets
            .values()
            .map(|(sum, count)| (sum / *count as f64) as f32)
            .collect();
        let total: f64 = values.iter().map(|v| *v as f64).sum();
        Some(ObservedSummary {
            minimum: values.iter().copied().fold(f32::INFINITY, f32::min),
            maximum: values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            average: (total / values.len() as f64) as f32,
            observed_seconds: values.len(),
            expected_seconds: duration.as_secs() as usize,
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
        self.samples.push_back(Sample {
            time,
            timestamp_ms,
            value,
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

    /// Extract points normalized to [0.0, 1.0] in time across the given range ending at `now`.
    /// Segments are split when missing data (None) or time gaps exceed `gap_threshold`.
    pub fn normalized_segments(
        &self,
        now: Instant,
        range: HistoryRange,
        gap_threshold: Duration,
    ) -> Vec<Vec<(f32, f32)>> {
        let dur = range.duration().as_secs_f32();
        if dur <= 0.0 || self.samples.is_empty() {
            return Vec::new();
        }
        let cutoff = now.checked_sub(range.duration()).unwrap_or(now);

        let mut segments: Vec<Vec<(f32, f32)>> = Vec::new();
        let mut current_segment: Vec<(f32, f32)> = Vec::new();
        let mut prev_time: Option<Instant> = None;

        for s in &self.samples {
            // Paused views use an earlier `now` and must not see future data.
            if s.time < cutoff || s.time > now {
                continue;
            }
            if let Some(prev) = prev_time
                && s.time.saturating_duration_since(prev) > gap_threshold
                && !current_segment.is_empty()
            {
                segments.push(std::mem::take(&mut current_segment));
            }
            prev_time = Some(s.time);

            if let Some(val) = s.value {
                let elapsed_from_cutoff = s.time.saturating_duration_since(cutoff).as_secs_f32();
                let norm_x = (elapsed_from_cutoff / dur).clamp(0.0, 1.0);
                current_segment.push((norm_x, val));
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

        segments
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
        let columns = pixel_columns.max(1);
        self.normalized_segments(now, range, gap_threshold)
            .into_iter()
            .map(|segment| {
                if segment.len() <= columns * 2 {
                    return segment;
                }
                let mut result = Vec::with_capacity(columns * 2);
                let mut start = 0;
                while start < segment.len() {
                    let bucket = (segment[start].0 * columns as f32).floor() as usize;
                    let mut end = start + 1;
                    while end < segment.len()
                        && (segment[end].0 * columns as f32).floor() as usize == bucket
                    {
                        end += 1;
                    }
                    let mut min_idx = start;
                    let mut max_idx = start;
                    for idx in start + 1..end {
                        if segment[idx].1 < segment[min_idx].1 {
                            min_idx = idx;
                        }
                        if segment[idx].1 > segment[max_idx].1 {
                            max_idx = idx;
                        }
                    }
                    if min_idx <= max_idx {
                        result.push(segment[min_idx]);
                        if max_idx != min_idx {
                            result.push(segment[max_idx]);
                        }
                    } else {
                        result.push(segment[max_idx]);
                        result.push(segment[min_idx]);
                    }
                    start = end;
                }
                result
            })
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
            if s.time < cutoff || s.time > now {
                continue;
            }
            let diff = if s.time > target_time {
                s.time.saturating_duration_since(target_time)
            } else {
                target_time.saturating_duration_since(s.time)
            };
            if diff < min_diff {
                min_diff = diff;
                let age = now.saturating_duration_since(s.time);
                best = Some((s, age));
            }
        }

        best
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

    // Selected process history (only recorded while observed)
    pub selected_identity: Option<ProcessIdentity>,
    pub selected_cpu: TimeSeries,
    pub selected_memory: TimeSeries,
}

impl ResourceHistories {
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
            self.selected_identity = None;
            self.selected_cpu.clear();
            self.selected_memory.clear();
        }
    }

    pub fn set_selected_process(&mut self, identity: Option<&ProcessIdentity>) {
        if self.selected_identity.as_ref() != identity {
            self.selected_identity = identity.cloned();
            self.selected_cpu.clear();
            self.selected_memory.clear();
        }
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
    fn downsampling_retains_spikes_and_missing_data_gaps() {
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
        let full = ts.normalized_segments(now, HistoryRange::Sec30, Duration::from_secs(2));
        let compact = ts.downsampled_segments(now, HistoryRange::Sec30, Duration::from_secs(2), 40);
        assert_eq!(full.len(), compact.len());
        assert!(compact.iter().flatten().any(|(_, value)| *value == 99.0));
        assert!(compact.iter().map(Vec::len).sum::<usize>() <= 160);
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
}
