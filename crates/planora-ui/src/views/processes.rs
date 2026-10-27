use crate::{
    components::{
        header::render_page_header,
        plot::{PlotSeries, PlotState, ValueScale, time_plot},
        table::{ProcessTableState, render_process_table, sort_processes},
    },
    formatting::{format_bytes, format_percent, format_rate},
    history::{HistoryRange, ResourceHistories},
    state::{AppState, DisplayPause},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Stroke, Vec2};
use planora_core::{Command, ProcessIdentity, ProcessRecord, TelemetryFrame, Topology};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::SyncSender,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct ProcessesViewState {
    pub table_state: ProcessTableState,
    pub affinity_selection: HashSet<u16>,
    pub pending_affinity_confirm: bool,
    pub show_column_chooser: bool,
    /// Adjustable inspector width on wide desktops (logical pixels).
    pub inspector_width: f32,
    pub affinity_identity: Option<ProcessIdentity>,
    pub affinity_requested: bool,
    pub affinity_initialized: bool,
    pub affinity_error: Option<String>,
    cached_order: Vec<ProcessIdentity>,
    last_sorted_at: Option<Instant>,
    last_sort_query: String,
    last_sort_column: crate::components::table::SortColumn,
    last_sort_descending: bool,
    last_quick_filter: QuickFilter,
    last_selected_identity: Option<ProcessIdentity>,
    pub quick_filter: QuickFilter,
    pub pinned: HashSet<ProcessIdentity>,
    pub inspector_trend: usize,
    inspector_plot: PlotState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QuickFilter {
    #[default]
    All,
    Selected,
    Pinned,
    Active,
}

impl ProcessesViewState {
    fn select_affinity_target(&mut self, identity: Option<&ProcessIdentity>) {
        if self.affinity_identity.as_ref() != identity {
            self.affinity_identity = identity.cloned();
            self.affinity_selection.clear();
            self.pending_affinity_confirm = false;
            self.affinity_requested = false;
            self.affinity_initialized = false;
            self.affinity_error = None;
        }
    }
}

pub fn render_processes_view(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    state: &mut ProcessesViewState,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    selected_identity: &mut Option<ProcessIdentity>,
    affinity_response: Option<&(ProcessIdentity, Vec<u16>)>,
    topology: Option<&Topology>,
    tx: &SyncSender<Command>,
    now: Instant,
    p: Palette,
) {
    state.select_affinity_target(selected_identity.as_ref());
    let current_affinity = affinity_response
        .filter(|(identity, _)| Some(identity) == selected_identity.as_ref())
        .map(|(_, cpus)| cpus);
    let actions_disabled = pause_state.is_paused || *app_state != AppState::Live;
    if !actions_disabled
        && !state.affinity_requested
        && let Some(identity) = selected_identity.as_ref()
    {
        match tx.try_send(Command::GetAffinity {
            process: identity.clone(),
        }) {
            Ok(()) => state.affinity_requested = true,
            Err(error) => state.affinity_error = Some(format!("Unable to query affinity: {error}")),
        }
    }
    // 1. Page Header
    render_page_header(
        ui,
        "Processes",
        "Active processes with micro-architectural resource metrics and CPU affinity management",
        app_state,
        pause_state,
        poll_interval_ms,
        p,
    );

    // 2. Action Toolbar: Search, Column Chooser, Total Count
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.table_state.search_query)
                .hint_text("Search by process name or PID...")
                .desired_width(280.0),
        );

        if !state.table_state.search_query.is_empty() && ui.button("✖").clicked() {
            state.table_state.search_query.clear();
        }

        ui.add_space(8.0);

        // Column chooser popup button
        ui.menu_button("Columns ▾", |ui| {
            ui.checkbox(&mut state.table_state.columns.show_pid, "PID");
            ui.checkbox(&mut state.table_state.columns.show_cpu, "CPU %");
            ui.checkbox(&mut state.table_state.columns.show_memory, "Memory");
            ui.checkbox(&mut state.table_state.columns.show_io_read, "I/O Read");
            ui.checkbox(&mut state.table_state.columns.show_io_write, "I/O Write");
            ui.checkbox(&mut state.table_state.columns.show_disk_read, "Disk Read");
            ui.checkbox(&mut state.table_state.columns.show_disk_write, "Disk Write");
            ui.checkbox(&mut state.table_state.columns.show_gpu, "GPU (optional)");
        });

        ui.menu_button("Filter", |ui| {
            for (filter, label) in [
                (QuickFilter::All, "All processes"),
                (QuickFilter::Selected, "Selected"),
                (QuickFilter::Pinned, "Pinned"),
                (QuickFilter::Active, "Resource activity"),
            ] {
                ui.selectable_value(&mut state.quick_filter, filter, label);
            }
        });
        if let Some(identity) = selected_identity.as_ref() {
            let pinned = state.pinned.contains(identity);
            if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                if pinned {
                    state.pinned.remove(identity);
                } else {
                    state.pinned.insert(identity.clone());
                }
                state.last_sorted_at = None;
            }
        }
        if (!state.table_state.search_query.is_empty() || state.quick_filter != QuickFilter::All)
            && ui.button("Clear filters").clicked()
        {
            state.table_state.search_query.clear();
            state.quick_filter = QuickFilter::All;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let total_count = frame.processes.len();
            ui.label(
                egui::RichText::new(format!("{total_count} processes"))
                    .color(p.secondary)
                    .size(12.0),
            );
        });
    });

    ui.add_space(10.0);

    // Filter & Sort
    let query_changed = state.last_sort_query != state.table_state.search_query
        || state.last_sort_column != state.table_state.sort_column
        || state.last_sort_descending != state.table_state.sort_descending
        || state.last_quick_filter != state.quick_filter
        || (state.quick_filter == QuickFilter::Selected
            && state.last_selected_identity != *selected_identity);
    let expired = state
        .last_sorted_at
        .is_none_or(|last| last.elapsed() >= Duration::from_secs(1));
    if query_changed || (expired && !ui.input(|i| i.pointer.any_down())) {
        let mut sorted = sort_processes(
            &frame.processes,
            &state.table_state.search_query,
            state.table_state.sort_column,
            state.table_state.sort_descending,
        );
        sorted.retain(|process| match state.quick_filter {
            QuickFilter::All => true,
            QuickFilter::Selected => selected_identity.as_ref() == Some(&process.identity),
            QuickFilter::Pinned => state.pinned.contains(&process.identity),
            QuickFilter::Active => {
                process.cpu_percent.value.is_some_and(|value| value > 0.0)
                    || process
                        .io_read_bytes_per_sec
                        .value
                        .is_some_and(|value| value > 0)
                    || process
                        .io_write_bytes_per_sec
                        .value
                        .is_some_and(|value| value > 0)
                    || process
                        .disk_read_bytes_per_sec
                        .value
                        .is_some_and(|value| value > 0)
                    || process
                        .disk_write_bytes_per_sec
                        .value
                        .is_some_and(|value| value > 0)
            }
        });
        state.cached_order = sorted.iter().map(|p| p.identity.clone()).collect();
        state.last_sorted_at = Some(Instant::now());
        state
            .last_sort_query
            .clone_from(&state.table_state.search_query);
        state.last_sort_column = state.table_state.sort_column;
        state.last_sort_descending = state.table_state.sort_descending;
        state.last_quick_filter = state.quick_filter;
        state.last_selected_identity.clone_from(selected_identity);
    }
    let by_identity: HashMap<_, _> = frame.processes.iter().map(|p| (&p.identity, p)).collect();
    let rows: Vec<&ProcessRecord> = state
        .cached_order
        .iter()
        .filter_map(|id| by_identity.get(id).copied())
        .collect();
    if rows.is_empty() {
        ui.label(
            egui::RichText::new(
                "No processes match these filters. Clear filters to show all processes.",
            )
            .color(p.secondary),
        );
    }

    let available_w = ui.available_width().max(1.0);
    let available_h = ui.available_height();
    let text_scale = crate::appearance::text_scale(ui.ctx()).max(1.0);
    let is_wide = available_w >= 1020.0 * text_scale.min(1.25);
    let selected_record = selected_identity
        .as_ref()
        .and_then(|id| frame.processes.iter().find(|p| &p.identity == id));

    if is_wide {
        let spacing_x = ui.spacing().item_spacing.x;
        let grip_w = 12.0;
        let total_grip = grip_w + 2.0 * spacing_x;
        let min_table_w = 380.0;
        let min_inspector_w = (320.0 * text_scale).min(420.0);
        let max_inspector = (available_w - min_table_w - total_grip).max(min_inspector_w);
        let default_inspector = (360.0 * text_scale).clamp(min_inspector_w, max_inspector);
        let inspector_w = if state.inspector_width <= 0.0 {
            default_inspector
        } else {
            state.inspector_width.clamp(min_inspector_w, max_inspector)
        };
        let table_w = (available_w - inspector_w - total_grip).max(min_table_w);

        ui.horizontal(|ui| {
            ui.set_min_height(available_h);
            ui.allocate_ui_with_layout(
                Vec2::new(table_w, available_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_min_height(available_h);
                    render_process_table(
                        ui,
                        &rows,
                        selected_identity,
                        &mut state.table_state,
                        30.0,
                        p,
                    );
                },
            );

            // Native draggable split handle; keep the table usable.
            let (grip, drag) = ui.allocate_exact_size(
                Vec2::new(grip_w, available_h.max(100.0)),
                egui::Sense::drag(),
            );
            ui.painter().line_segment(
                [grip.center_top(), grip.center_bottom()],
                Stroke::new(2.0, p.border_control),
            );
            if drag.dragged() {
                state.inspector_width =
                    (inspector_w - drag.drag_delta().x).clamp(min_inspector_w, max_inspector);
            }

            // Right Inspector Pane
            ui.allocate_ui_with_layout(
                Vec2::new(inspector_w, available_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_min_height(available_h);
                    render_inspector_pane(
                        ui,
                        selected_record,
                        selected_identity.as_ref(),
                        histories,
                        state,
                        actions_disabled || pause_state.is_paused,
                        current_affinity,
                        topology,
                        &frame.gpu_engines,
                        tx,
                        now,
                        p,
                    );
                },
            );
        });
    } else {
        // Narrow window: Top Table, Bottom Inspector if selected
        if let Some(record) = selected_record {
            let inspector_h = 240.0;
            let table_h = (available_h - inspector_h - 16.0).max(200.0);

            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), table_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    render_process_table(
                        ui,
                        &rows,
                        selected_identity,
                        &mut state.table_state,
                        28.0,
                        p,
                    );
                },
            );

            ui.add_space(16.0);
            render_inspector_pane(
                ui,
                Some(record),
                selected_identity.as_ref(),
                histories,
                state,
                actions_disabled || pause_state.is_paused,
                current_affinity,
                topology,
                &frame.gpu_engines,
                tx,
                now,
                p,
            );
        } else {
            render_process_table(
                ui,
                &rows,
                selected_identity,
                &mut state.table_state,
                30.0,
                p,
            );
            if let Some(identity) = selected_identity.as_ref() {
                ui.colored_label(
                    p.warning,
                    format!(
                        "Selected PID {} has exited or is unavailable in this snapshot.",
                        identity.pid
                    ),
                );
            }
        }
    }
}

fn render_inspector_pane(
    ui: &mut egui::Ui,
    selected_record: Option<&ProcessRecord>,
    selected_identity: Option<&ProcessIdentity>,
    histories: &ResourceHistories,
    state: &mut ProcessesViewState,
    is_paused: bool,
    current_affinity: Option<&Vec<u16>>,
    topology: Option<&Topology>,
    gpu_engines: &[planora_core::GpuEngineSample],
    tx: &SyncSender<Command>,
    now: Instant,
    p: Palette,
) {
    // Editing controls are Windows-only; other targets show read-only details.
    #[cfg(not(windows))]
    let _ = (
        &state,
        is_paused,
        current_affinity,
        topology,
        tx,
        gpu_engines,
    );
    let inner_w = (ui.available_width() - 30.0).max(1.0);
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(inner_w);
            ui.set_max_width(inner_w);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    ui.set_max_width(inner_w);
                    let Some(record) = selected_record else {
                        ui.heading("Process Inspector");
                        ui.add_space(8.0);
                        if let Some(identity) = selected_identity {
                            ui.colored_label(p.warning, format!("PID {} is no longer in the current process snapshot.", identity.pid));
                            ui.label("Its recorded selection remains until you choose another process or clear it.");
                            return;
                        }
                        ui.label(
                            egui::RichText::new("Select a process from the table to view real-time trends, grouped measurements, and CPU affinity controls.")
                                .color(p.secondary)
                                .size(12.5),
                        );
                        return;
                    };

            let pid_label = egui::RichText::new(format!("PID {}", record.identity.pid))
                .monospace()
                .strong()
                .color(p.primary);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(pid_label);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&record.name).heading()).truncate());
                    });
                });
            });

            ui.label(
                egui::RichText::new(format!(
                    "Started: {} ms · Identity: {:?}",
                    record.identity.started, record.identity.source
                ))
                .size(11.0)
                .color(p.secondary),
            );

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            ui.strong("Observed history (last 60 seconds)");
            if histories.selected_identity.as_ref() == Some(&record.identity) {
                ui.horizontal(|ui| {
                    for (index, label) in ["CPU", "Memory", "Process I/O"].iter().enumerate() {
                        ui.selectable_value(&mut state.inspector_trend, index, *label);
                    }
                });
                let (series, unit, val_scale): (Vec<PlotSeries<'_>>, &str, ValueScale) = match state.inspector_trend {
                    1 => (vec![PlotSeries { name: "Resident memory", samples: &histories.selected_memory, color: p.memory, dashed: false }], "bytes", ValueScale::Auto),
                    2 => (vec![
                        PlotSeries { name: "I/O read", samples: &histories.selected_io_read, color: p.disk, dashed: false },
                        PlotSeries { name: "I/O write", samples: &histories.selected_io_write, color: p.accent, dashed: true },
                    ], "B/s", ValueScale::Auto),
                    _ => (vec![PlotSeries { name: "CPU utilization", samples: &histories.selected_cpu, color: p.cpu, dashed: false }], "%", ValueScale::Percent { full: false }),
                };
                time_plot(ui, ("process_inspector", record.identity.pid, record.identity.started), &series, HistoryRange::Sec60, now, unit, val_scale, 178.0, &mut state.inspector_plot, p);
            } else {
                ui.label("Collecting history for this process identity.");
            }
            egui::Grid::new("selected_process_history")
                .striped(true)
                .num_columns(4)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("Metric");
                    ui.strong("Average");
                    ui.strong("Range");
                    ui.strong("Coverage");
                    ui.end_row();
                    let fmt_val = |v: f32, u: &str| -> String {
                        match u {
                            "%" => format_percent(Some(v)),
                            "bytes" => format_bytes(v.max(0.0) as u64),
                            "B/s" | "bytes/s" => format_rate(Some(v)),
                            _ => format!("{:.1} {u}", v),
                        }
                    };
                    for (label, series, unit, color) in [
                        ("CPU", &histories.selected_cpu, "%", p.cpu),
                        ("Memory", &histories.selected_memory, "bytes", p.memory),
                        ("I/O read", &histories.selected_io_read, "B/s", p.disk),
                        ("I/O write", &histories.selected_io_write, "B/s", p.disk),
                    ] {
                        ui.colored_label(color, label);
                        if let Some(summary) = series.observed_summary(now, HistoryRange::Sec60) {
                            ui.monospace(fmt_val(summary.average, unit));
                            let range_str = format!("{}–{}", fmt_val(summary.minimum, unit), fmt_val(summary.maximum, unit));
                            ui.add(egui::Label::new(egui::RichText::new(range_str).monospace()).truncate());
                            ui.label(format!("{}/{}s", summary.observed_seconds, summary.expected_seconds));
                        } else {
                            ui.label("—");
                            ui.label("—");
                            ui.label("Collecting");
                        }
                        ui.end_row();
                    }
                });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            // Grouped Measurements
            ui.strong("Resource Measurements");
            egui::Grid::new("inspector_metrics")
                .striped(true)
                .num_columns(2)
                .spacing([18.0, 4.0])
                .show(ui, |ui| {
                    ui.label("CPU Utilization:");
                    ui.label(egui::RichText::new(format_percent(record.cpu_percent.value)).monospace());
                    ui.end_row();

                    ui.label("Resident Memory:");
                    let mem_s = record.memory_bytes.value.map(format_bytes).unwrap_or_else(|| "Unavailable".into());
                    ui.label(egui::RichText::new(mem_s).monospace());
                    ui.end_row();

                    ui.label("Disk Read Rate:");
                    let dr_s = format_rate(record.disk_read_bytes_per_sec.value.map(|v| v as f32));
                    ui.label(egui::RichText::new(dr_s).monospace());
                    ui.end_row();

                    ui.label("Disk Write Rate:");
                    let dw_s = format_rate(record.disk_write_bytes_per_sec.value.map(|v| v as f32));
                    ui.label(egui::RichText::new(dw_s).monospace());
                    ui.end_row();

                    ui.label("Process I/O Read:").on_hover_text("All process I/O reported by Windows; this is not disk-only activity.");
                    let io_read = format_rate(record.io_read_bytes_per_sec.value.map(|v| v as f32));
                    ui.label(egui::RichText::new(io_read).monospace()).on_hover_text(format!(
                        "Source: {} · {}",
                        record.io_read_bytes_per_sec.source,
                        record.io_read_bytes_per_sec.unavailable_reason.as_ref().map_or_else(|| "Available".to_string(), ToString::to_string),
                    ));
                    ui.end_row();

                    ui.label("Process I/O Write:").on_hover_text("All process I/O reported by Windows; this is not disk-only activity.");
                    let io_write = format_rate(record.io_write_bytes_per_sec.value.map(|v| v as f32));
                    ui.label(egui::RichText::new(io_write).monospace()).on_hover_text(format!(
                        "Source: {} · {}",
                        record.io_write_bytes_per_sec.source,
                        record.io_write_bytes_per_sec.unavailable_reason.as_ref().map_or_else(|| "Available".to_string(), ToString::to_string),
                    ));
                    ui.end_row();

                    ui.label("GPU Utilization:");
                    ui.label(egui::RichText::new(format_percent(record.gpu_percent.value)).monospace());
                    ui.end_row();
                });

            let matching_engines: Vec<_> = gpu_engines
                .iter()
                .filter(|e| e.process_pid == Some(record.identity.pid))
                .collect();
            if !matching_engines.is_empty() {
                ui.add_space(8.0);
                ui.strong(format!("GPU Engines ({})", matching_engines.len()));
                egui::Grid::new("proc_gpu_engines_grid")
                    .striped(true)
                    .num_columns(3)
                    .spacing([14.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Adapter");
                        ui.strong("Engine");
                        ui.strong("Usage");
                        ui.end_row();
                        for eng in matching_engines {
                            let luid_str = eng
                                .adapter_luid
                                .map(|l| format!("0x{l:x}"))
                                .unwrap_or_else(|| "Adapter".into());
                            ui.label(luid_str);
                            ui.label(&eng.engine_type);
                            ui.monospace(format_percent(eng.utilization_percent.value));
                            ui.end_row();
                        }
                    });
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            // CPU Affinity Management
            ui.strong("CPU Affinity Configuration");
            #[cfg(target_os = "linux")]
            {
                ui.colored_label(
                    p.secondary,
                    "Affinity modification is disabled on Linux in v1. Linux manages affinity per-thread; multi-threaded process guarantees require frozen task-tree synchronization.",
                );
            }

            #[cfg(windows)]
            {
                if is_paused {
                    ui.colored_label(
                        p.warning,
                        "Affinity changes require live, unpaused telemetry (disabled during replay or pause).",
                    );
                    return;
                }

                let mut available_cpus: Vec<u16> = topology
                    .into_iter().flat_map(|t| &t.cores)
                    .flat_map(|core| core.logical_cpus.iter().copied())
                    .collect();
                available_cpus.sort_unstable();
                available_cpus.dedup();
                if available_cpus.is_empty() || available_cpus.iter().any(|cpu| *cpu >= 64) {
                    ui.label("Affinity editing is unavailable for this processor topology.");
                    return;
                }
                if let Some(error) = &state.affinity_error { ui.colored_label(p.error, error); }
                let Some(current_affinity) = current_affinity else {
                    ui.label("Current affinity is unavailable. Query it before applying changes.");
                    if ui.button("Retry affinity query").clicked() { state.affinity_requested = false; }
                    return;
                };

                // Initialize selection if empty
                if !state.affinity_initialized {
                    for cpu in current_affinity {
                        state.affinity_selection.insert(*cpu);
                    }
                    state.affinity_initialized = true;
                }

                ui.horizontal_wrapped(|ui| {
                    for cpu_idx in available_cpus.iter().copied() {
                        let mut checked = state.affinity_selection.contains(&cpu_idx);
                        if ui.checkbox(&mut checked, format!("CPU {cpu_idx}")).changed() {
                            if checked {
                                state.affinity_selection.insert(cpu_idx);
                            } else {
                                state.affinity_selection.remove(&cpu_idx);
                            }
                        }
                    }
                });

                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("All").clicked() {
                        for cpu_idx in available_cpus.iter().copied() {
                            state.affinity_selection.insert(cpu_idx);
                        }
                    }
                    if ui.small_button("Clear").clicked() {
                        state.affinity_selection.clear();
                    }
                    if ui.add_enabled(!state.affinity_selection.is_empty(), egui::Button::new("Apply Affinity...")).clicked() {
                        state.pending_affinity_confirm = true;
                    }
                });

                // Confirmation Modal / Expander
                if state.pending_affinity_confirm {
                    ui.add_space(6.0);
                    let mut proposed: Vec<u16> = state.affinity_selection.iter().copied().collect();
                    proposed.sort_unstable();

                    egui::Frame::group(ui.style())
                        .fill(p.surface_raised)
                        .stroke(Stroke::new(1.0, p.focus))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.colored_label(p.focus, "Confirm Affinity Change");
                            ui.label(format!("Target: PID {} ({})", record.identity.pid, record.name));
                            ui.label(format!("Old CPUs: {:?}", current_affinity));
                            ui.label(format!("Proposed CPUs: {:?}", proposed));
                            ui.small("Process creation identity will be revalidated through the opened handle.");

                            ui.horizontal_wrapped(|ui| {
                                if ui.add_enabled(!proposed.is_empty(), egui::Button::new("Confirm & Apply")).clicked() {
                                    state.pending_affinity_confirm = false;
                                    if let Err(error) = tx.try_send(Command::SetAffinity {
                                        process: record.identity.clone(),
                                        logical_cpus: proposed.clone(),
                                    }) { state.affinity_error = Some(format!("Unable to apply affinity: {error}")); }
                                }
                                if ui.button("Cancel").clicked() {
                                    state.pending_affinity_confirm = false;
                                }
                            });
                        });
                }
            }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_process_clears_pending_affinity_and_query_state() {
        let first = ProcessIdentity {
            pid: 42,
            started: 1,
            sequence_number: None,
            source: planora_core::IdentitySource::WindowsProcessFiletime,
        };
        let reused = ProcessIdentity {
            started: 2,
            ..first.clone()
        };
        let mut state = ProcessesViewState::default();
        state.select_affinity_target(Some(&first));
        state.affinity_selection.insert(3);
        state.pending_affinity_confirm = true;
        state.affinity_requested = true;
        state.affinity_initialized = true;
        state.select_affinity_target(Some(&first));
        assert!(state.pending_affinity_confirm);
        state.select_affinity_target(Some(&reused));
        assert!(state.affinity_selection.is_empty());
        assert!(!state.pending_affinity_confirm);
        assert!(!state.affinity_requested);
        assert!(!state.affinity_initialized);
    }
    #[test]
    fn processes_view_inspector_contained_within_viewport() {
        let ctx = egui::Context::default();
        let m = planora_core::Metric::available(10.0, "%", "test", 1000);
        let frame = planora_core::TelemetryFrame {
            protocol_version: planora_core::PROTOCOL_VERSION,
            session_id: 1,
            topology_generation: 1,
            sequence: 1,
            sampled_at_ms: 1000,
            collection_duration_ms: 0.1,
            cpu_percent: m.clone(),
            logical_cpus: Vec::new(),
            memory_used_bytes: planora_core::Metric::available(100_000_000, "bytes", "test", 1000),
            memory_available_bytes: planora_core::Metric::available(
                100_000_000,
                "bytes",
                "test",
                1000,
            ),
            memory_total_bytes: planora_core::Metric::available(200_000_000, "bytes", "test", 1000),
            commit_used_bytes: planora_core::Metric::available(100_000_000, "bytes", "test", 1000),
            commit_total_bytes: planora_core::Metric::available(200_000_000, "bytes", "test", 1000),
            disk_read_bytes_per_sec: planora_core::Metric::available(1024, "B/s", "test", 1000),
            disk_write_bytes_per_sec: planora_core::Metric::available(1024, "B/s", "test", 1000),
            network_rx_bytes_per_sec: planora_core::Metric::available(1024, "B/s", "test", 1000),
            network_tx_bytes_per_sec: planora_core::Metric::available(1024, "B/s", "test", 1000),
            gpu_percent: m.clone(),
            gpu_adapters: Vec::new(),
            gpu_engines: Vec::new(),
            gpu_engine_unavailable_reason: None,
            disks: Vec::new(),
            network_interfaces: Vec::new(),
            package_power_watts: planora_core::Metric::available(15.0, "W", "test", 1000),
            sensors: Vec::new(),
            processes: vec![planora_core::ProcessRecord {
                identity: ProcessIdentity {
                    pid: 13452,
                    started: 1234567,
                    sequence_number: None,
                    source: planora_core::IdentitySource::WindowsProcessFiletime,
                },
                name: "Wispr Flow.exe".into(),
                cpu_percent: planora_core::Metric::available(4.5, "%", "test", 1000),
                memory_bytes: planora_core::Metric::available(176_800_000, "bytes", "test", 1000),
                io_read_bytes_per_sec: planora_core::Metric::available(1024, "B/s", "test", 1000),
                io_write_bytes_per_sec: planora_core::Metric::available(2048, "B/s", "test", 1000),
                disk_read_bytes_per_sec: planora_core::Metric::available(1024, "B/s", "test", 1000),
                disk_write_bytes_per_sec: planora_core::Metric::available(
                    2048, "B/s", "test", 1000,
                ),
                gpu_percent: planora_core::Metric::available(0.0, "%", "test", 1000),
                affinity: planora_core::Metric::unavailable(
                    "cpus",
                    "test",
                    1000,
                    planora_core::AvailabilityReason::Unsupported("test".into()),
                ),
            }],
        };
        let mut histories = crate::history::ResourceHistories::default();
        let identity = frame.processes[0].identity.clone();
        histories.ingest_frame(&frame, &[], Some(&identity), Instant::now(), 1000);

        let p = crate::theme::get_palette(true);
        let (tx, _rx) = std::sync::mpsc::sync_channel(1);
        let mut pause_state = DisplayPause::default();
        let app_state = AppState::Live;

        for width in [800.0, 1024.0, 1180.0, 1536.0, 1920.0] {
            let mut state = ProcessesViewState::default();
            let mut selected = Some(identity.clone());
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(width, 790.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(width);
                        render_processes_view(
                            ui,
                            &frame,
                            &histories,
                            &mut state,
                            &app_state,
                            &mut pause_state,
                            1000,
                            &mut selected,
                            None,
                            None,
                            &tx,
                            Instant::now(),
                            p,
                        );
                        assert!(
                            ui.min_rect().right() <= ui.max_rect().right() + 1.5,
                            "Width {width} overflowed right edge: {} > {}",
                            ui.min_rect().right(),
                            width
                        );
                    });
                },
            );
            output.textures_delta.clear();
        }
    }
}
