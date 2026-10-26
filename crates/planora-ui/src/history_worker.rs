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

type PublishedHistory = Arc<Mutex<Option<(u64, Arc<ResourceHistories>)>>>;

pub struct HistoryWorker {
    input: Option<SyncSender<HistoryInput>>,
    output: PublishedHistory,
    thread: Option<JoinHandle<()>>,
    dropped: Arc<AtomicU64>,
}

impl HistoryWorker {
    pub fn start() -> Self {
        let (input_tx, input_rx) = mpsc::sync_channel::<HistoryInput>(2);
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
                while let Ok(input) = input_rx.recv() {
                    if input.epoch != epoch {
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
                    if selected_changed || last_publish.elapsed() >= Duration::from_secs(1) {
                        histories.enforce_budget();
                        let snapshot = Arc::new(histories.clone());
                        *worker_output.lock().unwrap() = Some((epoch, snapshot));
                        last_publish = Instant::now();
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

    pub fn submit(&self, input: HistoryInput) {
        if let Some(sender) = &self.input {
            match sender.try_send(input) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => {}
            }
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
        self.input.take();
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
