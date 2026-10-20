//! Isolated vendor sensor providers (Intel IGCL, NVIDIA NVML, AMD ADLX)
//! executed in an unelevated child worker process.

pub mod amd;
pub mod intel;
pub mod nvidia;
pub mod worker;

pub use worker::{WorkerCommand, WorkerResponse, run_worker};

use planora_core::{Capability, CapabilityState, SensorReading};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

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
    job_handle: usize,
    last_poll: Option<Instant>,
    last_readings: Vec<SensorReading>,
    consecutive_failures: u32,
    next_restart_at: Option<Instant>,
    build_id: String,
}

impl VendorSensorSupervisor {
    pub fn new(build_id: impl Into<String>) -> Self {
        let mut sup = Self {
            child: None,
            stdin: None,
            rx: None,
            job_handle: 0,
            last_poll: None,
            last_readings: Vec::new(),
            consecutive_failures: 0,
            next_restart_at: None,
            build_id: build_id.into(),
        };
        sup.ensure_worker_started();
        sup
    }

    fn setup_job_object(&mut self, child_handle: usize) {
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
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                AssignProcessToJobObject(job, child_handle as _);
                self.job_handle = job as usize;
            }
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

        let Ok(mut child) = cmd.spawn() else {
            self.consecutive_failures += 1;
            let backoff_secs = (1 << self.consecutive_failures.min(4)).min(16);
            self.next_restart_at = Some(Instant::now() + Duration::from_secs(backoff_secs));
            return;
        };

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            self.setup_job_object(child.as_raw_handle() as usize);
        }

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();

        if let (Some(stdin), Some(stdout)) = (stdin, stdout) {
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("sensor-worker-reader".into())
                .spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        match line {
                            Ok(l) => {
                                if tx.send(l).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })
                .ok();

            self.child = Some(child);
            self.stdin = Some(stdin);
            self.rx = Some(rx);
            self.next_restart_at = None;
            self.consecutive_failures = 0;
        }
    }

    pub fn poll_sensors(&mut self) -> Vec<SensorReading> {
        if self
            .last_poll
            .is_some_and(|t| t.elapsed() < Duration::from_millis(950))
        {
            return self.last_readings.clone();
        }

        self.ensure_worker_started();

        let req = serde_json::to_string(&WorkerCommand::Poll).unwrap();
        let success = self.send_request(&req);

        if !success {
            self.handle_failure();
            return self.last_readings.clone();
        }

        let Some(rx) = &self.rx else {
            return self.last_readings.clone();
        };

        // 2-second response deadline
        match rx.recv_timeout(Duration::from_millis(2000)) {
            Ok(line) => {
                if let Ok(WorkerResponse::Samples { sensors }) = serde_json::from_str(&line) {
                    self.last_readings = sensors.clone();
                    self.last_poll = Some(Instant::now());
                    self.consecutive_failures = 0;
                    return sensors;
                }
                self.handle_failure();
            }
            Err(_) => {
                self.handle_failure();
            }
        }

        self.last_readings.clone()
    }

    pub fn probe_capabilities(&mut self) -> Vec<Capability> {
        self.ensure_worker_started();
        let req = serde_json::to_string(&WorkerCommand::Probe).unwrap();
        if self.send_request(&req)
            && let Some(rx) = &self.rx
            && let Ok(line) = rx.recv_timeout(Duration::from_millis(2000))
            && let Ok(WorkerResponse::Capabilities { capabilities }) = serde_json::from_str(&line)
        {
            return capabilities;
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
        vec![
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
        ]
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
        #[cfg(windows)]
        if self.job_handle != 0 {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.job_handle as _);
            }
        }
    }
}
