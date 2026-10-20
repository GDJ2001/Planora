use crate::{
    components::header::render_page_header,
    formatting::{format_bytes, format_latency_ns},
    state::{AppState, DisplayPause},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Pos2, Rect, Stroke, Vec2};
use planora_core::{
    BenchmarkPoint, BenchmarkResult, BenchmarkState, Command, ProfileResult, ProfileState,
    ProfileTarget,
};
use std::sync::mpsc::SyncSender;

pub struct BenchmarksViewState {
    pub section: usize, // 0: Micro-architectural probes, 1: Advanced profiling (ETW/PMU)
    pub matrix_input: String,
    pub active_tab: usize, // 0: Cache latency table, 1: Core heatmap, 2: Raw table
    // Profiling options:
    pub profile_type: usize, // 0: Disk I/O, 1: PMU Sampling
    pub duration_seconds: u32,
    pub target_pid_input: String,
    pub pmu_source_cycles: bool,
    pub pmu_source_instructions: bool,
    pub pmu_source_branches: bool,
}

impl Default for BenchmarksViewState {
    fn default() -> Self {
        Self {
            section: 0,
            matrix_input: "0, 1, 2, 3".to_string(),
            active_tab: 0,
            profile_type: 0,
            duration_seconds: 15,
            target_pid_input: String::new(),
            pmu_source_cycles: true,
            pmu_source_instructions: true,
            pmu_source_branches: false,
        }
    }
}

pub fn render_benchmarks_view(
    ui: &mut egui::Ui,
    benchmark_state: Option<&BenchmarkState>,
    profile_state: Option<&ProfileState>,
    state: &mut BenchmarksViewState,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    tx: &SyncSender<Command>,
    p: Palette,
) {
    // 1. Page Header
    render_page_header(
        ui,
        "Probing & Profiling",
        "Deterministic single-cycle Sattolo cache line/page probes, inter-core latency, and bounded ETW profiling",
        app_state,
        pause_state,
        poll_interval_ms,
        p,
    );

    // Section Toggle Toolbar
    ui.horizontal(|ui| {
        if ui
            .selectable_label(
                state.section == 0,
                egui::RichText::new("Micro-Architectural Probes").strong(),
            )
            .clicked()
        {
            state.section = 0;
        }
        if ui
            .selectable_label(
                state.section == 1,
                egui::RichText::new("Advanced Profiling (ETW & PMU)").strong(),
            )
            .clicked()
        {
            state.section = 1;
        }
    });

    ui.add_space(10.0);

    if state.section == 0 {
        render_probes_section(ui, benchmark_state, state, app_state, tx, p);
    } else {
        render_profiling_section(ui, profile_state, benchmark_state, state, app_state, tx, p);
    }
}

fn render_probes_section(
    ui: &mut egui::Ui,
    benchmark_state: Option<&BenchmarkState>,
    state: &mut BenchmarksViewState,
    app_state: &AppState,
    tx: &SyncSender<Command>,
    p: Palette,
) {
    // Guardrails & Limits Disclosure Panel
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.strong("Execution limits and memory budget");
            ui.add_space(4.0);
            ui.label("• Working Set Ceiling: 64 MiB (strict total process memory allocation limit 72 MiB)");
            ui.label("• Cache Probes: 12-second deadline with cancellation checks before and during batch runs");
            ui.label("• Core Matrix: 20-second timeout, 2 to 8 logical CPUs");
            ui.label("• Architecture Honesty: RDTSCP / exact core-pinning reported as unavailable on Apple Silicon");
        });

    ui.add_space(12.0);

    let bm_state = benchmark_state.unwrap_or(&BenchmarkState::Idle);
    let is_running = matches!(
        bm_state,
        BenchmarkState::Running { .. } | BenchmarkState::Cancelling { .. }
    );
    let is_live = *app_state == AppState::Live;

    let disabled_reason = if !is_live {
        Some("Micro-architectural probes require an active, live helper connection.")
    } else if is_running {
        Some("A micro-architectural probe is currently running.")
    } else {
        None
    };

    if !is_live {
        ui.colored_label(
            p.warning,
            "Probes disabled: An active, live helper connection is required to run micro-architectural probes.",
        );
        ui.add_space(4.0);
    }

    // Action Controls
    ui.horizontal(|ui| {
        let mut line_btn = ui.add_enabled(
            is_live && !is_running,
            egui::Button::new("Run Line Cache Probe"),
        );
        if let Some(reason) = disabled_reason {
            line_btn = line_btn.on_disabled_hover_text(reason);
        }
        if line_btn.clicked() {
            let job_id = planora_telemetry::unix_ms();
            let _ = tx.send(Command::RunCacheProbe {
                job_id,
                page_mode: false,
            });
        }

        let mut page_btn = ui.add_enabled(
            is_live && !is_running,
            egui::Button::new("Run Page Cache Probe"),
        );
        if let Some(reason) = disabled_reason {
            page_btn = page_btn.on_disabled_hover_text(reason);
        }
        if page_btn.clicked() {
            let job_id = planora_telemetry::unix_ms();
            let _ = tx.send(Command::RunCacheProbe {
                job_id,
                page_mode: true,
            });
        }

        if is_running
            && let BenchmarkState::Running { job_id, .. } = *bm_state
            && ui.button("Cancel Active Probe ✖").clicked()
        {
            let _ = tx.send(Command::CancelBenchmark { job_id });
        }
    });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label("Core Latency CPUs (comma-separated):");
        ui.add(egui::TextEdit::singleline(&mut state.matrix_input).desired_width(120.0));
        let mut matrix_btn =
            ui.add_enabled(is_live && !is_running, egui::Button::new("Run Core Matrix"));
        if let Some(reason) = disabled_reason {
            matrix_btn = matrix_btn.on_disabled_hover_text(reason);
        }
        if matrix_btn.clicked()
            && let Ok(logical_cpus) = state
                .matrix_input
                .split(',')
                .map(|x| x.trim().parse::<u16>())
                .collect::<Result<Vec<_>, _>>()
        {
            let job_id = planora_telemetry::unix_ms();
            let _ = tx.send(Command::RunCoreMatrix {
                job_id,
                logical_cpus,
            });
        }
    });

    ui.add_space(14.0);

    // Status Indicator
    match bm_state {
        BenchmarkState::Running { job_id, elapsed_ms } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.colored_label(
                    p.primary,
                    format!("Job #{job_id} running in background worker ({elapsed_ms} ms)..."),
                );
            });
            ui.add_space(8.0);
        }
        BenchmarkState::Cancelling { job_id } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.colored_label(
                    p.warning,
                    format!("Cancelling probe #{job_id}; waiting for its worker to finish..."),
                );
            });
            ui.add_space(8.0);
        }
        BenchmarkState::Failed { message, .. } => {
            ui.colored_label(p.error, format!("Benchmark Error: {message}"));
            ui.add_space(8.0);
        }
        BenchmarkState::Cancelled { job_id } => {
            ui.colored_label(p.warning, format!("Probe #{job_id} was cancelled."));
            ui.add_space(8.0);
        }
        BenchmarkState::Completed { result, .. } => {
            render_benchmark_result(ui, result, state, p);
        }
        BenchmarkState::Idle => {
            ui.label(
                egui::RichText::new(
                    "Click a probe button above to begin micro-architectural benchmark execution.",
                )
                .color(p.secondary),
            );
        }
    }
}

fn render_profiling_section(
    ui: &mut egui::Ui,
    profile_state: Option<&ProfileState>,
    benchmark_state: Option<&BenchmarkState>,
    state: &mut BenchmarksViewState,
    app_state: &AppState,
    tx: &SyncSender<Command>,
    p: Palette,
) {
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.strong("Bounded Advanced Profiling (ETW & PMU)");
            ui.add_space(4.0);
            ui.label("• Default session: 15 seconds (maximum 60 seconds)");
            ui.label("• Single session mutual exclusion: at most one active profile or benchmark at a time");
            ui.label("• Memory-bounded tracing: buffers and result queues strictly bounded to 128 MiB combined");
            ui.label("• PMU Sampling: OS-enumerated sources only; sample counts are statistical hits, not exact IPC");
        });

    ui.add_space(12.0);

    let is_live = *app_state == AppState::Live;
    let bm_busy = benchmark_state.is_some_and(|b| {
        matches!(
            b,
            BenchmarkState::Running { .. } | BenchmarkState::Cancelling { .. }
        )
    });
    let prof_running = profile_state.is_some_and(|s| {
        matches!(
            s,
            ProfileState::Running { .. } | ProfileState::Cancelling { .. }
        )
    });
    let can_start = is_live && !bm_busy && !prof_running;

    // Controls Configuration
    ui.horizontal(|ui| {
        ui.label("Profile Type:");
        ui.radio_value(&mut state.profile_type, 0, "Physical Disk I/O");
        ui.radio_value(&mut state.profile_type, 1, "PMU Event Sampling");
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Duration (seconds):");
        ui.add(egui::Slider::new(&mut state.duration_seconds, 1..=60).suffix(" s"));
        if ui.button("15s (default)").clicked() {
            state.duration_seconds = 15;
        }
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Target Process PID (optional):");
        ui.add(
            egui::TextEdit::singleline(&mut state.target_pid_input)
                .hint_text("System-wide (leave empty)")
                .desired_width(120.0),
        );
    });

    if state.profile_type == 1 {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("PMU Sources (select up to 2):");
            ui.checkbox(&mut state.pmu_source_cycles, "TotalCycles");
            ui.checkbox(&mut state.pmu_source_instructions, "InstructionRetired");
            ui.checkbox(&mut state.pmu_source_branches, "BranchInstructions");
        });
    }

    ui.add_space(10.0);

    // Launch & Cancel Buttons
    ui.horizontal(|ui| {
        let mut start_btn = ui.add_enabled(can_start, egui::Button::new("Start Profiling Session"));
        if !is_live {
            start_btn = start_btn
                .on_disabled_hover_text("Profiling requires an active, live helper connection.");
        } else if bm_busy {
            start_btn =
                start_btn.on_disabled_hover_text("A benchmark probe is currently executing.");
        } else if prof_running {
            start_btn = start_btn.on_disabled_hover_text("A profiling session is already running.");
        }

        if start_btn.clicked() {
            let job_id = planora_telemetry::unix_ms();
            let duration = state.duration_seconds.clamp(1, 60);
            let target_proc = state
                .target_pid_input
                .trim()
                .parse::<u32>()
                .ok()
                .map(|pid| planora_core::ProcessIdentity {
                    pid,
                    started: 1,
                    sequence_number: None,
                    source: planora_core::IdentitySource::SysinfoEpochSeconds,
                });

            let target = if state.profile_type == 0 {
                ProfileTarget::DiskIo {
                    target_process: target_proc,
                    duration_seconds: duration,
                }
            } else {
                let mut sources = Vec::new();
                if state.pmu_source_cycles {
                    sources.push("TotalCycles".into());
                }
                if state.pmu_source_instructions {
                    sources.push("InstructionRetired".into());
                }
                if state.pmu_source_branches && sources.len() < 2 {
                    sources.push("BranchInstructions".into());
                }
                if sources.is_empty() {
                    sources.push("TotalCycles".into());
                }
                ProfileTarget::PmuSampling {
                    target_process: target_proc,
                    duration_seconds: duration,
                    sources,
                }
            };

            let _ = tx.send(Command::StartProfile { job_id, target });
        }

        if prof_running
            && let Some(ProfileState::Running { job_id, .. }) = profile_state
            && ui.button("Cancel Profiling Session ✖").clicked()
        {
            let _ = tx.send(Command::CancelProfile { job_id: *job_id });
        }
    });

    ui.add_space(14.0);

    // Profile Status & Results
    match profile_state.unwrap_or(&ProfileState::Idle) {
        ProfileState::Running {
            job_id,
            target_desc,
            elapsed_ms,
            duration_seconds,
        } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.colored_label(
                    p.primary,
                    format!(
                        "Profile #{job_id} running ({target_desc}): {elapsed_ms} ms / {} ms...",
                        duration_seconds * 1000
                    ),
                );
            });
            ui.add_space(8.0);
        }
        ProfileState::Cancelling { job_id } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.colored_label(
                    p.warning,
                    format!("Cancelling profile #{job_id}; stopping trace sessions safely..."),
                );
            });
            ui.add_space(8.0);
        }
        ProfileState::Failed { message, .. } => {
            ui.colored_label(p.error, format!("Profile Error: {message}"));
            ui.add_space(8.0);
        }
        ProfileState::Cancelled { job_id } => {
            ui.colored_label(
                p.warning,
                format!("Profile #{job_id} was cancelled. Tracing session closed."),
            );
            ui.add_space(8.0);
        }
        ProfileState::Completed { result, .. } => {
            render_profile_result(ui, result, p);
        }
        ProfileState::Idle => {
            ui.label(
                egui::RichText::new(
                    "Configure parameters above and click 'Start Profiling Session'.",
                )
                .color(p.secondary),
            );
        }
    }
}

fn render_profile_result(ui: &mut egui::Ui, result: &ProfileResult, p: Palette) {
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.heading(format!("Profile Results: {}", result.target_desc));
            ui.add_space(6.0);

            egui::Grid::new("profile_summary_grid")
                .striped(true)
                .num_columns(4)
                .spacing([20.0, 6.0])
                .show(ui, |ui| {
                    ui.strong("Configured Duration");
                    ui.strong("Elapsed Time");
                    ui.strong("Traced Events");
                    ui.strong("Attribution Coverage");
                    ui.end_row();

                    ui.monospace(format!("{} s", result.duration_seconds));
                    ui.monospace(format!("{} ms", result.elapsed_ms));
                    ui.monospace(format!("{} (Lost: {})", result.events_traced, result.events_lost));
                    ui.monospace(format!("{:.1}%", result.attribution_coverage_percent));
                    ui.end_row();
                });

            ui.add_space(12.0);

            // Disk records table
            if !result.disk_records.is_empty() {
                ui.strong(format!("Attributed Process Disk Activity ({})", result.disk_records.len()));
                egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                    egui::Grid::new("profile_disk_records_grid")
                        .striped(true)
                        .num_columns(5)
                        .spacing([18.0, 6.0])
                        .show(ui, |ui| {
                            ui.strong("PID");
                            ui.strong("Process Name");
                            ui.strong("Read Bytes");
                            ui.strong("Write Bytes");
                            ui.strong("Event Count");
                            ui.end_row();

                            for rec in &result.disk_records {
                                ui.monospace(format!("PID {}", rec.pid));
                                ui.label(&rec.process_name);
                                ui.monospace(format_bytes(rec.read_bytes));
                                ui.monospace(format_bytes(rec.write_bytes));
                                ui.monospace((rec.read_operations + rec.write_operations).to_string());
                                ui.end_row();
                            }
                        });
                });

                if result.unattributed_read_bytes > 0 || result.unattributed_write_bytes > 0 {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "Unattributed physical disk activity: Read {}, Write {}",
                            format_bytes(result.unattributed_read_bytes),
                            format_bytes(result.unattributed_write_bytes)
                        ))
                        .size(11.5)
                        .color(p.secondary),
                    );
                }
            }

            // PMU records table
            if !result.pmu_records.is_empty() {
                ui.strong("PMU Event Sample Counts");
                egui::Grid::new("profile_pmu_records_grid")
                    .striped(true)
                    .num_columns(4)
                    .spacing([20.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("Target");
                        ui.strong("PMU Event Source");
                        ui.strong("Sample Count");
                        ui.strong("Unit / Note");
                        ui.end_row();

                        for rec in &result.pmu_records {
                            ui.monospace(rec.target_pid.map_or_else(|| "System-wide".into(), |pid| format!("PID {pid}")));
                            ui.label(&rec.source);
                            ui.monospace(rec.sample_count.to_string());
                            ui.label("Samples (statistical hits)");
                            ui.end_row();
                        }
                    });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("PMU samples reflect interrupt sampling frequency, not exact instruction retirement counts.")
                        .size(11.0)
                        .color(p.secondary),
                );
            }
        });
}

fn render_benchmark_result(
    ui: &mut egui::Ui,
    result: &BenchmarkResult,
    state: &mut BenchmarksViewState,
    p: Palette,
) {
    ui.horizontal(|ui| {
        ui.strong(format!("Methodology: {}", result.methodology));
        if result.is_partial {
            ui.colored_label(p.warning, "Partial execution");
        }
    });

    ui.add_space(8.0);

    // View Tabs
    ui.horizontal(|ui| {
        if !result.points.is_empty() {
            ui.selectable_value(&mut state.active_tab, 0, "Latency Table");
        }
        if !result.core_pairs.is_empty() {
            ui.selectable_value(&mut state.active_tab, 1, "Core Latency Heatmap");
        }
        ui.selectable_value(&mut state.active_tab, 2, "Tabular Results");
    });

    ui.add_space(10.0);

    match state.active_tab {
        0 if !result.points.is_empty() => {
            render_points_table(ui, &result.points, p);
        }
        1 if !result.core_pairs.is_empty() => {
            render_core_heatmap(ui, result, p);
        }
        _ => {
            render_points_table(ui, &result.points, p);
        }
    }
}

fn render_core_heatmap(ui: &mut egui::Ui, result: &BenchmarkResult, p: Palette) {
    let mut cpus: Vec<u16> = result
        .core_pairs
        .iter()
        .flat_map(|cp| [cp.cpu_a, cp.cpu_b])
        .collect();
    cpus.sort_unstable();
    cpus.dedup();

    let n = cpus.len();
    if n == 0 {
        ui.label("No core pair data recorded.");
        return;
    }

    let cell_size = 48.0;
    let grid_w = cell_size * (n as f32 + 1.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(grid_w, grid_w), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, CornerRadius::same(CORNER_RADIUS as u8), p.surface);
    painter.rect_stroke(
        rect,
        CornerRadius::same(CORNER_RADIUS as u8),
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );

    // Find max latency for color scaling
    let mut max_lat = 1.0f64;
    for cp in &result.core_pairs {
        if cp.estimated_one_way_ns > max_lat {
            max_lat = cp.estimated_one_way_ns;
        }
    }

    let hover_pos = resp.hover_pos();
    let mut hovered_info = None;

    // Headers
    for (i, cpu) in cpus.iter().enumerate() {
        let x = rect.min.x + (i as f32 + 1.0) * cell_size;
        let y = rect.min.y + (i as f32 + 1.0) * cell_size;
        painter.text(
            Pos2::new(x + cell_size / 2.0, rect.min.y + cell_size / 2.0),
            egui::Align2::CENTER_CENTER,
            format!("C{cpu}"),
            egui::FontId::monospace(11.0),
            p.text,
        );
        painter.text(
            Pos2::new(rect.min.x + cell_size / 2.0, y + cell_size / 2.0),
            egui::Align2::CENTER_CENTER,
            format!("C{cpu}"),
            egui::FontId::monospace(11.0),
            p.text,
        );
    }

    // Cells
    for (i, cpu_a) in cpus.iter().enumerate() {
        for (j, cpu_b) in cpus.iter().enumerate() {
            let cell_rect = Rect::from_min_size(
                Pos2::new(
                    rect.min.x + (j as f32 + 1.0) * cell_size,
                    rect.min.y + (i as f32 + 1.0) * cell_size,
                ),
                Vec2::new(cell_size, cell_size),
            );

            if i == j {
                // Diagonal: Same core
                painter.rect_filled(cell_rect, CornerRadius::ZERO, p.surface_raised);
                painter.text(
                    cell_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "—",
                    egui::FontId::monospace(11.0),
                    p.secondary,
                );
            } else {
                let pair = result
                    .core_pairs
                    .iter()
                    .find(|cp| pair_matches(cp.cpu_a, cp.cpu_b, *cpu_a, *cpu_b));
                if let Some(cp) = pair {
                    let norm = (cp.estimated_one_way_ns / max_lat).clamp(0.0, 1.0) as f32;
                    let fill_color = if norm < 0.5 {
                        p.success.linear_multiply(0.25 + norm * 0.5)
                    } else {
                        p.error.linear_multiply(0.25 + (norm - 0.5) * 1.2)
                    };
                    painter.rect_filled(cell_rect, CornerRadius::ZERO, fill_color);
                    painter.text(
                        cell_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("{:.0}", cp.estimated_one_way_ns),
                        egui::FontId::monospace(11.0),
                        p.text,
                    );

                    if hover_pos.is_some_and(|pos| cell_rect.contains(pos)) {
                        hovered_info = Some(format!(
                            "Core {} ↔ Core {}\nOne-way estimated: {:.1} ns\nMin RTT/2: {:.1} ns · P95: {:.1} ns",
                            cpu_a, cpu_b, cp.estimated_one_way_ns, cp.min_ns, cp.p95_ns
                        ));
                    }
                } else {
                    painter.text(
                        cell_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "—",
                        egui::FontId::monospace(10.5),
                        p.secondary,
                    );
                }
            }
            painter.rect_stroke(
                cell_rect,
                CornerRadius::ZERO,
                Stroke::new(0.5, p.border),
                egui::StrokeKind::Inside,
            );
        }
    }

    if let Some(info) = hovered_info {
        resp.on_hover_text(info);
    }
}

/// The benchmark measures each unordered CPU pair once. The heatmap is symmetric.
fn pair_matches(measured_a: u16, measured_b: u16, cell_a: u16, cell_b: u16) -> bool {
    (measured_a == cell_a && measured_b == cell_b) || (measured_a == cell_b && measured_b == cell_a)
}

fn render_points_table(ui: &mut egui::Ui, points: &[BenchmarkPoint], _p: Palette) {
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .show(ui, |ui| {
            egui::Grid::new("bench_points_grid")
                .striped(true)
                .num_columns(5)
                .spacing([24.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("Working Set");
                    ui.strong("Min");
                    ui.strong("Median");
                    ui.strong("P95");
                    ui.strong("P99");
                    ui.end_row();

                    for pt in points {
                        ui.label(format_bytes(pt.working_set_bytes));
                        ui.label(format_latency_ns(pt.min_ns));
                        ui.label(format_latency_ns(pt.median_ns));
                        ui.label(format_latency_ns(pt.p95_ns));
                        ui.label(format_latency_ns(pt.p99_ns));
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod heatmap_tests {
    use super::pair_matches;

    #[test]
    fn measured_pair_is_visible_in_both_triangles() {
        assert!(pair_matches(2, 5, 2, 5));
        assert!(pair_matches(2, 5, 5, 2));
        assert!(!pair_matches(2, 5, 2, 7));
    }
}
