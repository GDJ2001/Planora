use interprocess::local_socket::prelude::*;
use planora_bench::{ChaseMode, run_cache_probe, run_core_matrix};
use planora_core::{
    BenchmarkResult, Capability, Command, PROTOCOL_VERSION, ProcessIdentity, Response,
};
use planora_helper::{listener, peer_authorized, receive, send};
use planora_telemetry::Sampler;
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

#[derive(Default)]
struct Job {
    running: bool,
    result: Option<BenchmarkResult>,
    error: Option<String>,
}
struct State {
    sampler: Mutex<Sampler>,
    job: Arc<Mutex<Job>>,
    cancel: Arc<AtomicBool>,
}
static STOP: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

#[cfg(unix)]
fn elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}
#[cfg(windows)]
fn elevated() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

fn capabilities() -> Vec<Capability> {
    vec![
        Capability {
            name: "CPU and process telemetry".into(),
            available: true,
            reason: None,
        },
        Capability {
            name: "GPU utilization".into(),
            available: false,
            reason: Some("GPU provider not installed".into()),
        },
        Capability {
            name: "Package power / PMU".into(),
            available: false,
            reason: Some("hardware counter provider not installed".into()),
        },
        Capability {
            name: "Process affinity editing".into(),
            available: cfg!(target_os = "linux") || cfg!(windows),
            reason: if cfg!(target_os = "linux") || cfg!(windows) {
                None
            } else {
                Some("native affinity provider unavailable on this platform".into())
            },
        },
    ]
}

fn dispatch(command: Command, state: &State) -> Response {
    match command {
        Command::Hello { version } if version == PROTOCOL_VERSION => Response::Hello {
            version,
            helper_authorized: true,
        },
        Command::Hello { .. } => Response::Error {
            code: "VERSION".into(),
            message: "protocol version mismatch".into(),
        },
        Command::Topology => Response::Topology(planora_topology::discover()),
        Command::Snapshot => Response::Snapshot(Box::new(state.sampler.lock().unwrap().sample())),
        Command::Capabilities => Response::Capabilities(capabilities()),
        Command::RunCacheProbe { page_mode } => {
            let mut job = state.job.lock().unwrap();
            if job.running {
                return Response::Error {
                    code: "BUSY".into(),
                    message: "a benchmark is already running".into(),
                };
            }
            state.cancel.store(false, Ordering::SeqCst);
            *job = Job {
                running: true,
                ..Job::default()
            };
            let shared = state.job.clone();
            let cancel = state.cancel.clone();
            thread::spawn(move || {
                let result = run_cache_probe(
                    if page_mode {
                        ChaseMode::Page
                    } else {
                        ChaseMode::Line
                    },
                    &cancel,
                );
                let mut job = shared.lock().unwrap();
                job.running = false;
                match result {
                    Ok(value) => job.result = Some(value),
                    Err(error) => job.error = Some(error),
                }
            });
            Response::BenchmarkStatus {
                running: true,
                result: None,
                error: None,
            }
        }
        Command::RunCoreMatrix { logical_cpus } => {
            let mut job = state.job.lock().unwrap();
            if job.running {
                return Response::Error {
                    code: "BUSY".into(),
                    message: "a benchmark is already running".into(),
                };
            }
            if logical_cpus.len() < 2 || logical_cpus.len() > 8 {
                return Response::Error {
                    code: "INVALID".into(),
                    message: "select 2–8 CPUs".into(),
                };
            }
            state.cancel.store(false, Ordering::SeqCst);
            *job = Job {
                running: true,
                ..Job::default()
            };
            let shared = state.job.clone();
            let cancel = state.cancel.clone();
            thread::spawn(move || {
                let result = run_core_matrix(&logical_cpus, cancel);
                let mut job = shared.lock().unwrap();
                job.running = false;
                match result {
                    Ok(value) => job.result = Some(value),
                    Err(error) => job.error = Some(error),
                }
            });
            Response::BenchmarkStatus {
                running: true,
                result: None,
                error: None,
            }
        }
        Command::CancelBenchmark => {
            state.cancel.store(true, Ordering::SeqCst);
            Response::BenchmarkStatus {
                running: state.job.lock().unwrap().running,
                result: None,
                error: Some("cancellation requested".into()),
            }
        }
        Command::BenchmarkStatus => {
            let job = state.job.lock().unwrap();
            Response::BenchmarkStatus {
                running: job.running,
                result: job.result.clone(),
                error: job.error.clone(),
            }
        }
        Command::GetAffinity { process } => match get_affinity(&process) {
            Ok(logical_cpus) => Response::Affinity {
                process,
                logical_cpus,
            },
            Err(message) => Response::Error {
                code: "AFFINITY".into(),
                message,
            },
        },
        Command::SetAffinity {
            process,
            logical_cpus,
        } => match set_affinity(&process, &logical_cpus) {
            Ok((previous_cpus, cpus)) => Response::AffinityChanged {
                process,
                previous_cpus,
                logical_cpus: cpus,
            },
            Err(message) => Response::Error {
                code: "AFFINITY".into(),
                message,
            },
        },
    }
}

#[cfg(target_os = "linux")]
fn get_affinity(process: &ProcessIdentity) -> Result<Vec<u16>, String> {
    if process.source != "linux proc start ticks" {
        return Err("process creation identity is not strong enough".into());
    }
    if planora_telemetry::linux_start_ticks(process.pid).map_err(|e| e.to_string())?
        != process.started
    {
        return Err("process identity changed".into());
    }
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
    if planora_telemetry::linux_start_ticks(process.pid).ok() != Some(process.started) {
        return Err("process identity changed during query".into());
    }
    Ok((0..libc::CPU_SETSIZE as usize)
        .filter(|i| unsafe { libc::CPU_ISSET(*i, &set) })
        .map(|i| i as u16)
        .collect())
}
#[cfg(windows)]
fn windows_handle(
    process: &ProcessIdentity,
    modify: bool,
) -> Result<windows_sys::Win32::Foundation::HANDLE, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_INFORMATION,
        },
    };
    if process.source.starts_with("windows process sequence") {
        let procs = planora_telemetry::query_windows_basic_processes()
            .map_err(|e| format!("failed to verify process sequence: NTSTATUS {e:#x}"))?;
        let entry = procs
            .iter()
            .find(|p| p.pid == process.pid)
            .ok_or_else(|| "process terminated or PID reused".to_string())?;
        if entry.sequence_number != process.started {
            return Err("process identity changed (PID reused)".into());
        }
    } else if process.source != "windows process FILETIME" {
        return Err("process creation identity is not strong enough".into());
    }

    let rights = PROCESS_QUERY_INFORMATION | if modify { PROCESS_SET_INFORMATION } else { 0 };
    let handle = unsafe { OpenProcess(rights, 0, process.pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error().to_string());
    }
    if process.source == "windows process FILETIME" {
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok =
            unsafe { GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) }
                != 0;
        let stamp = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
        if !ok || stamp != process.started {
            unsafe { CloseHandle(handle) };
            return Err("process identity changed (PID reused)".into());
        }
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
#[cfg(not(any(target_os = "linux", windows)))]
fn get_affinity(_process: &ProcessIdentity) -> Result<Vec<u16>, String> {
    Err("affinity editing unavailable on this platform".into())
}

#[cfg(target_os = "linux")]
fn set_affinity(process: &ProcessIdentity, cpus: &[u16]) -> Result<(Vec<u16>, Vec<u16>), String> {
    if cpus.is_empty()
        || cpus
            .iter()
            .any(|c| *c as usize >= libc::CPU_SETSIZE as usize)
    {
        return Err("CPU set is empty or invalid".into());
    }
    let previous = get_affinity(process)?;
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::CPU_ZERO(&mut set);
        for c in cpus {
            libc::CPU_SET(*c as usize, &mut set);
        }
    }
    let rc = unsafe {
        libc::sched_setaffinity(
            process.pid as i32,
            std::mem::size_of::<libc::cpu_set_t>(),
            &set,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let actual = get_affinity(process)?;
    Ok((previous, actual))
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
        return Err("multi-group affinity is unavailable".into());
    }
    if cpus.is_empty() || cpus.iter().any(|cpu| *cpu as u32 >= usize::BITS) {
        return Err("CPU set is empty or invalid".into());
    }
    let mut unique = cpus.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != cpus.len() {
        return Err("CPU set contains duplicate processors".into());
    }
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
        return Err("requested CPU is outside the system mask".into());
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
    if process.source.starts_with("windows process sequence")
        && let Ok(procs) = planora_telemetry::query_windows_basic_processes()
    {
        if let Some(entry) = procs.iter().find(|p| p.pid == process.pid) {
            if entry.sequence_number != process.started {
                return Err(
                    "process identity changed during affinity operation (PID reused)".into(),
                );
            }
        } else {
            return Err("process terminated during affinity operation".into());
        }
    }
    let decode = |bits: usize| {
        (0..usize::BITS)
            .filter(|i| bits & (1usize << i) != 0)
            .map(|i| i as u16)
            .collect()
    };
    Ok((decode(old), decode(actual)))
}
#[cfg(not(any(target_os = "linux", windows)))]
fn set_affinity(_process: &ProcessIdentity, _cpus: &[u16]) -> Result<(Vec<u16>, Vec<u16>), String> {
    Err("affinity editing is unavailable on this platform".into())
}

fn run_server(on_ready: impl FnOnce()) -> io::Result<()> {
    if !elevated() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Planora helper requires local administrator/root authorization",
        ));
    }
    let state = Arc::new(State {
        sampler: Mutex::new(Sampler::new()),
        job: Arc::new(Mutex::new(Job::default())),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    let listener = listener()?;
    on_ready();
    eprintln!("Planora helper ready (local IPC only)");
    for incoming in listener.incoming() {
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        let Ok(mut stream) = incoming else { continue };
        if !peer_authorized(&stream) {
            continue;
        }
        if ACTIVE.fetch_add(1, Ordering::SeqCst) >= 16 {
            ACTIVE.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let state = state.clone();
        thread::spawn(move || {
            let mut authenticated = false;
            for _ in 0..64 {
                let Ok(command) = receive::<Command>(&mut stream) else {
                    break;
                };
                if !authenticated {
                    authenticated = matches!(command, Command::Hello { version } if version == PROTOCOL_VERSION);
                    let response = if authenticated {
                        Response::Hello {
                            version: PROTOCOL_VERSION,
                            helper_authorized: true,
                        }
                    } else {
                        Response::Error {
                            code: "AUTH".into(),
                            message: "protocol handshake required".into(),
                        }
                    };
                    if send(&mut stream, &response).is_err() || !authenticated {
                        break;
                    }
                } else if send(&mut stream, &dispatch(command, &state)).is_err() {
                    break;
                }
            }
            ACTIVE.fetch_sub(1, Ordering::SeqCst);
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
    unsafe extern "system" fn control(
        code: u32,
        _kind: u32,
        _data: *mut std::ffi::c_void,
        _context: *mut std::ffi::c_void,
    ) -> u32 {
        if code == SERVICE_CONTROL_STOP {
            STOP.store(true, Ordering::SeqCst);
            unsafe { status(SERVICE_STOP_PENDING, 0) };
            let _ = planora_helper::connect();
        }
        0
    }
    unsafe extern "system" fn service_main(_count: u32, _args: *mut *mut u16) {
        let name = wide("PlanoraHelper");
        unsafe {
            HANDLE = RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control), std::ptr::null());
        }
        if unsafe { HANDLE.is_null() } {
            return;
        }
        unsafe { status(SERVICE_START_PENDING, 0) };
        let _ = run_server(|| unsafe { status(SERVICE_RUNNING, SERVICE_ACCEPT_STOP) });
        unsafe { status(SERVICE_STOPPED, 0) };
    }
    pub fn run() -> io::Result<()> {
        let mut name = wide("PlanoraHelper");
        let entries = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: name.as_mut_ptr(),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: std::ptr::null_mut(),
                lpServiceProc: None,
            },
        ];
        if unsafe { StartServiceCtrlDispatcherW(entries.as_ptr()) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

fn main() -> io::Result<()> {
    #[cfg(windows)]
    if std::env::args().any(|arg| arg == "--service") {
        return windows_service::run();
    }
    run_server(|| {})
}
