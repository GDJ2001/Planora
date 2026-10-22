use crate::components::table::{SortColumn, sort_processes};
use crate::history::{HistoryRange, ResourceHistories, TimeSeries};
use planora_core::{
    BenchmarkState, DiagnosticEntry, Metric, SensorReading, TelemetryFrame, Topology,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Instant,
};

#[derive(Clone, Default)]
pub struct ExportStatus {
    pub active: bool,
    pub message: String,
    pub path: Option<PathBuf>,
}

#[derive(Default)]
pub struct ExportManager {
    status: Arc<Mutex<ExportStatus>>,
    worker: Option<thread::JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}

fn export_dir() -> io::Result<PathBuf> {
    let root = super::recording::recordings_dir()?;
    Ok(root.parent().unwrap_or(&root).join("Exports"))
}

fn csv_cell(value: &str) -> String {
    let risky = value
        .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '\u{feff}' | '\u{200b}'))
        .starts_with(['=', '+', '-', '@']);
    let safe = if risky {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

fn csv_metric<T: ToString>(metric: &Metric<T>) -> [String; 5] {
    [
        metric
            .value
            .as_ref()
            .map_or_else(String::new, ToString::to_string),
        metric.unit.clone(),
        metric.source.clone(),
        metric.sampled_at_ms.to_string(),
        metric
            .unavailable_reason
            .as_ref()
            .map_or_else(String::new, ToString::to_string),
    ]
}

fn csv_line(writer: &mut impl Write, values: &[String]) -> io::Result<()> {
    let line = values
        .iter()
        .map(|value| csv_cell(value))
        .collect::<Vec<_>>()
        .join(",");
    writeln!(writer, "{line}")
}

impl ExportManager {
    pub fn status(&self) -> ExportStatus {
        self.status.lock().unwrap().clone()
    }

    fn start(
        &mut self,
        label: &str,
        extension: &str,
        task: impl FnOnce(PathBuf, &AtomicBool) -> io::Result<()> + Send + 'static,
    ) -> io::Result<()> {
        if self.status.lock().unwrap().active {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "another export is running",
            ));
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.cancel.store(false, Ordering::Release);
        let path = export_dir()?.join(format!(
            "planora-{label}-{}.{}",
            planora_telemetry::unix_ms(),
            extension
        ));
        *self.status.lock().unwrap() = ExportStatus {
            active: true,
            message: format!("Exporting {label}"),
            path: Some(path.clone()),
        };
        let status = self.status.clone();
        let cancel = self.cancel.clone();
        let worker = thread::Builder::new()
            .name("planora-export".into())
            .spawn(move || {
                let result = task(path.clone(), &cancel);
                if result.is_err() {
                    let _ = fs::remove_file(path);
                }
                let mut status = status.lock().unwrap();
                status.active = false;
                status.message = match result {
                    Ok(()) => "Export complete".into(),
                    Err(e) => format!("Export failed: {e}"),
                };
            });
        self.worker = Some(match worker {
            Ok(worker) => worker,
            Err(error) => {
                self.status.lock().unwrap().active = false;
                return Err(error);
            }
        });
        Ok(())
    }

    pub fn request_cancel(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(|w| w.is_finished())
    }

    pub fn cancel(&mut self) {
        self.request_cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    pub fn processes(
        &mut self,
        frame: Arc<TelemetryFrame>,
        query: String,
        sort: SortColumn,
        descending: bool,
    ) -> io::Result<()> {
        self.start("processes", "csv", move |path, cancel| {
            fs::create_dir_all(path.parent().unwrap())?;
            let mut writer =
                BufWriter::new(OpenOptions::new().create_new(true).write(true).open(path)?);
            let mut header = vec![
                "process_name".into(),
                "pid".into(),
                "started".into(),
                "identity_source".into(),
            ];
            for metric in [
                "cpu_percent",
                "memory_bytes",
                "disk_read_bytes_per_sec",
                "disk_write_bytes_per_sec",
                "io_read_bytes_per_sec",
                "io_write_bytes_per_sec",
                "gpu_percent",
            ] {
                for suffix in [
                    "value",
                    "unit",
                    "source",
                    "sampled_at_ms",
                    "unavailable_reason",
                ] {
                    header.push(format!("{metric}_{suffix}"));
                }
            }
            csv_line(&mut writer, &header)?;
            for p in sort_processes(&frame.processes, &query, sort, descending) {
                if cancel.load(Ordering::Acquire) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "export cancelled",
                    ));
                }
                let mut row = vec![
                    p.name.clone(),
                    p.identity.pid.to_string(),
                    p.identity.started.to_string(),
                    p.identity.source.to_string(),
                ];
                row.extend(csv_metric(&p.cpu_percent));
                row.extend(csv_metric(&p.memory_bytes));
                row.extend(csv_metric(&p.disk_read_bytes_per_sec));
                row.extend(csv_metric(&p.disk_write_bytes_per_sec));
                row.extend(csv_metric(&p.io_read_bytes_per_sec));
                row.extend(csv_metric(&p.io_write_bytes_per_sec));
                row.extend(csv_metric(&p.gpu_percent));
                csv_line(&mut writer, &row)?;
            }
            writer.flush()
        })
    }

    pub fn charts(
        &mut self,
        histories: ResourceHistories,
        ranges: [HistoryRange; 4],
        now: Instant,
    ) -> io::Result<()> {
        self.start("charts", "csv", move |path, cancel| {
            fs::create_dir_all(path.parent().unwrap())?;
            let mut writer =
                BufWriter::new(OpenOptions::new().create_new(true).write(true).open(path)?);
            csv_line(
                &mut writer,
                &[
                    "series".into(),
                    "range".into(),
                    "sampled_at_ms".into(),
                    "value".into(),
                    "unit".into(),
                    "source".into(),
                    "unavailable_reason".into(),
                ],
            )?;
            let all: [(&str, &TimeSeries, &str, HistoryRange); 6] = [
                ("CPU", &histories.cpu, "%", ranges[0]),
                ("Memory", &histories.memory_pct, "%", ranges[1]),
                ("Disk read", &histories.disk_read, "bytes/s", ranges[2]),
                ("Disk write", &histories.disk_write, "bytes/s", ranges[2]),
                ("Network receive", &histories.net_rx, "bytes/s", ranges[3]),
                ("Network transmit", &histories.net_tx, "bytes/s", ranges[3]),
            ];
            for (name, series, unit, range) in all {
                if cancel.load(Ordering::Acquire) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "export cancelled",
                    ));
                }
                let cutoff = now.checked_sub(range.duration()).unwrap_or(now);
                for sample in &series.samples {
                    if sample.time < cutoff || sample.time > now {
                        continue;
                    }
                    csv_line(
                        &mut writer,
                        &[
                            name.into(),
                            range.label().into(),
                            sample.timestamp_ms.to_string(),
                            sample.value.map_or_else(String::new, |v| v.to_string()),
                            unit.into(),
                            sample.source.clone(),
                            sample
                                .unavailable_reason
                                .as_ref()
                                .map_or_else(String::new, ToString::to_string),
                        ],
                    )?;
                }
            }
            writer.flush()
        })
    }

    pub fn resource_series(
        &mut self,
        resource_label: String,
        series_list: Vec<(String, TimeSeries, String)>,
        range: HistoryRange,
        now: Instant,
    ) -> io::Result<()> {
        let label = resource_label.to_lowercase().replace([' ', ':', '/'], "-");
        self.start(&label, "csv", move |path, cancel| {
            fs::create_dir_all(path.parent().unwrap())?;
            let mut writer =
                BufWriter::new(OpenOptions::new().create_new(true).write(true).open(path)?);
            csv_line(
                &mut writer,
                &[
                    "resource".into(),
                    "series_name".into(),
                    "aggregation_interval".into(),
                    "timestamp_ms".into(),
                    "value".into(),
                    "minimum".into(),
                    "maximum".into(),
                    "unit".into(),
                    "source".into(),
                    "observation_count".into(),
                    "unavailable_reason".into(),
                ],
            )?;
            let cutoff = now.checked_sub(range.duration()).unwrap_or(now);
            for (name, series, unit) in &series_list {
                if cancel.load(Ordering::Acquire) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "export cancelled",
                    ));
                }
                for sample in &series.samples {
                    if sample.time < cutoff || sample.time > now {
                        continue;
                    }
                    csv_line(
                        &mut writer,
                        &[
                            resource_label.clone(),
                            name.clone(),
                            "1s".into(),
                            sample.timestamp_ms.to_string(),
                            sample.value.map_or_else(String::new, |v| v.to_string()),
                            sample.minimum.map_or_else(String::new, |v| v.to_string()),
                            sample.maximum.map_or_else(String::new, |v| v.to_string()),
                            unit.clone(),
                            sample.source.clone(),
                            sample.count.to_string(),
                            sample
                                .unavailable_reason
                                .as_ref()
                                .map_or_else(String::new, ToString::to_string),
                        ],
                    )?;
                }
            }
            writer.flush()
        })
    }

    pub fn benchmark(
        &mut self,
        state: BenchmarkState,
        topology: Option<Topology>,
    ) -> io::Result<()> {
        self.start("benchmark", "json", move |path, cancel| {
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "export cancelled",
                ));
            }
            fs::create_dir_all(path.parent().unwrap())?;
            let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
            let document = serde_json::json!({
                "format_version": 1,
                "application_version": crate::state::BUILD_INFO.version,
                "source_commit": crate::state::BUILD_INFO.git_commit,
                "target": crate::state::BUILD_INFO.target,
                "captured_at_ms": planora_telemetry::unix_ms(),
                "topology": topology,
                "benchmark": state,
            });
            serde_json::to_writer_pretty(&mut file, &document).map_err(io::Error::other)?;
            file.sync_all()
        })
    }

    pub fn diagnostics(&mut self, entries: Vec<DiagnosticEntry>) -> io::Result<()> {
        self.start("diagnostics", "json", move |path, cancel| {
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "export cancelled",
                ));
            }
            fs::create_dir_all(path.parent().unwrap())?;
            let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
            let redacted: Vec<_> = entries
                .into_iter()
                .map(|mut entry| {
                    entry.message =
                        "Redacted; inspect the in-app diagnostics for local details".into();
                    entry
                })
                .collect();
            serde_json::to_writer_pretty(&mut file, &redacted).map_err(io::Error::other)?;
            file.sync_all()
        })
    }

    pub fn sensors(&mut self, sensors: Vec<SensorReading>) -> io::Result<()> {
        self.start("sensors", "csv", move |path, cancel| {
            fs::create_dir_all(path.parent().unwrap())?;
            let mut writer =
                BufWriter::new(OpenOptions::new().create_new(true).write(true).open(path)?);
            csv_line(
                &mut writer,
                &[
                    "device_id".into(),
                    "device_name".into(),
                    "scope".into(),
                    "sensor_id".into(),
                    "sensor_name".into(),
                    "sensor_type".into(),
                    "value".into(),
                    "unit".into(),
                    "source".into(),
                    "sampled_at_ms".into(),
                    "unavailable_reason".into(),
                ],
            )?;
            for s in &sensors {
                if cancel.load(Ordering::Acquire) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "export cancelled",
                    ));
                }
                csv_line(
                    &mut writer,
                    &[
                        s.device_id.clone(),
                        s.device_name.clone(),
                        s.scope.clone(),
                        s.id.clone(),
                        s.name.clone(),
                        format!("{:?}", s.sensor_type),
                        s.value.value.map_or_else(String::new, |v| v.to_string()),
                        s.value.unit.clone(),
                        s.provider.clone(),
                        s.value.sampled_at_ms.to_string(),
                        s.value
                            .unavailable_reason
                            .as_ref()
                            .map_or_else(String::new, ToString::to_string),
                    ],
                )?;
            }
            writer.flush()
        })
    }

    pub fn snapshot_json(
        &mut self,
        frame: Arc<TelemetryFrame>,
        sensors: Vec<SensorReading>,
    ) -> io::Result<()> {
        self.start("snapshot", "json", move |path, cancel| {
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "export cancelled",
                ));
            }
            fs::create_dir_all(path.parent().unwrap())?;
            let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
            let document = serde_json::json!({
                "format_version": 1,
                "application_version": crate::state::BUILD_INFO.version,
                "source_commit": crate::state::BUILD_INFO.git_commit,
                "target": crate::state::BUILD_INFO.target,
                "captured_at_ms": planora_telemetry::unix_ms(),
                "frame": &*frame,
                "sensors": sensors,
            });
            serde_json::to_writer_pretty(&mut file, &document).map_err(io::Error::other)?;
            file.sync_all()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_cells_cannot_execute_spreadsheet_formulas() {
        assert_eq!(csv_cell("=SUM(1,2)"), "\"'=SUM(1,2)\"");
        assert_eq!(csv_cell(" +cmd"), "\"' +cmd\"");
        assert_eq!(csv_cell("\t=cmd"), "\"'\t=cmd\"");
        assert_eq!(csv_cell("\u{feff}=cmd"), "\"'\u{feff}=cmd\"");
        assert_eq!(csv_cell("a\"b"), "\"a\"\"b\"");
    }
}
