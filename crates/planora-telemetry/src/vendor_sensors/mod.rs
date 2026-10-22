//! Isolated vendor sensor providers (Intel IGCL, NVIDIA NVML, AMD ADLX)
//! executed in an unelevated child worker process.

pub mod amd;
pub mod broker;
pub mod intel;
pub mod nvidia;
pub mod worker;

pub use worker::{WorkerCommand, WorkerResponse, run_worker};

#[cfg(windows)]
pub fn trusted_system_dll(name: &str) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    if name.is_empty() || name.contains(['/', '\\', ':']) {
        return None;
    }
    unsafe extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = [0u16; 32768];
    let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len == 0 || len >= buffer.len() {
        return None;
    }
    Some(std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])).join(name))
}

#[cfg(all(test, windows))]
mod path_tests {
    #[test]
    fn runtime_paths_cannot_escape_system_directory() {
        assert!(super::trusted_system_dll("ControlLib.dll").is_some());
        assert!(super::trusted_system_dll("..\\malicious.dll").is_none());
        assert!(super::trusted_system_dll("C:\\temp\\nvml.dll").is_none());
    }
}

#[cfg(test)]
mod framing_tests {
    #[test]
    fn sensor_messages_are_bounded_before_allocation() {
        let mut valid = std::io::Cursor::new(b"ok\n".to_vec());
        assert_eq!(
            super::read_line_bounded(&mut valid, 4).unwrap().as_deref(),
            Some("ok")
        );
        let mut oversized = std::io::Cursor::new(b"abcdef\n".to_vec());
        assert_eq!(
            super::read_line_bounded(&mut oversized, 4)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}

use planora_core::{Capability, CapabilityState, SensorReading};
use std::{
    io::{self, BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

fn read_line_bounded<R: BufRead>(reader: &mut R, limit: usize) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unterminated sensor message",
                ))
            };
        }
        let newline = buf.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(buf.len());
        if line.len().saturating_add(take) > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "sensor message size limit exceeded",
            ));
        }
        line.extend_from_slice(&buf[..take]);
        reader.consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            return String::from_utf8(line)
                .map(Some)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
        }
    }
}

#[cfg(windows)]
unsafe extern "system" {
    fn CreateJobObjectW(
        lpJobAttributes: *mut std::ffi::c_void,
        lpName: *const u16,
    ) -> *mut std::ffi::c_void;
}

pub struct VendorSensorSupervisor {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    rx: Option<mpsc::Receiver<String>>,
    reader: Option<std::thread::JoinHandle<()>>,
    job_handle: usize,
    last_poll: Option<Instant>,
    last_readings: Vec<SensorReading>,
    consecutive_failures: u32,
    next_restart_at: Option<Instant>,
    build_id: String,
    broker: Option<broker::SensorBrokerSupervisor>,
}

impl VendorSensorSupervisor {
    pub fn new(build_id: impl Into<String>) -> Self {
        let mut sup = Self {
            child: None,
            stdin: None,
            rx: None,
            reader: None,
            job_handle: 0,
            last_poll: None,
            last_readings: Vec::new(),
            consecutive_failures: 0,
            next_restart_at: None,
            build_id: build_id.into(),
            // The advanced broker is owned by the authorized helper. This
            // unelevated worker only loads vendor user-mode sensor libraries.
            broker: None,
        };
        sup.ensure_worker_started();
        sup
    }

    fn setup_job_object(&mut self, child_handle: usize) -> bool {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
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

    fn ensure_worker_started(&mut self) {
        if self.child.is_some() {
            return;
        }
        if let Some(restart_at) = self.next_restart_at
            && Instant::now() < restart_at
        {
            return;
        }

        let Ok(current_exe) = std::env::current_exe() else {
            return;
        };
        let mut cmd = Command::new(current_exe);
        cmd.arg("--sensor-worker");
        cmd.arg("--build-id");
        cmd.arg(&self.build_id);
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
                .name("sensor-worker-reader".into())
                .spawn(move || {
                    let mut reader = BufReader::new(stdout);
                    while let Ok(Some(line)) = read_line_bounded(&mut reader, 1024 * 1024) {
                        if tx.try_send(line).is_err() {
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
        let broker_readings = self
            .broker
            .as_mut()
            .map(|b| b.poll_sensors())
            .unwrap_or_default();

        if self
            .last_poll
            .is_some_and(|t| t.elapsed() < Duration::from_millis(950))
        {
            let mut combined = self.last_readings.clone();
            combined.extend(broker_readings);
            return combined;
        }

        self.ensure_worker_started();

        let req = serde_json::to_string(&WorkerCommand::Poll).unwrap();
        let success = self.send_request(&req);

        if !success {
            self.handle_failure();
            let mut combined = self.last_readings.clone();
            combined.extend(broker_readings);
            return combined;
        }

        let Some(rx) = &self.rx else {
            let mut combined = self.last_readings.clone();
            combined.extend(broker_readings);
            return combined;
        };

        // 2-second response deadline
        match rx.recv_timeout(Duration::from_millis(2000)) {
            Ok(line) => {
                if let Ok(WorkerResponse::Samples { sensors }) = serde_json::from_str(&line) {
                    self.last_readings = sensors.clone();
                    self.last_poll = Some(Instant::now());
                    self.consecutive_failures = 0;
                    let mut combined = sensors;
                    combined.extend(broker_readings);
                    return combined;
                }
                self.handle_failure();
            }
            Err(_) => {
                self.handle_failure();
            }
        }

        let mut combined = self.last_readings.clone();
        combined.extend(broker_readings);
        combined
    }

    pub fn probe_capabilities(&mut self) -> Vec<Capability> {
        let mut caps = Vec::new();
        if let Some(broker) = &mut self.broker {
            caps.push(broker.probe_capability());
        }

        self.ensure_worker_started();
        let req = serde_json::to_string(&WorkerCommand::Probe).unwrap();
        if self.send_request(&req)
            && let Some(rx) = &self.rx
            && let Ok(line) = rx.recv_timeout(Duration::from_millis(2000))
            && let Ok(WorkerResponse::Capabilities { capabilities }) = serde_json::from_str(&line)
        {
            caps.extend(capabilities);
            return caps;
        }

        // Fallback capability reporting
        let now = crate::unix_ms();
        let make = |name: &str, provider: &str, state, reason: &str| Capability {
            name: name.into(),
            scope: "GPU and platform sensors".into(),
            provider: provider.into(),
            state,
            last_probed_at_ms: now,
            reason: Some(reason.into()),
            recovery_action: Some("Restart Planora or retry sensor worker".into()),
        };
        caps.extend(vec![
            make(
                "Intel IGCL telemetry",
                "Intel IGCL (Experimental)",
                CapabilityState::TemporarilyFailed,
                "Sensor worker not responding",
            ),
            make(
                "NVIDIA NVML telemetry",
                "NVIDIA NVML (Experimental)",
                CapabilityState::MissingRuntime,
                "nvml.dll not found in system path",
            ),
            make(
                "AMD ADLX telemetry",
                "AMD ADLX (Experimental)",
                CapabilityState::MissingRuntime,
                "amdadlx64.dll not found in system path",
            ),
        ]);
        caps
    }

    fn send_request(&mut self, data: &str) -> bool {
        if let Some(stdin) = &mut self.stdin
            && writeln!(stdin, "{data}").is_ok()
            && stdin.flush().is_ok()
        {
            return true;
        }
        false
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

    pub fn restart(&mut self) {
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
        self.next_restart_at = None;
        self.consecutive_failures = 0;
        self.ensure_worker_started();
    }
}

impl Drop for VendorSensorSupervisor {
    fn drop(&mut self) {
        if let Some(stdin) = &mut self.stdin {
            let exit_cmd = serde_json::to_string(&WorkerCommand::Exit).unwrap();
            let _ = writeln!(stdin, "{exit_cmd}");
            let _ = stdin.flush();
        }
        if let Some(mut child) = self.child.take() {
            // Give worker 50ms to exit cleanly before terminating
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
        self.stdin = None;
        self.rx = None;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        #[cfg(windows)]
        if self.job_handle != 0 {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.job_handle as _);
            }
        }
    }
}
