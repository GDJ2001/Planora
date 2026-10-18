#![allow(clippy::too_many_arguments)]

pub mod components;
pub mod formatting;
pub mod history;
pub mod state;
pub mod theme;
pub mod views;

use components::{badge::render_compact_sidebar_status, icons, logo::LogoCache};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};
use history::{ResourceHistories, measurement_instant};
use planora_core::{Command, PROTOCOL_VERSION, ProcessIdentity};
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
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::sync_channel(64);
        let settings = UserSettings::load();
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
            page: Page::Overview,
            selected: None,
            display_pause: DisplayPause::default(),
            logo_cache: LogoCache::new(),
            histories: ResourceHistories::default(),
            overview_state: OverviewViewState::default(),
            processes_state: ProcessesViewState::default(),
            hardware_state: HardwareViewState::default(),
            benchmarks_state: BenchmarksViewState::default(),
            capabilities_state: CapabilitiesViewState::default(),
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
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.apply_theme(&ctx);
        let p = self.palette(&ctx);

        let (mut app_state, frame_opt, topo_opt, caps, shared_copy) = {
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
                diagnostic_logs: shared.diagnostic_logs.clone(),
                affinity: shared.affinity.clone(),
                action_error: shared.action_error.clone(),
            };
            (state, frame, topo, caps, shared_copy)
        };

        // Evaluate staleness if in Live state
        let current_poll = self.poll_ms.load(Ordering::Relaxed);
        if app_state == AppState::Live
            && let Some(elapsed) = is_data_stale(shared_copy.last_frame_received, current_poll)
        {
            app_state = AppState::Stale(elapsed);
        }

        // Ingest new frame into histories
        let now = Instant::now();
        let wall_clock_now_ms = planora_telemetry::unix_ms();
        if let Some(frame) = &frame_opt {
            self.histories.reset_if_new_session(frame.session_id);

            if frame.sequence != self.histories.last_sequence {
                self.histories.last_sequence = frame.sequence;

                // CPU (sample time preserved)
                if frame.cpu_percent.sampled_at_ms != self.histories.last_cpu_sampled_at_ms {
                    self.histories.last_cpu_sampled_at_ms = frame.cpu_percent.sampled_at_ms;
                    let measured_at = measurement_instant(
                        now, wall_clock_now_ms, frame.cpu_percent.sampled_at_ms,
                    );
                    self.histories.cpu.push(
                        measured_at, frame.cpu_percent.sampled_at_ms, frame.cpu_percent.value,
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
                    now, wall_clock_now_ms, frame.memory_used_bytes.sampled_at_ms,
                );
                self.histories.memory_pct.push(
                    measured_at, frame.memory_used_bytes.sampled_at_ms, mem_pct,
                );

                // Disk
                let disk_r = frame.disk_read_bytes_per_sec.value.map(|v| v as f32);
                let disk_w = frame.disk_write_bytes_per_sec.value.map(|v| v as f32);
                self.histories.disk_read.push(
                    measurement_instant(now, wall_clock_now_ms, frame.disk_read_bytes_per_sec.sampled_at_ms),
                    frame.disk_read_bytes_per_sec.sampled_at_ms,
                    disk_r,
                );
                self.histories.disk_write.push(
                    measurement_instant(now, wall_clock_now_ms, frame.disk_write_bytes_per_sec.sampled_at_ms),
                    frame.disk_write_bytes_per_sec.sampled_at_ms,
                    disk_w,
                );

                // Network
                let net_rx = frame.network_rx_bytes_per_sec.value.map(|v| v as f32);
                let net_tx = frame.network_tx_bytes_per_sec.value.map(|v| v as f32);
                self.histories.net_rx.push(
                    measurement_instant(now, wall_clock_now_ms, frame.network_rx_bytes_per_sec.sampled_at_ms),
                    frame.network_rx_bytes_per_sec.sampled_at_ms,
                    net_rx,
                );
                self.histories.net_tx.push(
                    measurement_instant(now, wall_clock_now_ms, frame.network_tx_bytes_per_sec.sampled_at_ms),
                    frame.network_tx_bytes_per_sec.sampled_at_ms,
                    net_tx,
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
                        measurement_instant(now, wall_clock_now_ms, proc.memory_bytes.sampled_at_ms),
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
                self.display_pause.frozen_histories = Some(self.histories.clone());
            }
        } else {
            self.display_pause.frozen_frame = None;
            self.display_pause.frozen_instant = None;
            self.display_pause.frozen_histories = None;
        }

        let render_frame: Option<planora_core::TelemetryFrame> = if self.display_pause.is_paused {
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
        let render_histories = if self.display_pause.is_paused {
            self.display_pause.frozen_histories.as_ref().unwrap_or(&self.histories)
        } else {
            &self.histories
        };

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
                    if ui.button("Authorize Windows Helper (Administrator Approval)").clicked() {
                        let script = std::env::current_exe()
                            .ok()
                            .and_then(|p| p.parent().map(|p| p.join("Authorize-Planora.ps1")));
                        if let Some(script) = script {
                            let _ = std::process::Command::new("powershell.exe")
                                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                                .arg(script)
                                .spawn();
                        }
                    }

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
                Page::Processes => render_processes_view(
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
                ),
                Page::Hardware => render_hardware_view(
                    ui,
                    topo_opt.as_ref(),
                    &mut self.hardware_state,
                    &app_state,
                    &mut self.display_pause,
                    current_poll,
                    p,
                ),
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
                Page::Settings => render_settings_view(
                    ui,
                    &mut self.settings,
                    &self.poll_ms,
                    &shared_copy.diagnostic_logs,
                    &app_state,
                    &mut self.display_pause,
                    &self.tx,
                    p,
                ),
            }
        });

        // Background telemetry and direct UI events request repaints.
        // A permanent animation timer would keep repainting an idle window.
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_title("Planora"),
        ..Default::default()
    };
    eframe::run_native(
        "Planora",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use components::table::{SortColumn, sort_processes};
    use planora_core::{AvailabilityReason, IdentitySource, Metric, ProcessRecord, TelemetryFrame};
    use state::UserSettings;
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
            theme: Theme::Dark,
            density: theme::Density::Compact,
            poll_interval_ms: 200,
            hardware_view_mode: state::HardwareViewMode::Tree,
            reduced_motion: true,
            default_history_range: history::HistoryRange::Min5,
        };
        let serialized = serde_json::to_string(&s).unwrap();
        let deserialized: UserSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.theme, Theme::Dark);
        assert_eq!(deserialized.poll_interval_ms, 200);
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
}
