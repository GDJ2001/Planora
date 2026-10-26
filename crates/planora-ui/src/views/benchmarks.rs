use crate::{
    components::header::render_page_header,
    formatting::{format_bytes, format_latency_ns},
    state::{AppState, DisplayPause},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Pos2, Rect, Stroke, Vec2};
use planora_core::{BenchmarkPoint, BenchmarkResult, BenchmarkState, Command, ProfileState};
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
        "Cancellable cache and inter-core probes with measured methodology",
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
    _profile_state: Option<&ProfileState>,
    _benchmark_state: Option<&BenchmarkState>,
    _state: &mut BenchmarksViewState,
    _app_state: &AppState,
    _tx: &SyncSender<Command>,
    p: Palette,
) {
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.heading("Advanced profiling unavailable");
            ui.label("This build does not collect ETW disk events or sampled PMU data. No event counts or attribution percentages are inferred.");
            ui.label("The capability table records this limitation. Profiling controls will be enabled after a measured provider and cancellation path pass native tests.");
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
