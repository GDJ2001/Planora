//! Windows process acquisition runs independently of the 100 ms system sampler.
//! Its completed records retain their acquisition timestamps when reused by a
//! faster snapshot. Requests and published results are both bounded.
use super::{query_windows_basic_processes, rate, unix_ms, windows_start_filetime};
use planora_core::{AvailabilityReason, IdentitySource, Metric, ProcessIdentity, ProcessRecord};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// sysinfo requires a nonzero interval for meaningful process CPU deltas.
const REFRESH_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Default)]
struct Published {
    records: Arc<Vec<ProcessRecord>>,
    refresh_ms: f32,
    generation: u64,
}

pub(super) struct WindowsProcessWorker {
    request_tx: mpsc::SyncSender<()>,
    published: Arc<Mutex<Published>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    last_request: Option<Instant>,
}

impl WindowsProcessWorker {
    pub(super) fn new() -> Self {
        let (request_tx, request_rx) = mpsc::sync_channel(1);
        let published = Arc::new(Mutex::new(Published::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_published = Arc::clone(&published);
        let worker_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("planora-process-collector".into())
            .spawn(move || run(request_rx, worker_published, worker_stop))
            .expect("process collector thread");
        let mut worker = Self {
            request_tx,
            published,
            stop,
            handle: Some(handle),
            last_request: None,
        };
        worker.request_update(Instant::now());
        worker
    }

    pub(super) fn request_update(&mut self, now: Instant) {
        if self
            .last_request
            .is_some_and(|last| now.duration_since(last) < REFRESH_INTERVAL)
        {
            return;
        }
        if self.request_tx.try_send(()).is_ok() {
            self.last_request = Some(now);
        }
    }

    pub(super) fn latest(&self) -> (Arc<Vec<ProcessRecord>>, f32, u64) {
        let value = self.published.lock().unwrap_or_else(|e| e.into_inner());
        (
            Arc::clone(&value.records),
            value.refresh_ms,
            value.generation,
        )
    }
}

impl Drop for WindowsProcessWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.request_tx.try_send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run(requests: mpsc::Receiver<()>, published: Arc<Mutex<Published>>, stop: Arc<AtomicBool>) {
    // The CPU list is needed to normalize process usage to machine capacity.
    let mut system = System::new_all();
    let mut last_refresh: Option<Instant> = None;
    while !stop.load(Ordering::Acquire) {
        match requests.recv_timeout(Duration::from_millis(100)) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if stop.load(Ordering::Acquire) {
            break;
        }
        let start = Instant::now();
        let stamp = unix_ms();
        let elapsed = last_refresh.map(|prior| start.duration_since(prior));
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::everything(),
        );
        last_refresh = Some(start);
        let basic_map: HashMap<u32, u64> = query_windows_basic_processes()
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.pid, p.sequence_number))
            .collect();
        let mut records = Vec::with_capacity(system.processes().len());
        let cpu_count = system.cpus().len().max(1) as f32;
        for (pid, process) in system.processes() {
            let pid = pid.as_u32();
            let (started, sequence_number, source) =
                if let Some(filetime) = windows_start_filetime(pid) {
                    (
                        filetime,
                        basic_map.get(&pid).copied(),
                        IdentitySource::WindowsProcessFiletime,
                    )
                } else if let Some(&sequence) = basic_map.get(&pid) {
                    (
                        sequence,
                        Some(sequence),
                        IdentitySource::WindowsBasicProcessSequence,
                    )
                } else {
                    (
                        process.start_time(),
                        None,
                        IdentitySource::SysinfoEpochSeconds,
                    )
                };
            let identity = ProcessIdentity {
                pid,
                started,
                sequence_number,
                source,
            };
            let io = process.disk_usage();
            let io_metric = |bytes| {
                elapsed.map_or_else(
                    || {
                        Metric::unavailable(
                            planora_core::UNIT_BYTES_PER_SEC,
                            "sysinfo process I/O",
                            stamp,
                            AvailabilityReason::WarmingUp,
                        )
                    },
                    |interval| {
                        Metric::available(
                            rate(bytes, interval),
                            planora_core::UNIT_BYTES_PER_SEC,
                            "Windows process I/O delta",
                            stamp,
                        )
                    },
                )
            };
            let cpu_percent = elapsed.map_or_else(
                || {
                    Metric::unavailable(
                        "%",
                        "sysinfo process time delta",
                        stamp,
                        AvailabilityReason::WarmingUp,
                    )
                },
                |_| {
                    Metric::available(
                        (process.cpu_usage() / cpu_count).clamp(0.0, 100.0),
                        "%",
                        "sysinfo normalized to machine capacity",
                        stamp,
                    )
                },
            );
            let physical_disk_unavailable = || {
                Metric::unavailable(
                    planora_core::UNIT_BYTES_PER_SEC,
                    "sysinfo",
                    stamp,
                    AvailabilityReason::Unsupported(
                        "Windows counts all process I/O, not disk I/O".into(),
                    ),
                )
            };
            records.push(ProcessRecord {
                identity,
                name: process.name().to_string_lossy().into_owned(),
                cpu_percent,
                memory_bytes: Metric::available(
                    process.memory(),
                    "bytes",
                    "sysinfo resident memory",
                    stamp,
                ),
                disk_read_bytes_per_sec: physical_disk_unavailable(),
                disk_write_bytes_per_sec: physical_disk_unavailable(),
                io_read_bytes_per_sec: io_metric(io.read_bytes),
                io_write_bytes_per_sec: io_metric(io.written_bytes),
                gpu_percent: Metric::unavailable(
                    "%",
                    "OS GPU provider",
                    stamp,
                    AvailabilityReason::Unsupported("per-process GPU counter unavailable".into()),
                ),
                affinity: Metric::unavailable(
                    "cpus",
                    "OS affinity provider",
                    stamp,
                    AvailabilityReason::Unsupported("not queried in baseline sampler".into()),
                ),
            });
        }
        records.sort_by(|a, b| {
            b.cpu_percent
                .value
                .unwrap_or_default()
                .total_cmp(&a.cpu_percent.value.unwrap_or_default())
        });
        let mut result = published.lock().unwrap_or_else(|e| e.into_inner());
        result.records = Arc::new(records);
        result.refresh_ms = start.elapsed().as_secs_f32() * 1000.0;
        result.generation += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_publishes_and_joins() {
        let mut worker = WindowsProcessWorker::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        while worker.latest().2 == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let (records, duration, generation) = worker.latest();
        assert!(generation > 0);
        assert!(!records.is_empty());
        assert!(duration.is_finite());
        let sampled_at = records[0].memory_bytes.sampled_at_ms;
        thread::sleep(REFRESH_INTERVAL);
        assert_eq!(worker.latest().2, generation);
        assert_eq!(worker.latest().0[0].memory_bytes.sampled_at_ms, sampled_at);
        worker.request_update(Instant::now());
        let deadline = Instant::now() + Duration::from_secs(3);
        while worker.latest().2 == generation && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let (updated, _, new_generation) = worker.latest();
        assert!(new_generation > generation);
        assert!(updated[0].memory_bytes.sampled_at_ms >= sampled_at);
        let shutdown = Instant::now();
        drop(worker);
        assert!(shutdown.elapsed() < Duration::from_secs(2));
    }
}
