use interprocess::local_socket::prelude::*;
use planora_bench::{ChaseMode, run_cache_probe, run_core_matrix};
use planora_core::{
    BenchmarkState, Capability, CapabilityState, Command, DiagnosticEntry, ErrorCode,
    PROTOCOL_VERSION, ProcessIdentity, ProfileState, ProfileTarget, Response, ResponseEnvelope,
    SUPPORTED_SAMPLE_INTERVALS_MS, TelemetryFrame,
};
use planora_helper::{
    HANDSHAKE_TIMEOUT, REQUEST_TIMEOUT, caller_id, listener, peer_authorized,
    receive_request_deadline, send_response_deadline,
};
use planora_telemetry::Sampler;
mod profiler;
use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

static STOP: AtomicBool = AtomicBool::new(false);
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);
static CALLER_CONNECTIONS: Mutex<Option<HashMap<u64, usize>>> = Mutex::new(None);

const MAX_TOTAL_CONNECTIONS: usize = 16;
const MAX_PER_CALLER_CONNECTIONS: usize = 4;
const MAX_DIAGNOSTIC_ENTRIES: usize = 256;

struct ConnectionGuard {
    caller_id: u64,
    connection_id: u64,
    sampling: Arc<SamplingCoordinator>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.sampling.remove(self.connection_id);
        ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
        let mut map = CALLER_CONNECTIONS.lock().unwrap();
        if let Some(map) = map.as_mut()
            && let Some(cnt) = map.get_mut(&self.caller_id)
        {
            if *cnt <= 1 {
                map.remove(&self.caller_id);
            } else {
                *cnt -= 1;
            }
        }
    }
}

#[derive(Default)]
struct SamplingCoordinator {
    requests: Mutex<HashMap<u64, u64>>,
}

impl SamplingCoordinator {
    fn set(&self, connection_id: u64, interval_ms: u64) -> Result<u64, ()> {
        if !SUPPORTED_SAMPLE_INTERVALS_MS.contains(&interval_ms) {
            return Err(());
        }
        let mut requests = self.requests.lock().unwrap();
        requests.insert(connection_id, interval_ms);
        Ok(requests.values().copied().min().unwrap_or(1_000))
    }

    fn remove(&self, connection_id: u64) {
        self.requests.lock().unwrap().remove(&connection_id);
    }

    fn effective_ms(&self) -> u64 {
        self.requests
            .lock()
            .unwrap()
            .values()
            .copied()
            .min()
            .unwrap_or(1_000)
    }
}

struct DiagnosticLogger {
    entries: Mutex<VecDeque<DiagnosticEntry>>,
}

impl DiagnosticLogger {
    fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(MAX_DIAGNOSTIC_ENTRIES)),
        }
    }

    fn log(&self, operation_id: u64, code: ErrorCode, message: impl Into<String>) {
        let entry = DiagnosticEntry {
            timestamp_ms: planora_telemetry::unix_ms(),
            operation_id,
            code,
            message: message.into(),
        };
        let mut lock = self.entries.lock().unwrap();
        if lock.len() >= MAX_DIAGNOSTIC_ENTRIES {
            lock.pop_front();
        }
        lock.push_back(entry);
    }

    fn snapshot(&self) -> Vec<DiagnosticEntry> {
        self.entries.lock().unwrap().iter().cloned().collect()
    }
}

struct BenchmarkManager {
    current_job_id: AtomicU64,
    state: Mutex<BenchmarkState>,
    cancel: Mutex<Arc<AtomicBool>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl BenchmarkManager {
    fn new() -> Self {
        Self {
            current_job_id: AtomicU64::new(0),
            state: Mutex::new(BenchmarkState::Idle),
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            worker: Mutex::new(None),
        }
    }

    fn status(&self) -> BenchmarkState {
        self.state.lock().unwrap().clone()
    }

    fn cancel(&self, job_id: u64) -> Response {
        let active = self.current_job_id.load(Ordering::SeqCst);
        let mut state = self.state.lock().unwrap();
        if active == job_id
            && matches!(*state, BenchmarkState::Running { job_id: running_id, .. } if running_id == job_id)
        {
            self.cancel.lock().unwrap().store(true, Ordering::SeqCst);
            *state = BenchmarkState::Cancelling { job_id };
            Response::BenchmarkStatus(state.clone())
        } else {
            Response::Error {
                code: ErrorCode::NotFound,
                message: format!("no active benchmark job {job_id} to cancel"),
            }
        }
    }

    fn stop_with_deadline(&self, timeout: Duration) -> bool {
        self.cancel.lock().unwrap().store(true, Ordering::SeqCst);
        let deadline = Instant::now() + timeout;
        loop {
            let finished = self
                .worker
                .lock()
                .unwrap()
                .as_ref()
                .is_none_or(thread::JoinHandle::is_finished);
            if finished {
                if let Some(handle) = self.worker.lock().unwrap().take() {
                    return handle.join().is_ok();
                }
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

struct ProfileManager {
    current_job_id: AtomicU64,
    state: Mutex<ProfileState>,
    cancel: Mutex<Arc<AtomicBool>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl ProfileManager {
    fn new() -> Self {
        Self {
            current_job_id: AtomicU64::new(0),
            state: Mutex::new(ProfileState::Idle),
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            worker: Mutex::new(None),
        }
    }

    fn status(&self) -> ProfileState {
        self.state.lock().unwrap().clone()
    }

    fn cancel(&self, job_id: u64) -> Response {
        let active = self.current_job_id.load(Ordering::SeqCst);
        let mut state = self.state.lock().unwrap();
        if active == job_id
            && matches!(
                *state,
                ProfileState::Running {
                    job_id: running_id,
                    ..
                } if running_id == job_id
            )
        {
            self.cancel.lock().unwrap().store(true, Ordering::SeqCst);
            *state = ProfileState::Cancelling { job_id };
            Response::ProfileStatus(state.clone())
        } else {
            Response::Error {
                code: ErrorCode::NotFound,
                message: format!("no active profile job {job_id} to cancel"),
            }
        }
    }

    fn stop_with_deadline(&self, timeout: Duration) -> bool {
        self.cancel.lock().unwrap().store(true, Ordering::SeqCst);
        let deadline = Instant::now() + timeout;
        loop {
            let finished = self
                .worker
                .lock()
                .unwrap()
                .as_ref()
                .is_none_or(thread::JoinHandle::is_finished);
            if finished {
                if let Some(handle) = self.worker.lock().unwrap().take() {
                    return handle.join().is_ok();
                }
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

struct State {
    session_id: u64,
    latest_frame: Arc<RwLock<TelemetryFrame>>,
    benchmarks: Arc<BenchmarkManager>,
    profiling: Arc<ProfileManager>,
    diagnostics: Arc<DiagnosticLogger>,
    sampling: Arc<SamplingCoordinator>,
}

// The Linux systemd unit uses an unprivileged service account
// with CAP_PERFMON. Privileged affinity writes remain disabled on Linux.
#[cfg(target_os = "linux")]
fn has_effective_capability(status: &str, capability: u32) -> bool {
    if capability >= u64::BITS {
        return false;
    }
    status
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:"))
        .and_then(|value| u64::from_str_radix(value.trim(), 16).ok())
        .is_some_and(|bits| bits & (1_u64 << capability) != 0)
}

#[cfg(target_os = "linux")]
fn elevated() -> bool {
    const CAP_PERFMON: u32 = 38;
    let is_root = unsafe { libc::geteuid() == 0 };
    is_root
        || std::fs::read_to_string("/proc/self/status")
            .ok()
            .is_some_and(|status| has_effective_capability(&status, CAP_PERFMON))
}

#[cfg(target_os = "macos")]
fn elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(windows)]
fn elevated() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

fn capabilities(frame: &TelemetryFrame) -> Vec<Capability> {
    #[cfg(target_os = "linux")]
    let (gpu, power) = (
        planora_telemetry::linux_amd_gpu_busy_available(),
        planora_telemetry::linux_rapl_available(),
    );
    #[cfg(windows)]
    let (gpu, power) = (
        planora_telemetry::windows_gpu_counter_available(),
        frame.sensors.iter().any(|sensor| {
            sensor.sensor_type == planora_core::SensorType::CpuPackagePowerWatts
                && sensor.value.value.is_some()
        }),
    );
    #[cfg(not(any(target_os = "linux", windows)))]
    let (gpu, power) = (false, false);
    #[cfg(windows)]
    let advanced_present =
        std::path::Path::new(r"C:\Program Files\Planora\planora-sensor-broker.exe").is_file();
    #[cfg(not(windows))]
    let advanced_present = false;

    let now = planora_telemetry::unix_ms();
    let pmu_sources_discovered = profiler::enumerate_pmu_sources().len();
    let pmu_reason = if cfg!(windows) {
        format!(
            "Windows exposes {pmu_sources_discovered} profiling sources, but the sampled-event collector is not implemented"
        )
    } else {
        "PMU sampled profiling requires Windows ETW profiling".to_owned()
    };
    let make = |name: &str,
                scope: &str,
                provider: &str,
                state,
                reason: Option<&str>,
                recovery: Option<&str>| Capability {
        name: name.into(),
        scope: scope.into(),
        provider: provider.into(),
        state,
        last_probed_at_ms: now,
        reason: reason.map(str::to_owned),
        recovery_action: recovery.map(str::to_owned),
    };
    let result = vec![
        make(
            "CPU and process telemetry",
            "System and processes",
            "OS telemetry",
            CapabilityState::Available,
            None,
            None,
        ),
        make(
            "GPU engine utilization",
            "GPU engines",
            "Windows PDH / platform provider",
            if gpu {
                CapabilityState::Available
            } else {
                CapabilityState::Unsupported
            },
            (!gpu).then_some("No supported GPU engine counter was returned"),
            (!gpu).then_some("Refresh capabilities after updating the GPU driver"),
        ),
        make(
            "Package power",
            "CPU package",
            "Platform energy provider",
            if power {
                CapabilityState::Available
            } else {
                CapabilityState::Unsupported
            },
            (!power).then_some(if cfg!(windows) && !advanced_present {
                "Advanced sensors is not installed"
            } else {
                "No supported CPU package energy counter was found"
            }),
            None,
        ),
        make(
            "GPU memory telemetry",
            "GPU adapters",
            "Windows DXGI / PDH",
            if gpu {
                CapabilityState::Available
            } else {
                CapabilityState::Unsupported
            },
            (!gpu).then_some("No supported GPU adapter counters were returned"),
            None,
        ),
        make(
            "PMU sampled profiling",
            "Selected process",
            "Windows ETW profiling",
            if cfg!(windows) {
                CapabilityState::NotImplemented
            } else {
                CapabilityState::Unsupported
            },
            Some(&pmu_reason),
            None,
        ),
        make(
            "Disk I/O profiling",
            "System and processes",
            "Windows ETW profiling",
            if cfg!(windows) {
                CapabilityState::Available
            } else {
                CapabilityState::Unsupported
            },
            (!cfg!(windows)).then_some("Disk I/O profiling requires Windows ETW profiling"),
            None,
        ),
        make(
            "Process affinity editing",
            "Selected process",
            "OS affinity API",
            if cfg!(windows) {
                CapabilityState::Available
            } else {
                CapabilityState::Unsupported
            },
            (!cfg!(windows))
                .then_some("Process-wide affinity editing is not supported on this platform"),
            None,
        ),
    ];
    #[cfg(windows)]
    {
        let mut result = result;
        result.push(make(
            "Advanced sensors",
            "CPU and storage",
            "LibreHardwareMonitor / PawnIO",
            if !advanced_present {
                CapabilityState::MissingRuntime
            } else if frame.sensors.is_empty() {
                CapabilityState::WarmingUp
            } else if frame
                .sensors
                .iter()
                .any(|sensor| sensor.value.value.is_some())
            {
                CapabilityState::Available
            } else {
                CapabilityState::Unsupported
            },
            if !advanced_present {
                Some("Advanced sensors was not selected in Setup")
            } else if frame.sensors.is_empty() {
                Some("Waiting for the first broker sample")
            } else if frame
                .sensors
                .iter()
                .all(|sensor| sensor.value.value.is_none())
            {
                Some("The broker found sensors, but none returned a supported reading")
            } else {
                None
            },
            (!advanced_present).then_some("Run Setup and select Advanced sensors"),
        ));
        for sensor in frame.sensors.iter().take(128) {
            let stale = now.saturating_sub(sensor.value.sampled_at_ms) > 5000;
            result.push(Capability {
                name: format!("{} — {}", sensor.device_name, sensor.name),
                scope: sensor.scope.clone(),
                provider: sensor.provider.clone(),
                state: if stale {
                    CapabilityState::TemporarilyFailed
                } else if sensor.value.value.is_some() {
                    CapabilityState::Available
                } else {
                    CapabilityState::Unsupported
                },
                last_probed_at_ms: sensor.value.sampled_at_ms,
                reason: if stale {
                    Some("Last sensor reading is stale".into())
                } else {
                    sensor
                        .value
                        .unavailable_reason
                        .as_ref()
                        .map(ToString::to_string)
                },
                recovery_action: (sensor.value.value.is_none())
                    .then_some("Check signed PawnIO installation and hardware support".into()),
            });
        }
        result
    }
    #[cfg(not(windows))]
    {
        result
    }
}

#[cfg(windows)]
fn windows_handle(
    process: &ProcessIdentity,
    modify: bool,
) -> Result<windows_sys::Win32::Foundation::HANDLE, String> {
    if !process.valid_for_windows_action() {
        return Err("a nonzero Windows FILETIME process creation identity is required".into());
    }
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_INFORMATION,
        },
    };

    let rights =
        PROCESS_QUERY_LIMITED_INFORMATION | if modify { PROCESS_SET_INFORMATION } else { 0 };
    let handle = unsafe { OpenProcess(rights, 0, process.pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error().to_string());
    }

    let mut created = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let ok =
        unsafe { GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) } != 0;
    let stamp = ((created.dwHighDateTime as u64) << 32) | (created.dwLowDateTime as u64);

    if !ok || stamp != process.started {
        unsafe { CloseHandle(handle) };
        return Err("process identity changed or PID reused".into());
    }
    Ok(handle)
}

#[cfg(windows)]
fn get_affinity(process: &ProcessIdentity) -> Result<Vec<u16>, String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetActiveProcessorGroupCount, GetProcessAffinityMask},
    };
    if unsafe { GetActiveProcessorGroupCount() } > 1 {
        return Err("multi-group affinity is unavailable".into());
    }
    let handle = windows_handle(process, false)?;
    let mut current = 0usize;
    let mut system = 0usize;
    let ok = unsafe { GetProcessAffinityMask(handle, &mut current, &mut system) } != 0;
    unsafe { CloseHandle(handle) };
    if !ok {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok((0..usize::BITS)
        .filter(|i| current & (1usize << i) != 0)
        .map(|i| i as u16)
        .collect())
}

#[cfg(windows)]
fn set_affinity(process: &ProcessIdentity, cpus: &[u16]) -> Result<(Vec<u16>, Vec<u16>), String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            GetActiveProcessorGroupCount, GetProcessAffinityMask, SetProcessAffinityMask,
        },
    };
    if unsafe { GetActiveProcessorGroupCount() } > 1 {
        return Err("multi-group processor affinity is unavailable".into());
    }
    if cpus.is_empty() || cpus.iter().any(|cpu| *cpu as u32 >= usize::BITS) {
        return Err("CPU set is empty or contains out-of-range processors".into());
    }
    let mut unique = cpus.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != cpus.len() {
        return Err("CPU set contains duplicate processors".into());
    }

    // Revalidate identity and apply affinity through the EXACT SAME handle
    let handle = windows_handle(process, true)?;
    let mut old = 0usize;
    let mut system = 0usize;
    if unsafe { GetProcessAffinityMask(handle, &mut old, &mut system) } == 0 {
        let e = io::Error::last_os_error();
        unsafe { CloseHandle(handle) };
        return Err(e.to_string());
    }

    let mask = cpus.iter().fold(0usize, |mask, cpu| mask | (1usize << cpu));
    if mask & !system != 0 {
        unsafe { CloseHandle(handle) };
        return Err(
            "requested CPU set contains processors outside the system affinity mask".into(),
        );
    }

    if unsafe { SetProcessAffinityMask(handle, mask) } == 0 {
        let e = io::Error::last_os_error();
        unsafe { CloseHandle(handle) };
        return Err(e.to_string());
    }

    let mut actual = 0usize;
    let ok = unsafe { GetProcessAffinityMask(handle, &mut actual, &mut system) } != 0;
    unsafe { CloseHandle(handle) };
    if !ok {
        return Err(io::Error::last_os_error().to_string());
    }

    let decode = |bits: usize| {
        (0..usize::BITS)
            .filter(|i| bits & (1usize << i) != 0)
            .map(|i| i as u16)
            .collect()
    };
    Ok((decode(old), decode(actual)))
}

#[cfg(target_os = "linux")]
fn get_affinity(process: &ProcessIdentity) -> Result<Vec<u16>, String> {
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::sched_getaffinity(
            process.pid as i32,
            std::mem::size_of::<libc::cpu_set_t>(),
            &mut set,
        )
    } != 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    if let Ok(started) = planora_telemetry::linux_start_ticks(process.pid)
        && started != process.started
        && process.started != 0
    {
        return Err("process identity changed during query (PID reused)".into());
    }
    Ok((0..libc::CPU_SETSIZE as usize)
        .filter(|i| unsafe { libc::CPU_ISSET(*i, &set) })
        .map(|i| i as u16)
        .collect())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn get_affinity(_process: &ProcessIdentity) -> Result<Vec<u16>, String> {
    Err("affinity query is unavailable on this platform".into())
}

#[cfg(not(windows))]
fn set_affinity(_process: &ProcessIdentity, _cpus: &[u16]) -> Result<(Vec<u16>, Vec<u16>), String> {
    #[cfg(target_os = "linux")]
    {
        Err("Linux affinity mutation is disabled in Planora v1: sched_setaffinity applies per-thread rather than process-wide, and thread identity cannot be guaranteed race-free without full task-tree enumeration.".into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("affinity mutation is unsupported on this platform".into())
    }
}

fn dispatch(command: Command, req_id: u64, connection_id: u64, state: &State) -> Response {
    match command {
        Command::Hello { version } if version == PROTOCOL_VERSION => Response::Hello {
            version,
            session_id: state.session_id,
            helper_authorized: true,
        },
        Command::Hello { version } => {
            state.diagnostics.log(
                req_id,
                ErrorCode::VersionMismatch,
                format!("client version {version} does not match protocol v{PROTOCOL_VERSION}"),
            );
            Response::Error {
                code: ErrorCode::VersionMismatch,
                message: format!(
                    "protocol version mismatch: helper is v{PROTOCOL_VERSION}, client is v{version}"
                ),
            }
        }
        Command::Topology => Response::Topology(planora_topology::discover()),
        Command::SetSamplingInterval { interval_ms } => {
            match state.sampling.set(connection_id, interval_ms) {
                Ok(effective_ms) => Response::SamplingStatus {
                    requested_ms: interval_ms,
                    effective_ms,
                },
                Err(()) => Response::Error {
                    code: ErrorCode::InvalidInput,
                    message: format!("unsupported sampling interval {interval_ms} ms"),
                },
            }
        }
        Command::Snapshot => {
            let frame = state.latest_frame.read().unwrap().clone();
            Response::Snapshot(Box::new(frame))
        }
        Command::Capabilities | Command::RefreshCapabilities => {
            let frame = state.latest_frame.read().unwrap();
            Response::Capabilities(capabilities(&frame))
        }
        Command::RunCacheProbe { job_id, page_mode } => {
            let prof_state = state.profiling.state.lock().unwrap();
            if matches!(
                *prof_state,
                ProfileState::Running { .. } | ProfileState::Cancelling { .. }
            ) {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::Busy,
                    "benchmark start rejected: a profiling session is already running",
                );
                return Response::Error {
                    code: ErrorCode::Busy,
                    message: "a profiling session is already running".into(),
                };
            }
            drop(prof_state);

            let mut bm_state = state.benchmarks.state.lock().unwrap();
            if matches!(
                *bm_state,
                BenchmarkState::Running { .. } | BenchmarkState::Cancelling { .. }
            ) {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::Busy,
                    "benchmark start rejected: another benchmark is already running",
                );
                return Response::Error {
                    code: ErrorCode::Busy,
                    message: "a benchmark is already running".into(),
                };
            }
            let cancel = Arc::new(AtomicBool::new(false));
            *state.benchmarks.cancel.lock().unwrap() = cancel.clone();
            state
                .benchmarks
                .current_job_id
                .store(job_id, Ordering::SeqCst);
            *bm_state = BenchmarkState::Running {
                job_id,
                elapsed_ms: 0,
            };

            let bm = state.benchmarks.clone();
            let diag = state.diagnostics.clone();
            let worker = thread::spawn(move || {
                let _start = Instant::now();
                let mode = if page_mode {
                    ChaseMode::Page
                } else {
                    ChaseMode::Line
                };
                let res = run_cache_probe(mode, &cancel);
                let mut lock = bm.state.lock().unwrap();
                if bm.current_job_id.load(Ordering::SeqCst) != job_id {
                    return;
                }
                match res {
                    Ok(result) => {
                        *lock = BenchmarkState::Completed { job_id, result };
                        diag.log(
                            job_id,
                            ErrorCode::Internal,
                            "cache probe completed successfully",
                        );
                    }
                    Err(err) if err == "cancelled" => {
                        *lock = BenchmarkState::Cancelled { job_id };
                        diag.log(
                            job_id,
                            ErrorCode::Cancelled,
                            "cache probe cancelled by user",
                        );
                    }
                    Err(err) => {
                        *lock = BenchmarkState::Failed {
                            job_id,
                            code: ErrorCode::Internal,
                            message: err.clone(),
                        };
                        diag.log(
                            job_id,
                            ErrorCode::Internal,
                            format!("cache probe failed: {err}"),
                        );
                    }
                }
            });
            *state.benchmarks.worker.lock().unwrap() = Some(worker);

            Response::BenchmarkStatus(BenchmarkState::Running {
                job_id,
                elapsed_ms: 0,
            })
        }
        Command::RunCoreMatrix {
            job_id,
            logical_cpus,
        } => {
            if logical_cpus.len() < 2 || logical_cpus.len() > 8 {
                return Response::Error {
                    code: ErrorCode::InvalidInput,
                    message: "select between 2 and 8 logical CPUs".into(),
                };
            }
            let prof_state = state.profiling.state.lock().unwrap();
            if matches!(
                *prof_state,
                ProfileState::Running { .. } | ProfileState::Cancelling { .. }
            ) {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::Busy,
                    "matrix start rejected: a profiling session is already running",
                );
                return Response::Error {
                    code: ErrorCode::Busy,
                    message: "a profiling session is already running".into(),
                };
            }
            drop(prof_state);

            let mut bm_state = state.benchmarks.state.lock().unwrap();
            if matches!(
                *bm_state,
                BenchmarkState::Running { .. } | BenchmarkState::Cancelling { .. }
            ) {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::Busy,
                    "matrix start rejected: another benchmark is already running",
                );
                return Response::Error {
                    code: ErrorCode::Busy,
                    message: "a benchmark is already running".into(),
                };
            }
            let cancel = Arc::new(AtomicBool::new(false));
            *state.benchmarks.cancel.lock().unwrap() = cancel.clone();
            state
                .benchmarks
                .current_job_id
                .store(job_id, Ordering::SeqCst);
            *bm_state = BenchmarkState::Running {
                job_id,
                elapsed_ms: 0,
            };

            let bm = state.benchmarks.clone();
            let diag = state.diagnostics.clone();
            let worker = thread::spawn(move || {
                let res = run_core_matrix(&logical_cpus, cancel);
                let mut lock = bm.state.lock().unwrap();
                if bm.current_job_id.load(Ordering::SeqCst) != job_id {
                    return;
                }
                match res {
                    Ok(result) => {
                        *lock = BenchmarkState::Completed { job_id, result };
                        diag.log(
                            job_id,
                            ErrorCode::Internal,
                            "core matrix completed successfully",
                        );
                    }
                    Err(err) if err == "cancelled" => {
                        *lock = BenchmarkState::Cancelled { job_id };
                        diag.log(
                            job_id,
                            ErrorCode::Cancelled,
                            "core matrix cancelled by user",
                        );
                    }
                    Err(err) => {
                        *lock = BenchmarkState::Failed {
                            job_id,
                            code: ErrorCode::Internal,
                            message: err.clone(),
                        };
                        diag.log(
                            job_id,
                            ErrorCode::Internal,
                            format!("core matrix failed: {err}"),
                        );
                    }
                }
            });
            *state.benchmarks.worker.lock().unwrap() = Some(worker);

            Response::BenchmarkStatus(BenchmarkState::Running {
                job_id,
                elapsed_ms: 0,
            })
        }
        Command::CancelBenchmark { job_id } => state.benchmarks.cancel(job_id),
        Command::BenchmarkStatus => Response::BenchmarkStatus(state.benchmarks.status()),
        Command::StartProfile { job_id, target } => {
            if matches!(&target, ProfileTarget::PmuSampling { .. }) {
                return Response::Error {
                    code: ErrorCode::Unsupported,
                    message: "PMU sampled profiling is not implemented; source discovery alone cannot produce samples".into(),
                };
            }
            if let ProfileTarget::DiskIo {
                target_process: Some(process),
                ..
            } = &target
            {
                #[cfg(windows)]
                match windows_handle(process, false) {
                    Ok(handle) => unsafe {
                        windows_sys::Win32::Foundation::CloseHandle(handle);
                    },
                    Err(reason) => {
                        return Response::Error {
                            code: ErrorCode::InvalidInput,
                            message: format!("profile target identity is invalid: {reason}"),
                        };
                    }
                }
            }
            let bm_state = state.benchmarks.state.lock().unwrap();
            if matches!(
                *bm_state,
                BenchmarkState::Running { .. } | BenchmarkState::Cancelling { .. }
            ) {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::Busy,
                    "profile start rejected: a benchmark is already running",
                );
                return Response::Error {
                    code: ErrorCode::Busy,
                    message: "a benchmark is already running".into(),
                };
            }
            drop(bm_state);

            let mut prof_state = state.profiling.state.lock().unwrap();
            if matches!(
                *prof_state,
                ProfileState::Running { .. } | ProfileState::Cancelling { .. }
            ) {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::Busy,
                    "profile start rejected: another profiling session is already running",
                );
                return Response::Error {
                    code: ErrorCode::Busy,
                    message: "a profiling session is already running".into(),
                };
            }

            let target_desc = match &target {
                ProfileTarget::DiskIo { target_process, .. } => {
                    if let Some(target) = target_process {
                        format!("Disk I/O profile (PID {})", target.pid)
                    } else {
                        "System-wide Disk I/O profile".to_string()
                    }
                }
                ProfileTarget::PmuSampling {
                    target_process,
                    sources,
                    ..
                } => {
                    if let Some(target) = target_process {
                        format!(
                            "PMU profile (PID {}) with {} sources",
                            target.pid,
                            sources.len()
                        )
                    } else {
                        format!("System PMU profile with {} sources", sources.len())
                    }
                }
            };
            let duration_raw = match &target {
                ProfileTarget::DiskIo {
                    duration_seconds, ..
                } => *duration_seconds,
                ProfileTarget::PmuSampling {
                    duration_seconds,
                    sources,
                    ..
                } => {
                    if sources.len() > 2 {
                        state.diagnostics.log(
                            req_id,
                            ErrorCode::InvalidInput,
                            "profile start rejected: maximum of 2 PMU sources allowed",
                        );
                        return Response::Error {
                            code: ErrorCode::InvalidInput,
                            message: "PMU sampled profiling is limited to a maximum of 2 concurrent sources".into(),
                        };
                    }
                    *duration_seconds
                }
            };
            if duration_raw > 60 {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::InvalidInput,
                    "profile start rejected: duration exceeds 60 seconds",
                );
                return Response::Error {
                    code: ErrorCode::InvalidInput,
                    message: "profile duration cannot exceed 60 seconds".into(),
                };
            }
            if !cfg!(windows) {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::Unsupported,
                    "profile start rejected: profiling requires Windows ETW",
                );
                return Response::Error {
                    code: ErrorCode::Unsupported,
                    message: "profiling requires Windows ETW".into(),
                };
            }
            let duration_seconds = if duration_raw == 0 { 15 } else { duration_raw };

            let cancel = Arc::new(AtomicBool::new(false));
            *state.profiling.cancel.lock().unwrap() = cancel.clone();
            state
                .profiling
                .current_job_id
                .store(job_id, Ordering::SeqCst);
            *prof_state = ProfileState::Running {
                job_id,
                target_desc: target_desc.clone(),
                elapsed_ms: 0,
                duration_seconds,
            };

            let prof = state.profiling.clone();
            let diag = state.diagnostics.clone();
            let target_clone = target.clone();
            let worker = thread::spawn(move || {
                let res = profiler::run_profile(job_id, target_clone, cancel);
                let mut lock = prof.state.lock().unwrap();
                if prof.current_job_id.load(Ordering::SeqCst) != job_id {
                    return;
                }
                match res {
                    Ok(result) => {
                        *lock = ProfileState::Completed { job_id, result };
                        diag.log(
                            job_id,
                            ErrorCode::Internal,
                            "profiling session completed successfully",
                        );
                    }
                    Err((ErrorCode::Cancelled, _err)) => {
                        *lock = ProfileState::Cancelled { job_id };
                        diag.log(
                            job_id,
                            ErrorCode::Cancelled,
                            "profiling session cancelled by user",
                        );
                    }
                    Err((code, err)) => {
                        *lock = ProfileState::Failed {
                            job_id,
                            code,
                            message: err.clone(),
                        };
                        diag.log(job_id, code, format!("profiling session failed: {err}"));
                    }
                }
            });
            *state.profiling.worker.lock().unwrap() = Some(worker);

            Response::ProfileStatus(ProfileState::Running {
                job_id,
                target_desc,
                elapsed_ms: 0,
                duration_seconds,
            })
        }
        Command::CancelProfile { job_id } => state.profiling.cancel(job_id),
        Command::ProfileStatus => Response::ProfileStatus(state.profiling.status()),
        Command::GetAffinity { process } => match get_affinity(&process) {
            Ok(logical_cpus) => Response::Affinity {
                process,
                logical_cpus,
            },
            Err(e) => {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::NotFound,
                    format!("failed to query process affinity: {e}"),
                );
                Response::Error {
                    code: ErrorCode::NotFound,
                    message: e,
                }
            }
        },
        Command::SetAffinity {
            process,
            logical_cpus,
        } => match set_affinity(&process, &logical_cpus) {
            Ok((previous_cpus, logical_cpus)) => {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::Internal,
                    format!("affinity updated for PID {}", process.pid),
                );
                Response::AffinityChanged {
                    process,
                    previous_cpus,
                    logical_cpus,
                }
            }
            Err(e) => {
                state.diagnostics.log(
                    req_id,
                    ErrorCode::PermissionDenied,
                    format!("affinity change failed for PID {}: {e}", process.pid),
                );
                Response::Error {
                    code: if e.contains("creation identity is required") || e.contains("CPU set") {
                        ErrorCode::InvalidInput
                    } else if e.contains("disabled in Planora v1") || e.contains("unsupported") {
                        ErrorCode::Unsupported
                    } else if e.contains("PID reused") {
                        ErrorCode::PidReused
                    } else {
                        ErrorCode::PermissionDenied
                    },
                    message: e,
                }
            }
        },
        Command::DiagnosticLogs => Response::DiagnosticLogs(state.diagnostics.snapshot()),
    }
}

fn run_server(on_ready: impl FnOnce()) -> io::Result<()> {
    if !elevated() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Planora helper requires administrator/root or approved Linux service capabilities",
        ));
    }

    let session_id = planora_telemetry::unix_ms();
    let mut initial_sampler = Sampler::new_with_session(session_id, 1);
    let initial_frame = initial_sampler.sample();
    initial_frame.validate().map_err(io::Error::other)?;

    let latest_frame = Arc::new(RwLock::new(initial_frame));
    let diagnostics = Arc::new(DiagnosticLogger::new());
    let sampling = Arc::new(SamplingCoordinator::default());
    diagnostics.log(
        0,
        ErrorCode::Internal,
        format!("helper session {session_id} initialized"),
    );

    #[cfg(windows)]
    let sensor_data = Arc::new(RwLock::new(Vec::<planora_core::SensorReading>::new()));
    #[cfg(windows)]
    let sensor_worker = {
        let sensor_sink = sensor_data.clone();
        thread::Builder::new()
            .name("planora-advanced-sensors".into())
            .spawn(move || {
                let mut broker = planora_telemetry::vendor_sensors::broker::SensorBrokerSupervisor::new_with_expected_commit(Some(env!("PLANORA_GIT_COMMIT")));
                while !STOP.load(Ordering::Relaxed) {
                    *sensor_sink.write().unwrap() = broker.poll_sensors();
                    for _ in 0..20 {
                        if STOP.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            })?
    };

    // Spawn dedicated sampling worker
    let frame_sink = latest_frame.clone();
    let diag_sink = diagnostics.clone();
    let sampling_for_worker = sampling.clone();
    #[cfg(windows)]
    let sensor_source = sensor_data.clone();
    let sampling_worker = thread::spawn(move || {
        let mut sampler = initial_sampler;
        let mut current_interval = Duration::from_secs(1);
        let mut next_due = Instant::now() + current_interval;
        while !STOP.load(Ordering::Relaxed) {
            let interval = Duration::from_millis(sampling_for_worker.effective_ms());
            if interval != current_interval {
                current_interval = interval;
                next_due = Instant::now() + interval;
            }
            let now = Instant::now();
            if now < next_due {
                thread::sleep((next_due - now).min(Duration::from_millis(25)));
                continue;
            }
            let mut frame = sampler.sample();
            #[cfg(windows)]
            {
                frame.sensors = sensor_source.read().unwrap().clone();
            }
            if let Err(err) = frame.validate() {
                diag_sink.log(
                    0,
                    ErrorCode::Internal,
                    format!("telemetry frame rejected: {err}"),
                );
            } else {
                *frame_sink.write().unwrap() = frame;
            }
            next_due += interval;
            if next_due <= Instant::now() {
                next_due = Instant::now() + interval;
            }
        }
    });

    let state = Arc::new(State {
        session_id,
        latest_frame,
        benchmarks: Arc::new(BenchmarkManager::new()),
        profiling: Arc::new(ProfileManager::new()),
        diagnostics: diagnostics.clone(),
        sampling: sampling.clone(),
    });

    let listener = listener()?;
    on_ready();
    eprintln!(
        "Planora helper protocol v{PROTOCOL_VERSION} ready (session {session_id}, local IPC only)"
    );

    for incoming in listener.incoming() {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        let Ok(mut stream) = incoming else { continue };
        if !peer_authorized(&stream) {
            diagnostics.log(
                0,
                ErrorCode::Unauthorized,
                "connection rejected: unauthorized peer",
            );
            continue;
        }

        let cid = caller_id(&stream);
        let current_conns = ACTIVE_CONNECTIONS.fetch_add(1, Ordering::SeqCst);
        if current_conns >= MAX_TOTAL_CONNECTIONS {
            ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
            diagnostics.log(
                0,
                ErrorCode::Busy,
                "connection rejected: maximum total connection limit reached",
            );
            continue;
        }

        {
            let mut callers = CALLER_CONNECTIONS.lock().unwrap();
            let map = callers.get_or_insert_with(HashMap::new);
            let count = map.entry(cid).or_insert(0);
            if *count >= MAX_PER_CALLER_CONNECTIONS {
                ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
                diagnostics.log(
                    0,
                    ErrorCode::Busy,
                    format!("connection rejected: per-caller limit reached for caller {cid}"),
                );
                continue;
            }
            *count += 1;
        }

        let state = state.clone();
        let connection_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
        thread::spawn(move || {
            let _guard = ConnectionGuard {
                caller_id: cid,
                connection_id,
                sampling: state.sampling.clone(),
            };
            state.diagnostics.log(
                0,
                ErrorCode::Internal,
                format!("connection established from caller {cid}"),
            );
            // 1. Initial handshake with strict HANDSHAKE_TIMEOUT
            let handshake_deadline = Instant::now() + HANDSHAKE_TIMEOUT;
            let initial_req = match receive_request_deadline(&mut stream, handshake_deadline) {
                Ok(r) => r,
                Err(e) => {
                    state.diagnostics.log(
                        0,
                        ErrorCode::Unauthorized,
                        format!("handshake failed or timed out: {e}"),
                    );
                    return;
                }
            };

            let authenticated = matches!(
                initial_req.command,
                Command::Hello { version } if version == PROTOCOL_VERSION
            );
            let resp = if authenticated {
                Response::Hello {
                    version: PROTOCOL_VERSION,
                    session_id: state.session_id,
                    helper_authorized: true,
                }
            } else {
                Response::Error {
                    code: ErrorCode::Unauthorized,
                    message: "protocol handshake required with matching version".into(),
                }
            };

            let env = ResponseEnvelope {
                request_id: initial_req.request_id,
                response: resp,
            };
            let handshake_resp_deadline = Instant::now() + HANDSHAKE_TIMEOUT;
            if send_response_deadline(&mut stream, &env, handshake_resp_deadline).is_err()
                || !authenticated
            {
                return;
            }

            // 2. Active request loop with 30-second idle deadline per request
            const CLIENT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
            while !STOP.load(Ordering::Relaxed) {
                let req_deadline = Instant::now() + CLIENT_IDLE_TIMEOUT;
                let Ok(request) = receive_request_deadline(&mut stream, req_deadline) else {
                    break;
                };

                let resp = dispatch(request.command, request.request_id, connection_id, &state);
                let env = ResponseEnvelope {
                    request_id: request.request_id,
                    response: resp,
                };
                let resp_deadline = Instant::now() + REQUEST_TIMEOUT;
                if send_response_deadline(&mut stream, &env, resp_deadline).is_err() {
                    break;
                }
            }

            state.diagnostics.log(
                0,
                ErrorCode::Internal,
                format!("connection closed for caller {cid}"),
            );
        });
    }

    STOP.store(true, Ordering::Relaxed);
    if !state.benchmarks.stop_with_deadline(Duration::from_secs(2)) {
        diagnostics.log(
            0,
            ErrorCode::Busy,
            "benchmark did not acknowledge service shutdown within 2 seconds",
        );
    }
    if !state.profiling.stop_with_deadline(Duration::from_secs(2)) {
        diagnostics.log(
            0,
            ErrorCode::Busy,
            "profiling did not acknowledge service shutdown within 2 seconds",
        );
    }
    if sampling_worker.is_finished() {
        let _ = sampling_worker.join();
    } else {
        diagnostics.log(
            0,
            ErrorCode::Busy,
            "sampler still exiting at service shutdown",
        );
    }
    #[cfg(windows)]
    {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !sensor_worker.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if sensor_worker.is_finished() {
            let _ = sensor_worker.join();
        } else {
            diagnostics.log(
                0,
                ErrorCode::Busy,
                "advanced sensor worker still exiting at service shutdown",
            );
        }
    }
    Ok(())
}

#[cfg(windows)]
mod windows_service {
    use super::*;
    use windows_sys::Win32::System::Services::*;

    static mut HANDLE: SERVICE_STATUS_HANDLE = std::ptr::null_mut();

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn status(state: u32, accept: u32) {
        let data = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: accept,
            dwWin32ExitCode: 0,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: 0,
            dwWaitHint: 3000,
        };
        unsafe {
            SetServiceStatus(HANDLE, &data);
        }
    }

    unsafe extern "system" fn handler(control: u32) {
        if control == SERVICE_CONTROL_STOP || control == SERVICE_CONTROL_SHUTDOWN {
            STOP.store(true, Ordering::Relaxed);
            unsafe {
                status(SERVICE_STOP_PENDING, 0);
            }
            std::thread::spawn(|| {
                let _ = planora_helper::connect();
            });
        }
    }

    unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
        let name = wide("PlanoraHelper");
        unsafe {
            HANDLE = RegisterServiceCtrlHandlerW(name.as_ptr(), Some(handler));
            status(SERVICE_START_PENDING, 0);
        }
        let result = run_server(|| unsafe {
            status(
                SERVICE_RUNNING,
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN,
            );
        });
        unsafe {
            status(SERVICE_STOPPED, 0);
        }
        if let Err(error) = result {
            eprintln!("Planora helper service failed: {error}");
        }
    }

    pub fn start() -> io::Result<()> {
        let name = wide("PlanoraHelper");
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: name.as_ptr().cast_mut(),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: std::ptr::null_mut(),
                lpServiceProc: None,
            },
        ];
        let ok = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } != 0;
        if ok {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

fn main() -> io::Result<()> {
    if std::env::args().any(|arg| arg == "--build-info") {
        println!(
            "{}",
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "protocolVersion": PROTOCOL_VERSION,
                "gitCommit": env!("PLANORA_GIT_COMMIT"),
                "target": env!("PLANORA_TARGET"),
                "profile": env!("PLANORA_PROFILE"),
            })
        );
        return Ok(());
    }
    #[cfg(windows)]
    if std::env::args().any(|arg| arg == "--service") {
        return windows_service::start();
    }

    run_server(|| {})
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn invalid_profile_never_starts_worker_or_fabricates_result() {
        let state = State {
            session_id: 1,
            latest_frame: Arc::new(RwLock::new(Sampler::new().sample())),
            benchmarks: Arc::new(BenchmarkManager::new()),
            profiling: Arc::new(ProfileManager::new()),
            diagnostics: Arc::new(DiagnosticLogger::new()),
            sampling: Arc::new(SamplingCoordinator::default()),
        };
        // Duration > 60s exceeds limit
        let response = dispatch(
            Command::StartProfile {
                job_id: 17,
                target: planora_core::ProfileTarget::DiskIo {
                    target_process: None,
                    duration_seconds: 90,
                },
            },
            17,
            1,
            &state,
        );
        assert!(matches!(
            response,
            Response::Error {
                code: ErrorCode::InvalidInput,
                ..
            }
        ));
        assert!(matches!(state.profiling.status(), ProfileState::Idle));
        assert!(state.profiling.worker.lock().unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn affinity_rejects_missing_creation_identity_before_opening_pid() {
        let identity = ProcessIdentity {
            pid: std::process::id(),
            started: 0,
            sequence_number: None,
            source: planora_core::IdentitySource::WindowsProcessFiletime,
        };
        assert!(
            windows_handle(&identity, true)
                .unwrap_err()
                .contains("creation identity is required")
        );
        assert!(
            set_affinity(&identity, &[0])
                .unwrap_err()
                .contains("creation identity is required")
        );
    }

    #[test]
    fn sampling_interval_uses_fastest_connected_client() {
        let sampling = SamplingCoordinator::default();
        assert_eq!(sampling.effective_ms(), 1_000);
        assert_eq!(sampling.set(1, 500), Ok(500));
        assert_eq!(sampling.set(2, 100), Ok(100));
        assert_eq!(sampling.set(3, 2_000), Ok(100));
        assert_eq!(sampling.set(1, 50), Err(()));
        assert_eq!(sampling.effective_ms(), 100);
        sampling.remove(2);
        assert_eq!(sampling.effective_ms(), 500);
        sampling.remove(1);
        sampling.remove(3);
        assert_eq!(sampling.effective_ms(), 1_000);
    }

    #[test]
    fn cancellation_stays_busy_until_worker_acknowledges() {
        let manager = BenchmarkManager::new();
        manager.current_job_id.store(42, Ordering::SeqCst);
        *manager.state.lock().unwrap() = BenchmarkState::Running {
            job_id: 42,
            elapsed_ms: 0,
        };
        assert!(matches!(
            manager.cancel(41),
            Response::Error {
                code: ErrorCode::NotFound,
                ..
            }
        ));
        assert!(matches!(
            manager.cancel(42),
            Response::BenchmarkStatus(BenchmarkState::Cancelling { job_id: 42 })
        ));
        assert!(manager.cancel.lock().unwrap().load(Ordering::SeqCst));
        assert!(matches!(
            manager.status(),
            BenchmarkState::Cancelling { .. }
        ));
        assert!(matches!(
            manager.cancel(42),
            Response::Error {
                code: ErrorCode::NotFound,
                ..
            }
        ));
    }

    #[test]
    fn shutdown_signals_and_joins_owned_benchmark_worker() {
        let manager = BenchmarkManager::new();
        let cancel = manager.cancel.lock().unwrap().clone();
        *manager.worker.lock().unwrap() = Some(thread::spawn(move || {
            while !cancel.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(1));
            }
        }));
        assert!(manager.stop_with_deadline(Duration::from_secs(1)));
        assert!(manager.worker.lock().unwrap().is_none());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_service_tests {
    use super::has_effective_capability;

    #[test]
    fn cap_perfmon_is_accepted_without_root() {
        let status = "Name:\tplanora-helper\nCapEff:\t0000004000000000\n";
        assert!(has_effective_capability(status, 38));
    }

    #[test]
    fn missing_or_unrelated_capability_is_denied() {
        assert!(!has_effective_capability("CapEff:\t0000000000800000\n", 38));
        assert!(!has_effective_capability("Name:\tplanora-helper\n", 38));
        assert!(!has_effective_capability("CapEff:\tnot-hex\n", 38));
        assert!(!has_effective_capability("CapEff:\tffffffffffffffff\n", 64));
    }
}
