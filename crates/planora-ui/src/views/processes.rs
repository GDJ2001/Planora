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
    collections::HashSet,
    sync::mpsc::SyncSender,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct ProcessesViewState {
    pub table_state: ProcessTableState,
    pub affinity_selection: HashSet<u16>,
    pub pending_affinity_confirm: bool,
    pub show_column_chooser: bool,
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
    current_affinity: Option<&Vec<u16>>,
    topology: Option<&Topology>,
    tx: &SyncSender<Command>,
    now: Instant,
    p: Palette,
) {
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
    let rows = sort_processes(
        &frame.processes,
        &state.table_state.search_query,
        state.table_state.sort_column,
        state.table_state.sort_descending,
    );

    let is_wide = ui.available_width() >= 1100.0;
    let selected_record = selected_identity
        .as_ref()
        .and_then(|id| frame.processes.iter().find(|p| &p.identity == id));

    if is_wide {
        // Wide window: Left Table (70%), Right Inspector (30%)
        let inspector_w = 330.0;
        let table_w = ui.available_width() - inspector_w - 16.0;

        ui.horizontal(|ui| {
            ui.allocate_ui(Vec2::new(table_w, ui.available_height()), |ui| {
                render_process_table(
                    ui,
                    &rows,
                    selected_identity,
                    &mut state.table_state,
                    30.0,
                    p,
                );
            });

            ui.add_space(16.0);

            // Right Inspector Pane
            ui.allocate_ui(Vec2::new(inspector_w, ui.available_height()), |ui| {
                render_inspector_pane(
                    ui,
                    selected_record,
                    histories,
                    state,
                    pause_state.is_paused,
                    current_affinity,
                    topology,
                    tx,
                    now,
                    p,
                );
            });
        });
    } else {
        // Narrow window: Top Table, Bottom Inspector if selected
        if let Some(record) = selected_record {
            let inspector_h = 240.0;
            let table_h = (ui.available_height() - inspector_h - 16.0).max(200.0);

            ui.allocate_ui(Vec2::new(ui.available_width(), table_h), |ui| {
                render_process_table(
                    ui,
                    &rows,
                    selected_identity,
                    &mut state.table_state,
                    28.0,
                    p,
                );
            });

            ui.add_space(16.0);
            render_inspector_pane(
                ui,
                Some(record),
                histories,
                state,
                pause_state.is_paused,
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
        }
    }
}

fn render_inspector_pane(
    ui: &mut egui::Ui,
    selected_record: Option<&ProcessRecord>,
    histories: &ResourceHistories,
    state: &mut ProcessesViewState,
    is_paused: bool,
    current_affinity: Option<&Vec<u16>>,
    topology: Option<&Topology>,
    tx: &SyncSender<Command>,
    now: Instant,
    p: Palette,
) {
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            let Some(record) = selected_record else {
                ui.heading("Process Inspector");
                ui.add_space(8.0);
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
                        "⏸ Display is paused. Resume live telemetry to modify process affinity.",
                    );
                    return;
                }

                let total_cpus = topology
                    .map(|t| t.cores.iter().map(|c| c.logical_cpus.len()).sum::<usize>())
                    .unwrap_or(8)
                    .min(64);

                // Initialize selection if empty
                if state.affinity_selection.is_empty()
                    && let Some(cur) = current_affinity
                {
                    for cpu in cur {
                        state.affinity_selection.insert(*cpu);
                    }
                }

                ui.horizontal_wrapped(|ui| {
                    for cpu_idx in 0..total_cpus as u16 {
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
                        for cpu_idx in 0..total_cpus as u16 {
                            state.affinity_selection.insert(cpu_idx);
                        }
                    }
                    if ui.small_button("Clear").clicked() {
                        state.affinity_selection.clear();
                    }
                    if ui.button("Apply Affinity...").clicked() {
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
                            ui.label(format!("Old CPUs: {:?}", current_affinity.unwrap_or(&vec![])));
                            ui.label(format!("Proposed CPUs: {:?}", proposed));
                            ui.small("Process creation identity will be revalidated through the opened handle.");

                            ui.horizontal(|ui| {
                                if ui.button("Confirm & Apply").clicked() {
                                    state.pending_affinity_confirm = false;
                                    let _ = tx.send(Command::SetAffinity {
                                        process: record.identity.clone(),
                                        logical_cpus: proposed.clone(),
                                    });
                                }
                                if ui.button("Cancel").clicked() {
                                    state.pending_affinity_confirm = false;
                                }
                            });
                        });
                }
            }
        });
}
