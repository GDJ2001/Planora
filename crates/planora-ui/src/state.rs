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
    io::{self, Write},
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
    Resources,
    Processes,
    History,
    Hardware,
    Benchmarks,
    Capabilities,
    Settings,
    Recordings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HardwareViewMode {
    #[default]
    Tree,
    Canvas,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    pub schema_version: u32,
    pub theme: Theme,
    pub density: Density,
    pub poll_interval_ms: u64,
    pub hardware_view_mode: HardwareViewMode,
    pub reduced_motion: bool,
    pub default_history_range: HistoryRange,
    pub process_columns: crate::components::table::TableColumnsConfig,
    pub process_sort_column: crate::components::table::SortColumn,
    pub process_sort_descending: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            schema_version: 3,
            theme: Theme::System,
            density: Density::Normal,
            poll_interval_ms: 100,
            hardware_view_mode: HardwareViewMode::Canvas,
            reduced_motion: false,
            default_history_range: HistoryRange::Sec60,
            process_columns: crate::components::table::TableColumnsConfig::default(),
            process_sort_column: crate::components::table::SortColumn::Cpu,
            process_sort_descending: true,
        }
    }
}

impl UserSettings {
    pub fn file_path() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| p.join("Library").join("Application Support"));
        #[cfg(windows)]
        let base = std::env::var_os("APPDATA").map(PathBuf::from);
        #[cfg(all(not(windows), not(target_os = "macos")))]
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")));

        base.map(|b| b.join("Planora").join("settings.json"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::file_path() {
            let paths = vec![path];
            #[cfg(target_os = "macos")]
            let paths = {
                let mut paths = paths;
                if let Some(legacy) = std::env::var_os("HOME") {
                    paths.push(PathBuf::from(legacy).join(".config/Planora/settings.json"));
                }
                paths
            };
            for path in paths {
                if let Ok(data) = std::fs::read_to_string(path)
                    && let Ok(mut settings) = serde_json::from_str::<Self>(&data)
                {
                    settings.schema_version = 3;
                    settings.poll_interval_ms = match settings.poll_interval_ms {
                        50 => 100,
                        200 => 250,
                        other => other,
                    };
                    if !planora_core::SUPPORTED_SAMPLE_INTERVALS_MS
                        .contains(&settings.poll_interval_ms)
                    {
                        settings.poll_interval_ms = Self::default().poll_interval_ms;
                    }
                    return settings;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> io::Result<()> {
        static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(1);
        let path = Self::file_path().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "configuration directory unavailable",
            )
        })?;
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid settings path"))?;
        std::fs::create_dir_all(parent)?;
        let temp = parent.join(format!(
            "settings.{}.{}.{}.tmp",
            std::process::id(),
            planora_telemetry::unix_ms(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let data = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let result = (|| -> io::Result<()> {
            file.write_all(&data)?;
            file.sync_all()?;
            drop(file);
            replace_settings_file(&temp, &path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
}

#[cfg(not(windows))]
fn replace_settings_file(temp: &std::path::Path, target: &std::path::Path) -> io::Result<()> {
    std::fs::rename(temp, target)
}

#[cfg(windows)]
fn replace_settings_file(temp: &std::path::Path, target: &std::path::Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let from: Vec<u16> = temp
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let to: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppState {
    FirstLaunch,
    AwaitingAuthorization,
    Connecting,
    Live,
    Replay,
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
    pub profile_state: Option<planora_core::ProfileState>,
    pub sampling_effective_ms: Option<u64>,
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

fn handle_response(response: Response, state: &Arc<Mutex<Shared>>) -> bool {
    let mut shared = state.lock().unwrap();
    match response {
        Response::Snapshot(frame) => {
            if let Err(error) = frame.validate() {
                shared.action_error = Some(format!("Invalid telemetry frame: {error}"));
                return false;
            }
            let is_new = shared.frame.as_ref().is_none_or(|previous| {
                previous.session_id != frame.session_id || previous.sequence != frame.sequence
            });
            shared.state = Some(AppState::Live);
            if is_new {
                shared.last_frame_received = Some(Instant::now());
                shared.frame = Some(Arc::from(frame));
            }
        }
        Response::BenchmarkStatus(bms) => {
            shared.benchmark_state = Some(bms);
        }
        Response::ProfileStatus(ps) => {
            shared.profile_state = Some(ps);
        }
        Response::Capabilities(caps) => {
            shared.capabilities = caps;
        }
        Response::SamplingStatus { effective_ms, .. } => {
            shared.sampling_effective_ms = Some(effective_ms);
        }
        Response::DiagnosticLogs(logs) => {
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
            shared.affinity = Some((process, logical_cpus));
        }
        Response::Error { code, message } => {
            if code == ErrorCode::Unauthorized {
                shared.state = Some(AppState::AwaitingAuthorization);
                return false;
            } else {
                shared.action_error = Some(format!("[{:?}] {}", code, message));
            }
        }
        _ => {}
    }
    true
}

pub fn background_worker(
    state: Arc<Mutex<Shared>>,
    rx: Receiver<Command>,
    poll_ms: Arc<AtomicU64>,
    context: eframe::egui::Context,
) {
    let mut next_request_id = 1u64;
    let mut _current_session_id: Option<u64> = None;
    let mut reconnect_backoff = Duration::from_millis(200);

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
                thread::sleep(reconnect_backoff);
                reconnect_backoff = (reconnect_backoff * 2).min(Duration::from_millis(2000));
                continue;
            }
        };

        // Handshake with HANDSHAKE_TIMEOUT
        next_request_id = next_request_id.wrapping_add(1);
        let handshake_req = Request {
            request_id: next_request_id,
            command: Command::Hello {
                version: PROTOCOL_VERSION,
            },
        };

        match planora_helper::exchange_request_timeout(
            &mut stream,
            &handshake_req,
            planora_helper::HANDSHAKE_TIMEOUT,
        ) {
            Ok(ResponseEnvelope {
                request_id,
                response:
                    Response::Hello {
                        version,
                        session_id,
                        helper_authorized,
                    },
            }) if request_id == handshake_req.request_id => {
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
                // Reset reconnect backoff on successful handshake
                reconnect_backoff = Duration::from_millis(200);
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
                // Timeout, request ID mismatch, or broken pipe on handshake
                let mut shared = state.lock().unwrap();
                if shared.frame.is_some() {
                    shared.state = Some(AppState::Reconnecting);
                }
                context.request_repaint();
                thread::sleep(reconnect_backoff);
                reconnect_backoff = (reconnect_backoff * 2).min(Duration::from_millis(2000));
                continue;
            }
        }

        // Register this connection's collection cadence before fetching data.
        let mut configured_poll_ms = poll_ms.load(Ordering::Relaxed);
        next_request_id = next_request_id.wrapping_add(1);
        let sampling_req = Request {
            request_id: next_request_id,
            command: Command::SetSamplingInterval {
                interval_ms: configured_poll_ms,
            },
        };
        match planora_helper::exchange_request_timeout(
            &mut stream,
            &sampling_req,
            planora_helper::REQUEST_TIMEOUT,
        ) {
            Ok(ResponseEnvelope {
                request_id,
                response:
                    Response::SamplingStatus {
                        requested_ms,
                        effective_ms,
                    },
            }) if request_id == sampling_req.request_id && requested_ms == configured_poll_ms => {
                state.lock().unwrap().sampling_effective_ms = Some(effective_ms);
            }
            _ => {
                state.lock().unwrap().state = Some(AppState::Reconnecting);
                context.request_repaint();
                continue;
            }
        }

        // Fetch topology and capabilities on connect with REQUEST_TIMEOUT
        next_request_id = next_request_id.wrapping_add(1);
        let topo_req_id = next_request_id;
        let topo_req = Request {
            request_id: topo_req_id,
            command: Command::Topology,
        };
        let topo_result = planora_helper::exchange_request_timeout(
            &mut stream,
            &topo_req,
            planora_helper::REQUEST_TIMEOUT,
        );
        match topo_result {
            Ok(ResponseEnvelope {
                response: Response::Topology(topo),
                ..
            }) => state.lock().unwrap().topology = Some(topo),
            _ => {
                state.lock().unwrap().state = Some(AppState::Reconnecting);
                context.request_repaint();
                thread::sleep(reconnect_backoff);
                reconnect_backoff = (reconnect_backoff * 2).min(Duration::from_millis(2000));
                continue;
            }
        }

        next_request_id = next_request_id.wrapping_add(1);
        let caps_req_id = next_request_id;
        let caps_req = Request {
            request_id: caps_req_id,
            command: Command::Capabilities,
        };
        let caps_result = planora_helper::exchange_request_timeout(
            &mut stream,
            &caps_req,
            planora_helper::REQUEST_TIMEOUT,
        );
        match caps_result {
            Ok(ResponseEnvelope {
                response: Response::Capabilities(caps),
                ..
            }) => state.lock().unwrap().capabilities = caps,
            _ => {
                state.lock().unwrap().state = Some(AppState::Reconnecting);
                context.request_repaint();
                thread::sleep(reconnect_backoff);
                reconnect_backoff = (reconnect_backoff * 2).min(Duration::from_millis(2000));
                continue;
            }
        }

        // Fetch initial snapshot immediately so Live state has fresh data right away
        next_request_id = next_request_id.wrapping_add(1);
        let init_req_id = next_request_id;
        let init_req = Request {
            request_id: init_req_id,
            command: Command::Snapshot,
        };
        let initial_snapshot = planora_helper::exchange_request_timeout(
            &mut stream,
            &init_req,
            planora_helper::REQUEST_TIMEOUT,
        );
        match initial_snapshot {
            Ok(ResponseEnvelope {
                response: Response::Snapshot(frame),
                ..
            }) => {
                if !handle_response(Response::Snapshot(frame), &state) {
                    state.lock().unwrap().state = Some(AppState::Reconnecting);
                    context.request_repaint();
                    continue;
                }
            }
            _ => {
                state.lock().unwrap().state = Some(AppState::Reconnecting);
                context.request_repaint();
                thread::sleep(reconnect_backoff);
                reconnect_backoff = (reconnect_backoff * 2).min(Duration::from_millis(2000));
                continue;
            }
        }
        let mut last_sample_request = Instant::now();
        context.request_repaint();

        // Main command/sample loop with strict request-response pairing and deadlines
        loop {
            let desired_poll_ms = poll_ms.load(Ordering::Relaxed);
            if desired_poll_ms != configured_poll_ms {
                next_request_id = next_request_id.wrapping_add(1);
                let req = Request {
                    request_id: next_request_id,
                    command: Command::SetSamplingInterval {
                        interval_ms: desired_poll_ms,
                    },
                };
                match planora_helper::exchange_request_timeout(
                    &mut stream,
                    &req,
                    planora_helper::REQUEST_TIMEOUT,
                ) {
                    Ok(ResponseEnvelope {
                        request_id,
                        response:
                            Response::SamplingStatus {
                                requested_ms,
                                effective_ms,
                            },
                    }) if request_id == req.request_id && requested_ms == desired_poll_ms => {
                        configured_poll_ms = desired_poll_ms;
                        state.lock().unwrap().sampling_effective_ms = Some(effective_ms);
                        context.request_repaint();
                    }
                    _ => {
                        state.lock().unwrap().state = Some(AppState::Reconnecting);
                        break;
                    }
                }
            }
            // 1. Drain up to 4 pending user commands from UI to keep sampling responsive
            let mut connection_healthy = true;
            let mut commands_processed = 0;
            while commands_processed < 4
                && let Ok(cmd) = rx.try_recv()
            {
                commands_processed += 1;
                next_request_id = next_request_id.wrapping_add(1);
                let cur_req_id = next_request_id;
                let req = Request {
                    request_id: cur_req_id,
                    command: cmd,
                };
                match planora_helper::exchange_request_timeout(
                    &mut stream,
                    &req,
                    planora_helper::REQUEST_TIMEOUT,
                ) {
                    Ok(env) => {
                        if !handle_response(env.response, &state) {
                            connection_healthy = false;
                            break;
                        }
                    }
                    Err(_) => {
                        connection_healthy = false;
                        break;
                    }
                }
                context.request_repaint();
            }

            if !connection_healthy {
                let mut shared = state.lock().unwrap();
                let had_frame = shared.frame.is_some();
                if had_frame {
                    shared.state = Some(AppState::Reconnecting);
                } else {
                    shared.state = Some(AppState::Connecting);
                }
                break; // break to reconnect
            }

            // 2. Determine time until next scheduled snapshot
            let interval = Duration::from_millis(poll_ms.load(Ordering::Relaxed));
            let elapsed = last_sample_request.elapsed();
            let time_until_sample = interval.saturating_sub(elapsed);

            // 3. Wait on rx for user commands until next sample is due
            if time_until_sample > Duration::from_millis(5) {
                match rx.recv_timeout(time_until_sample) {
                    Ok(cmd) => {
                        next_request_id = next_request_id.wrapping_add(1);
                        let cur_req_id = next_request_id;
                        let req = Request {
                            request_id: cur_req_id,
                            command: cmd,
                        };
                        match planora_helper::exchange_request_timeout(
                            &mut stream,
                            &req,
                            planora_helper::REQUEST_TIMEOUT,
                        ) {
                            Ok(env) => {
                                handle_response(env.response, &state);
                            }
                            Err(_) => {
                                let mut shared = state.lock().unwrap();
                                shared.state = Some(AppState::Reconnecting);
                                break;
                            }
                        }
                        context.request_repaint();
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        // Timeout reached, proceed to sample snapshot
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return; // UI shutdown
                    }
                }
            }

            // 4. Sample snapshot with REQUEST_TIMEOUT deadline
            last_sample_request = Instant::now();
            next_request_id = next_request_id.wrapping_add(1);
            let sample_req_id = next_request_id;
            let sample_cmd = Request {
                request_id: sample_req_id,
                command: Command::Snapshot,
            };
            match planora_helper::exchange_request_timeout(
                &mut stream,
                &sample_cmd,
                planora_helper::REQUEST_TIMEOUT,
            ) {
                Ok(env) => {
                    if let Response::Snapshot(frame) = env.response {
                        if !handle_response(Response::Snapshot(frame), &state) {
                            state.lock().unwrap().state = Some(AppState::Reconnecting);
                            break;
                        }
                    } else {
                        // Response type mismatch for Snapshot command
                        let mut shared = state.lock().unwrap();
                        shared.state = Some(AppState::Reconnecting);
                        break;
                    }
                }
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
                    break;
                }
            }
            context.request_repaint();
        }

        thread::sleep(reconnect_backoff);
        reconnect_backoff = (reconnect_backoff * 2).min(Duration::from_millis(2000));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use planora_core::{AvailabilityReason, Metric, PROTOCOL_VERSION, TelemetryFrame};

    fn dummy_frame(seq: u64) -> TelemetryFrame {
        TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            session_id: 42,
            topology_generation: 1,
            sequence: seq,
            sampled_at_ms: 1000 + seq * 100,
            collection_duration_ms: 1.5,
            cpu_percent: Metric::available(25.0, "%", "test", 1000),
            logical_cpus: Vec::new(),
            memory_used_bytes: Metric::available(1024 * 1024, "bytes", "test", 1000),
            memory_available_bytes: Metric::available(3 * 1024 * 1024, "bytes", "test", 1000),
            memory_total_bytes: Metric::available(4 * 1024 * 1024, "bytes", "test", 1000),
            commit_used_bytes: Metric::available(2 * 1024 * 1024, "bytes", "test", 1000),
            commit_total_bytes: Metric::available(8 * 1024 * 1024, "bytes", "test", 1000),
            disk_read_bytes_per_sec: Metric::available(0, "B/s", "test", 1000),
            disk_write_bytes_per_sec: Metric::available(0, "B/s", "test", 1000),
            network_rx_bytes_per_sec: Metric::available(0, "B/s", "test", 1000),
            network_tx_bytes_per_sec: Metric::available(0, "B/s", "test", 1000),
            gpu_percent: Metric::unavailable(
                "%",
                "test",
                1000,
                AvailabilityReason::Unsupported("None".into()),
            ),
            gpu_adapters: Vec::new(),
            gpu_engines: Vec::new(),
            gpu_engine_unavailable_reason: Some(planora_core::AvailabilityReason::WarmingUp),
            disks: Vec::new(),
            network_interfaces: Vec::new(),
            package_power_watts: Metric::unavailable(
                "W",
                "test",
                1000,
                AvailabilityReason::Unsupported("None".into()),
            ),
            sensors: Vec::new(),
            processes: Vec::new(),
        }
    }

    #[test]
    fn test_telemetry_stall_and_recovery_cycle() {
        let state = Arc::new(Mutex::new(Shared::default()));

        // Step 1: Initial state is Connecting
        assert_eq!(state.lock().unwrap().state, None);

        // Step 2: Receive snapshot -> transitions to Live
        let frame1 = dummy_frame(1);
        handle_response(Response::Snapshot(Box::new(frame1)), &state);

        {
            let shared = state.lock().unwrap();
            assert_eq!(shared.state, Some(AppState::Live));
            assert!(shared.last_frame_received.is_some());
            assert_eq!(shared.frame.as_ref().unwrap().sequence, 1);
        }

        // Step 3: Check freshness within poll interval (100ms)
        let last_rcvd = state.lock().unwrap().last_frame_received.unwrap();
        assert!(is_data_stale(Some(last_rcvd), 100).is_none());

        // Step 4: Simulate telemetry stall past 3 * 100ms + 500ms threshold (e.g. 1500ms ago)
        let stale_time = Instant::now()
            .checked_sub(Duration::from_millis(1500))
            .unwrap();
        let elapsed = is_data_stale(Some(stale_time), 100);
        assert!(
            elapsed.is_some(),
            "Data older than 800ms must be flagged as stale"
        );

        // UI evaluates this to AppState::Stale
        let stale_state = AppState::Stale(elapsed.unwrap());
        match stale_state {
            AppState::Stale(d) => assert!(d >= Duration::from_millis(1500)),
            _ => panic!("Expected AppState::Stale"),
        }

        // Step 5: Telemetry recovers -> helper delivers frame 2
        let frame2 = dummy_frame(2);
        handle_response(Response::Snapshot(Box::new(frame2)), &state);

        {
            let shared = state.lock().unwrap();
            assert_eq!(shared.state, Some(AppState::Live));
            assert_eq!(shared.frame.as_ref().unwrap().sequence, 2);
            // Staleness check on fresh frame immediately returns None (not stale)
            let new_rcvd = shared.last_frame_received.unwrap();
            assert!(is_data_stale(Some(new_rcvd), 100).is_none());
        }
    }

    #[test]
    fn test_reconnect_transition_preserves_frame() {
        let state = Arc::new(Mutex::new(Shared::default()));

        // Receive snapshot
        handle_response(Response::Snapshot(Box::new(dummy_frame(1))), &state);

        // Simulate IPC disconnection: since frame exists, transition to Reconnecting
        {
            let mut shared = state.lock().unwrap();
            assert!(shared.frame.is_some());
            shared.state = Some(AppState::Reconnecting);
        }

        // Verify that frame is preserved so UI can display last known state with Reconnecting badge
        {
            let shared = state.lock().unwrap();
            assert_eq!(shared.state, Some(AppState::Reconnecting));
            assert_eq!(shared.frame.as_ref().unwrap().sequence, 1);
        }
    }

    #[test]
    fn repeated_or_invalid_frame_cannot_freshen_display() {
        let state = Arc::new(Mutex::new(Shared::default()));
        assert!(handle_response(
            Response::Snapshot(Box::new(dummy_frame(1))),
            &state
        ));
        let received = state.lock().unwrap().last_frame_received.unwrap();
        let original = state.lock().unwrap().frame.clone().unwrap();
        assert!(handle_response(
            Response::Snapshot(Box::new(dummy_frame(1))),
            &state
        ));
        assert_eq!(state.lock().unwrap().last_frame_received, Some(received));
        assert!(Arc::ptr_eq(
            &original,
            state.lock().unwrap().frame.as_ref().unwrap()
        ));

        let mut invalid = dummy_frame(2);
        invalid.cpu_percent.value = Some(f32::NAN);
        assert!(!handle_response(
            Response::Snapshot(Box::new(invalid)),
            &state
        ));
        let shared = state.lock().unwrap();
        assert_eq!(shared.last_frame_received, Some(received));
        assert_eq!(shared.frame.as_ref().unwrap().sequence, 1);
    }

    #[test]
    fn test_ipc_stream_stall_triggers_timeout_and_staleness() {
        use interprocess::local_socket::prelude::*;
        #[cfg(not(windows))]
        use interprocess::local_socket::{GenericFilePath, ToFsName};
        #[cfg(windows)]
        use interprocess::local_socket::{GenericNamespaced, ToNsName};
        use interprocess::local_socket::{ListenerOptions, Stream};
        use planora_helper::{receive_request, receive_response_timeout};

        let name = {
            #[cfg(windows)]
            {
                format!("planora-ui-test-stall-{}", std::process::id())
                    .to_ns_name::<GenericNamespaced>()
                    .unwrap()
            }
            #[cfg(not(windows))]
            {
                format!("/tmp/planora-ui-test-stall-{}.sock", std::process::id())
                    .to_fs_name::<GenericFilePath>()
                    .unwrap()
            }
        };

        let listener = ListenerOptions::new()
            .name(name.clone())
            .create_sync()
            .unwrap();

        // Server accepts connection and stalls without answering request
        let server = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let _req = receive_request(&mut stream).unwrap();
            // Stalls for 2 seconds
            thread::sleep(Duration::from_millis(2000));
        });

        let mut client = Stream::connect(name).unwrap();
        planora_helper::send_request_deadline(
            &mut client,
            &Request {
                request_id: 1,
                command: Command::Snapshot,
            },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();

        let res = receive_response_timeout(&mut client, Duration::from_millis(300));
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), io::ErrorKind::TimedOut);

        // Verify that when timeout occurs, shared state reports Reconnecting and retains age
        let state = Arc::new(Mutex::new(Shared::default()));
        {
            let mut s = state.lock().unwrap();
            s.frame = Some(Arc::from(dummy_frame(1)));
            s.last_frame_received = Some(Instant::now() - Duration::from_millis(1500));
            s.state = Some(AppState::Reconnecting);
        }

        let staleness = is_data_stale(state.lock().unwrap().last_frame_received, 100);
        assert!(staleness.is_some());
        assert_eq!(state.lock().unwrap().state, Some(AppState::Reconnecting));

        server.join().unwrap();
    }
}
