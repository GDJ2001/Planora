use interprocess::local_socket::prelude::*;
use planora_bench::{ChaseMode, run_cache_probe, run_core_matrix};
use planora_core::{
    BenchmarkState, Capability, Command, DiagnosticEntry, ErrorCode, PROTOCOL_VERSION,
    ProcessIdentity, Response, ResponseEnvelope, TelemetryFrame,
};
use planora_helper::{caller_id, listener, peer_authorized, receive_request, send_response};
use planora_telemetry::Sampler;
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
static CALLER_CONNECTIONS: Mutex<Option<HashMap<u64, usize>>> = Mutex::new(None);

const MAX_TOTAL_CONNECTIONS: usize = 16;
const MAX_PER_CALLER_CONNECTIONS: usize = 4;
const MAX_DIAGNOSTIC_ENTRIES: usize = 256;

struct ConnectionGuard {
    caller_id: u64,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
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
    cancel: Arc<AtomicBool>,
}

impl BenchmarkManager {
    fn new() -> Self {
        Self {
            current_job_id: AtomicU64::new(0),
            state: Mutex::new(BenchmarkState::Idle),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn status(&self) -> BenchmarkState {
        self.state.lock().unwrap().clone()
    }

    fn cancel(&self, job_id: u64) -> Response {
        let active = self.current_job_id.load(Ordering::SeqCst);
        if active == job_id && active != 0 {
            self.cancel.store(true, Ordering::SeqCst);
            *self.state.lock().unwrap() = BenchmarkState::Cancelled { job_id };
            Response::BenchmarkStatus(BenchmarkState::Cancelled { job_id })
        } else {
            Response::Error {
                code: ErrorCode::NotFound,
                message: format!("no active benchmark job {job_id} to cancel"),
            }
        }
    }
}

struct State {
    session_id: u64,
    latest_frame: Arc<RwLock<TelemetryFrame>>,
    benchmarks: Arc<BenchmarkManager>,
    diagnostics: Arc<DiagnosticLogger>,
}

// The Linux systemd unit uses an unprivileged service account
// with CAP_SYS_NICE (for affinity) and/or CAP_PERFMON.
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
    const CAP_SYS_NICE: u32 = 23;
    const CAP_PERFMON: u32 = 38;
    let is_root = unsafe { libc::geteuid() == 0 };
    is_root
        || std::fs::read_to_string("/proc/self/status")
            .ok()
            .is_some_and(|status| {
                has_effective_capability(&status, CAP_SYS_NICE)
                    || has_effective_capability(&status, CAP_PERFMON)
            })
}

#[cfg(target_os = "macos")]
fn elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(windows)]
fn elevated() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

fn capabilities() -> Vec<Capability> {
    #[cfg(target_os = "linux")]
    let (gpu, power) = (
        planora_telemetry::linux_amd_gpu_busy_available(),
        planora_telemetry::linux_rapl_available(),
    );
    #[cfg(not(target_os = "linux"))]
    let (gpu, power) = (false, false);

    vec![
        Capability {
            name: "CPU and process telemetry".into(),
            available: true,
            reason: None,
        },
        Capability {
            name: "GPU utilization".into(),
            available: gpu,
            reason: (!gpu).then(|| "supported GPU utilization counter unavailable".into()),
        },
        Capability {
            name: "Package power".into(),
            available: power,
            reason: (!power).then(|| "supported package energy counter unavailable".into()),
        },
        Capability {
            name: "PMU counters".into(),
            available: false,
            reason: Some("PMU provider not implemented in v1".into()),
        },
        Capability {
            name: "Process affinity editing".into(),
            available: cfg!(windows),
            reason: if cfg!(windows) {
                None
            } else if cfg!(target_os = "linux") {
                Some("Linux affinity writes disabled in v1 (affinity is per-thread without atomic process guarantees)".into())
            } else {
                Some("native affinity provider unavailable on this platform".into())
            },
        },
    ]
}

#[cfg(windows)]
fn windows_handle(
    process: &ProcessIdentity,
    modify: bool,
) -> Result<windows_sys::Win32::Foundation::HANDLE, String> {
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

    if !ok || (process.started != 0 && stamp != process.started) {
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

fn dispatch(command: Command, req_id: u64, state: &State) -> Response {
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
        Command::Snapshot => {
            let frame = state.latest_frame.read().unwrap().clone();
            Response::Snapshot(Box::new(frame))
        }
        Command::Capabilities => Response::Capabilities(capabilities()),
        Command::RunCacheProbe { job_id, page_mode } => {
            let mut bm_state = state.benchmarks.state.lock().unwrap();
            if matches!(*bm_state, BenchmarkState::Running { .. }) {
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
            state.benchmarks.cancel.store(false, Ordering::SeqCst);
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
            thread::spawn(move || {
                let _start = Instant::now();
                let mode = if page_mode {
                    ChaseMode::Page
                } else {
                    ChaseMode::Line
                };
                let res = run_cache_probe(mode, &bm.cancel);
                let mut lock = bm.state.lock().unwrap();
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
            let mut bm_state = state.benchmarks.state.lock().unwrap();
            if matches!(*bm_state, BenchmarkState::Running { .. }) {
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
            state.benchmarks.cancel.store(false, Ordering::SeqCst);
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
            thread::spawn(move || {
                let res = run_core_matrix(&logical_cpus, bm.cancel.clone());
                let mut lock = bm.state.lock().unwrap();
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

            Response::BenchmarkStatus(BenchmarkState::Running {
                job_id,
                elapsed_ms: 0,
            })
        }
        Command::CancelBenchmark { job_id } => state.benchmarks.cancel(job_id),
        Command::BenchmarkStatus => Response::BenchmarkStatus(state.benchmarks.status()),
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
                    code: if e.contains("disabled in Planora v1") || e.contains("unsupported") {
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

    let latest_frame = Arc::new(RwLock::new(initial_frame));
    let diagnostics = Arc::new(DiagnosticLogger::new());
    diagnostics.log(
        0,
        ErrorCode::Internal,
        format!("helper session {session_id} initialized"),
    );

    // Spawn dedicated sampling worker
    let frame_sink = latest_frame.clone();
    let diag_sink = diagnostics.clone();
    thread::spawn(move || {
        let mut sampler = initial_sampler;
        while !STOP.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(100));
            let frame = sampler.sample();
            if let Err(err) = frame.validate() {
                diag_sink.log(
                    0,
                    ErrorCode::Internal,
                    format!("telemetry frame validation warning: {err}"),
                );
            }
            *frame_sink.write().unwrap() = frame;
        }
    });

    let state = Arc::new(State {
        session_id,
        latest_frame,
        benchmarks: Arc::new(BenchmarkManager::new()),
        diagnostics: diagnostics.clone(),
    });

    let listener = listener()?;
    on_ready();
    eprintln!("Planora helper v2 ready (session {session_id}, local IPC only)");

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
        thread::spawn(move || {
            let _guard = ConnectionGuard { caller_id: cid };
            let mut authenticated = false;

            for _ in 0..128 {
                if STOP.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(request) = receive_request(&mut stream) else {
                    break;
                };

                if !authenticated {
                    authenticated = matches!(
                        request.command,
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
                        request_id: request.request_id,
                        response: resp,
                    };
                    if send_response(&mut stream, &env).is_err() || !authenticated {
                        break;
                    }
                } else {
                    let resp = dispatch(request.command, request.request_id, &state);
                    let env = ResponseEnvelope {
                        request_id: request.request_id,
                        response: resp,
                    };
                    if send_response(&mut stream, &env).is_err() {
                        break;
                    }
                }
            }
        });
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
    #[cfg(windows)]
    if std::env::args().any(|arg| arg == "--service") {
        return windows_service::start();
    }

    run_server(|| {})
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
