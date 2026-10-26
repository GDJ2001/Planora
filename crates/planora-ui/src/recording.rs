//! User-initiated local recordings. The privileged helper never opens user files.
use eframe::egui;
use planora_core::{
    Metric, PROTOCOL_VERSION, ProcessIdentity, ProcessRecord, TelemetryFrame, Topology,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::Duration,
};

const FORMAT_VERSION: u32 = 3;
const MAX_RECORDING_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RECORDING_MS: u64 = 2 * 60 * 60 * 1_000;
const MAX_LINE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordedSnapshot {
    pub sampled_at_ms: u64,
    pub sequence: u64,
    pub session_id: u64,
    pub topology_generation: u64,
    pub collection_duration_ms: f32,
    pub cpu_percent: Metric<f32>,
    #[serde(default)]
    pub logical_cpus: Vec<planora_core::LogicalCpuSample>,
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
    pub gpu_adapters: Vec<planora_core::GpuAdapterSample>,
    pub gpu_engines: Vec<planora_core::GpuEngineSample>,
    #[serde(default)]
    pub gpu_engine_unavailable_reason: Option<planora_core::AvailabilityReason>,
    #[serde(default)]
    pub disks: Vec<planora_core::ResourceDeviceSample>,
    #[serde(default)]
    pub network_interfaces: Vec<planora_core::ResourceDeviceSample>,
    pub package_power_watts: Metric<f32>,
    #[serde(default)]
    pub sensors: Vec<planora_core::SensorReading>,
    pub selected_process: Option<ProcessRecord>,
}

impl RecordedSnapshot {
    fn from_frame(frame: &TelemetryFrame, selected: Option<&ProcessIdentity>) -> Self {
        Self {
            sampled_at_ms: frame.sampled_at_ms,
            sequence: frame.sequence,
            session_id: frame.session_id,
            topology_generation: frame.topology_generation,
            collection_duration_ms: frame.collection_duration_ms,
            cpu_percent: frame.cpu_percent.clone(),
            logical_cpus: frame.logical_cpus.clone(),
            memory_used_bytes: frame.memory_used_bytes.clone(),
            memory_available_bytes: frame.memory_available_bytes.clone(),
            memory_total_bytes: frame.memory_total_bytes.clone(),
            commit_used_bytes: frame.commit_used_bytes.clone(),
            commit_total_bytes: frame.commit_total_bytes.clone(),
            disk_read_bytes_per_sec: frame.disk_read_bytes_per_sec.clone(),
            disk_write_bytes_per_sec: frame.disk_write_bytes_per_sec.clone(),
            network_rx_bytes_per_sec: frame.network_rx_bytes_per_sec.clone(),
            network_tx_bytes_per_sec: frame.network_tx_bytes_per_sec.clone(),
            gpu_percent: frame.gpu_percent.clone(),
            gpu_adapters: frame.gpu_adapters.clone(),
            gpu_engines: frame.gpu_engines.clone(),
            gpu_engine_unavailable_reason: frame.gpu_engine_unavailable_reason.clone(),
            disks: frame.disks.clone(),
            network_interfaces: frame.network_interfaces.clone(),
            package_power_watts: frame.package_power_watts.clone(),
            sensors: frame.sensors.clone(),
            selected_process: selected
                .and_then(|id| frame.processes.iter().find(|p| &p.identity == id).cloned()),
        }
    }

    fn into_frame(self) -> TelemetryFrame {
        TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            session_id: self.session_id,
            topology_generation: self.topology_generation,
            sequence: self.sequence,
            sampled_at_ms: self.sampled_at_ms,
            collection_duration_ms: self.collection_duration_ms,
            cpu_percent: self.cpu_percent,
            logical_cpus: self.logical_cpus,
            memory_used_bytes: self.memory_used_bytes,
            memory_available_bytes: self.memory_available_bytes,
            memory_total_bytes: self.memory_total_bytes,
            commit_used_bytes: self.commit_used_bytes,
            commit_total_bytes: self.commit_total_bytes,
            disk_read_bytes_per_sec: self.disk_read_bytes_per_sec,
            disk_write_bytes_per_sec: self.disk_write_bytes_per_sec,
            network_rx_bytes_per_sec: self.network_rx_bytes_per_sec,
            network_tx_bytes_per_sec: self.network_tx_bytes_per_sec,
            gpu_percent: self.gpu_percent,
            gpu_adapters: self.gpu_adapters,
            gpu_engines: self.gpu_engines,
            gpu_engine_unavailable_reason: self.gpu_engine_unavailable_reason,
            disks: self.disks,
            network_interfaces: self.network_interfaces,
            package_power_watts: self.package_power_watts,
            sensors: self.sensors,
            processes: self.selected_process.into_iter().collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RecordLine {
    Header {
        format_version: u32,
        created_at_ms: u64,
        build_version: String,
        build_commit: String,
        topology: Box<Option<Topology>>,
    },
    Sample {
        snapshot: Box<RecordedSnapshot>,
        dropped_before: u64,
    },
    End {
        ended_at_ms: u64,
        reason: String,
    },
}

#[derive(Clone, Debug, Default)]
pub struct RecordingStatus {
    pub path: Option<PathBuf>,
    pub active: bool,
    pub bytes_written: u64,
    pub dropped: u64,
    pub message: String,
}

pub struct Recorder {
    tx: Option<SyncSender<RecordLine>>,
    status: Arc<Mutex<RecordingStatus>>,
    dropped_pending: AtomicU64,
    last_sampled_at_ms: u64,
    record_selected: bool,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            tx: None,
            status: Arc::new(Mutex::new(RecordingStatus::default())),
            dropped_pending: AtomicU64::new(0),
            last_sampled_at_ms: 0,
            record_selected: false,
        }
    }
}

pub fn recordings_dir() -> io::Result<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("USERPROFILE").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME").map(PathBuf::from);
    base.map(|p| p.join("Documents").join("Planora").join("Recordings"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory unavailable"))
}

fn write_line(writer: &mut BufWriter<File>, line: &RecordLine, bytes: &mut u64) -> io::Result<()> {
    let encoded = serde_json::to_vec(line).map_err(io::Error::other)?;
    let next_bytes = bytes.saturating_add(encoded.len() as u64 + 1);
    if next_bytes > MAX_RECORDING_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "recording reached 256 MiB limit",
        ));
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    *bytes = next_bytes;
    Ok(())
}

impl Recorder {
    pub fn status(&self) -> RecordingStatus {
        self.status.lock().unwrap().clone()
    }
    pub fn is_active(&self) -> bool {
        self.tx.is_some() && self.status.lock().unwrap().active
    }

    pub fn start(
        &mut self,
        topology: Option<Topology>,
        record_selected: bool,
        version: &str,
        commit: &str,
    ) -> io::Result<PathBuf> {
        if self.status.lock().unwrap().active {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "recording already active",
            ));
        }
        let created_at_ms = planora_telemetry::unix_ms();
        let path = recordings_dir()?.join(format!("planora-{created_at_ms}.planora.jsonl"));
        let (tx, rx) = mpsc::sync_channel::<RecordLine>(64);
        let status = self.status.clone();
        let worker_path = path.clone();
        let header = RecordLine::Header {
            format_version: FORMAT_VERSION,
            created_at_ms,
            build_version: version.into(),
            build_commit: commit.into(),
            topology: Box::new(topology),
        };
        *status.lock().unwrap() = RecordingStatus {
            path: Some(path.clone()),
            active: true,
            bytes_written: 0,
            dropped: 0,
            message: "Starting recording".into(),
        };
        if let Err(error) = thread::Builder::new()
            .name("planora-recording".into())
            .spawn(move || {
                let result = write_recording(&worker_path, header, rx, &status, created_at_ms);
                let mut state = status.lock().unwrap();
                state.active = false;
                state.message = match result {
                    Ok(()) => "Recording saved".into(),
                    Err(e) => format!("Recording stopped: {e}"),
                };
            })
        {
            self.status.lock().unwrap().active = false;
            return Err(error);
        }
        self.tx = Some(tx);
        self.record_selected = record_selected;
        self.last_sampled_at_ms = 0;
        self.dropped_pending.store(0, Ordering::Relaxed);
        Ok(path)
    }

    pub fn stop(&mut self) {
        self.tx = None;
    }

    pub fn record(
        &mut self,
        frame: &TelemetryFrame,
        selected: Option<&ProcessIdentity>,
        sensors: &[planora_core::SensorReading],
    ) {
        if !self.is_active() || frame.sampled_at_ms < self.last_sampled_at_ms.saturating_add(1_000)
        {
            return;
        }
        self.last_sampled_at_ms = frame.sampled_at_ms;
        let selected = if self.record_selected { selected } else { None };
        let mut snapshot = RecordedSnapshot::from_frame(frame, selected);
        snapshot.sensors = sensors.to_vec();
        let dropped_before = self.dropped_pending.swap(0, Ordering::Relaxed);
        if let Some(tx) = &self.tx {
            match tx.try_send(RecordLine::Sample {
                snapshot: Box::new(snapshot),
                dropped_before,
            }) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    self.dropped_pending
                        .fetch_add(dropped_before + 1, Ordering::Relaxed);
                    self.status.lock().unwrap().dropped += 1;
                }
                Err(TrySendError::Disconnected(_)) => self.tx = None,
            }
        }
    }
}

fn write_recording(
    path: &Path,
    header: RecordLine,
    rx: Receiver<RecordLine>,
    status: &Arc<Mutex<RecordingStatus>>,
    started_ms: u64,
) -> io::Result<()> {
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid recording path"))?,
    )?;
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut writer = BufWriter::new(file);
    let mut bytes = 0;
    write_line(&mut writer, &header, &mut bytes)?;
    status.lock().unwrap().message = "Recording".into();
    let mut reason = "Stopped by user".to_string();
    loop {
        if planora_telemetry::unix_ms().saturating_sub(started_ms) >= MAX_RECORDING_MS {
            reason = "Two-hour limit reached".into();
            break;
        }
        let event = match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if let Err(e) = write_line(&mut writer, &event, &mut bytes) {
            reason = e.to_string();
            break;
        }
        status.lock().unwrap().bytes_written = bytes;
    }
    let end = RecordLine::End {
        ended_at_ms: planora_telemetry::unix_ms(),
        reason: reason.clone(),
    };
    let _ = write_line(&mut writer, &end, &mut bytes);
    writer.flush()?;
    writer.get_ref().sync_all()?;
    status.lock().unwrap().bytes_written = bytes;
    if reason == "Stopped by user" {
        Ok(())
    } else {
        Err(io::Error::other(reason))
    }
}

enum ReplayMessage {
    Header(Option<Topology>),
    Sample(Box<TelemetryFrame>),
    Done(String),
}

#[derive(Default)]
pub struct Replay {
    receiver: Option<Receiver<ReplayMessage>>,
    stop: Arc<AtomicBool>,
    pub frame: Option<Arc<TelemetryFrame>>,
    pub topology: Option<Topology>,
    pub message: String,
    pub path: Option<PathBuf>,
}

impl Replay {
    pub fn is_active(&self) -> bool {
        self.path.is_some()
    }
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.receiver = None;
        self.frame = None;
        self.topology = None;
        self.path = None;
        self.message = "Replay closed".into();
    }
    pub fn start(&mut self, path: PathBuf, ctx: egui::Context) -> io::Result<()> {
        let size = fs::metadata(&path)?.len();
        if size > MAX_RECORDING_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "recording exceeds 256 MiB",
            ));
        }
        self.stop();
        self.stop = Arc::new(AtomicBool::new(false));
        let stop = self.stop.clone();
        let (tx, rx) = mpsc::sync_channel(32);
        let source = path.clone();
        thread::Builder::new()
            .name("planora-replay".into())
            .spawn(move || {
                let result = read_recording(&source, &tx, &stop, &ctx);
                let _ = tx.send(ReplayMessage::Done(match result {
                    Ok(()) => "Replay finished".into(),
                    Err(e) => format!("Replay stopped: {e}"),
                }));
                ctx.request_repaint();
            })?;
        self.path = Some(path);
        self.receiver = Some(rx);
        self.message = "Opening recording".into();
        Ok(())
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.receiver {
            for _ in 0..4 {
                match rx.try_recv() {
                    Ok(ReplayMessage::Header(topology)) => {
                        self.topology = topology;
                        changed = true;
                    }
                    Ok(ReplayMessage::Sample(frame)) => {
                        self.frame = Some(Arc::from(frame));
                        self.message = "Replaying".into();
                        changed = true;
                    }
                    Ok(ReplayMessage::Done(message)) => {
                        self.message = message;
                        changed = true;
                        break;
                    }
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }
        }
        changed
    }
}

fn read_line_limited(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let count = reader
        .take(MAX_LINE_BYTES + 1)
        .read_until(b'\n', &mut line)?;
    if count == 0 {
        return Ok(None);
    }
    if count as u64 > MAX_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "record line too large",
        ));
    }
    Ok(Some(line))
}

fn read_recording(
    path: &Path,
    tx: &SyncSender<ReplayMessage>,
    stop: &AtomicBool,
    ctx: &egui::Context,
) -> io::Result<()> {
    let mut reader = BufReader::new(File::open(path)?);
    let header = read_line_limited(&mut reader)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty recording"))?;
    let header: RecordLine = serde_json::from_slice(&header).map_err(io::Error::other)?;
    let RecordLine::Header {
        format_version,
        topology,
        ..
    } = header
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing recording header",
        ));
    };
    if !(1..=FORMAT_VERSION).contains(&format_version) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported recording version",
        ));
    }
    tx.send(ReplayMessage::Header(*topology))
        .map_err(io::Error::other)?;
    let mut previous_ms = None::<u64>;
    while !stop.load(Ordering::Relaxed) {
        let Some(line) = read_line_limited(&mut reader)? else {
            break;
        };
        let event: RecordLine = match serde_json::from_slice(&line) {
            Ok(event) => event,
            Err(_) if !line.ends_with(b"\n") => break, // interrupted final write
            Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
        };
        match event {
            RecordLine::Sample { snapshot, .. } => {
                if let Some(previous) = previous_ms {
                    let delay = snapshot.sampled_at_ms.saturating_sub(previous).min(2_000);
                    let mut waited = 0;
                    while waited < delay && !stop.load(Ordering::Relaxed) {
                        let step = (delay - waited).min(50);
                        thread::sleep(Duration::from_millis(step));
                        waited += step;
                    }
                }
                previous_ms = Some(snapshot.sampled_at_ms);
                let frame = (*snapshot).into_frame();
                frame.validate().map_err(io::Error::other)?;
                tx.send(ReplayMessage::Sample(Box::new(frame)))
                    .map_err(io::Error::other)?;
                ctx.request_repaint();
            }
            RecordLine::End { .. } => break,
            RecordLine::Header { .. } => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate recording header",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_final_line_preserves_complete_samples() {
        let frame = crate::smoke_test::test_frame();
        let sample = RecordedSnapshot::from_frame(&frame, None);
        assert!(sample.selected_process.is_none());
        let selected = RecordedSnapshot::from_frame(&frame, Some(&frame.processes[0].identity));
        assert_eq!(
            selected.selected_process.as_ref().unwrap().identity,
            frame.processes[0].identity
        );

        let path = std::env::temp_dir().join(format!(
            "planora-recording-parser-{}-{}.jsonl",
            std::process::id(),
            planora_telemetry::unix_ms()
        ));
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::to_string(&RecordLine::Header {
                format_version: FORMAT_VERSION,
                created_at_ms: frame.sampled_at_ms,
                build_version: "test".into(),
                build_commit: "test".into(),
                topology: Box::new(None),
            })
            .unwrap()
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::to_string(&RecordLine::Sample {
                snapshot: Box::new(sample),
                dropped_before: 0
            })
            .unwrap()
        )
        .unwrap();
        file.write_all(b"{\"type\":\"sample\"").unwrap();
        drop(file);

        let (tx, rx) = mpsc::sync_channel(8);
        let ctx = egui::Context::default();
        read_recording(&path, &tx, &AtomicBool::new(false), &ctx).unwrap();
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], ReplayMessage::Header(_)));
        assert!(
            matches!(&events[1], ReplayMessage::Sample(replayed) if replayed.sequence == frame.sequence)
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn recording_limit_refuses_next_record_without_partial_write() {
        let path = std::env::temp_dir().join(format!(
            "planora-limit-{}-{}.jsonl",
            std::process::id(),
            planora_telemetry::unix_ms()
        ));
        let mut writer = BufWriter::new(File::create(&path).unwrap());
        let mut bytes = MAX_RECORDING_BYTES - 2;
        let result = write_line(
            &mut writer,
            &RecordLine::End {
                ended_at_ms: 1,
                reason: "test".into(),
            },
            &mut bytes,
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::FileTooLarge);
        writer.flush().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);
        fs::remove_file(path).unwrap();
    }
}
