use crate::{
    components::{
        header::render_page_header,
        table::{ProcessTableState, render_process_table, sort_processes},
    },
    formatting::{format_bytes, format_percent, format_rate},
    history::{HistoryRange, ResourceHistories},
    state::{AppState, DisplayPause},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Pos2, Stroke, Vec2};
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
                .hint_text("🔍 Search by process name or PID...")
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

    let available_h = ui.available_height();
    let is_wide = ui.available_width() >= 1100.0;
    let selected_record = selected_identity
        .as_ref()
        .and_then(|id| frame.processes.iter().find(|p| &p.identity == id));

    if is_wide {
        // The middle grip is draggable without assuming a fixed 30% inspector.
        let max_inspector = (ui.available_width() - 620.0).max(280.0);
        let inspector_w = if state.inspector_width <= 0.0 {
            330.0f32.min(max_inspector)
        } else {
            state.inspector_width.clamp(280.0, max_inspector)
        };
        let table_w = (ui.available_width() - inspector_w - 12.0).max(300.0);

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
            let (grip, drag) = ui
                .allocate_exact_size(Vec2::new(12.0, available_h.max(100.0)), egui::Sense::drag());
            ui.painter().line_segment(
                [grip.center_top(), grip.center_bottom()],
                Stroke::new(2.0, p.border_control),
            );
            if drag.dragged() {
                state.inspector_width =
                    (inspector_w - drag.drag_delta().x).clamp(280.0, max_inspector);
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
    tx: &SyncSender<Command>,
    now: Instant,
    p: Palette,
) {
    // Editing controls are Windows-only; other targets show read-only details.
    #[cfg(not(windows))]
    let _ = (&state, is_paused, current_affinity, topology, tx);
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
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

            ui.horizontal(|ui| {
                ui.heading(&record.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("PID {}", record.identity.pid))
                            .monospace()
                            .strong()
                            .color(p.primary),
                    );
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

            // Selected Process Sparkline Trends (recorded only while observed!)
            ui.strong("Observed Trends (Since Selection)");
            ui.add_space(4.0);

            let (spark_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), egui::Sense::hover());
            ui.painter().rect_filled(spark_rect, CornerRadius::same(3), p.surface_raised);

            let segs_cpu = histories.selected_cpu.normalized_segments(now, HistoryRange::Sec60, Duration::from_secs(3));
            let segs_mem = histories.selected_memory.normalized_segments(now, HistoryRange::Sec60, Duration::from_secs(3));

            for seg in segs_cpu {
                if seg.len() >= 2 {
                    let pts: Vec<Pos2> = seg
                        .iter()
                        .map(|(norm_x, val)| {
                            let x = spark_rect.min.x + norm_x * spark_rect.width();
                            let norm_y = (val / 100.0).clamp(0.0, 1.0);
                            let y = spark_rect.max.y - norm_y * (spark_rect.height() - 4.0) - 2.0;
                            Pos2::new(x, y)
                        })
                        .collect();
                    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.8, p.cpu)));
                }
            }

            for seg in segs_mem {
                if seg.len() >= 2 {
                    let max_mem = 1024.0 * 1024.0 * 1024.0 * 2.0; // 2 GiB
                    let pts: Vec<Pos2> = seg
                        .iter()
                        .map(|(norm_x, val)| {
                            let x = spark_rect.min.x + norm_x * spark_rect.width();
                            let norm_y = (val / max_mem).clamp(0.0, 1.0);
                            let y = spark_rect.max.y - norm_y * (spark_rect.height() - 4.0) - 2.0;
                            Pos2::new(x, y)
                        })
                        .collect();
                    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.5, p.memory)));
                }
            }

            ui.horizontal(|ui| {
                ui.colored_label(p.cpu, "● CPU %");
                ui.add_space(10.0);
                ui.colored_label(p.memory, "● Memory");
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
                        "Affinity changes require live, unpaused telemetry.",
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
                ui.horizontal(|ui| {
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

                            ui.horizontal(|ui| {
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
}
