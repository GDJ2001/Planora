//! Child process supervisor for the .NET LibreHardwareMonitor sensor broker.
use planora_core::{
    AvailabilityReason, Capability, CapabilityState, Metric, SensorReading, SensorType,
};
use serde::Deserialize;
use std::{
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const MAX_BROKER_LINE: usize = 1024 * 1024;
const MAX_BROKER_SENSORS: usize = 4096;

fn matches_build(version: &str, commit: &str, expected_commit: Option<&str>) -> bool {
    version == env!("CARGO_PKG_VERSION")
        && expected_commit.is_none_or(|expected| commit == expected)
}

fn read_bounded_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }
        let length = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if bytes.len().saturating_add(length) > MAX_BROKER_LINE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "sensor broker response exceeds 1 MiB",
            ));
        }
        bytes.extend_from_slice(&available[..length]);
        reader.consume(length);
        if bytes.last() == Some(&b'\n') {
            break;
        }
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[derive(Deserialize)]
struct BrokerReadingRaw {
    id: String,
    device_id: String,
    device_name: String,
    hardware_type: String,
    name: String,
    #[serde(rename = "type")]
    sensor_type: String,
    scope: String,
    provider: String,
    unit: String,
    value: Option<f32>,
    sampled_at_ms: u64,
}

#[derive(Deserialize)]
struct BrokerSampleResponse {
    status: String,
    version: String,
    build_commit: String,
    #[serde(default)]
    sensors: Vec<BrokerReadingRaw>,
}

#[derive(Deserialize)]
struct BrokerPingResponse {
    status: String,
    version: Option<String>,
    build_commit: Option<String>,
    #[allow(dead_code)]
    lhm_version: Option<String>,
}

pub struct SensorBrokerSupervisor {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    rx: Option<mpsc::Receiver<String>>,
    reader: Option<std::thread::JoinHandle<()>>,
    job_handle: usize,
    last_poll: Option<Instant>,
    last_readings: Vec<SensorReading>,
    consecutive_failures: u32,
    next_restart_at: Option<Instant>,
    broker_exe: Option<PathBuf>,
    expected_commit: Option<String>,
}

impl Default for SensorBrokerSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorBrokerSupervisor {
    pub fn new() -> Self {
        Self::new_with_expected_commit(None)
    }

    pub fn new_with_expected_commit(expected_commit: Option<&str>) -> Self {
        let broker_exe = Self::locate_broker_binary();
        let mut supervisor = Self {
            child: None,
            stdin: None,
            rx: None,
            reader: None,
            job_handle: 0,
            last_poll: None,
            last_readings: Vec::new(),
            consecutive_failures: 0,
            next_restart_at: None,
            broker_exe,
            expected_commit: expected_commit.map(str::to_owned),
        };
        supervisor.ensure_started();
        supervisor
    }

    pub fn locate_broker_binary() -> Option<PathBuf> {
        // The helper may run as LocalSystem. Never execute a broker next to a
        // development binary or from a caller-controlled working directory.
        let installed = PathBuf::from(r"C:\Program Files\Planora\planora-sensor-broker.exe");
        if installed.is_file() {
            return Some(installed);
        }
        None
    }

    fn setup_job_object(&mut self, child_handle: usize) -> bool {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            };

            if self.job_handle != 0 {
                windows_sys::Win32::Foundation::CloseHandle(self.job_handle as _);
                self.job_handle = 0;
            }

            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if !job.is_null() {
                let mut info = std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let configured = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) != 0;
                let assigned = configured && AssignProcessToJobObject(job, child_handle as _) != 0;
                if assigned {
                    self.job_handle = job as usize;
                    return true;
                }
                windows_sys::Win32::Foundation::CloseHandle(job);
            }
            false
        }
        #[cfg(not(windows))]
        {
            let _ = child_handle;
            true
        }
    }

    fn ensure_started(&mut self) {
        if self.child.is_some() {
            return;
        }
        if self.broker_exe.is_none() {
            self.broker_exe = Self::locate_broker_binary();
        }
        let Some(broker_exe) = &self.broker_exe else {
            return;
        };
        if let Some(restart_at) = self.next_restart_at
            && Instant::now() < restart_at
        {
            return;
        }

        let mut cmd = Command::new(broker_exe);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        let Ok(mut child) = cmd.spawn() else {
            self.consecutive_failures += 1;
            let backoff_secs = (1 << self.consecutive_failures.min(4)).min(16);
            self.next_restart_at = Some(Instant::now() + Duration::from_secs(backoff_secs));
            return;
        };

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            if !self.setup_job_object(child.as_raw_handle() as usize) {
                let _ = child.kill();
                let _ = child.wait();
                self.consecutive_failures += 1;
                self.next_restart_at = Some(Instant::now() + Duration::from_secs(2));
                return;
            }
        }

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();

        if let (Some(stdin), Some(stdout)) = (stdin, stdout) {
            let (tx, rx) = mpsc::sync_channel(4);
            let reader = std::thread::Builder::new()
                .name("sensor-broker-reader".into())
                .spawn(move || {
                    let mut reader = BufReader::new(stdout);
                    while let Ok(Some(line)) = read_bounded_line(&mut reader) {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() && tx.try_send(trimmed.to_owned()).is_err() {
                            break;
                        }
                    }
                })
                .ok();

            self.child = Some(child);
            self.stdin = Some(stdin);
            self.rx = Some(rx);
            self.reader = reader;
            self.next_restart_at = None;
            self.consecutive_failures = 0;
        }
    }

    fn cached_readings_with_staleness(&self) -> Vec<SensorReading> {
        let mut readings = self.last_readings.clone();
        if let Some(last_poll) = self.last_poll {
            let elapsed_ms = last_poll.elapsed().as_millis() as u64;
            if elapsed_ms > 5000 {
                let now_ms = crate::unix_ms();
                for r in &mut readings {
                    r.value = Metric::unavailable(
                        &r.value.unit,
                        &r.value.source,
                        now_ms,
                        AvailabilityReason::Stale(elapsed_ms),
                    );
                }
            }
        }
        readings
    }

    pub fn poll_sensors(&mut self) -> Vec<SensorReading> {
        if self
            .last_poll
            .is_some_and(|t| t.elapsed() < Duration::from_millis(900))
        {
            return self.last_readings.clone();
        }

        self.ensure_started();
        let Some(stdin) = &mut self.stdin else {
            return self.cached_readings_with_staleness();
        };

        if writeln!(stdin, "{{\"command\":\"sample\"}}").is_err() || stdin.flush().is_err() {
            self.handle_failure();
            return self.cached_readings_with_staleness();
        }

        let Some(rx) = &self.rx else {
            return self.cached_readings_with_staleness();
        };

        match rx.recv_timeout(Duration::from_millis(2000)) {
            Ok(line) => {
                if let Ok(resp) = serde_json::from_str::<BrokerSampleResponse>(&line)
                    && resp.status == "ok"
                    && matches_build(
                        &resp.version,
                        &resp.build_commit,
                        self.expected_commit.as_deref(),
                    )
                {
                    let mut converted = Vec::with_capacity(resp.sensors.len());
                    if resp.sensors.len() > MAX_BROKER_SENSORS {
                        self.handle_failure();
                        return self.cached_readings_with_staleness();
                    }
                    for r in resp.sensors {
                        if r.id.len() > 256
                            || r.device_id.len() > 256
                            || r.device_name.len() > 256
                            || r.name.len() > 256
                            || r.hardware_type.len() > 32
                            || r.scope.len() > 64
                            || r.provider.len() > 64
                            || r.unit.len() > 32
                            || r.sampled_at_ms == 0
                            || r.value.is_some_and(|value| !value.is_finite())
                        {
                            continue;
                        }
                        let is_cpu = r.hardware_type == "Cpu";
                        let is_gpu = r.hardware_type.starts_with("Gpu");
                        let is_storage = r.hardware_type == "Storage";
                        let st = match r.sensor_type.as_str() {
                            "Power" if is_cpu => SensorType::CpuPackagePowerWatts,
                            "Power"
                                if is_gpu && (r.scope == "Board" || r.name.contains("Board")) =>
                            {
                                SensorType::GpuBoardPowerWatts
                            }
                            "Power" if is_gpu => SensorType::GpuChipPowerWatts,
                            "Power" => continue,
                            "Temperature" => {
                                if is_storage {
                                    SensorType::StorageTemperatureCelsius
                                } else if is_gpu {
                                    SensorType::GpuTemperatureCelsius
                                } else if is_cpu {
                                    SensorType::CpuTemperatureCelsius
                                } else {
                                    SensorType::ThermalZoneCelsius
                                }
                            }
                            "Fan" => SensorType::FanRpm,
                            "Clock" => SensorType::ClockMhz,
                            "Voltage" => SensorType::VoltageVolts,
                            "Current" => SensorType::CurrentAmperes,
                            "Load" => SensorType::LoadPercent,
                            "Energy" => SensorType::EnergyJoules,
                            _ => continue,
                        };

                        let canonical_unit = planora_core::canonicalize_unit(&r.unit);
                        let metric = if let Some(val) = r.value {
                            if st.is_plausible_value(val)
                                && planora_core::canonicalize_unit(canonical_unit)
                                    == planora_core::canonicalize_unit(st.expected_unit())
                            {
                                Metric::available(val, canonical_unit, &r.provider, r.sampled_at_ms)
                            } else {
                                Metric::unavailable(
                                    canonical_unit,
                                    &r.provider,
                                    r.sampled_at_ms,
                                    AvailabilityReason::TemporarilyFailed(
                                        "Sensor reading failed physical bounds validation".into(),
                                    ),
                                )
                            }
                        } else {
                            Metric::unavailable(
                                canonical_unit,
                                &r.provider,
                                r.sampled_at_ms,
                                AvailabilityReason::Unsupported(
                                    "Sensor returned no reading (PawnIO driver or elevated privileges may be required)".into(),
                                ),
                            )
                        };

                        converted.push(SensorReading {
                            id: r.id,
                            device_id: r.device_id,
                            device_name: r.device_name,
                            name: r.name,
                            sensor_type: st,
                            scope: r.scope,
                            provider: r.provider,
                            value: metric,
                        });
                    }

                    self.last_readings = converted.clone();
                    self.last_poll = Some(Instant::now());
                    self.consecutive_failures = 0;
                    return converted;
                }
                self.handle_failure();
            }
            Err(_) => {
                self.handle_failure();
            }
        }

        self.cached_readings_with_staleness()
    }

    pub fn probe_capability(&mut self) -> Capability {
        let now = crate::unix_ms();
        if self.broker_exe.is_none() {
            self.broker_exe = Self::locate_broker_binary();
        }
        let Some(_broker_exe) = &self.broker_exe else {
            return Capability {
                name: "Advanced sensors (LibreHardwareMonitor & PawnIO)".into(),
                scope: "Platform and storage sensors".into(),
                provider: "LibreHardwareMonitor".into(),
                state: CapabilityState::MissingRuntime,
                last_probed_at_ms: now,
                reason: Some("planora-sensor-broker.exe not found. Re-run installer and select Advanced sensors.".into()),
                recovery_action: Some("Run Planora setup installer and enable Advanced sensors feature".into()),
            };
        };

        self.ensure_started();
        let Some(stdin) = &mut self.stdin else {
            return Capability {
                name: "Advanced sensors (LibreHardwareMonitor & PawnIO)".into(),
                scope: "Platform and storage sensors".into(),
                provider: "LibreHardwareMonitor".into(),
                state: CapabilityState::TemporarilyFailed,
                last_probed_at_ms: now,
                reason: Some("Sensor broker child process failed to start".into()),
                recovery_action: Some("Restart Planora or retry helper service".into()),
            };
        };

        if writeln!(stdin, "{{\"command\":\"ping\"}}").is_ok()
            && stdin.flush().is_ok()
            && let Some(rx) = &self.rx
            && let Ok(line) = rx.recv_timeout(Duration::from_millis(2000))
            && let Ok(resp) = serde_json::from_str::<BrokerPingResponse>(&line)
            && resp.status == "ok"
            && resp
                .version
                .as_deref()
                .zip(resp.build_commit.as_deref())
                .is_some_and(|(version, commit)| {
                    matches_build(version, commit, self.expected_commit.as_deref())
                })
        {
            let has_reading = self
                .last_readings
                .iter()
                .any(|reading| reading.value.value.is_some());
            Capability {
                name: "Advanced sensors (LibreHardwareMonitor & PawnIO)".into(),
                scope: "Platform and storage sensors".into(),
                provider: "LibreHardwareMonitor".into(),
                state: if has_reading {
                    CapabilityState::Available
                } else if self.last_poll.is_some() {
                    CapabilityState::Unsupported
                } else {
                    CapabilityState::WarmingUp
                },
                last_probed_at_ms: now,
                reason: (!has_reading).then_some(if self.last_poll.is_some() {
                    "Broker responded, but supplied no available sensor readings".into()
                } else {
                    "Waiting for the first sensor sample".into()
                }),
                recovery_action: None,
            }
        } else {
            Capability {
                name: "Advanced sensors (LibreHardwareMonitor & PawnIO)".into(),
                scope: "Platform and storage sensors".into(),
                provider: "LibreHardwareMonitor".into(),
                state: CapabilityState::TemporarilyFailed,
                last_probed_at_ms: now,
                reason: Some("Sensor broker did not respond within 2-second deadline".into()),
                recovery_action: Some(
                    "Check background process execution and restart Planora".into(),
                ),
            }
        }
    }

    fn handle_failure(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(windows)]
        if self.job_handle != 0 {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.job_handle as _);
            }
            self.job_handle = 0;
        }
        self.stdin = None;
        self.rx = None;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.consecutive_failures += 1;
        let backoff_secs = (1 << self.consecutive_failures.min(4)).min(16);
        self.next_restart_at = Some(Instant::now() + Duration::from_secs(backoff_secs));
    }
}

impl Drop for SensorBrokerSupervisor {
    fn drop(&mut self) {
        if let Some(stdin) = &mut self.stdin {
            let _ = writeln!(stdin, "{{\"command\":\"exit\"}}");
            let _ = stdin.flush();
        }
        if let Some(mut child) = self.child.take() {
            let start = Instant::now();
            loop {
                if child.try_wait().ok().flatten().is_some()
                    || start.elapsed() > Duration::from_millis(50)
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(windows)]
        if self.job_handle != 0 {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.job_handle as _);
            }
            self.job_handle = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_frames_are_bounded_and_preserve_lines() {
        let mut reader = BufReader::new(b"{\"status\":\"ok\"}\nsecond\n".as_slice());
        assert_eq!(
            read_bounded_line(&mut reader).unwrap().unwrap(),
            "{\"status\":\"ok\"}\n"
        );
        assert_eq!(read_bounded_line(&mut reader).unwrap().unwrap(), "second\n");
        assert!(read_bounded_line(&mut reader).unwrap().is_none());

        let oversized = vec![b'a'; MAX_BROKER_LINE + 1];
        let mut reader = BufReader::new(oversized.as_slice());
        assert_eq!(
            read_bounded_line(&mut reader).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn helper_rejects_stale_or_mismatched_broker_builds() {
        assert!(matches_build(
            env!("CARGO_PKG_VERSION"),
            "abc123",
            Some("abc123")
        ));
        assert!(!matches_build(
            env!("CARGO_PKG_VERSION"),
            "old",
            Some("abc123")
        ));
        assert!(!matches_build("0.0.0", "abc123", Some("abc123")));
    }

    #[test]
    fn broker_cached_readings_expire_to_stale_during_restart_backoff() {
        let mut sup = SensorBrokerSupervisor::new();
        if let Some(mut child) = sup.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        sup.stdin = None;
        sup.rx = None;
        sup.reader = None;
        sup.next_restart_at = Some(Instant::now() + Duration::from_secs(60));
        sup.last_poll = Some(Instant::now() - Duration::from_secs(6));
        sup.last_readings = vec![SensorReading {
            id: "cpu/0/temp".into(),
            device_id: "cpu/0".into(),
            device_name: "CPU".into(),
            name: "CPU Package".into(),
            sensor_type: SensorType::CpuTemperatureCelsius,
            scope: "Package".into(),
            provider: "LibreHardwareMonitor".into(),
            value: Metric::available(62.0, "°C", "LibreHardwareMonitor", 1000),
        }];

        let polled = sup.poll_sensors();
        assert_eq!(polled.len(), 1);
        assert!(polled[0].value.value.is_none());
        assert!(matches!(
            polled[0].value.unavailable_reason,
            Some(AvailabilityReason::Stale(ms)) if ms > 5000
        ));
    }
}
