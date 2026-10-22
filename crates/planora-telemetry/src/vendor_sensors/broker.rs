//! Child process supervisor for the .NET LibreHardwareMonitor sensor broker.
use planora_core::{
    AvailabilityReason, Capability, CapabilityState, Metric, SensorReading, SensorType,
};
use serde::Deserialize;
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct BrokerReadingRaw {
    id: String,
    device_id: String,
    device_name: String,
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
    #[serde(default)]
    sensors: Vec<BrokerReadingRaw>,
}

#[derive(Deserialize)]
struct BrokerPingResponse {
    status: String,
    #[allow(dead_code)]
    version: Option<String>,
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
}

impl Default for SensorBrokerSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorBrokerSupervisor {
    pub fn new() -> Self {
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
        };
        supervisor.ensure_started();
        supervisor
    }

    pub fn locate_broker_binary() -> Option<PathBuf> {
        if let Ok(current_exe) = std::env::current_exe()
            && let Some(parent) = current_exe.parent()
        {
            let candidate = parent.join("planora-sensor-broker.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        let installed = PathBuf::from(r"C:\Program Files\Planora\planora-sensor-broker.exe");
        if installed.is_file() {
            return Some(installed);
        }
        if let Ok(cwd) = std::env::current_dir() {
            let release_target = cwd
                .join("target")
                .join("release")
                .join("planora-sensor-broker.exe");
            if release_target.is_file() {
                return Some(release_target);
            }
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
                    let mut line = String::new();
                    while reader.read_line(&mut line).is_ok() && !line.is_empty() {
                        let trimmed = line.trim().to_string();
                        line.clear();
                        if !trimmed.is_empty() && tx.try_send(trimmed).is_err() {
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

    pub fn poll_sensors(&mut self) -> Vec<SensorReading> {
        if self
            .last_poll
            .is_some_and(|t| t.elapsed() < Duration::from_millis(900))
        {
            return self.last_readings.clone();
        }

        self.ensure_started();
        let Some(stdin) = &mut self.stdin else {
            return self.last_readings.clone();
        };

        if writeln!(stdin, "{{\"command\":\"sample\"}}").is_err() || stdin.flush().is_err() {
            self.handle_failure();
            return self.last_readings.clone();
        }

        let Some(rx) = &self.rx else {
            return self.last_readings.clone();
        };

        match rx.recv_timeout(Duration::from_millis(2000)) {
            Ok(line) => {
                if let Ok(resp) = serde_json::from_str::<BrokerSampleResponse>(&line)
                    && resp.status == "ok"
                {
                    let mut converted = Vec::with_capacity(resp.sensors.len());
                    for r in resp.sensors {
                        let st = match r.sensor_type.as_str() {
                            "Power" => {
                                if r.scope.eq_ignore_ascii_case("Package")
                                    || r.name.to_lowercase().contains("package")
                                {
                                    SensorType::CpuPackagePowerWatts
                                } else if r.scope.eq_ignore_ascii_case("Board") {
                                    SensorType::GpuBoardPowerWatts
                                } else {
                                    SensorType::GpuChipPowerWatts
                                }
                            }
                            "Temperature" => {
                                let dev_lower = r.device_id.to_lowercase();
                                if r.scope.eq_ignore_ascii_case("Drive")
                                    || dev_lower.contains("nvme")
                                    || dev_lower.contains("storage")
                                    || dev_lower.contains("hdd")
                                    || dev_lower.contains("ssd")
                                {
                                    SensorType::StorageTemperatureCelsius
                                } else if r.scope.eq_ignore_ascii_case("GPU")
                                    || dev_lower.contains("gpu")
                                {
                                    SensorType::GpuTemperatureCelsius
                                } else {
                                    SensorType::CpuTemperatureCelsius
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

                        let metric = if let Some(val) = r.value {
                            Metric::available(val, r.unit, &r.provider, r.sampled_at_ms)
                        } else {
                            Metric::unavailable(
                                r.unit,
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

        self.last_readings.clone()
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
        {
            Capability {
                name: "Advanced sensors (LibreHardwareMonitor & PawnIO)".into(),
                scope: "Platform and storage sensors".into(),
                provider: "LibreHardwareMonitor".into(),
                state: CapabilityState::Available,
                last_probed_at_ms: now,
                reason: None,
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
