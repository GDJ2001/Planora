use crate::{
    components::{
        cards::{render_secondary_resource_panel, render_summary_cards},
        charts::{ChartPanelState, ChartScale, ChartSeriesConfig, render_chart_panel},
        header::render_page_header,
        table::{ProcessTableState, SortColumn, render_process_table, sort_processes},
    },
    history::ResourceHistories,
    state::{AppState, DisplayPause, Page},
    theme::Palette,
};
use eframe::egui::{self, CornerRadius, Stroke, Vec2};
use planora_core::{ProcessIdentity, TelemetryFrame};
use std::time::Instant;

#[derive(Default)]
pub struct OverviewViewState {
    pub cpu_chart: ChartPanelState,
    pub mem_chart: ChartPanelState,
    pub disk_chart: ChartPanelState,
    pub net_chart: ChartPanelState,
    pub top_by_cpu: bool, // true: by CPU, false: by Memory
    pub top_table: ProcessTableState,
    pub shared_crosshair_x: Option<f32>,
}

pub fn render_overview_view(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    state: &mut OverviewViewState,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    selected_identity: &mut Option<ProcessIdentity>,
    active_page: &mut Page,
    now: Instant,
    p: Palette,
) {
    // 1. Consistent Page Header
    render_page_header(
        ui,
        "System Overview",
        "Comprehensive real-time micro-architectural telemetry and resource breakdown",
        app_state,
        pause_state,
        poll_interval_ms,
        p,
    );

    // 2. 4 Summary Cards (CPU, Memory, Disk, Network)
    render_summary_cards(ui, frame, histories, now, p);
    ui.add_space(14.0);

    // 3. Secondary panel: GPU & Package Power
    render_secondary_resource_panel(ui, frame, p);
    ui.add_space(18.0);

    // 4. 4 Chart Panels in 2-column responsive grid (collapsing to 1-column if < 780px)
    let available_w = (ui.available_width() - 4.0).max(100.0);
    let chart_gap = 16.0;
    let chart_cols = calculate_chart_columns(available_w);
    let chart_w = calculate_chart_width(available_w, chart_gap, chart_cols);
    let is_wide = chart_cols == 2;

    let mut crosshair_x = state.shared_crosshair_x;

    if is_wide {
        // Row 1: CPU and Memory
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = chart_gap;
            ui.allocate_ui_with_layout(
                Vec2::new(chart_w, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    render_chart_panel(
                        ui,
                        "CPU Utilization",
                        &mut state.cpu_chart,
                        ChartScale::Fixed0To100,
                        vec![ChartSeriesConfig {
                            name: "System CPU",
                            color: p.cpu,
                            series: &histories.cpu,
                            unit: "%",
                        }],
                        now,
                        &mut crosshair_x,
                        p,
                    );
                },
            );

            ui.allocate_ui_with_layout(
                Vec2::new(chart_w, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    render_chart_panel(
                        ui,
                        "Memory Usage",
                        &mut state.mem_chart,
                        ChartScale::Fixed0To100,
                        vec![ChartSeriesConfig {
                            name: "Memory Used",
                            color: p.memory,
                            series: &histories.memory_pct,
                            unit: "%",
                        }],
                        now,
                        &mut crosshair_x,
                        p,
                    );
                },
            );
        });

        ui.add_space(chart_gap);

        // Row 2: Disk and Network
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = chart_gap;
            ui.allocate_ui_with_layout(
                Vec2::new(chart_w, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    render_chart_panel(
                        ui,
                        "Disk Throughput",
                        &mut state.disk_chart,
                        ChartScale::Throughput,
                        vec![
                            ChartSeriesConfig {
                                name: "Read",
                                color: p.disk,
                                series: &histories.disk_read,
                                unit: "B/s",
                            },
                            ChartSeriesConfig {
                                name: "Write",
                                color: p.accent,
                                series: &histories.disk_write,
                                unit: "B/s",
                            },
                        ],
                        now,
                        &mut crosshair_x,
                        p,
                    );
                },
            );

            ui.allocate_ui_with_layout(
                Vec2::new(chart_w, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    render_chart_panel(
                        ui,
                        "Network Activity",
                        &mut state.net_chart,
                        ChartScale::Throughput,
                        vec![
                            ChartSeriesConfig {
                                name: "Receive (↓)",
                                color: p.network,
                                series: &histories.net_rx,
                                unit: "B/s",
                            },
                            ChartSeriesConfig {
                                name: "Transmit (↑)",
                                color: p.focus,
                                series: &histories.net_tx,
                                unit: "B/s",
                            },
                        ],
                        now,
                        &mut crosshair_x,
                        p,
                    );
                },
            );
        });
    } else {
        // Narrow window: 1-column layout
        ui.vertical(|ui| {
            render_chart_panel(
                ui,
                "CPU Utilization",
                &mut state.cpu_chart,
                ChartScale::Fixed0To100,
                vec![ChartSeriesConfig {
                    name: "System CPU",
                    color: p.cpu,
                    series: &histories.cpu,
                    unit: "%",
                }],
                now,
                &mut crosshair_x,
                p,
            );

            ui.add_space(chart_gap);

            render_chart_panel(
                ui,
                "Memory Usage",
                &mut state.mem_chart,
                ChartScale::Fixed0To100,
                vec![ChartSeriesConfig {
                    name: "Memory Used",
                    color: p.memory,
                    series: &histories.memory_pct,
                    unit: "%",
                }],
                now,
                &mut crosshair_x,
                p,
            );

            ui.add_space(chart_gap);

            render_chart_panel(
                ui,
                "Disk Throughput",
                &mut state.disk_chart,
                ChartScale::Throughput,
                vec![
                    ChartSeriesConfig {
                        name: "Read",
                        color: p.disk,
                        series: &histories.disk_read,
                        unit: "B/s",
                    },
                    ChartSeriesConfig {
                        name: "Write",
                        color: p.accent,
                        series: &histories.disk_write,
                        unit: "B/s",
                    },
                ],
                now,
                &mut crosshair_x,
                p,
            );

            ui.add_space(chart_gap);

            render_chart_panel(
                ui,
                "Network Activity",
                &mut state.net_chart,
                ChartScale::Throughput,
                vec![
                    ChartSeriesConfig {
                        name: "Receive (↓)",
                        color: p.network,
                        series: &histories.net_rx,
                        unit: "B/s",
                    },
                    ChartSeriesConfig {
                        name: "Transmit (↑)",
                        color: p.focus,
                        series: &histories.net_tx,
                        unit: "B/s",
                    },
                ],
                now,
                &mut crosshair_x,
                p,
            );
        });
    }

    // Clear a stale crosshair once no chart is hovered or keyboard-focused.
    let panels = [
        &state.cpu_chart,
        &state.mem_chart,
        &state.disk_chart,
        &state.net_chart,
    ];
    if !panels
        .iter()
        .any(|panel| panel.pointer_inside || panel.keyboard_cursor_x.is_some())
    {
        crosshair_x = None;
    }
    state.shared_crosshair_x = crosshair_x;
    ui.add_space(20.0);

    // 5. Top Processes Table with ranking tabs and "View all processes" action
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Top Active Processes");

                ui.add_space(16.0);
                // Ranking tabs
                if ui
                    .selectable_label(state.top_by_cpu, "Top by CPU")
                    .clicked()
                {
                    state.top_by_cpu = true;
                }
                if ui
                    .selectable_label(!state.top_by_cpu, "Top by Memory")
                    .clicked()
                {
                    state.top_by_cpu = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("View all processes →").clicked() {
                        *active_page = Page::Processes;
                    }
                });
            });

            ui.add_space(8.0);

            let sort_col = if state.top_by_cpu {
                SortColumn::Cpu
            } else {
                SortColumn::Memory
            };
            let all_sorted = sort_processes(&frame.processes, "", sort_col, true);
            let top_rows = &all_sorted[..all_sorted.len().min(8)];

            state.top_table.sort_column = sort_col;
            state.top_table.sort_descending = true;

            render_process_table(
                ui,
                top_rows,
                selected_identity,
                &mut state.top_table,
                28.0,
                p,
            );
        });
}

pub fn calculate_chart_columns(available_w: f32) -> usize {
    if available_w >= 780.0 { 2 } else { 1 }
}

pub fn calculate_chart_width(available_w: f32, gap: f32, columns: usize) -> f32 {
    let stroke_safety = 2.0;
    if columns == 2 {
        (((available_w - gap) / 2.0) - stroke_safety)
            .floor()
            .max(100.0)
    } else {
        (available_w - stroke_safety).floor().max(100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chart_grid_breakpoints() {
        assert_eq!(calculate_chart_columns(1280.0), 2);
        assert_eq!(calculate_chart_columns(780.0), 2);
        assert_eq!(calculate_chart_columns(779.0), 1);
        assert_eq!(calculate_chart_columns(500.0), 1);
    }

    #[test]
    fn test_chart_width_containment_at_target_resolutions() {
        // Target window widths: 1024, 1180, 1366, 1536, 1920
        let gap = 16.0;
        let window_widths = [1024.0, 1180.0, 1366.0, 1536.0, 1920.0];

        for &win_w in &window_widths {
            let usable_w = win_w - 208.0 - 48.0;
            let cols = calculate_chart_columns(usable_w);
            let chart_w = calculate_chart_width(usable_w, gap, cols);

            let total_row_w = (cols as f32 * chart_w) + ((cols - 1) as f32 * gap);
            assert!(
                total_row_w <= usable_w + 0.1,
                "Window {win_w}px (usable {usable_w}px): charts row width {total_row_w}px exceeds usable width"
            );

            // Chart panel inner frame width: (chart_w - 26.0)
            let inner_w = chart_w - 26.0;
            assert!(
                inner_w >= 74.0,
                "Inner chart panel width must remain positive"
            );
        }
    }
}
