#![allow(clippy::too_many_arguments)]
#![cfg_attr(windows, windows_subsystem = "windows")]

pub mod appearance;
pub mod components;
pub mod export;
pub mod formatting;
pub mod history;
pub mod history_worker;
pub mod instance;
pub mod recording;
pub mod smoke_test;
pub mod startup;
#[cfg(windows)]
pub mod startup_authorization;
pub mod state;
pub mod theme;
pub mod views;
pub mod window_placement;

use components::{badge::render_compact_sidebar_status, icons, logo::LogoCache};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};
use export::ExportManager;
use history::ResourceHistories;
use history_worker::{HistoryInput, HistoryWorker};
use planora_core::{Command, PROTOCOL_VERSION, ProcessIdentity};
use recording::{Recorder, Replay};
use state::{AppState, DisplayPause, Page, Shared, UserSettings, background_worker, is_data_stale};
#[cfg(windows)]
use std::sync::OnceLock;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::Instant,
};
use theme::{Palette, SIDEBAR_WIDTH, accessible_palette, configure_theme, theme_is_dark};
use views::{
    benchmarks::{BenchmarksViewState, render_benchmarks_view},
    capabilities::{CapabilitiesViewState, render_capabilities_view},
    hardware::{HardwareViewState, render_hardware_view},
    history::{HistoryViewState, render_history_view},
    overview::{OverviewViewState, render_overview_view},
    processes::{ProcessesViewState, render_processes_view},
    resources::{ResourcesViewState, render_resources_view},
    settings::{SettingsViewState, render_settings_view},
};
use window_placement::{Placement, WorkArea};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn planora_register_daemon() -> i32;
    fn planora_unregister_daemon() -> i32;
}

pub struct App {
    shared: Arc<Mutex<Shared>>,
    tx: SyncSender<Command>,
    poll_ms: Arc<AtomicU64>,
    client_stop: Arc<AtomicBool>,
    client_worker: Option<std::thread::JoinHandle<()>>,
    #[cfg(windows)]
    sensor_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    #[cfg(windows)]
    sensor_refresh: Option<Arc<std::sync::atomic::AtomicBool>>,
    #[cfg(windows)]
    sensor_worker: Option<std::thread::JoinHandle<()>>,
    appearance: appearance::AppearanceMonitor,
    applied_appearance: Option<appearance::SystemAppearance>,
    settings: UserSettings,
    settings_error: Option<String>,
    recorder: Recorder,
    exporter: ExportManager,
    replay: Replay,
    replay_anchor: Option<(u64, Instant)>,
    replay_input: String,
    record_selected_process: bool,
    recording_error: Option<String>,
    #[cfg(windows)]
    authorization_child: Option<std::process::Child>,
    #[cfg(windows)]
    authorization_error: Option<String>,

    // Navigation & View states
    page: Page,
    selected: Option<ProcessIdentity>,
    display_pause: DisplayPause,

    logo_cache: LogoCache,
    histories: Arc<ResourceHistories>,
    history_worker: HistoryWorker,
    history_epoch: u64,
    history_last_enqueued: Option<(u64, u64, Option<ProcessIdentity>)>,

    // View specific state
    overview_state: OverviewViewState,
    resources_state: ResourcesViewState,
    processes_state: ProcessesViewState,
    history_state: HistoryViewState,
    hardware_state: HardwareViewState,
    benchmarks_state: BenchmarksViewState,
    capabilities_state: CapabilitiesViewState,
    settings_state: SettingsViewState,
    auto_close: Option<(Instant, std::time::Duration)>,
    screenshot_out: Option<std::path::PathBuf>,
    screenshot_pending: bool,
    frames_rendered: usize,
    fixture_mode: bool,
    window_placement: Placement,
    window_work_area: WorkArea,
    #[cfg(windows)]
    native_handle: isize,
    last_monitor_geometry: Option<(Option<Rect>, Option<f32>)>,
    normal_window: bool,
    window_recovered: bool,
    startup_connected_logged: bool,
    pub lifecycle: LifecycleState,
    shutdown_started: Option<Instant>,
    shutdown_completed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    Starting,
    Running,
    Stopping,
}

#[cfg(windows)]
static STARTUP_AUTH_ERROR: OnceLock<String> = OnceLock::new();

fn show_page_scroll<R>(
    ui: &mut egui::Ui,
    id_salt: &'static str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    let available_w = ui.available_width().max(1.0);
    let mut scroll = egui::ScrollArea::vertical();
    if let Some(offset) = ui
        .ctx()
        .data(|d| d.get_temp::<f32>(egui::Id::unique("fixture_scroll_y")))
    {
        scroll = scroll.vertical_scroll_offset(offset);
    }
    scroll
        .id_salt(id_salt)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let max_w = ui.available_width().min(available_w);
            ui.set_width(max_w);
            ui.set_max_width(max_w);
            let clip_x = Rect::from_x_y_ranges(ui.max_rect().x_range(), ui.clip_rect().y_range())
                .intersect(ui.clip_rect());
            ui.set_clip_rect(clip_x);
            add_contents(ui)
        })
}

impl App {
    fn reset_history(&mut self) {
        self.history_epoch = self.history_epoch.wrapping_add(1);
        self.history_last_enqueued = None;
        self.histories = Arc::new(ResourceHistories::default());
    }

    #[cfg(windows)]
    fn launch_helper_script(&mut self, script_name: &str) {
        if self.authorization_child.is_some() {
            return;
        }
        let script = if script_name == "Authorize-Planora.ps1" {
            startup_authorization::trusted_script()
        } else {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join(script_name)))
        };
        let Some(script) = script.filter(|path| path.is_file()) else {
            self.authorization_error = Some(format!(
                "{script_name} is missing from the installed application."
            ));
            return;
        };
        let powershell = match startup_authorization::system_powershell() {
            Ok(path) => path,
            Err(error) => {
                self.authorization_error = Some(format!(
                    "Could not find system PowerShell for helper authorization: {error}"
                ));
                return;
            }
        };
        let mut command = std::process::Command::new(powershell);
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW; UAC remains an OS dialog.
        match command.spawn() {
            Ok(child) => {
                self.authorization_child = Some(child);
                self.authorization_error = None;
            }
            Err(error) => {
                self.authorization_error = Some(format!(
                    "Could not launch helper authorization command: {error}"
                ))
            }
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            startup::record(&format!(
                "renderer-init backend={:?} adapter={}",
                rs.adapter.get_info().backend,
                rs.adapter.get_info().name,
            ));
        } else {
            startup::record("renderer-init backend=default");
        }
        let (tx, rx) = mpsc::sync_channel(64);
        let settings = UserSettings::load();
        let mut processes_state = ProcessesViewState::default();
        processes_state.table_state.columns = settings.process_columns;
        processes_state.table_state.sort_column = settings.process_sort_column;
        processes_state.table_state.sort_descending = settings.process_sort_descending;
        let mut resources_state = ResourcesViewState::default();
        resources_state.selected_resource = settings.performance_resource.clone();
        resources_state.range = settings.performance_range;
        resources_state.selected_engines = settings.performance_engines.iter().cloned().collect();
        let poll_ms = Arc::new(AtomicU64::new(settings.poll_interval_ms));
        let shared = Arc::new(Mutex::new(Shared::default()));

        let worker_shared = shared.clone();
        let worker_poll = poll_ms.clone();
        let context = cc.egui_ctx.clone();

        let client_stop = Arc::new(AtomicBool::new(false));
        let worker_stop = client_stop.clone();
        let history_worker = HistoryWorker::start();
        let history_sender = history_worker.sender();
        let client_worker = thread::Builder::new()
            .name("planora-client".into())
            .spawn(move || {
                background_worker(
                    worker_shared,
                    rx,
                    worker_poll,
                    context,
                    worker_stop,
                    history_sender,
                );
            })
            .expect("failed to spawn client worker thread");

        #[cfg(windows)]
        let sensor_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        #[cfg(windows)]
        let sensor_refresh = Arc::new(std::sync::atomic::AtomicBool::new(false));
        #[cfg(windows)]
        let sensor_worker = {
            let worker_shared = shared.clone();
            let stop = sensor_stop.clone();
            let refresh = sensor_refresh.clone();
            let context = cc.egui_ctx.clone();
            std::thread::Builder::new()
                .name("planora-vendor-sensors".into())
                .spawn(move || {
                    let mut supervisor =
                        planora_telemetry::vendor_sensors::VendorSensorSupervisor::new(env!(
                            "CARGO_PKG_VERSION"
                        ));
                    {
                        worker_shared.lock().unwrap().sensor_capabilities =
                            Arc::new(supervisor.probe_capabilities());
                    }
                    let mut last_probe = Instant::now();
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        if refresh.swap(false, std::sync::atomic::Ordering::SeqCst)
                            || last_probe.elapsed() >= std::time::Duration::from_secs(30)
                        {
                            supervisor.restart();
                            worker_shared.lock().unwrap().sensor_capabilities =
                                Arc::new(supervisor.probe_capabilities());
                            last_probe = Instant::now();
                        }
                        let readings = supervisor.poll_sensors();
                        worker_shared.lock().unwrap().sensor_readings = Arc::new(readings);
                        context.request_repaint();
                        std::thread::park_timeout(std::time::Duration::from_secs(1));
                    }
                })
                .ok()
        };

        #[cfg(windows)]
        let native_handle = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            cc.window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                    _ => None,
                })
                .unwrap_or(0)
        };
        let (placement, work_area) = window_placement::startup_placement();
        Self {
            shared,
            tx,
            poll_ms,
            client_stop,
            client_worker: Some(client_worker),
            #[cfg(windows)]
            sensor_stop: Some(sensor_stop),
            #[cfg(windows)]
            sensor_refresh: Some(sensor_refresh),
            #[cfg(windows)]
            sensor_worker,
            appearance: appearance::AppearanceMonitor::start(cc.egui_ctx.clone()),
            applied_appearance: None,
            settings,
            settings_error: None,
            recorder: Recorder::default(),
            exporter: ExportManager::default(),
            replay: Replay::default(),
            replay_anchor: None,
            replay_input: String::new(),
            record_selected_process: false,
            recording_error: None,
            #[cfg(windows)]
            authorization_child: None,
            #[cfg(windows)]
            authorization_error: STARTUP_AUTH_ERROR.get().cloned(),
            page: Page::Performance,
            selected: None,
            display_pause: DisplayPause::default(),
            logo_cache: LogoCache::new(),
            histories: Arc::new(ResourceHistories::default()),
            history_worker,
            history_epoch: 0,
            history_last_enqueued: None,
            overview_state: OverviewViewState::default(),
            resources_state,
            processes_state,
            history_state: HistoryViewState::default(),
            hardware_state: HardwareViewState::default(),
            benchmarks_state: BenchmarksViewState::default(),
            capabilities_state: CapabilitiesViewState::default(),
            settings_state: SettingsViewState::default(),
            auto_close: None,
            screenshot_out: None,
            screenshot_pending: false,
            frames_rendered: 0,
            fixture_mode: false,
            window_placement: placement,
            window_work_area: work_area,
            #[cfg(windows)]
            native_handle,
            last_monitor_geometry: None,
            normal_window: true,
            window_recovered: false,
            startup_connected_logged: false,
            lifecycle: LifecycleState::Starting,
            shutdown_started: None,
            shutdown_completed: false,
        }
    }

    fn palette(&self, ctx: &egui::Context) -> Palette {
        let preference = if self.normal_window {
            theme::Theme::System
        } else {
            self.settings.theme
        };
        let is_dark = theme_is_dark(preference, ctx.system_theme());
        accessible_palette(
            is_dark,
            self.applied_appearance.unwrap_or_default().contrast,
        )
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let preference = if self.normal_window {
            theme::Theme::System
        } else {
            self.settings.theme
        };
        ctx.set_theme(match preference {
            theme::Theme::System => egui::ThemePreference::System,
            theme::Theme::Dark => egui::ThemePreference::Dark,
            theme::Theme::Light => egui::ThemePreference::Light,
        });
        let snapshot = self.appearance.snapshot();
        if self.applied_appearance != Some(snapshot) {
            ctx.set_zoom_factor(1.0); // OS DPI is supplied by winit; discard stored app zoom.
            ctx.options_mut(|options| options.zoom_with_keyboard = false);
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::unique("system_text_scale"), snapshot.text_scale)
            });
            for dark in [false, true] {
                configure_theme(
                    ctx,
                    accessible_palette(dark, snapshot.contrast),
                    theme::Density::Normal,
                    snapshot.text_scale,
                    snapshot.animations,
                );
            }
            self.applied_appearance = Some(snapshot);
        }
    }

    fn render_recordings_page(
        &mut self,
        ui: &mut egui::Ui,
        frame: Option<Arc<planora_core::TelemetryFrame>>,
        topology: Option<&planora_core::Topology>,
        benchmark: Option<&planora_core::BenchmarkState>,
        diagnostics: &[planora_core::DiagnosticEntry],
        app_state: &AppState,
        p: Palette,
    ) {
        ui.heading("Recordings & Exports");
        ui.label(
            egui::RichText::new("Save measured data locally only when you choose to record.")
                .color(p.secondary),
        );
        ui.add_space(14.0);
        let status = self.recorder.status();
        egui::Frame::group(ui.style()).fill(p.surface).show(ui, |ui| {
            ui.strong("Live recording");
            ui.label("System measurements are saved once per second. A selected process is included only when enabled below.");
            ui.checkbox(&mut self.record_selected_process, "Include selected process name and measurements");
            if status.active {
                ui.label(format!("{} · {} bytes written · {} dropped samples", status.message, status.bytes_written, status.dropped));
                if self.recorder.is_active() && ui.button("Stop Recording").clicked() { self.recorder.stop(); }
            } else if ui.add_enabled(*app_state == AppState::Live, egui::Button::new("Start Recording")).clicked() {
                match self.recorder.start(topology.cloned(), self.record_selected_process, state::BUILD_INFO.version, state::BUILD_INFO.git_commit) {
                    Ok(path) => { self.replay_input = path.display().to_string(); self.recording_error = None; },
                    Err(e) => self.recording_error = Some(format!("Could not start recording: {e}")),
                }
            }
            if let Some(path) = &status.path { ui.label(format!("File: {}", path.display())); }
            if !status.active && !status.message.is_empty() { ui.label(&status.message); }
        });
        ui.add_space(12.0);
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .show(ui, |ui| {
                ui.strong("Replay a recording");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.replay_input)
                            .hint_text("Path to a .planora.jsonl file")
                            .desired_width((ui.available_width() - 190.0).max(160.0)),
                    );
                    if ui
                        .add_enabled(!status.active, egui::Button::new("Open Recording"))
                        .clicked()
                    {
                        match self.replay.start(
                            std::path::PathBuf::from(self.replay_input.trim()),
                            ui.ctx().clone(),
                        ) {
                            Ok(()) => {
                                self.reset_history();
                                self.replay_anchor = None;
                                self.display_pause = DisplayPause::default();
                                self.recording_error = None;
                                self.page = Page::Performance;
                            }
                            Err(e) => {
                                self.recording_error =
                                    Some(format!("Could not open recording: {e}"))
                            }
                        }
                    }
                    if self.replay.is_active() && ui.button("Return to Live").clicked() {
                        self.replay.stop();
                        self.reset_history();
                        self.replay_anchor = None;
                    }
                });
                ui.label(&self.replay.message);
                if let Ok(dir) = recording::recordings_dir() {
                    let mut recent: Vec<_> = std::fs::read_dir(&dir)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
                        .collect();
                    recent.sort_unstable_by(|a, b| b.cmp(a));
                    for path in recent.into_iter().take(8) {
                        if ui
                            .button(path.file_name().unwrap_or_default().to_string_lossy())
                            .clicked()
                        {
                            self.replay_input = path.display().to_string();
                        }
                    }
                }
            });
        if let Some(error) = &self.recording_error {
            ui.colored_label(p.error, error);
        }
        ui.add_space(12.0);
        egui::Frame::group(ui.style()).fill(p.surface).show(ui, |ui| {
            ui.strong("Exports");
            ui.label("Exports use the current snapshot or the latest benchmark. Diagnostic messages are redacted by default.");
            ui.horizontal_wrapped(|ui| {
                if let Some(frame) = frame.clone()
                    && ui.button("Export Process Table CSV").clicked()
                {
                    let table = &self.processes_state.table_state;
                    if let Err(error) = self.exporter.processes(frame, table.search_query.clone(), table.sort_column, table.sort_descending) {
                        self.recording_error = Some(format!("Process export failed: {error}"));
                    }
                }
                if ui.button("Export Visible History CSV").clicked() {
                    let ranges = [self.overview_state.range; 4];
                    if let Err(error) = self.exporter.charts(self.histories.as_ref().clone(), ranges, Instant::now()) {
                        self.recording_error = Some(format!("History export failed: {error}"));
                    }
                }
                if let Some(benchmark) = benchmark
                    && ui.button("Export Benchmark JSON").clicked()
                    && let Err(error) = self.exporter.benchmark(benchmark.clone(), topology.cloned())
                {
                    self.recording_error = Some(format!("Benchmark export failed: {error}"));
                }
                if ui.button("Export Redacted Diagnostics JSON").clicked()
                    && let Err(error) = self.exporter.diagnostics(diagnostics.to_vec())
                {
                    self.recording_error = Some(format!("Diagnostics export failed: {error}"));
                }
            });
            let status = self.exporter.status();
            if !status.message.is_empty() { ui.label(status.message); }
            if let Some(path) = status.path { ui.label(format!("File: {}", path.display())); }
        });
    }

    pub fn begin_shutdown(&mut self, ctx: &egui::Context) {
        if self.lifecycle == LifecycleState::Stopping {
            return;
        }
        self.lifecycle = LifecycleState::Stopping;
        self.shutdown_started = Some(Instant::now());
        startup::record("shutdown-begin");

        if let Ok(guard) = self.shared.lock() {
            if let Some(planora_core::BenchmarkState::Running { job_id, .. }) =
                guard.benchmark_state
            {
                let _ = self.tx.send(Command::CancelBenchmark { job_id });
            }
            if let Some(planora_core::ProfileState::Running { job_id, .. }) = guard.profile_state {
                let _ = self.tx.send(Command::CancelProfile { job_id });
            }
        }
        self.client_stop.store(true, Ordering::Release);
        if let Some(worker) = &self.client_worker {
            worker.thread().unpark();
        }
        #[cfg(windows)]
        {
            if let Some(stop) = &self.sensor_stop {
                stop.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if let Some(worker) = &self.sensor_worker {
                worker.thread().unpark();
            }
        }
        self.history_worker.stop();
        self.appearance.request_stop();
        self.recorder.request_stop();
        self.exporter.request_cancel();
        self.replay.stop();
        #[cfg(windows)]
        if let Some(mut child) = self.authorization_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        if self.normal_window {
            if let Err(error) = window_placement::save(&self.window_placement) {
                startup::record(&format!("placement-save-error {error}"));
            }
            self.settings_error = self
                .settings
                .save()
                .err()
                .map(|e| format!("Could not save preferences: {e}"));
            if let Some(error) = &self.settings_error {
                startup::record(&format!("settings-save-error {error}"));
            }
        }

        ctx.request_repaint();
    }

    pub fn update_shutdown(&mut self, ctx: &egui::Context) {
        let client_done = self.client_worker.as_ref().is_none_or(|w| w.is_finished());
        let history_done = self.history_worker.is_finished();
        #[cfg(windows)]
        let sensor_done = self.sensor_worker.as_ref().is_none_or(|w| w.is_finished());
        #[cfg(not(windows))]
        let sensor_done = true;
        let recording_done = self.recorder.is_finished();
        let export_done = self.exporter.is_finished();

        let all_done = client_done
            && history_done
            && sensor_done
            && recording_done
            && export_done
            && self.appearance.is_finished();
        if all_done {
            self.appearance.join();
            if let Some(worker) = self.client_worker.take() {
                let _ = worker.join();
            }
            self.history_worker.join();
            #[cfg(windows)]
            if let Some(worker) = self.sensor_worker.take() {
                let _ = worker.join();
            }
            self.recorder.join();
            self.exporter.cancel();
            self.shutdown_completed = true;
            startup::record("shutdown-complete");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        ctx.request_repaint_after(std::time::Duration::from_millis(50));

        let elapsed = self
            .shutdown_started
            .map_or(std::time::Duration::ZERO, |t| t.elapsed());
        if elapsed >= std::time::Duration::from_secs(5) {
            egui::Window::new("Planora Shutdown")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading("Waiting for operations to complete...");
                    ui.add_space(8.0);
                    if !recording_done {
                        ui.label("• Finalizing recording output to disk...");
                    }
                    if !export_done {
                        ui.label("• Finalizing export file...");
                    }
                    if !client_done {
                        ui.label("• Closing telemetry connection...");
                    }
                    if !history_done {
                        ui.label("• Finalizing history buffer...");
                    }
                    #[cfg(windows)]
                    if !sensor_done {
                        ui.label("• Stopping sensor worker...");
                    }
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui.button("Keep waiting").clicked() {
                            self.shutdown_started = Some(Instant::now());
                        }
                        if ui
                            .button("Exit without completing pending writes")
                            .clicked()
                        {
                            startup::record("shutdown-abandon-pending-writes");
                            self.client_worker.take();
                            #[cfg(windows)]
                            self.sensor_worker.take();
                            self.shutdown_completed = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
        }
    }

    pub fn render_ui(&mut self, root: &mut egui::Ui) {
        if self.lifecycle == LifecycleState::Starting {
            self.lifecycle = LifecycleState::Running;
        }

        let close_requested = root.ctx().input(|i| i.viewport().close_requested());
        if close_requested && self.lifecycle == LifecycleState::Running {
            self.begin_shutdown(root.ctx());
        }

        if self.lifecycle == LifecycleState::Stopping {
            self.update_shutdown(root.ctx());
            return;
        }

        if let Some((start, duration)) = self.auto_close
            && start.elapsed() >= duration
        {
            self.begin_shutdown(root.ctx());
            return;
        }

        if self.normal_window && self.frames_rendered == 0 {
            let viewport = root.ctx().input(|input| input.viewport().clone());
            startup::record(&format!(
                "first-painted-frame inner={:?} outer={:?} scale={:?} maximized={:?}",
                viewport.inner_rect.map(|r| r.size()),
                viewport.outer_rect.map(|r| r.size()),
                viewport.native_pixels_per_point,
                viewport.maximized,
            ));
        }
        if self.normal_window {
            let viewport = root.ctx().input(|input| input.viewport().clone());
            let geometry = (viewport.outer_rect, viewport.native_pixels_per_point);
            if self.last_monitor_geometry != Some(geometry) {
                #[cfg(windows)]
                if self.native_handle != 0 {
                    self.window_work_area = window_placement::window_work_area(self.native_handle);
                    let width = self.window_work_area.max[0] - self.window_work_area.min[0];
                    let height = self.window_work_area.max[1] - self.window_work_area.min[1];
                    root.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::new(
                            window_placement::MIN_SIZE[0].min(width),
                            window_placement::MIN_SIZE[1].min(height),
                        )));
                }
                self.last_monitor_geometry = Some(geometry);
            }
            if self.frames_rendered >= 2
                && !self.window_recovered
                && viewport.minimized != Some(true)
                && let Some(rect) = viewport.inner_rect
                && (rect.width() < self.window_placement.normal_size[0] * 0.70
                    || rect.height() < self.window_placement.normal_size[1] * 0.70)
            {
                startup::record(&format!(
                    "window-recovery inner={:?} expected={:?}",
                    rect.size(),
                    self.window_placement.normal_size,
                ));
                root.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(
                        self.window_placement.expected_size(),
                    ));
                self.window_recovered = true;
            }
            if self.frames_rendered >= 3 {
                self.window_placement
                    .observe(&viewport, self.window_work_area);
            }
        }
        #[cfg(windows)]
        if let Some(child) = self.authorization_child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.authorization_error = if status.success() {
                        None
                    } else {
                        Some(format!(
                            "Authorization command exited with {status}. Check the administrator prompt and retry."
                        ))
                    };
                    self.authorization_child = None;
                }
                Ok(None) => root
                    .ctx()
                    .request_repaint_after(std::time::Duration::from_millis(250)),
                Err(error) => {
                    self.authorization_error =
                        Some(format!("Could not check authorization: {error}"));
                    self.authorization_child = None;
                }
            }
        }
        if let Some((start, dur)) = self.auto_close
            && start.elapsed() >= dur
        {
            if self.screenshot_out.is_some() || self.screenshot_pending {
                eprintln!(
                    "Native screenshot timed out ({} frames rendered)",
                    self.frames_rendered
                );
                std::process::exit(1);
            }
            root.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let ctx = root.ctx().clone();

        self.frames_rendered = self.frames_rendered.saturating_add(1);
        if self.screenshot_out.is_some() || self.screenshot_pending {
            ctx.request_repaint();
        }
        if self.frames_rendered >= 3
            && self
                .auto_close
                .is_none_or(|(start, _)| start.elapsed() >= std::time::Duration::from_secs(2))
            && let Some(path) = self.screenshot_out.take()
        {
            self.screenshot_pending = true;
            ctx.request_screenshot(move |img| {
                let mut raw = Vec::with_capacity(img.pixels.len() * 4);
                for p in &img.pixels {
                    raw.push(p.r());
                    raw.push(p.g());
                    raw.push(p.b());
                    raw.push(p.a());
                }
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let result = image::save_buffer(
                    &path,
                    &raw,
                    img.size[0] as u32,
                    img.size[1] as u32,
                    image::ExtendedColorType::Rgba8,
                );
                match result {
                    Ok(()) => std::process::exit(0),
                    Err(error) => {
                        eprintln!(
                            "Failed to save native screenshot {}: {error}",
                            path.display()
                        );
                        std::process::exit(1);
                    }
                }
            });
        }

        self.apply_theme(&ctx);
        let p = self.palette(&ctx);

        self.replay.poll();
        let (mut app_state, mut frame_opt, mut topo_opt, mut caps, shared_copy) = {
            let shared = self.shared.lock().unwrap();
            let state = shared.state.clone().unwrap_or(AppState::Connecting);
            let frame = shared.frame.clone();
            let topo = shared.topology.clone();
            let caps = shared.capabilities.clone();
            let shared_copy = Shared {
                state: shared.state.clone(),
                last_frame_received: shared.last_frame_received,
                frame: shared.frame.clone(),
                topology: shared.topology.clone(),
                capabilities: shared.capabilities.clone(),
                #[cfg(windows)]
                sensor_readings: shared.sensor_readings.clone(),
                #[cfg(windows)]
                sensor_capabilities: shared.sensor_capabilities.clone(),
                benchmark_state: shared.benchmark_state.clone(),
                profile_state: shared.profile_state.clone(),
                sampling_effective_ms: shared.sampling_effective_ms,
                diagnostic_logs: shared.diagnostic_logs.clone(),
                affinity: shared.affinity.clone(),
                action_error: shared.action_error.clone(),
                live_history_enabled: shared.live_history_enabled,
                history_epoch: shared.history_epoch,
                selected_identity: shared.selected_identity.clone(),
                priority_details: shared.priority_details.clone(),
            };
            (state, frame, topo, caps, shared_copy)
        };
        #[cfg(windows)]
        caps.extend(shared_copy.sensor_capabilities.iter().cloned());

        if self.replay.is_active() {
            app_state = AppState::Replay;
            frame_opt = self.replay.frame.clone();
            topo_opt = self.replay.topology.clone();
        }
        #[cfg(windows)]
        let live_sensors = {
            let helper_sensors = frame_opt
                .as_ref()
                .map(|frame| frame.sensors.as_slice())
                .unwrap_or_default();
            if !self.fixture_mode && app_state != AppState::Replay {
                Arc::new(merge_sensor_readings(
                    helper_sensors,
                    &shared_copy.sensor_readings,
                ))
            } else {
                Arc::new(helper_sensors.to_vec())
            }
        };
        if app_state == AppState::Live
            && let Some(frame) = &frame_opt
        {
            #[cfg(windows)]
            let recording_sensors = live_sensors.as_ref();
            #[cfg(not(windows))]
            let recording_sensors = &frame.sensors;
            self.recorder
                .record(frame, self.selected.as_ref(), recording_sensors);
        }
        if self.normal_window && !self.startup_connected_logged {
            match &app_state {
                AppState::Live => {
                    startup::record("helper-connection result=connected");
                    self.startup_connected_logged = true;
                }
                AppState::AwaitingAuthorization => {
                    startup::record("helper-connection result=awaiting-authorization");
                    self.startup_connected_logged = true;
                }
                AppState::IncompatibleVersion { helper_version } => {
                    startup::record(&format!(
                        "helper-connection result=incompatible-version helper_version={helper_version}"
                    ));
                    self.startup_connected_logged = true;
                }
                AppState::Failed(msg) => {
                    startup::record(&format!("helper-connection result=failed message={msg}"));
                    self.startup_connected_logged = true;
                }
                _ => {}
            }
        }

        // Evaluate staleness if in Live state
        let current_poll = self.poll_ms.load(Ordering::Relaxed);
        if !self.fixture_mode
            && app_state == AppState::Live
            && let Some(elapsed) = is_data_stale(shared_copy.last_frame_received, current_poll)
        {
            app_state = AppState::Stale(elapsed);
        }

        // The history worker owns aggregation and publishes a bounded immutable snapshot.
        let now = Instant::now();
        let visual_now = if app_state == AppState::Replay {
            frame_opt.as_ref().map_or(now, |frame| {
                let anchor = self.replay_anchor.get_or_insert((frame.sampled_at_ms, now));
                if frame.sampled_at_ms < anchor.0 {
                    *anchor = (frame.sampled_at_ms, now);
                }
                anchor
                    .1
                    .checked_add(std::time::Duration::from_millis(
                        frame.sampled_at_ms.saturating_sub(anchor.0),
                    ))
                    .unwrap_or(now)
            })
        } else {
            now
        };
        let is_replay_or_fixture = app_state == AppState::Replay || self.fixture_mode;
        if is_replay_or_fixture {
            if let Some(frame) = &frame_opt {
                let key = (frame.session_id, frame.sequence, self.selected.clone());
                if self.history_last_enqueued.as_ref() != Some(&key) {
                    #[cfg(windows)]
                    let sensors = live_sensors.clone();
                    #[cfg(not(windows))]
                    let sensors = Arc::new(frame.sensors.clone());
                    self.history_worker.submit(HistoryInput {
                        frame: frame.clone(),
                        priority_details: self
                            .resources_state
                            .priority_history_keys(sensors.as_ref()),
                        sensors,
                        selected: self.selected.clone(),
                        received_at: visual_now,
                        wall_clock_at_receipt_ms: if app_state == AppState::Replay {
                            frame.sampled_at_ms
                        } else {
                            planora_telemetry::unix_ms()
                        },
                        epoch: self.history_epoch,
                    });
                    self.history_last_enqueued = Some(key);
                }
            }
        } else {
            let priority = self
                .resources_state
                .priority_history_keys(live_sensors.as_ref());
            let mut shared = self.shared.lock().unwrap();
            shared.live_history_enabled = true;
            shared.history_epoch = self.history_epoch;
            shared.selected_identity = self.selected.clone();
            shared.priority_details = priority;
        }
        if let Some(snapshot) = self.history_worker.latest(self.history_epoch) {
            self.histories = snapshot;
        }

        // Display Pause handling
        if self.display_pause.is_paused {
            if self.display_pause.frozen_frame.is_none() {
                self.display_pause.frozen_frame = frame_opt.clone();
                self.display_pause.frozen_instant = Some(visual_now);
                self.display_pause.frozen_histories = Some(self.histories.clone());
                #[cfg(windows)]
                {
                    self.display_pause.frozen_sensors = Some(live_sensors.clone());
                }
            }
        } else {
            self.display_pause.frozen_frame = None;
            self.display_pause.frozen_instant = None;
            self.display_pause.frozen_histories = None;
            self.display_pause.frozen_sensors = None;
        }

        let render_frame: Option<Arc<planora_core::TelemetryFrame>> =
            if self.display_pause.is_paused {
                self.display_pause
                    .frozen_frame
                    .clone()
                    .or_else(|| frame_opt.clone())
            } else {
                frame_opt.clone()
            };

        let render_now = if self.display_pause.is_paused {
            self.display_pause.frozen_instant.unwrap_or(visual_now)
        } else {
            visual_now
        };
        // Snapshot the chart buffers as well as the table frame when paused.
        // Live collection continues in self.histories without mutating the display.
        // Clone only the Arc handle so page controls can mutably borrow pause state.
        let frozen_histories = self.display_pause.frozen_histories.clone();
        #[cfg(windows)]
        let frozen_sensors = self.display_pause.frozen_sensors.clone();

        // At compact widths, preserve the full content area with an icon rail.
        let compact_nav = root.available_width() < 1180.0;
        egui::Panel::left("nav_sidebar")
            .exact_size(if compact_nav { 64.0 } else { SIDEBAR_WIDTH })
            .show(root, |ui| {
                ui.add_space(14.0);

                // Official Planora Logo
                ui.horizontal(|ui| {
                    if compact_nav {
                        ui.add_space(8.0);
                        self.logo_cache.render_mark(ui, 36.0);
                    } else {
                        ui.add_space(12.0);
                        self.logo_cache.render(ui, p.is_dark, 168.0);
                    }
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // Navigation items with clean icons
                let nav_items = [
                    (
                        Page::Performance,
                        "Performance",
                        icons::draw_resources as fn(&egui::Painter, Rect, Color32),
                    ),
                    (
                        Page::Overview,
                        "Overview",
                        icons::draw_overview as fn(&egui::Painter, Rect, Color32),
                    ),
                    (Page::Processes, "Processes", icons::draw_processes),
                    (Page::History, "History", icons::draw_history),
                    (Page::Hardware, "Hardware", icons::draw_hardware),
                    (Page::Benchmarks, "Benchmarks", icons::draw_benchmarks),
                    (Page::Capabilities, "Capabilities", icons::draw_capabilities),
                    (Page::Recordings, "Recordings", icons::draw_recordings),
                    (Page::Settings, "Settings", icons::draw_settings),
                ];

                for (page, label, draw_icon) in nav_items {
                    let is_active = self.page == page;
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 36.0),
                        egui::Sense::click(),
                    );
                    let painter = ui.painter();

                    if is_active {
                        painter.rect_filled(rect, CornerRadius::same(6), p.selected);
                        painter.rect_stroke(
                            rect,
                            CornerRadius::same(6),
                            Stroke::new(1.0, p.primary),
                            egui::StrokeKind::Inside,
                        );
                    } else if resp.hovered() {
                        painter.rect_filled(rect, CornerRadius::same(6), p.hover);
                    }

                    let icon_rect = Rect::from_min_size(
                        rect.min
                            + Vec2::new(
                                if compact_nav {
                                    (rect.width() - 18.0) / 2.0
                                } else {
                                    12.0
                                },
                                9.0,
                            ),
                        Vec2::new(18.0, 18.0),
                    );
                    draw_icon(
                        painter,
                        icon_rect,
                        if is_active { p.primary } else { p.secondary },
                    );

                    if compact_nav {
                        resp.clone().on_hover_text(label);
                    } else {
                        painter.text(
                            Pos2::new(rect.min.x + 38.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(13.5),
                            if is_active { p.text } else { p.secondary },
                        );
                    }

                    if resp.clicked() {
                        self.page = page;
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.add_space(12.0);
                    if compact_nav {
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::new(36.0, 30.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            rect.center(),
                            5.0,
                            if app_state == AppState::Live {
                                p.success
                            } else {
                                p.warning
                            },
                        );
                        response.on_hover_text(format!(
                            "{}{:?}",
                            if self.display_pause.is_paused {
                                "Display paused · "
                            } else {
                                ""
                            },
                            app_state
                        ));
                    } else {
                        render_compact_sidebar_status(
                            ui,
                            &app_state,
                            self.display_pause.is_paused,
                            p,
                        );
                    }
                    ui.separator();
                });
            });

        // 2. Central Panel
        egui::CentralPanel::default().show(root, |ui| {
            if self.fixture_mode {
                ui.colored_label(p.warning, "Fixture preview · simulated sample data");
            }
            if self.page == Page::Recordings {
                show_page_scroll(ui, "recordings_page_scroll", |ui| {
                    self.render_recordings_page(
                        ui,
                        frame_opt.clone(),
                        topo_opt.as_ref(),
                        shared_copy.benchmark_state.as_ref(),
                        &shared_copy.diagnostic_logs,
                        &app_state,
                        p,
                    );
                });
                return;
            }
            let render_histories = frozen_histories.as_deref().unwrap_or(self.histories.as_ref());
            if matches!(self.page, Page::Performance | Page::History)
                && self.history_worker.dropped() > 0
            {
                ui.colored_label(p.warning, format!("History worker skipped {} delivery frames; coverage uses only observed samples.", self.history_worker.dropped()));
            }
            // Action error banner
            if let Some(err_msg) = &shared_copy.action_error {
                egui::Frame::group(ui.style())
                    .fill(p.surface_raised)
                    .stroke(Stroke::new(1.0, p.error))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(p.error, format!("Action Error: {err_msg}"));
                            if ui.button("Dismiss [X]").clicked() {
                                self.shared.lock().unwrap().action_error = None;
                            }
                        });
                    });
                ui.add_space(8.0);
            }

            // Connection state views
            match &app_state {
                AppState::AwaitingAuthorization => {
                    ui.add_space(32.0);
                    let card_width = ui.available_width().min(720.0);
                    let inset = ((ui.available_width() - card_width) / 2.0).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(inset);
                        ui.vertical(|ui| {
                            ui.set_width(card_width);
                            egui::Frame::group(ui.style())
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0, p.border))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::same(24))
                            .show(ui, |ui| {
                                ui.colored_label(p.warning, egui::RichText::new("SETUP REQUIRED").strong().size(11.0));
                                ui.add_space(6.0);
                                ui.heading("Connect the local helper");
                                #[cfg(windows)]
                                ui.label("Planora could not connect to its installed helper. Windows approval is requested before this window opens when needed. If approval was declined or the helper still cannot start, retry below. The main window stays at normal user privileges.");
                                #[cfg(not(windows))]
                                ui.label("Planora could not connect to its installed helper. Authorize or restart it for this account to read system and process measurements. The main window stays at normal user privileges.");
                                ui.add_space(18.0);
                                egui::Grid::new("authorization_steps")
                                    .num_columns(2)
                                    .spacing([18.0, 12.0])
                                    .show(ui, |ui| {
                                        for (step, detail) in [
                                            ("1  Install", "Install Planora and its local helper using the platform package."),
                                            ("2  Authorize", "Approve your platform's helper authorization for this account."),
                                            ("3  Connect", "Planora will reconnect and begin collecting measurements."),
                                        ] {
                                            ui.strong(step);
                                            ui.label(detail);
                                            ui.end_row();
                                        }
                                    });
                                ui.add_space(20.0);
                                #[cfg(windows)]
                                {
                                    let authorization_script_available =
                                        startup_authorization::trusted_script().is_some();
                                    if ui.add_enabled(
                                        self.authorization_child.is_none() && authorization_script_available,
                                        egui::Button::new("Retry administrator approval…").min_size(Vec2::new(220.0, 36.0)),
                                    ).clicked() {
                                        self.launch_helper_script("Authorize-Planora.ps1");
                                    }
                                    if !authorization_script_available {
                                        ui.colored_label(p.warning, "Install Planora with Setup to enable helper authorization.");
                                    }
                                    if self.authorization_child.is_some() {
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.label("Waiting for administrator approval and helper restart…");
                                        });
                                    }
                                    if let Some(error) = &self.authorization_error {
                                        ui.colored_label(p.error, error);
                                    }
                                }
                                #[cfg(target_os = "macos")]
                                if ui.button("Request macOS daemon approval…").clicked()
                                    && unsafe { planora_register_daemon() } != 0
                                {
                                    ui.colored_label(p.error, "Registration failed; check Console.app for details.");
                                }
                                ui.add_space(14.0);
                                ui.colored_label(p.secondary, "Only local IPC is used. No network listener or automatic recording is started.");
                            });
                        });
                    });
                    return;
                }
                AppState::IncompatibleVersion { helper_version } => {
                    ui.add_space(32.0);
                    let card_width = ui.available_width().min(720.0);
                    let inset = ((ui.available_width() - card_width) / 2.0).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(inset);
                        ui.vertical(|ui| {
                            ui.set_width(card_width);
                            egui::Frame::group(ui.style())
                                .fill(p.surface)
                                .stroke(Stroke::new(1.0, p.error))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(egui::Margin::same(24))
                                .show(ui, |ui| {
                                    ui.colored_label(p.error, egui::RichText::new("VERSION MISMATCH").strong().size(11.0));
                                    ui.add_space(6.0);
                                    ui.heading("Planora and its helper need the same version");
                                    ui.label("The running helper cannot supply data to this version of Planora. Install the matching package, then reopen the application.");
                                    ui.add_space(18.0);
                                    egui::Grid::new("helper_version_details")
                                        .num_columns(2)
                                        .spacing([24.0, 10.0])
                                        .show(ui, |ui| {
                                            ui.label("Application protocol");
                                            ui.monospace(format!("v{PROTOCOL_VERSION}"));
                                            ui.end_row();
                                            ui.label("Running helper protocol");
                                            ui.monospace(format!("v{helper_version}"));
                                            ui.end_row();
                                        });
                                    ui.add_space(16.0);
                                    ui.colored_label(p.secondary, "Monitoring will resume after the installed helper matches this application.");
                                });
                        });
                    });
                    return;
                }
                AppState::Failed(msg) if render_frame.is_none() => {
                    ui.heading("Unable to Connect to Local Helper");
                    ui.colored_label(p.error, msg);
                    ui.add_space(8.0);
                    if ui.button("Retry Connection").clicked() {
                        self.shared.lock().unwrap().state = Some(AppState::Connecting);
                    }
                    return;
                }
                AppState::Connecting if render_frame.is_none() => {
                    ui.spinner();
                    ui.label("Connecting to Planora local helper daemon...");
                    return;
                }
                _ => {}
            }

            let Some(frame) = render_frame.as_ref() else {
                ui.spinner();
                ui.label("Waiting for initial telemetry frame...");
                return;
            };

            // Dispatch active page
            match self.page {
                Page::Overview => {
                    // Overview has multiple stacked charts and a process table;
                    // always make the whole page scroll at the minimum window size.
                    show_page_scroll(ui, "overview_page_scroll", |ui| {
                        let previous_selected = self.resources_state.selected_resource.clone();
                        render_overview_view(
                            ui,
                            frame,
                            render_histories,
                            &mut self.overview_state,
                            &app_state,
                            &mut self.display_pause,
                            current_poll,
                            &mut self.selected,
                            &mut self.resources_state.selected_resource,
                            &mut self.page,
                            render_now,
                            p,
                        );
                        if self.resources_state.selected_resource != previous_selected {
                            self.resources_state.reveal_selected_resource = true;
                            self.settings.performance_resource =
                                self.resources_state.selected_resource.clone();
                            self.settings_error = self
                                .settings
                                .save()
                                .err()
                                .map(|error| format!("Could not save preferences: {error}"));
                        }
                    });
                },
                Page::Performance => {
                    let previous_graph_modes = self.settings.resource_graphs;
                    let previous_legends = self.settings.resource_legends;
                    let previous_scale = self.settings.percent_scale;
                            render_resources_view(
                                ui,
                                frame,
                                render_histories,
                                {
                                    #[cfg(windows)]
                                    { frozen_sensors.as_deref().unwrap_or(live_sensors.as_ref()) }
                                    #[cfg(not(windows))]
                                    { &frame.sensors }
                                },
                                topo_opt.as_ref(),
                                Some(&mut self.exporter),
                                &mut self.resources_state,
                                &mut self.settings.percent_scale,
                                &app_state,
                                &mut self.display_pause,
                                current_poll,
                                &mut self.settings.resource_graphs,
                                &mut self.settings.resource_legends,
                                render_now,
                                p,
                            );
                    let selection_changed = self.settings.performance_resource != self.resources_state.selected_resource
                        || self.settings.performance_range != self.resources_state.range
                        || self.settings.performance_engines.iter().cloned().collect::<std::collections::BTreeSet<_>>() != self.resources_state.selected_engines;
                    if self.settings.resource_graphs != previous_graph_modes || self.settings.resource_legends != previous_legends || self.settings.percent_scale != previous_scale || selection_changed {
                        self.settings.performance_resource = self.resources_state.selected_resource.clone();
                        self.settings.performance_range = self.resources_state.range;
                        self.settings.performance_engines = self.resources_state.selected_engines.iter().take(4).cloned().collect();
                        self.settings_error = self.settings.save().err().map(|error| format!("Could not save preferences: {error}"));
                    }
                },
                Page::Processes => {
                    render_processes_view(
                        ui,
                        frame,
                        render_histories,
                        &mut self.processes_state,
                        &app_state,
                        &mut self.display_pause,
                        current_poll,
                        &mut self.selected,
                        shared_copy.affinity.as_ref(),
                        topo_opt.as_ref(),
                        &self.tx,
                        render_now,
                        p,
                    );
                    let table = &self.processes_state.table_state;
                    if self.settings.process_columns != table.columns
                        || self.settings.process_sort_column != table.sort_column
                        || self.settings.process_sort_descending != table.sort_descending
                    {
                        self.settings.process_columns = table.columns;
                        self.settings.process_sort_column = table.sort_column;
                        self.settings.process_sort_descending = table.sort_descending;
                        self.settings_error = self.settings.save().err().map(|e| format!("Could not save preferences: {e}"));
                    }
                },
                Page::History => {
                    let previous_scale = self.settings.percent_scale;
                    show_page_scroll(ui, "history_page_scroll", |ui| {
                        render_history_view(
                            ui,
                            render_histories,
                            &mut self.history_state,
                            &mut self.settings.percent_scale,
                            &app_state,
                            &mut self.display_pause,
                            current_poll,
                            render_now,
                            p,
                        );
                    });
                    if self.settings.percent_scale != previous_scale {
                        self.settings_error = self.settings.save().err().map(|e| format!("Could not save preferences: {e}"));
                    }
                },
                Page::Hardware => {
                    show_page_scroll(ui, "hardware_page_scroll", |ui| {
                        render_hardware_view(
                            ui,
                            topo_opt.as_ref(),
                            render_frame.as_deref(),
                            &mut self.hardware_state,
                            &app_state,
                            &mut self.display_pause,
                            current_poll,
                            p,
                        );
                    });
                }
                Page::Benchmarks => {
                    show_page_scroll(ui, "benchmarks_page_scroll", |ui| {
                        render_benchmarks_view(
                            ui,
                            shared_copy.benchmark_state.as_ref(),
                            shared_copy.profile_state.as_ref(),
                            self.selected.as_ref(),
                            &mut self.benchmarks_state,
                            &app_state,
                            &mut self.display_pause,
                            current_poll,
                            &self.tx,
                            p,
                        );
                    });
                },
                Page::Capabilities => render_capabilities_view(
                    ui,
                    &caps,
                    &mut self.capabilities_state,
                    &app_state,
                    &mut self.display_pause,
                    current_poll,
                    &self.tx,
                    {
                        #[cfg(windows)]
                        { self.sensor_refresh.as_ref() }
                        #[cfg(not(windows))]
                        { None }
                    },
                    p,
                ),
                Page::Settings => {
                    show_page_scroll(ui, "settings_outer_page_scroll", |ui| {
                        render_settings_view(
                            ui, &mut self.settings, &mut self.settings_error, &mut self.settings_state, &self.poll_ms,
                            shared_copy.sampling_effective_ms, &shared_copy.diagnostic_logs,
                            &app_state, &mut self.display_pause, &self.tx, p,
                        );
                        #[cfg(windows)] {
                            ui.separator();
                            ui.strong("Helper authorization");
                            ui.label("Revoking stops the local helper until an administrator authorizes it again.");
                            if ui.add_enabled(self.authorization_child.is_none(), egui::Button::new("Revoke Helper Authorization")).clicked() {
                                self.launch_helper_script("Revoke-Planora.ps1");
                            }
                            if self.authorization_child.is_some() { ui.label("Waiting for administrator approval…"); }
                            if let Some(error) = &self.authorization_error { ui.colored_label(p.error, error); }
                        }
                    });
                },
                Page::Recordings => unreachable!("recordings page rendered before frame selection"),
            }
        });
        if self.normal_window && self.settings_state.reset_window_requested {
            self.settings_state.reset_window_requested = false;
            self.window_placement = Placement::default().validate(self.window_work_area);
            if let Some(path) = window_placement::path() {
                let _ = std::fs::remove_file(path);
            }
            let size = self.window_placement.expected_size();
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(
                self.window_work_area.min[0]
                    + ((self.window_work_area.max[0] - self.window_work_area.min[0] - size.x)
                        / 2.0)
                        .max(0.0),
                self.window_work_area.min[1]
                    + ((self.window_work_area.max[1] - self.window_work_area.min[1] - size.y)
                        / 2.0)
                        .max(0.0),
            )));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.render_ui(root);
    }

    fn on_exit(&mut self) {
        if self.normal_window {
            if let Err(error) = window_placement::save(&self.window_placement) {
                startup::record(&format!("placement-save-error {error}"));
            }
            self.settings_error = self
                .settings
                .save()
                .err()
                .map(|e| format!("Could not save preferences: {e}"));
            if let Some(error) = &self.settings_error {
                startup::record(&format!("settings-save-error {error}"));
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if !self.shutdown_completed {
            self.client_stop.store(true, Ordering::Release);
            self.recorder.stop();
            self.replay.stop();
            self.exporter.cancel();
            if let Some(worker) = self.client_worker.take() {
                worker.thread().unpark();
                let _ = worker.join();
            }
            self.history_worker.join();
            #[cfg(windows)]
            {
                if let Some(stop) = &self.sensor_stop {
                    stop.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                if let Some(worker) = self.sensor_worker.take() {
                    worker.thread().unpark();
                    let _ = worker.join();
                }
                if let Some(mut child) = self.authorization_child.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    if std::env::args().any(|a| a == "--sensor-worker") {
        let expected = std::env::args()
            .position(|a| a == "--build-id")
            .and_then(|idx| std::env::args().nth(idx + 1));
        planora_telemetry::vendor_sensors::run_worker(expected.as_deref());
        return Ok(());
    }

    if std::env::args().any(|a| a == "--build-info") {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        let info = state::BUILD_INFO;
        let val = serde_json::json!({
            "version": info.version,
            "protocolVersion": info.protocol_version,
            "gitCommit": info.git_commit,
            "target": info.target,
            "profile": info.profile,
        });
        let text = serde_json::to_string_pretty(&val).unwrap_or_else(|_| "{}".into());
        println!("{text}");
        if let Some(out_path) = std::env::args()
            .position(|a| a == "--output" || a == "-o")
            .and_then(|idx| std::env::args().nth(idx + 1))
        {
            let _ = std::fs::write(out_path, &text);
        }
        return Ok(());
    }

    if std::env::args().any(|a| a == "--diagnostics") {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        let info = state::BUILD_INFO;
        let log_content = startup::log_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_else(|| "No startup log recorded".into());
        let diag = serde_json::json!({
            "version": info.version,
            "protocolVersion": info.protocol_version,
            "gitCommit": info.git_commit,
            "target": info.target,
            "profile": info.profile,
            "startupLog": log_content,
        });
        let text = serde_json::to_string_pretty(&diag).unwrap_or_else(|_| "{}".into());
        println!("{text}");
        if let Some(out_path) = std::env::args()
            .position(|a| a == "--output" || a == "-o")
            .and_then(|idx| std::env::args().nth(idx + 1))
        {
            let _ = std::fs::write(out_path, &text);
        }
        return Ok(());
    }
    if std::env::args().any(|a| a == "--headless-smoke-test") {
        let out_dir = std::env::args()
            .nth(2)
            .unwrap_or_else(|| "dist/fixture-smoke".into());
        if let Err(e) = smoke_test::run_smoke_suite(std::path::Path::new(&out_dir), true) {
            eprintln!("Visual smoke suite failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    #[cfg(windows)]
    {
        let main_thread = std::thread::current().id();
        std::panic::set_hook(Box::new(move |info| {
            startup::record(&format!("panic {info}"));
            if std::thread::current().id() == main_thread {
                startup::show_fatal("An unexpected error occurred while opening Planora.");
            }
        }));
    }

    let mut initial_page = Page::Performance;
    let mut initial_theme = None;
    let mut width = 1180.0;
    let mut height = 790.0;
    let mut auto_close_ms = None;
    let mut screenshot_out = None;
    let args: Vec<String> = std::env::args().collect();
    let native_fixture = args.iter().any(|arg| arg == "--native-fixture");
    let fixture_tables_only = args.iter().any(|arg| arg == "--tables-only");
    let mut fixture_resource = None;
    let mut fixture_scale = None;
    let mut fixture_text_scale = None;
    let mut fixture_scroll = None;
    let mut replace_existing = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--view" if i + 1 < args.len() => {
                initial_page = match args[i + 1].to_lowercase().as_str() {
                    "performance" | "resources" => Page::Performance,
                    "overview" => Page::Overview,
                    "processes" => Page::Processes,
                    "history" => Page::History,
                    "hardware" => Page::Hardware,
                    "benchmarks" => Page::Benchmarks,
                    "capabilities" => Page::Capabilities,
                    "settings" => Page::Settings,
                    "recordings" => Page::Recordings,
                    _ => Page::Overview,
                };
                i += 2;
            }
            "--theme" if i + 1 < args.len() => {
                initial_theme = match args[i + 1].to_lowercase().as_str() {
                    "dark" => Some(theme::Theme::Dark),
                    "light" => Some(theme::Theme::Light),
                    _ => None,
                };
                i += 2;
            }
            "--width" if i + 1 < args.len() => {
                if let Ok(w) = args[i + 1].parse::<f32>() {
                    width = w;
                }
                i += 2;
            }
            "--height" if i + 1 < args.len() => {
                if let Ok(h) = args[i + 1].parse::<f32>() {
                    height = h;
                }
                i += 2;
            }
            "--auto-close-ms" if i + 1 < args.len() => {
                if let Ok(ms) = args[i + 1].parse::<u64>() {
                    auto_close_ms = Some(ms);
                }
                i += 2;
            }
            "--screenshot-out" if i + 1 < args.len() => {
                screenshot_out = Some(std::path::PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--fixture-scale" | "--fixture-text-scale" | "--fixture-scroll"
                if i + 1 < args.len() =>
            {
                if let Ok(value) = args[i + 1].parse::<f32>()
                    && value.is_finite()
                    && native_fixture
                {
                    match args[i].as_str() {
                        "--fixture-scale" if (1.0..=2.0).contains(&value) => {
                            fixture_scale = Some(value)
                        }
                        "--fixture-text-scale" if (1.0..=2.0).contains(&value) => {
                            fixture_text_scale = Some(value)
                        }
                        "--fixture-scroll" if (0.0..=10000.0).contains(&value) => {
                            fixture_scroll = Some(value)
                        }
                        _ => {}
                    }
                }
                i += 2;
            }
            "--resource" if i + 1 < args.len() => {
                fixture_resource = Some(args[i + 1].clone());
                i += 2;
            }
            "--replace" => {
                replace_existing = true;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    let capture_mode = screenshot_out.is_some();
    #[cfg(windows)]
    let _instance_guard = if capture_mode || native_fixture {
        None
    } else {
        match instance::InstanceGuard::acquire_or_replace(replace_existing) {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => return Ok(()),
            Err(error) => {
                startup::record(&format!("instance-lock-error {error}"));
                startup::show_fatal(&format!("Could not open the application window: {error}"));
                return Ok(());
            }
        }
    };
    #[cfg(windows)]
    if !capture_mode
        && !native_fixture
        && let Some(error) = startup_authorization::prepare_before_window()
    {
        let _ = STARTUP_AUTH_ERROR.set(error);
    }
    // Native capture must honor the requested size, independent of the user's
    // saved window geometry. Give every image an isolated, empty state file.
    let capture_state_path = screenshot_out
        .as_ref()
        .map(|path| path.with_extension("capture.ron"));
    let (placement, work_area) = window_placement::startup_placement();
    let mut viewport = if capture_mode || !cfg!(windows) {
        egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_min_inner_size([1024.0, 640.0])
            .with_title("Planora")
    } else {
        placement.viewport(work_area)
    };
    if let Ok(icon) = image::load_from_memory(include_bytes!(
        "../../../assets/brand/platforms/windows/planora-64.png"
    )) {
        let rgba = icon.to_rgba8();
        viewport = viewport.with_icon(egui::IconData {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        });
    }
    // Eframe reads its old geometry even with persist_window=false. Isolate its
    // other UI state from the legacy app.ron that caused miniature launches.
    let persistence_path = if cfg!(windows) {
        capture_state_path.or_else(|| {
            state::UserSettings::file_path().map(|p| p.with_file_name("egui-state-v2.ron"))
        })
    } else {
        capture_state_path
    };
    #[cfg(windows)]
    let dpi_scale =
        (unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() }.max(96) as f32) / 96.0;
    #[cfg(not(windows))]
    let dpi_scale = 1.0f32;

    if !capture_mode && !native_fixture {
        startup::record(&format!(
            "build-identity version={} commit={} protocol={} target={} profile={}",
            state::BUILD_INFO.version,
            state::BUILD_INFO.git_commit,
            PROTOCOL_VERSION,
            state::BUILD_INFO.target,
            state::BUILD_INFO.profile,
        ));
        startup::record("launch-mode normal");
        startup::record(&format!(
            "monitor-dpi work_area={:.1}x{:.1} dpi_scale={:.2}",
            work_area.max[0] - work_area.min[0],
            work_area.max[1] - work_area.min[1],
            dpi_scale,
        ));
        startup::record(&format!(
            "restored-geometry requested={:.1}x{:.1} maximized={}",
            placement.normal_size[0], placement.normal_size[1], placement.maximized,
        ));
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: !cfg!(windows) && !capture_mode,
        persistence_path,
        ..Default::default()
    };
    let result = eframe::run_native(
        "Planora",
        options,
        Box::new(move |cc| {
            let mut app = if native_fixture {
                smoke_test::create_native_fixture_app(
                    initial_theme.unwrap_or(theme::Theme::Dark),
                    initial_page,
                )
            } else {
                App::new(cc)
            };
            app.normal_window = cfg!(windows) && !capture_mode && !native_fixture;
            app.page = initial_page;
            if let Some(resource) = fixture_resource.as_ref() {
                app.resources_state.selected_resource = Some(resource.clone());
            }
            if native_fixture && fixture_tables_only {
                app.settings.resource_graphs = [false; 6];
                app.history_state.show_graphs = false;
            }
            if (capture_mode || native_fixture)
                && let Some(th) = initial_theme
            {
                app.settings.theme = th;
            }
            if native_fixture {
                if let Some(text_scale) = fixture_text_scale {
                    app.appearance
                        .set_fixture_snapshot(appearance::SystemAppearance {
                            text_scale,
                            ..Default::default()
                        });
                }
                app.apply_theme(&cc.egui_ctx);
                if let Some(scale) = fixture_scale {
                    cc.egui_ctx.set_pixels_per_point(scale);
                }
                if let Some(offset) = fixture_scroll {
                    cc.egui_ctx.data_mut(|data| {
                        data.insert_temp(egui::Id::unique("fixture_scroll_y"), offset)
                    });
                }
            }
            if let Some(ms) = auto_close_ms {
                app.auto_close = Some((Instant::now(), std::time::Duration::from_millis(ms)));
            }
            app.screenshot_out = screenshot_out;
            Ok(Box::new(app))
        }),
    );
    if let Err(error) = &result {
        startup::record(&format!("native-startup-error {error}"));
        #[cfg(windows)]
        startup::show_fatal(&error.to_string());
    }
    result
}

pub use crate::history::merge_sensor_readings;

#[cfg(test)]
mod tests {
    use super::*;
    use components::table::{SortColumn, sort_processes};
    use planora_core::{AvailabilityReason, IdentitySource, Metric, ProcessRecord, TelemetryFrame};
    use state::UserSettings;
    use std::time::Duration;
    use theme::Theme;

    fn test_frame() -> TelemetryFrame {
        let m = Metric::unavailable(
            "bytes",
            "fixture",
            1,
            AvailabilityReason::Unsupported("missing".into()),
        );
        let make = |pid, started, name: &str, cpu| ProcessRecord {
            identity: ProcessIdentity {
                pid,
                started,
                sequence_number: None,
                source: IdentitySource::SysinfoEpochSeconds,
            },
            name: name.into(),
            cpu_percent: cpu,
            memory_bytes: Metric::available(1024, "bytes", "fixture", 1),
            disk_read_bytes_per_sec: m.clone(),
            disk_write_bytes_per_sec: m.clone(),
            io_read_bytes_per_sec: m.clone(),
            io_write_bytes_per_sec: m.clone(),
            gpu_percent: Metric::unavailable(
                "%",
                "fixture",
                1,
                AvailabilityReason::Unsupported("missing".into()),
            ),
            affinity: Metric::unavailable(
                "cpus",
                "fixture",
                1,
                AvailabilityReason::Unsupported("missing".into()),
            ),
        };
        TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            session_id: 1,
            topology_generation: 1,
            sequence: 1,
            sampled_at_ms: 1,
            collection_duration_ms: 0.1,
            cpu_percent: Metric::unavailable(
                "%",
                "fixture",
                1,
                AvailabilityReason::Unsupported("missing".into()),
            ),
            logical_cpus: Vec::new(),
            memory_used_bytes: m.clone(),
            memory_available_bytes: m.clone(),
            memory_total_bytes: m.clone(),
            commit_used_bytes: m.clone(),
            commit_total_bytes: m.clone(),
            disk_read_bytes_per_sec: m.clone(),
            disk_write_bytes_per_sec: m.clone(),
            network_rx_bytes_per_sec: m.clone(),
            network_tx_bytes_per_sec: m.clone(),
            gpu_percent: Metric::unavailable(
                "%",
                "fixture",
                1,
                AvailabilityReason::Unsupported("missing".into()),
            ),
            gpu_adapters: Vec::new(),
            gpu_engines: Vec::new(),
            gpu_engine_unavailable_reason: Some(planora_core::AvailabilityReason::WarmingUp),
            disks: Vec::new(),
            network_interfaces: Vec::new(),
            package_power_watts: Metric::unavailable(
                "W",
                "fixture",
                1,
                AvailabilityReason::Unsupported("missing".into()),
            ),
            sensors: Vec::new(),
            processes: vec![
                make(4, 1, "Alpha", Metric::available(20.0, "%", "fixture", 1)),
                make(
                    4,
                    2,
                    "Alpha",
                    Metric::unavailable(
                        "%",
                        "fixture",
                        1,
                        AvailabilityReason::Unsupported("missing".into()),
                    ),
                ),
                make(7, 1, "Beta", Metric::available(5.0, "%", "fixture", 1)),
            ],
        }
    }

    #[test]
    fn filtering_and_missing_cpu_sort() {
        let f = test_frame();
        let rows = sort_processes(&f.processes, "alpha", SortColumn::Cpu, true);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].identity.started, 1);
        assert!(rows[1].cpu_percent.value.is_none());
    }

    #[test]
    fn unavailable_values_sorted_last_in_both_directions() {
        let f = test_frame();
        // Descending sort: Available (20.0) -> Available (5.0) -> Unavailable (None)
        let desc = sort_processes(&f.processes, "", SortColumn::Cpu, true);
        assert_eq!(desc[0].cpu_percent.value, Some(20.0));
        assert_eq!(desc[1].cpu_percent.value, Some(5.0));
        assert_eq!(desc[2].cpu_percent.value, None);

        // Ascending sort: Available (5.0) -> Available (20.0) -> Unavailable (None)
        let asc = sort_processes(&f.processes, "", SortColumn::Cpu, false);
        assert_eq!(asc[0].cpu_percent.value, Some(5.0));
        assert_eq!(asc[1].cpu_percent.value, Some(20.0));
        assert_eq!(asc[2].cpu_percent.value, None);
    }

    #[test]
    fn sorting_by_name_and_pid() {
        let f = test_frame();
        let rows = sort_processes(&f.processes, "", SortColumn::Name, false);
        assert_eq!(rows[0].name, "Alpha");
        assert_eq!(rows[2].name, "Beta");

        let rows_pid = sort_processes(&f.processes, "", SortColumn::Pid, true);
        assert_eq!(rows_pid[0].identity.pid, 7);
    }

    #[test]
    fn theme_override_wins() {
        assert!(!theme_is_dark(Theme::Light, Some(egui::Theme::Dark)));
        assert!(theme_is_dark(Theme::Dark, Some(egui::Theme::Light)));
    }

    #[test]
    fn staleness_detection_respects_threshold() {
        let now = Instant::now();
        assert!(is_data_stale(Some(now), 100).is_none());
        let old = now.checked_sub(Duration::from_millis(1500)).unwrap();
        assert!(is_data_stale(Some(old), 100).is_some());
    }

    #[test]
    fn settings_serialization_round_trip() {
        let s = UserSettings {
            schema_version: 2,
            theme: Theme::Dark,
            density: theme::Density::Compact,
            poll_interval_ms: 250,
            hardware_view_mode: state::HardwareViewMode::Tree,
            reduced_motion: true,
            percent_scale: state::PercentScale::Fit,
            default_history_range: history::HistoryRange::Min5,
            process_columns: components::table::TableColumnsConfig::default(),
            process_sort_column: components::table::SortColumn::Cpu,
            process_sort_descending: true,
            resource_graphs: [true; 6],
            resource_legends: [true; 6],
            performance_resource: None,
            performance_range: history::HistoryRange::Sec60,
            performance_engines: Vec::new(),
        };
        let serialized = serde_json::to_string(&s).unwrap();
        let deserialized: UserSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.theme, Theme::System);
        assert_eq!(deserialized.poll_interval_ms, 250);
        assert_eq!(deserialized.density, theme::Density::Normal);
        assert_eq!(
            deserialized.hardware_view_mode,
            state::HardwareViewMode::Tree
        );
        assert!(!deserialized.reduced_motion);
        assert_eq!(
            deserialized.default_history_range,
            history::HistoryRange::Min5
        );
    }

    #[test]
    fn test_headless_smoke_suite_executes() {
        smoke_test::run_smoke_fast_check().expect("Headless smoke fast check should pass");
    }

    #[test]
    fn test_lifecycle_and_coordinated_shutdown() {
        let mut app = smoke_test::create_test_app(Theme::Dark, Page::Performance);
        assert_eq!(app.lifecycle, LifecycleState::Running);
        assert!(app.shutdown_started.is_none());
        assert!(!app.shutdown_completed);

        // Simulate active benchmark
        {
            let mut shared = app.shared.lock().unwrap();
            shared.benchmark_state = Some(planora_core::BenchmarkState::Running {
                job_id: 42,
                elapsed_ms: 100,
            });
        }

        let ctx = egui::Context::default();
        app.begin_shutdown(&ctx);
        assert_eq!(app.lifecycle, LifecycleState::Stopping);
        assert!(app.shutdown_started.is_some());
        assert!(app.client_stop.load(Ordering::Acquire));

        std::thread::sleep(std::time::Duration::from_millis(50));
        app.update_shutdown(&ctx);
        assert!(app.shutdown_completed);
    }

    #[test]
    fn system_appearance_changes_replace_overrides_and_scale_controls() {
        let mut app = smoke_test::create_test_app(Theme::Dark, Page::Performance);
        app.normal_window = true;
        let ctx = egui::Context::default();
        ctx.set_zoom_factor(1.7);
        for theme in [egui::Theme::Light, egui::Theme::Dark, egui::Theme::Light] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    system_theme: Some(theme),
                    ..Default::default()
                },
                |_ui| {
                    app.apply_theme(&ctx);
                    assert_eq!(app.palette(&ctx).is_dark, theme == egui::Theme::Dark);
                },
            );
            output.textures_delta.clear();
        }
        app.appearance
            .set_fixture_snapshot(appearance::SystemAppearance {
                text_scale: 1.5,
                animations: false,
                contrast: Some((Color32::WHITE, Color32::BLACK)),
            });
        app.apply_theme(&ctx);
        assert_eq!(ctx.zoom_factor(), 1.0);
        assert!(!ctx.options(|o| o.zoom_with_keyboard));
        assert_eq!(ctx.style_of(ctx.theme()).spacing.interact_size.y, 36.0);
        assert_eq!(ctx.style_of(ctx.theme()).animation_time, 0.0);
        assert_eq!(app.palette(&ctx).text, Color32::WHITE);
        assert_eq!(app.palette(&ctx).background, Color32::BLACK);
        // Keep this fixture from saving placement/preferences during shutdown.
        app.normal_window = false;
    }

    #[test]
    fn sensor_deduplication_prefers_direct_vendor_readings() {
        use planora_core::{Metric, SensorReading, SensorType};
        let vendor = vec![SensorReading {
            id: "nvidia_gpu/0/temperature".into(),
            device_id: "nvidia_gpu/0".into(),
            device_name: "NVIDIA GeForce RTX 3080".into(),
            name: "GPU Temperature".into(),
            sensor_type: SensorType::GpuTemperatureCelsius,
            scope: "GPU 0".into(),
            provider: "NVIDIA NVML".into(),
            value: Metric::available(65.0, "°C", "NVIDIA NVML", 1000),
        }];
        let helper = vec![
            SensorReading {
                id: "lhm/gpu/0/temperature".into(),
                device_id: "lhm_gpu_0".into(),
                device_name: "NVIDIA GeForce RTX 3080".into(),
                name: "GPU Temperature".into(),
                sensor_type: SensorType::GpuTemperatureCelsius,
                scope: "GPU".into(),
                provider: "LibreHardwareMonitor".into(),
                value: Metric::available(64.5, "°C", "LibreHardwareMonitor", 1000),
            },
            SensorReading {
                id: "lhm/cpu/power".into(),
                device_id: "lhm_cpu_0".into(),
                device_name: "Intel Core i9".into(),
                name: "CPU Package".into(),
                sensor_type: SensorType::CpuPackagePowerWatts,
                scope: "Package".into(),
                provider: "LibreHardwareMonitor".into(),
                value: Metric::available(85.0, "W", "LibreHardwareMonitor", 1000),
            },
        ];
        let merged = merge_sensor_readings(&helper, &vendor);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].provider, "NVIDIA NVML");
        assert_eq!(merged[1].provider, "LibreHardwareMonitor");
        assert_eq!(merged[1].sensor_type, SensorType::CpuPackagePowerWatts);

        let igcl_vendor = vec![SensorReading {
            id: "igcl:00:02.0:temp:0".into(),
            device_id: "0000:00:02.0".into(),
            device_name: "Intel(R) Iris(R) Xe Graphics".into(),
            name: "GPU Global Temperature".into(),
            sensor_type: SensorType::GpuTemperatureCelsius,
            scope: "GPU".into(),
            provider: "Intel IGCL".into(),
            value: Metric::available(49.0, "°C", "Intel IGCL", 1000),
        }];
        let lhm_helper = vec![SensorReading {
            id: "/gpu-intel-integrated/0/temperature/0".into(),
            device_id: "/gpu-intel-integrated/0".into(),
            device_name: "Intel Iris Xe Graphics".into(),
            name: "GPU Global Temperature".into(),
            sensor_type: SensorType::GpuTemperatureCelsius,
            scope: "GPU".into(),
            provider: "LibreHardwareMonitor".into(),
            value: Metric::available(48.5, "°C", "LibreHardwareMonitor", 1000),
        }];
        let merged_trademark = merge_sensor_readings(&lhm_helper, &igcl_vendor);
        assert_eq!(merged_trademark.len(), 1);
        assert_eq!(merged_trademark[0].provider, "Intel IGCL");
    }
}
