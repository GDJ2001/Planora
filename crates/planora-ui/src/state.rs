use crate::{
    history::HistoryRange,
    theme::{Density, Theme},
};
use planora_core::{
    BenchmarkState, Capability, Command, DiagnosticEntry, ErrorCode, PROTOCOL_VERSION, Request,
    Response, ResponseEnvelope, TelemetryFrame, Topology,
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::Receiver,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Page {
    #[default]
    Overview,
    Processes,
    Hardware,
    Benchmarks,
    Capabilities,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HardwareViewMode {
    #[default]
    Canvas,
    Tree,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserSettings {
    pub theme: Theme,
    pub density: Density,
    pub poll_interval_ms: u64,
    pub hardware_view_mode: HardwareViewMode,
    pub reduced_motion: bool,
    pub default_history_range: HistoryRange,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            density: Density::Normal,
            poll_interval_ms: 100,
            hardware_view_mode: HardwareViewMode::Canvas,
            reduced_motion: false,
            default_history_range: HistoryRange::Sec60,
        }
    }
}

impl UserSettings {
    pub fn file_path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")));

        base.map(|b| b.join("Planora").join("settings.json"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::file_path()
            && let Ok(data) = std::fs::read_to_string(path)
            && let Ok(settings) = serde_json::from_str(&data)
        {
            return settings;
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::file_path()
            && let Some(parent) = path.parent()
        {
            let _ = std::fs::create_dir_all(parent);
            if let Ok(data) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, data);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppState {
    FirstLaunch,
    AwaitingAuthorization,
    Connecting,
    Live,
    Reconnecting,
    Stale(Duration),
    IncompatibleVersion { helper_version: u32 },
    Failed(String),
}

#[derive(Clone, Debug, Default)]
pub struct DisplayPause {
    pub is_paused: bool,
    pub frozen_frame: Option<Arc<TelemetryFrame>>,
    pub frozen_instant: Option<Instant>,
    pub frozen_histories: Option<Arc<crate::history::ResourceHistories>>,
}

#[derive(Clone, Copy, Debug)]
pub struct BuildInfo {
    pub version: &'static str,
    pub protocol_version: u32,
    pub git_commit: &'static str,
    pub target: &'static str,
    pub profile: &'static str,
}

pub const BUILD_INFO: BuildInfo = BuildInfo {
    version: env!("CARGO_PKG_VERSION"),
    protocol_version: PROTOCOL_VERSION,
    git_commit: env!("PLANORA_GIT_COMMIT"),
    target: env!("PLANORA_TARGET"),
    profile: env!("PLANORA_PROFILE"),
};

#[derive(Default)]
pub struct Shared {
    pub state: Option<AppState>,
    pub last_frame_received: Option<Instant>,
    pub frame: Option<Arc<TelemetryFrame>>,
    pub topology: Option<Topology>,
    pub capabilities: Vec<Capability>,
    pub benchmark_state: Option<BenchmarkState>,
    pub diagnostic_logs: Vec<DiagnosticEntry>,
    pub affinity: Option<(planora_core::ProcessIdentity, Vec<u16>)>,
    pub action_error: Option<String>,
}

pub fn is_data_stale(last_received: Option<Instant>, poll_interval_ms: u64) -> Option<Duration> {
    let last = last_received?;
    let elapsed = last.elapsed();
    let threshold = Duration::from_millis(1000).max(Duration::from_millis(poll_interval_ms * 3));
    if elapsed > threshold {
        Some(elapsed)
    } else {
        None
    }
}

pub fn background_worker(
    state: Arc<Mutex<Shared>>,
    rx: Receiver<Command>,
    poll_ms: Arc<AtomicU64>,
    context: eframe::egui::Context,
) {
    let mut next_request_id = 1u64;
    let mut last_sample_request = Instant::now() - Duration::from_secs(10);
    let mut _current_session_id: Option<u64> = None;

    loop {
        let mut stream = match planora_helper::connect() {
            Ok(s) => s,
            Err(error) => {
                {
                    let mut shared = state.lock().unwrap();
                    let had_frame = shared.frame.is_some();
                    if had_frame {
                        shared.state = Some(AppState::Reconnecting);
                    } else if error.kind() == io::ErrorKind::PermissionDenied {
                        shared.state = Some(AppState::AwaitingAuthorization);
                    } else {
                        shared.state = Some(AppState::Connecting);
                    }
                }
                context.request_repaint();
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        };

        // Handshake
        next_request_id = next_request_id.wrapping_add(1);
        let handshake_req = Request {
            request_id: next_request_id,
            command: Command::Hello {
                version: PROTOCOL_VERSION,
            },
        };

        if planora_helper::send_request(&mut stream, &handshake_req).is_err() {
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        match planora_helper::receive_response(&mut stream) {
            Ok(ResponseEnvelope {
                response:
                    Response::Hello {
                        version,
                        session_id,
                        helper_authorized,
                    },
                ..
            }) => {
                if version != PROTOCOL_VERSION {
                    let mut shared = state.lock().unwrap();
                    shared.state = Some(AppState::IncompatibleVersion {
                        helper_version: version,
                    });
                    context.request_repaint();
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
                if !helper_authorized {
                    let mut shared = state.lock().unwrap();
                    shared.state = Some(AppState::AwaitingAuthorization);
                    context.request_repaint();
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
                _current_session_id = Some(session_id);
                {
                    let mut shared = state.lock().unwrap();
                    shared.state = Some(AppState::Live);
                }
            }
            Ok(ResponseEnvelope {
                response:
                    Response::Error {
                        code: ErrorCode::Unauthorized,
                        ..
                    },
                ..
            }) => {
                let mut shared = state.lock().unwrap();
                shared.state = Some(AppState::AwaitingAuthorization);
                context.request_repaint();
                thread::sleep(Duration::from_secs(1));
                continue;
            }
            _ => {
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        }

        // Fetch topology and capabilities on connect
        next_request_id = next_request_id.wrapping_add(1);
        let _ = planora_helper::send_request(
            &mut stream,
            &Request {
                request_id: next_request_id,
                command: Command::Topology,
            },
        );
        if let Ok(ResponseEnvelope {
            response: Response::Topology(topo),
            ..
        }) = planora_helper::receive_response(&mut stream)
        {
            let mut shared = state.lock().unwrap();
            shared.topology = Some(topo);
        }

        next_request_id = next_request_id.wrapping_add(1);
        let _ = planora_helper::send_request(
            &mut stream,
            &Request {
                request_id: next_request_id,
                command: Command::Capabilities,
            },
        );
        if let Ok(ResponseEnvelope {
            response: Response::Capabilities(caps),
            ..
        }) = planora_helper::receive_response(&mut stream)
        {
            let mut shared = state.lock().unwrap();
            shared.capabilities = caps;
        }

        context.request_repaint();

        // Main command/sample loop
        loop {
            let interval = Duration::from_millis(poll_ms.load(Ordering::Relaxed));
            if last_sample_request.elapsed() >= interval {
                last_sample_request = Instant::now();
                next_request_id = next_request_id.wrapping_add(1);
                let sample_cmd = Request {
                    request_id: next_request_id,
                    command: Command::Snapshot,
                };
                if planora_helper::send_request(&mut stream, &sample_cmd).is_err() {
                    break;
                }
            }

            // Drain any pending UI user actions
            while let Ok(cmd) = rx.try_recv() {
                next_request_id = next_request_id.wrapping_add(1);
                let req = Request {
                    request_id: next_request_id,
                    command: cmd,
                };
                if planora_helper::send_request(&mut stream, &req).is_err() {
                    break;
                }
            }

            // Read available response
            match planora_helper::receive_response(&mut stream) {
                Ok(ResponseEnvelope { response, .. }) => match response {
                    Response::Snapshot(frame) => {
                        let mut shared = state.lock().unwrap();
                        shared.state = Some(AppState::Live);
                        shared.last_frame_received = Some(Instant::now());
                        shared.frame = Some(Arc::from(frame));
                    }
                    Response::BenchmarkStatus(bms) => {
                        let mut shared = state.lock().unwrap();
                        shared.benchmark_state = Some(bms);
                    }
                    Response::DiagnosticLogs(logs) => {
                        let mut shared = state.lock().unwrap();
                        shared.diagnostic_logs = logs;
                    }
                    Response::Affinity {
                        process,
                        logical_cpus,
                    }
                    | Response::AffinityChanged {
                        process,
                        logical_cpus,
                        ..
                    } => {
                        let mut shared = state.lock().unwrap();
                        shared.affinity = Some((process, logical_cpus));
                    }
                    Response::Error { code, message } => {
                        let mut shared = state.lock().unwrap();
                        if code == ErrorCode::Unauthorized {
                            shared.state = Some(AppState::AwaitingAuthorization);
                        } else {
                            shared.action_error = Some(format!("[{:?}] {}", code, message));
                        }
                    }
                    _ => {}
                },
                Err(error) => {
                    let mut shared = state.lock().unwrap();
                    let had_frame = shared.frame.is_some();
                    if had_frame {
                        shared.state = Some(AppState::Reconnecting);
                    } else if error.kind() == io::ErrorKind::PermissionDenied {
                        shared.state = Some(AppState::AwaitingAuthorization);
                    } else {
                        shared.state = Some(AppState::Connecting);
                    }
                    break; // reconnect
                }
            }
            context.request_repaint();
            thread::sleep(Duration::from_millis(10));
        }

        thread::sleep(Duration::from_millis(50));
    }
}
