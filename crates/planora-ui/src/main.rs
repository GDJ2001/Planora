#![allow(clippy::too_many_arguments)]

pub mod components;
pub mod export;
pub mod formatting;
pub mod history;
pub mod recording;
pub mod smoke_test;
pub mod state;
pub mod theme;
pub mod views;

use components::{badge::render_compact_sidebar_status, icons, logo::LogoCache};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};
use export::ExportManager;
use history::{ResourceHistories, measurement_instant};
use planora_core::{Command, PROTOCOL_VERSION, ProcessIdentity};
use recording::{Recorder, Replay};
use state::{AppState, DisplayPause, Page, Shared, UserSettings, background_worker, is_data_stale};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::Instant,
};
use theme::{Palette, SIDEBAR_WIDTH, apply_egui_theme, get_palette, theme_is_dark};
use views::{
    benchmarks::{BenchmarksViewState, render_benchmarks_view},
    capabilities::{CapabilitiesViewState, render_capabilities_view},
    hardware::{HardwareViewState, render_hardware_view},
    overview::{OverviewViewState, render_overview_view},
    processes::{ProcessesViewState, render_processes_view},
    settings::render_settings_view,
};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn planora_register_daemon() -> i32;
    fn planora_unregister_daemon() -> i32;
}

pub struct App {
    shared: Arc<Mutex<Shared>>,
    tx: SyncSender<Command>,
    poll_ms: Arc<AtomicU64>,
    settings: UserSettings,
    settings_error: Option<String>,
    recorder: Recorder,
    exporter: ExportManager,
    replay: Replay,
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
    histories: ResourceHistories,

    // View specific state
    overview_state: OverviewViewState,
    processes_state: ProcessesViewState,
    hardware_state: HardwareViewState,
    benchmarks_state: BenchmarksViewState,
    capabilities_state: CapabilitiesViewState,
    auto_close: Option<(Instant, std::time::Duration)>,
    screenshot_out: Option<std::path::PathBuf>,
    frames_rendered: usize,
}

impl App {
    #[cfg(windows)]
    fn launch_helper_script(&mut self, script_name: &str) {
        if self.authorization_child.is_some() {
            return;
        }
        let script = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join(script_name)));
        let Some(script) = script.filter(|path| path.is_file()) else {
            self.authorization_error = Some(format!(
                "{script_name} is missing from the installed application."
            ));
            return;
        };
        match std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .spawn()
        {
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
        let (tx, rx) = mpsc::sync_channel(64);
        let settings = UserSettings::load();
        let mut processes_state = ProcessesViewState::default();
        processes_state.table_state.columns = settings.process_columns;
        processes_state.table_state.sort_column = settings.process_sort_column;
        processes_state.table_state.sort_descending = settings.process_sort_descending;
        let poll_ms = Arc::new(AtomicU64::new(settings.poll_interval_ms));
        let shared = Arc::new(Mutex::new(Shared::default()));

        let worker_shared = shared.clone();
        let worker_poll = poll_ms.clone();
        let context = cc.egui_ctx.clone();

        thread::Builder::new()
            .name("planora-client".into())
            .spawn(move || {
                background_worker(worker_shared, rx, worker_poll, context);
            })
            .expect("failed to spawn client worker thread");

        Self {
            shared,
            tx,
            poll_ms,
            settings,
            settings_error: None,
            recorder: Recorder::default(),
            exporter: ExportManager::default(),
            replay: Replay::default(),
            replay_input: String::new(),
            record_selected_process: false,
            recording_error: None,
            #[cfg(windows)]
            authorization_child: None,
            #[cfg(windows)]
            authorization_error: None,
            page: Page::Overview,
            selected: None,
            display_pause: DisplayPause::default(),
            logo_cache: LogoCache::new(),
            histories: ResourceHistories::default(),
            overview_state: OverviewViewState::default(),
            processes_state,
            hardware_state: HardwareViewState::default(),
            benchmarks_state: BenchmarksViewState::default(),
            capabilities_state: CapabilitiesViewState::default(),
            auto_close: None,
            screenshot_out: None,
            frames_rendered: 0,
        }
    }

    fn palette(&self, ctx: &egui::Context) -> Palette {
        let is_dark = theme_is_dark(self.settings.theme, Some(ctx.theme()));
        get_palette(is_dark)
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let is_dark = theme_is_dark(self.settings.theme, Some(ctx.theme()));
        apply_egui_theme(ctx, is_dark, self.settings.density);
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
                                self.histories = ResourceHistories::default();
                                self.display_pause = DisplayPause::default();
                                self.recording_error = None;
                                self.page = Page::Overview;
                            }
                            Err(e) => {
                                self.recording_error =
                                    Some(format!("Could not open recording: {e}"))
                            }
                        }
                    }
                    if self.replay.is_active() && ui.button("Return to Live").clicked() {
                        self.replay.stop();
                        self.histories = ResourceHistories::default();
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
                if ui.button("Export Visible Chart Ranges CSV").clicked() {
                    let ranges = [self.overview_state.cpu_chart.range, self.overview_state.mem_chart.range, self.overview_state.disk_chart.range, self.overview_state.net_chart.range];
                    if let Err(error) = self.exporter.charts(self.histories.clone(), ranges, Instant::now()) {
                        self.recording_error = Some(format!("Chart export failed: {error}"));
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

    pub fn render_ui(&mut self, root: &mut egui::Ui) {
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
            root.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let ctx = root.ctx().clone();

        self.frames_rendered = self.frames_rendered.saturating_add(1);
        if self.frames_rendered >= 45
            && let Some(path) = self.screenshot_out.take()
        {
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
        let (mut app_state, mut frame_opt, mut topo_opt, caps, shared_copy) = {
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
                benchmark_state: shared.benchmark_state.clone(),
                sampling_effective_ms: shared.sampling_effective_ms,
                diagnostic_logs: shared.diagnostic_logs.clone(),
                affinity: shared.affinity.clone(),
                action_error: shared.action_error.clone(),
            };
            (state, frame, topo, caps, shared_copy)
        };

        if self.replay.is_active() {
            app_state = AppState::Replay;
            frame_opt = self.replay.frame.clone();
            topo_opt = self.replay.topology.clone();
        } else if app_state == AppState::Live
            && let Some(frame) = &frame_opt
        {
            self.recorder.record(frame, self.selected.as_ref());
        }

        // Evaluate staleness if in Live state
        let current_poll = self.poll_ms.load(Ordering::Relaxed);
        if app_state == AppState::Live
            && let Some(elapsed) = is_data_stale(shared_copy.last_frame_received, current_poll)
        {
            app_state = AppState::Stale(elapsed);
        }

        // Ingest new frame into histories
        let now = Instant::now();
        let wall_clock_now_ms = if app_state == AppState::Replay {
            frame_opt
                .as_ref()
                .map_or_else(planora_telemetry::unix_ms, |f| f.sampled_at_ms)
        } else {
            planora_telemetry::unix_ms()
        };
        if let Some(frame) = &frame_opt {
            self.histories.reset_if_new_session(frame.session_id);

            if frame.sequence != self.histories.last_sequence {
                self.histories.last_sequence = frame.sequence;

                // CPU (sample time preserved)
                if frame.cpu_percent.sampled_at_ms != self.histories.last_cpu_sampled_at_ms {
                    self.histories.last_cpu_sampled_at_ms = frame.cpu_percent.sampled_at_ms;
                    let measured_at = measurement_instant(
                        now,
                        wall_clock_now_ms,
                        frame.cpu_percent.sampled_at_ms,
                    );
                    self.histories.cpu.push_with_metadata(
                        measured_at,
                        frame.cpu_percent.sampled_at_ms,
                        frame.cpu_percent.value,
                        &frame.cpu_percent.source,
                        frame.cpu_percent.unavailable_reason.as_ref(),
                    );
                }

                // Memory %
                let mem_pct = match (
                    frame.memory_used_bytes.value,
                    frame.memory_total_bytes.value,
                ) {
                    (Some(u), Some(t)) if t > 0 => Some(((u as f64 / t as f64) * 100.0) as f32),
                    _ => None,
                };
                let measured_at = measurement_instant(
                    now,
                    wall_clock_now_ms,
                    frame.memory_used_bytes.sampled_at_ms,
                );
                self.histories.memory_pct.push_with_metadata(
                    measured_at,
                    frame.memory_used_bytes.sampled_at_ms,
                    mem_pct,
                    &frame.memory_used_bytes.source,
                    frame
                        .memory_used_bytes
                        .unavailable_reason
                        .as_ref()
                        .or(frame.memory_total_bytes.unavailable_reason.as_ref()),
                );

                // Disk
                let disk_r = frame.disk_read_bytes_per_sec.value.map(|v| v as f32);
                let disk_w = frame.disk_write_bytes_per_sec.value.map(|v| v as f32);
                self.histories.disk_read.push_with_metadata(
                    measurement_instant(
                        now,
                        wall_clock_now_ms,
                        frame.disk_read_bytes_per_sec.sampled_at_ms,
                    ),
                    frame.disk_read_bytes_per_sec.sampled_at_ms,
                    disk_r,
                    &frame.disk_read_bytes_per_sec.source,
                    frame.disk_read_bytes_per_sec.unavailable_reason.as_ref(),
                );
                self.histories.disk_write.push_with_metadata(
                    measurement_instant(
                        now,
                        wall_clock_now_ms,
                        frame.disk_write_bytes_per_sec.sampled_at_ms,
                    ),
                    frame.disk_write_bytes_per_sec.sampled_at_ms,
                    disk_w,
                    &frame.disk_write_bytes_per_sec.source,
                    frame.disk_write_bytes_per_sec.unavailable_reason.as_ref(),
                );

                // Network
                let net_rx = frame.network_rx_bytes_per_sec.value.map(|v| v as f32);
                let net_tx = frame.network_tx_bytes_per_sec.value.map(|v| v as f32);
                self.histories.net_rx.push_with_metadata(
                    measurement_instant(
                        now,
                        wall_clock_now_ms,
                        frame.network_rx_bytes_per_sec.sampled_at_ms,
                    ),
                    frame.network_rx_bytes_per_sec.sampled_at_ms,
                    net_rx,
                    &frame.network_rx_bytes_per_sec.source,
                    frame.network_rx_bytes_per_sec.unavailable_reason.as_ref(),
                );
                self.histories.net_tx.push_with_metadata(
                    measurement_instant(
                        now,
                        wall_clock_now_ms,
                        frame.network_tx_bytes_per_sec.sampled_at_ms,
                    ),
                    frame.network_tx_bytes_per_sec.sampled_at_ms,
                    net_tx,
                    &frame.network_tx_bytes_per_sec.source,
                    frame.network_tx_bytes_per_sec.unavailable_reason.as_ref(),
                );

                // Selected Process trends (only recorded while observed!)
                self.histories.set_selected_process(self.selected.as_ref());
                if let Some(identity) = self.selected.as_ref()
                    && let Some(proc) = frame.processes.iter().find(|pr| &pr.identity == identity)
                {
                    self.histories.selected_cpu.push(
                        measurement_instant(now, wall_clock_now_ms, proc.cpu_percent.sampled_at_ms),
                        proc.cpu_percent.sampled_at_ms,
                        proc.cpu_percent.value,
                    );
                    self.histories.selected_memory.push(
                        measurement_instant(
                            now,
                            wall_clock_now_ms,
                            proc.memory_bytes.sampled_at_ms,
                        ),
                        proc.memory_bytes.sampled_at_ms,
                        proc.memory_bytes.value.map(|b| b as f32),
                    );
                }
            }
        }

        // Display Pause handling
        if self.display_pause.is_paused {
            if self.display_pause.frozen_frame.is_none() {
                self.display_pause.frozen_frame = frame_opt.clone();
                self.display_pause.frozen_instant = Some(now);
                self.display_pause.frozen_histories = Some(Arc::new(self.histories.clone()));
            }
        } else {
            self.display_pause.frozen_frame = None;
            self.display_pause.frozen_instant = None;
            self.display_pause.frozen_histories = None;
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
            self.display_pause.frozen_instant.unwrap_or(now)
        } else {
            now
        };
        // Snapshot the chart buffers as well as the table frame when paused.
        // Live collection continues in self.histories without mutating the display.
        // Clone only the Arc handle so page controls can mutably borrow pause state.
        let frozen_histories = self.display_pause.frozen_histories.clone();

        // 1. Sidebar (208 px)
        egui::Panel::left("nav_sidebar")
            .exact_size(SIDEBAR_WIDTH)
            .show(root, |ui| {
                ui.add_space(14.0);

                // Official Planora Logo
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    self.logo_cache.render(ui, p.is_dark, 168.0);
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // Navigation items with clean icons
                let nav_items = [
                    (
                        Page::Overview,
                        "Overview",
                        icons::draw_overview as fn(&egui::Painter, Rect, Color32),
                    ),
                    (Page::Processes, "Processes", icons::draw_processes),
                    (Page::Hardware, "Hardware", icons::draw_hardware),
                    (Page::Benchmarks, "Benchmarks", icons::draw_benchmarks),
                    (Page::Capabilities, "Capabilities", icons::draw_capabilities),
                    (Page::Recordings, "Recordings", icons::draw_benchmarks),
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

                    let icon_rect =
                        Rect::from_min_size(rect.min + Vec2::new(12.0, 9.0), Vec2::new(18.0, 18.0));
                    draw_icon(
                        painter,
                        icon_rect,
                        if is_active { p.primary } else { p.secondary },
                    );

                    painter.text(
                        Pos2::new(rect.min.x + 38.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(13.5),
                        if is_active { p.text } else { p.secondary },
                    );

                    if resp.clicked() {
                        self.page = page;
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.add_space(12.0);
                    render_compact_sidebar_status(ui, &app_state, self.display_pause.is_paused, p);
                    ui.separator();
                });
            });

        // 2. Central Panel
        egui::CentralPanel::default().show(root, |ui| {
            if self.page == Page::Recordings {
                self.render_recordings_page(ui, frame_opt.clone(), topo_opt.as_ref(), shared_copy.benchmark_state.as_ref(), &shared_copy.diagnostic_logs, &app_state, p);
                return;
            }
            let render_histories = frozen_histories.as_deref().unwrap_or(&self.histories);
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
                    ui.heading("Helper Authorization Required");
                    ui.label("Planora requires an authorized local privileged helper before monitoring starts.");
                    ui.add_space(8.0);

                    #[cfg(windows)]
                    if ui.add_enabled(self.authorization_child.is_none(), egui::Button::new("Authorize Windows Helper (Administrator Approval)")).clicked() {
                        self.launch_helper_script("Authorize-Planora.ps1");
                    }
                    #[cfg(windows)]
                    if self.authorization_child.is_some() { ui.spinner(); ui.label("Waiting for administrator approval and helper restart…"); }
                    #[cfg(windows)]
                    if let Some(error) = &self.authorization_error { ui.colored_label(p.error, error); }

                    #[cfg(target_os = "macos")]
                    if ui.button("Request macOS Daemon Approval").clicked()
                        && unsafe { planora_register_daemon() } != 0
                    {
                        ui.colored_label(p.error, "Registration failed; check Console.app for details.");
                    }
                    return;
                }
                AppState::IncompatibleVersion { helper_version } => {
                    ui.heading("Incompatible Helper Version");
                    ui.colored_label(
                        p.error,
                        format!("The running helper reports protocol v{helper_version}, but Planora UI requires protocol v{PROTOCOL_VERSION}."),
                    );
                    ui.label("Please upgrade or restart the PlanoraHelper service to continue.");
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
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            render_overview_view(
                                ui,
                                frame,
                                render_histories,
                                &mut self.overview_state,
                                &app_state,
                                &mut self.display_pause,
                                current_poll,
                                &mut self.selected,
                                &mut self.page,
                                render_now,
                                p,
                            );
                        });
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
                Page::Hardware => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            render_hardware_view(
                                ui,
                                topo_opt.as_ref(),
                                &mut self.hardware_state,
                                &app_state,
                                &mut self.display_pause,
                                current_poll,
                                p,
                            );
                        });
                }
                Page::Benchmarks => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            render_benchmarks_view(
                                ui,
                                shared_copy.benchmark_state.as_ref(),
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
                    p,
                ),
                Page::Settings => {
                    render_settings_view(
                        ui, &mut self.settings, &mut self.settings_error, &self.poll_ms,
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
                },
                Page::Recordings => unreachable!("recordings page rendered before frame selection"),
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.render_ui(root);
    }
}

fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--build-info") {
        let info = state::BUILD_INFO;
        println!(
            "{}",
            serde_json::json!({
                "version": info.version,
                "protocolVersion": info.protocol_version,
                "gitCommit": info.git_commit,
                "target": info.target,
                "profile": info.profile,
            })
        );
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

    let mut initial_page = Page::Overview;
    let mut initial_theme = None;
    let mut width = 1180.0;
    let mut height = 760.0;
    let mut auto_close_ms = None;
    let mut screenshot_out = None;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--view" if i + 1 < args.len() => {
                initial_page = match args[i + 1].to_lowercase().as_str() {
                    "overview" => Page::Overview,
                    "processes" => Page::Processes,
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
            _ => {
                i += 1;
            }
        }
    }

    let capture_mode = screenshot_out.is_some();
    // Native capture must honor the requested size, independent of the user's
    // saved window geometry. Give every image an isolated, empty state file.
    let capture_state_path = screenshot_out
        .as_ref()
        .map(|path| path.with_extension("capture.ron"));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_min_inner_size([1024.0, 640.0])
            .with_clamp_size_to_monitor_size(!capture_mode)
            .with_title("Planora"),
        persist_window: !capture_mode,
        persistence_path: capture_state_path,
        ..Default::default()
    };
    eframe::run_native(
        "Planora",
        options,
        Box::new(move |cc| {
            let mut app = App::new(cc);
            app.page = initial_page;
            if let Some(th) = initial_theme {
                app.settings.theme = th;
            }
            if let Some(ms) = auto_close_ms {
                app.auto_close = Some((Instant::now(), std::time::Duration::from_millis(ms)));
            }
            app.screenshot_out = screenshot_out;
            Ok(Box::new(app))
        }),
    )
}

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
            memory_used_bytes: m.clone(),
            memory_total_bytes: m.clone(),
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
            default_history_range: history::HistoryRange::Min5,
            process_columns: components::table::TableColumnsConfig::default(),
            process_sort_column: components::table::SortColumn::Cpu,
            process_sort_descending: true,
        };
        let serialized = serde_json::to_string(&s).unwrap();
        let deserialized: UserSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.theme, Theme::Dark);
        assert_eq!(deserialized.poll_interval_ms, 250);
        assert_eq!(deserialized.density, theme::Density::Compact);
        assert_eq!(
            deserialized.hardware_view_mode,
            state::HardwareViewMode::Tree
        );
        assert!(deserialized.reduced_motion);
        assert_eq!(
            deserialized.default_history_range,
            history::HistoryRange::Min5
        );
    }

    #[test]
    fn test_headless_smoke_suite_executes() {
        smoke_test::run_smoke_fast_check().expect("Headless smoke fast check should pass");
    }
}
