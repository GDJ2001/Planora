use crate::history::ResourceHistories;
use planora_core::{ProcessIdentity, SensorReading, TelemetryFrame};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct HistoryInput {
    pub frame: Arc<TelemetryFrame>,
    pub sensors: Arc<Vec<SensorReading>>,
    pub selected: Option<ProcessIdentity>,
    pub received_at: Instant,
    pub wall_clock_at_receipt_ms: u64,
    pub epoch: u64,
}

pub struct HistoryWorker {
    input: Option<SyncSender<HistoryInput>>,
    output: Receiver<(u64, Arc<ResourceHistories>)>,
    thread: Option<JoinHandle<()>>,
    dropped: Arc<AtomicU64>,
}

impl HistoryWorker {
    pub fn start() -> Self {
        let (input_tx, input_rx) = mpsc::sync_channel::<HistoryInput>(2);
        let (output_tx, output_rx) = mpsc::sync_channel::<(u64, Arc<ResourceHistories>)>(1);
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
                    histories.ingest_frame(
                        &input.frame,
                        &input.sensors,
                        input.selected.as_ref(),
                        input.received_at,
                        input.wall_clock_at_receipt_ms,
                    );
                    if selected_changed || last_publish.elapsed() >= Duration::from_secs(1) {
                        histories.enforce_budget();
                        if output_tx
                            .try_send((epoch, Arc::new(histories.clone())))
                            .is_ok()
                        {
                            last_publish = Instant::now();
                        }
                    }
                }
            })
            .expect("history worker thread");
        Self {
            input: Some(input_tx),
            output: output_rx,
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
        let mut latest = None;
        loop {
            match self.output.try_recv() {
                Ok((snapshot_epoch, snapshot)) if snapshot_epoch == epoch => {
                    latest = Some(snapshot)
                }
                Ok(_) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        latest
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
