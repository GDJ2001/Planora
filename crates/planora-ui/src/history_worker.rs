use crate::history::ResourceHistories;
use planora_core::{ProcessIdentity, SensorReading, TelemetryFrame};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct HistoryInput {
    pub frame: Arc<TelemetryFrame>,
    pub sensors: Arc<Vec<SensorReading>>,
    pub selected: Option<ProcessIdentity>,
    pub priority_details: BTreeSet<String>,
    pub received_at: Instant,
    pub wall_clock_at_receipt_ms: u64,
    pub epoch: u64,
}

pub enum HistoryCommand {
    Ingest(Box<HistoryInput>),
    Shutdown,
}

#[derive(Clone)]
pub struct HistorySender {
    input: SyncSender<HistoryCommand>,
    dropped: Arc<AtomicU64>,
}

impl HistorySender {
    pub fn submit(&self, input: HistoryInput) {
        match self.input.try_send(HistoryCommand::Ingest(Box::new(input))) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

type PublishedHistory = Arc<Mutex<Option<(u64, Arc<ResourceHistories>)>>>;

pub struct HistoryWorker {
    input: Option<SyncSender<HistoryCommand>>,
    output: PublishedHistory,
    thread: Option<JoinHandle<()>>,
    dropped: Arc<AtomicU64>,
}

impl HistoryWorker {
    pub fn start() -> Self {
        let (input_tx, input_rx) = mpsc::sync_channel::<HistoryCommand>(64);
        // The UI only needs the latest complete snapshot. Replacing an unread
        // snapshot avoids a full clone on every poll when rendering is busy.
        let output = Arc::new(Mutex::new(None));
        let worker_output = Arc::clone(&output);
        let dropped = Arc::new(AtomicU64::new(0));
        let thread = thread::Builder::new()
            .name("planora-history".into())
            .spawn(move || {
                let mut histories = ResourceHistories::default();
                let mut epoch = 0;
                let mut last_publish = Instant::now() - Duration::from_secs(1);
                let mut pending_publish = false;
                loop {
                    let cmd = match input_rx.recv_timeout(Duration::from_millis(250)) {
                        Ok(cmd) => Some(cmd),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let (epoch_changed, selected_changed) = if let Some(cmd) = cmd {
                        let input = match cmd {
                            HistoryCommand::Ingest(input) => input,
                            HistoryCommand::Shutdown => break,
                        };
                        let epoch_changed = input.epoch != epoch;
                        if epoch_changed {
                            epoch = input.epoch;
                            histories = ResourceHistories::default();
                            last_publish = Instant::now() - Duration::from_secs(1);
                        }
                        let selected_changed = histories.selected_identity != input.selected;
                        histories.set_priority_details(input.priority_details);
                        histories.ingest_frame(
                            &input.frame,
                            &input.sensors,
                            input.selected.as_ref(),
                            input.received_at,
                            input.wall_clock_at_receipt_ms,
                        );
                        pending_publish = true;
                        (epoch_changed, selected_changed)
                    } else {
                        (false, false)
                    };
                    if pending_publish
                        && (epoch_changed
                            || selected_changed
                            || last_publish.elapsed() >= Duration::from_millis(250))
                    {
                        histories.enforce_budget();
                        let snapshot = Arc::new(histories.clone());
                        *worker_output.lock().unwrap() = Some((epoch, snapshot));
                        last_publish = Instant::now();
                        pending_publish = false;
                    }
                }
            })
            .expect("history worker thread");
        Self {
            input: Some(input_tx),
            output,
            thread: Some(thread),
            dropped,
        }
    }

    pub fn sender(&self) -> Option<HistorySender> {
        self.input.as_ref().map(|input| HistorySender {
            input: input.clone(),
            dropped: Arc::clone(&self.dropped),
        })
    }

    pub fn submit(&self, input: HistoryInput) {
        if let Some(sender) = self.sender() {
            sender.submit(input);
        }
    }

    pub fn latest(&self, epoch: u64) -> Option<Arc<ResourceHistories>> {
        self.output
            .lock()
            .unwrap()
            .take()
            .and_then(|(snapshot_epoch, snapshot)| (snapshot_epoch == epoch).then_some(snapshot))
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) {
        if let Some(input) = self.input.take() {
            let _ = input.try_send(HistoryCommand::Shutdown);
        }
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }

    pub fn join(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for HistoryWorker {
    fn drop(&mut self) {
        self.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use planora_core::{AvailabilityReason, Metric, PROTOCOL_VERSION};

    fn make_test_frame(seq: u64, sampled_at_ms: u64, cpu_pct: f32) -> TelemetryFrame {
        TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            session_id: 42,
            topology_generation: 1,
            sequence: seq,
            sampled_at_ms,
            collection_duration_ms: 1.5,
            cpu_percent: Metric::available(cpu_pct, "%", "test", sampled_at_ms),
            logical_cpus: Vec::new(),
            memory_used_bytes: Metric::available(1024 * 1024, "bytes", "test", sampled_at_ms),
            memory_available_bytes: Metric::available(
                3 * 1024 * 1024,
                "bytes",
                "test",
                sampled_at_ms,
            ),
            memory_total_bytes: Metric::available(4 * 1024 * 1024, "bytes", "test", sampled_at_ms),
            commit_used_bytes: Metric::available(2 * 1024 * 1024, "bytes", "test", sampled_at_ms),
            commit_total_bytes: Metric::available(8 * 1024 * 1024, "bytes", "test", sampled_at_ms),
            disk_read_bytes_per_sec: Metric::available(0, "B/s", "test", sampled_at_ms),
            disk_write_bytes_per_sec: Metric::available(0, "B/s", "test", sampled_at_ms),
            network_rx_bytes_per_sec: Metric::available(0, "B/s", "test", sampled_at_ms),
            network_tx_bytes_per_sec: Metric::available(0, "B/s", "test", sampled_at_ms),
            gpu_percent: Metric::unavailable(
                "%",
                "test",
                sampled_at_ms,
                AvailabilityReason::Unsupported("None".into()),
            ),
            gpu_adapters: Vec::new(),
            gpu_engines: Vec::new(),
            gpu_engine_unavailable_reason: None,
            disks: Vec::new(),
            network_interfaces: Vec::new(),
            package_power_watts: Metric::unavailable(
                "W",
                "test",
                sampled_at_ms,
                AvailabilityReason::Unsupported("None".into()),
            ),
            sensors: Vec::new(),
            processes: Vec::new(),
        }
    }

    #[test]
    fn history_worker_background_ingestion_and_snapshot_publication() {
        let mut worker = HistoryWorker::start();
        let sender = worker.sender().expect("sender available");

        let frame1 = Arc::new(make_test_frame(1, 1000, 25.0));
        let sensors = Arc::new(Vec::new());

        sender.submit(HistoryInput {
            frame: frame1,
            sensors: Arc::clone(&sensors),
            selected: None,
            priority_details: BTreeSet::new(),
            received_at: Instant::now(),
            wall_clock_at_receipt_ms: 1000,
            epoch: 1,
        });

        // First frame triggers snapshot publication because epoch changed from 0 to 1
        let mut snapshot = None;
        for _ in 0..50 {
            if let Some(snap) = worker.latest(1) {
                snapshot = Some(snap);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(
            snapshot.is_some(),
            "Snapshot must be published on epoch transition"
        );
        let snap = snapshot.unwrap();
        assert_eq!(snap.cpu.samples.len(), 1);

        // Subsequent frame at different timestamp
        let frame2 = Arc::new(make_test_frame(2, 2000, 45.0));
        sender.submit(HistoryInput {
            frame: frame2,
            sensors: Arc::clone(&sensors),
            selected: None,
            priority_details: BTreeSet::new(),
            received_at: Instant::now(),
            wall_clock_at_receipt_ms: 2000,
            epoch: 1,
        });

        // Wait for publish throttle (250ms)
        let mut snapshot2 = None;
        for _ in 0..50 {
            if let Some(snap) = worker.latest(1)
                && snap.cpu.samples.len() == 2
            {
                snapshot2 = Some(snap);
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            snapshot2.is_some(),
            "Snapshot must be updated with second sample"
        );

        // Verify wrong epoch query returns None
        assert!(worker.latest(2).is_none());

        worker.stop();
        worker.join();
        assert!(worker.is_finished());
    }
}
