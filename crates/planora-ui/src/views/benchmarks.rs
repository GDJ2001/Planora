use crate::{
    components::header::render_page_header,
    formatting::{format_bytes, format_latency_ns},
    state::{AppState, DisplayPause},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Pos2, Rect, Shape, Stroke, Vec2};
use planora_core::{
    BenchmarkPoint, BenchmarkResult, BenchmarkState, Command, PmuSourceInfo, ProcessIdentity,
    ProfileKind, ProfileState, ProfileTarget,
};
use std::sync::mpsc::SyncSender;

pub struct BenchmarksViewState {
    pub section: usize, // 0: Micro-architectural probes, 1: Advanced profiling
    pub matrix_input: String,
    pub active_tab: usize, // 0: Cache latency table, 1: Core heatmap, 2: Raw table
    // Profiling options:
    pub duration_seconds: u32,
    pub profile_selected_process: bool,
    pub pmu_selected: Vec<String>,
}

impl Default for BenchmarksViewState {
    fn default() -> Self {
        Self {
            section: 0,
            matrix_input: "0, 1, 2, 3".to_string(),
            active_tab: 0,
            duration_seconds: 15,
            profile_selected_process: false,
            pmu_selected: Vec::new(),
        }
    }
}

pub fn render_benchmarks_view(
    ui: &mut egui::Ui,
    benchmark_state: Option<&BenchmarkState>,
    profile_state: Option<&ProfileState>,
    pmu_sources: &[PmuSourceInfo],
    selected_process: Option<&ProcessIdentity>,
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
        #[cfg(windows)]
        let profiling_label = "Advanced Profiling (ETW & PMU)";
        #[cfg(target_os = "linux")]
        let profiling_label = "Advanced Profiling (perf & block I/O)";
        #[cfg(not(any(windows, target_os = "linux")))]
        let profiling_label = "Advanced Profiling";
        if ui
            .selectable_label(
                state.section == 1,
                egui::RichText::new(profiling_label).strong(),
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
        render_profiling_section(
            ui,
            profile_state,
            pmu_sources,
            benchmark_state,
            selected_process,
            state,
            app_state,
            tx,
            p,
        );
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
    pmu_sources: &[PmuSourceInfo],
    benchmark_state: Option<&BenchmarkState>,
    selected_process: Option<&ProcessIdentity>,
    state: &mut BenchmarksViewState,
    app_state: &AppState,
    tx: &SyncSender<Command>,
    p: Palette,
) {
    #[cfg(not(target_os = "linux"))]
    let _ = pmu_sources;
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            #[cfg(windows)]
            ui.heading("Disk I/O profiling · Windows ETW");
            #[cfg(target_os = "linux")]
            ui.heading("Advanced profiling · Linux perf and block tracing");
            #[cfg(not(any(windows, target_os = "linux")))]
            ui.heading("Advanced profiling");
            #[cfg(windows)]
            ui.label("An explicit, bounded trace of physical disk read and write events. Attribution uses issuing-thread and process creation identity; event loss and unattributed activity remain visible.");
            #[cfg(target_os = "linux")]
            ui.label("Sample a selected process with up to two OS-exposed CPU PMU sources, or trace completed physical disk requests. PMU results are samples, not exact instruction or cycle counts; disk results show lost and unattributed events.");
            #[cfg(windows)]
            ui.colored_label(p.warning, "Experimental: native attribution and cancellation acceptance is still in progress.");
            #[cfg(windows)]
            ui.label("PMU sampled profiling is not implemented. Source discovery is not a measurement.");
        });

    ui.add_space(12.0);
    let profile = profile_state.unwrap_or(&ProfileState::Idle);
    let profile_busy = matches!(
        profile,
        ProfileState::Running { .. } | ProfileState::Cancelling { .. }
    );
    let benchmark_busy = matches!(
        benchmark_state,
        Some(BenchmarkState::Running { .. } | BenchmarkState::Cancelling { .. })
    );
    let can_start = *app_state == AppState::Live && !profile_busy && !benchmark_busy;

    #[cfg(target_os = "linux")]
    {
        if ui.button("Refresh PMU sources").clicked() {
            let _ = tx.try_send(Command::PmuSources);
        }
        if pmu_sources.is_empty() {
            ui.label("No PMU sources loaded. Refresh to query this kernel; permission is checked when a session starts.");
        }
        ui.horizontal_wrapped(|ui| {
            for source in pmu_sources {
                let mut checked = state.pmu_selected.contains(&source.name);
                if ui
                    .checkbox(&mut checked, &source.name)
                    .on_hover_text(
                        source
                            .description
                            .as_deref()
                            .unwrap_or("Sampled hardware event"),
                    )
                    .changed()
                {
                    if checked && state.pmu_selected.len() < 2 {
                        state.pmu_selected.push(source.name.clone());
                    } else if !checked {
                        state.pmu_selected.retain(|name| name != &source.name);
                    }
                }
            }
        });
        if ui
            .add_enabled(
                can_start && selected_process.is_some() && !state.pmu_selected.is_empty(),
                egui::Button::new("Start PMU sample"),
            )
            .clicked()
        {
            let _ = tx.try_send(Command::StartProfile {
                job_id: planora_telemetry::unix_ms(),
                target: ProfileTarget::PmuSampling {
                    target_process: selected_process.cloned(),
                    duration_seconds: state.duration_seconds,
                    sources: state.pmu_selected.clone(),
                },
            });
        }
    }

    if selected_process.is_none() {
        state.profile_selected_process = false;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Duration:");
        egui::ComboBox::from_id_salt("disk_profile_duration")
            .selected_text(format!("{} seconds", state.duration_seconds))
            .show_ui(ui, |ui| {
                for seconds in [15, 30, 60] {
                    ui.selectable_value(
                        &mut state.duration_seconds,
                        seconds,
                        format!("{seconds} seconds"),
                    );
                }
            });
        ui.add_enabled(
            selected_process.is_some(),
            egui::Checkbox::new(&mut state.profile_selected_process, "Selected process only"),
        );
        if let Some(process) = selected_process {
            ui.label(format!(
                "PID {} · creation {}",
                process.pid, process.started
            ));
        } else {
            ui.label("System-wide");
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let button = ui.add_enabled(
            can_start && (cfg!(windows) || cfg!(target_os = "linux")),
            egui::Button::new("Start disk I/O profile"),
        );
        if button.clicked() {
            let job_id = planora_telemetry::unix_ms();
            let target_process = state
                .profile_selected_process
                .then(|| selected_process.cloned())
                .flatten();
            if let Err(error) = tx.try_send(Command::StartProfile {
                job_id,
                target: ProfileTarget::DiskIo {
                    target_process,
                    duration_seconds: state.duration_seconds,
                },
            }) {
                ui.colored_label(p.error, format!("Could not queue profile: {error}"));
            }
        }
        if let ProfileState::Running { job_id, .. } = profile
            && ui.button("Cancel profile").clicked()
        {
            let _ = tx.try_send(Command::CancelProfile { job_id: *job_id });
        }
    });

    ui.add_space(12.0);
    match profile {
        ProfileState::Idle => {
            ui.label("Choose a profile, scope, and duration to begin collection.");
        }
        ProfileState::Running {
            job_id,
            target_desc,
            elapsed_ms,
            duration_seconds,
        } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "Profile #{job_id}: {target_desc} · {:.1}/{duration_seconds} seconds",
                    *elapsed_ms as f32 / 1000.0
                ));
            });
        }
        ProfileState::Cancelling { job_id } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "Cancelling profile #{job_id}; waiting for collector cleanup…"
                ));
            });
        }
        ProfileState::Failed { code, message, .. } => {
            ui.colored_label(p.error, format!("Profile failed ({code:?}): {message}"));
        }
        ProfileState::Cancelled { job_id } => {
            ui.colored_label(p.warning, format!("Profile #{job_id} was cancelled."));
        }
        ProfileState::Completed { result, .. } => {
            if result.kind == Some(ProfileKind::PmuSampling)
                || (result.kind.is_none() && !result.pmu_records.is_empty())
            {
                ui.heading("PMU sample result");
                ui.label(format!(
                    "Elapsed: {:.1} s · lost records: {}",
                    result.elapsed_ms as f64 / 1000.0,
                    result.events_lost
                ));
                if let Some(semantics) = &result.sampling_semantics {
                    ui.label(semantics);
                }
                if let Some(hz) = result.effective_sample_hz {
                    ui.label(format!(
                        "Requested frequency: {hz} Hz per thread; the kernel may adjust it."
                    ));
                }
                if let Some(percent) = result.pmu_scheduled_percent {
                    ui.label(format!("PMU scheduled: {percent:.1}% of enabled time"));
                }
                if result.complete == Some(false) {
                    ui.colored_label(
                        p.warning,
                        "The target exited or records were lost; this profile is incomplete.",
                    );
                }
                for record in &result.pmu_records {
                    ui.label(format!(
                        "{}: {} samples · PID {}",
                        record.source,
                        record.sample_count,
                        record.target_pid.unwrap_or_default()
                    ));
                }
                return;
            }
            ui.heading("Disk I/O result");
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Elapsed: {:.1} s",
                    result.elapsed_ms as f64 / 1000.0
                ));
                ui.label(format!("Events: {}", result.events_traced));
                ui.label(format!("Lost: {}", result.events_lost));
                ui.label(format!(
                    "Attributed bytes: {:.1}%",
                    result.attribution_coverage_percent
                ));
            });
            if result.events_lost > 0 {
                ui.colored_label(
                    p.warning,
                    "The collector reported lost events; totals and attribution are incomplete.",
                );
            } else if result.events_traced == 0 {
                ui.label("No disk I/O events were observed during this trace.");
            }
            ui.label(format!(
                "Unattributed: {} read, {} written, {} operations",
                format_bytes(result.unattributed_read_bytes),
                format_bytes(result.unattributed_write_bytes),
                result.unattributed_operations
            ));
            ui.add_space(8.0);
            egui::ScrollArea::horizontal()
                .id_salt("disk_profile_columns")
                .show(ui, |ui| {
                    let widths = [200.0, 60.0, 90.0, 90.0, 90.0, 90.0];
                    ui.horizontal(|ui| {
                        for (title, width) in
                            ["Process", "PID", "Read", "Write", "Read ops", "Write ops"]
                                .into_iter()
                                .zip(widths)
                        {
                            ui.add_sized(
                                [width, 24.0],
                                egui::Label::new(egui::RichText::new(title).strong()),
                            );
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("disk_profile_rows")
                        .max_height(320.0)
                        .show_rows(ui, 28.0, result.disk_records.len(), |ui, range| {
                            for record in &result.disk_records[range] {
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        [widths[0], 26.0],
                                        egui::Label::new(&record.process_name),
                                    )
                                    .on_hover_text(
                                        record.identity.as_ref().map_or_else(
                                            || "Process creation identity unavailable".to_owned(),
                                            |identity| {
                                                format!(
                                                    "Created: {} ({:?})",
                                                    identity.started, identity.source
                                                )
                                            },
                                        ),
                                    );
                                    for (value, width) in [
                                        (record.pid.to_string(), widths[1]),
                                        (format_bytes(record.read_bytes), widths[2]),
                                        (format_bytes(record.write_bytes), widths[3]),
                                        (record.read_operations.to_string(), widths[4]),
                                        (record.write_operations.to_string(), widths[5]),
                                    ] {
                                        ui.add_sized([width, 26.0], egui::Label::new(value));
                                    }
                                });
                            }
                        });
                });
            ui.label("Physical disk event totals; process I/O counters shown elsewhere include non-disk I/O.");
        }
    }
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
            render_latency_plot(ui, &result.points, p);
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

fn render_latency_plot(ui: &mut egui::Ui, points: &[BenchmarkPoint], p: Palette) {
    let mut data: Vec<_> = points
        .iter()
        .filter(|point| {
            point.working_set_bytes > 0
                && point.min_ns.is_finite()
                && point.median_ns.is_finite()
                && point.p95_ns.is_finite()
                && point.p99_ns.is_finite()
        })
        .collect();
    data.sort_by_key(|point| point.working_set_bytes);
    if data.is_empty() {
        return;
    }
    ui.heading("Working set versus latency");
    ui.label(egui::RichText::new("Median line; vertical marks span reported minimum to p99. The p95 tick is a reported percentile, not a reconstructed distribution.").color(p.secondary));
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(300.0), 270.0),
        egui::Sense::hover(),
    );
    let chart = Rect::from_min_max(
        rect.min + Vec2::new(56.0, 18.0),
        rect.max - Vec2::new(14.0, 36.0),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(8), p.surface);
    painter.rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    let ymin = 0.0f32;
    let ymax = data
        .iter()
        .fold(1.0f32, |max, point| max.max(point.p99_ns as f32))
        * 1.08;
    for row in 0..=4 {
        let y = chart.top() + chart.height() * row as f32 / 4.0;
        painter.line_segment(
            [Pos2::new(chart.left(), y), Pos2::new(chart.right(), y)],
            Stroke::new(1.0, p.border),
        );
        painter.text(
            Pos2::new(chart.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{:.0}", ymin + (ymax - ymin) * (4 - row) as f32 / 4.0),
            egui::FontId::proportional(11.0),
            p.secondary,
        );
    }
    painter.text(
        rect.left_top() + Vec2::new(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        "ns",
        egui::FontId::proportional(11.0),
        p.secondary,
    );
    let log_min = (data[0].working_set_bytes as f64).log2();
    let log_max = (data[data.len() - 1].working_set_bytes as f64).log2();
    let x_for = |bytes: u64| {
        chart.left()
            + (((bytes as f64).log2() - log_min) / (log_max - log_min).max(1.0)) as f32
                * chart.width()
    };
    let y_for = |ns: f64| chart.bottom() - ((ns as f32 / ymax).clamp(0.0, 1.0) * chart.height());
    let mut median_points = Vec::with_capacity(data.len());
    for point in &data {
        let x = x_for(point.working_set_bytes);
        let median = Pos2::new(x, y_for(point.median_ns));
        median_points.push(median);
        painter.line_segment(
            [
                Pos2::new(x, y_for(point.min_ns)),
                Pos2::new(x, y_for(point.p99_ns)),
            ],
            Stroke::new(3.0, p.cpu.gamma_multiply(0.5)),
        );
        painter.line_segment(
            [
                Pos2::new(x - 4.0, y_for(point.p95_ns)),
                Pos2::new(x + 4.0, y_for(point.p95_ns)),
            ],
            Stroke::new(1.5, p.focus),
        );
        painter.circle_filled(median, 3.0, p.cpu);
    }
    if median_points.len() >= 2 {
        painter.add(Shape::line(median_points, Stroke::new(2.0, p.cpu)));
    }
    painter.text(
        chart.left_bottom() + Vec2::new(0.0, 20.0),
        egui::Align2::LEFT_CENTER,
        format_bytes(data[0].working_set_bytes),
        egui::FontId::proportional(11.0),
        p.secondary,
    );
    painter.text(
        chart.right_bottom() + Vec2::new(0.0, 20.0),
        egui::Align2::RIGHT_CENTER,
        format_bytes(data[data.len() - 1].working_set_bytes),
        egui::FontId::proportional(11.0),
        p.secondary,
    );
    if let Some(pointer) = response.hover_pos()
        && chart.contains(pointer)
        && let Some(point) = data.iter().min_by(|a, b| {
            (x_for(a.working_set_bytes) - pointer.x)
                .abs()
                .total_cmp(&(x_for(b.working_set_bytes) - pointer.x).abs())
        })
    {
        response.on_hover_text(format!(
            "{} working set\nMinimum {} · Median {}\nP95 {} · P99 {}",
            format_bytes(point.working_set_bytes),
            format_latency_ns(point.min_ns),
            format_latency_ns(point.median_ns),
            format_latency_ns(point.p95_ns),
            format_latency_ns(point.p99_ns)
        ));
    }
    ui.add_space(10.0);
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
