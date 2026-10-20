use crate::{
    components::header::render_page_header,
    formatting::{format_bytes, format_percent, format_rate},
    state::{AppState, DisplayPause},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Rect, Stroke, Vec2};
use planora_core::TelemetryFrame;

#[derive(Default)]
pub struct ResourcesViewState {
    pub active_tab: usize, // 0: CPU, 1: Memory, 2: GPU, 3: Storage, 4: Network, 5: Sensors
    pub search_query: String,
}

pub fn render_resources_view(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    state: &mut ResourcesViewState,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    p: Palette,
) {
    render_page_header(
        ui,
        "Resource Measurements",
        "Detailed device, engine, and hardware sensor telemetry across system subsystems",
        app_state,
        pause_state,
        poll_interval_ms,
        p,
    );

    // Tab Navigation Bar
    ui.horizontal(|ui| {
        let tabs = [
            (0, "CPU & Logical Cores"),
            (1, "Memory & Commit"),
            (2, "GPU & Engines"),
            (3, "Storage & Disks"),
            (4, "Network Interfaces"),
            (5, "Hardware Sensors"),
        ];

        for (idx, label) in tabs {
            let is_active = state.active_tab == idx;
            if ui
                .selectable_label(is_active, egui::RichText::new(label).strong())
                .clicked()
            {
                state.active_tab = idx;
            }
        }
    });

    ui.add_space(10.0);

    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| match state.active_tab {
            0 => render_cpu_tab(ui, frame, p),
            1 => render_memory_tab(ui, frame, p),
            2 => render_gpu_tab(ui, frame, p),
            3 => render_storage_tab(ui, frame, p),
            4 => render_network_tab(ui, frame, p),
            5 => render_sensors_tab(ui, frame, p),
            _ => state.active_tab = 0,
        });
}

fn render_cpu_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.horizontal(|ui| {
        ui.heading("Logical Processors");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Total System CPU: {}",
                    format_percent(frame.cpu_percent.value)
                ))
                .strong()
                .color(p.cpu),
            );
        });
    });

    ui.label(
        egui::RichText::new(
            "Individual logical processor loads preserving Windows processor-group addressing.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if frame.logical_cpus.is_empty() {
        ui.label("No individual logical core metrics were reported in this frame.");
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("resources_cpu_grid")
            .striped(true)
            .num_columns(5)
            .spacing([24.0, 8.0])
            .show(ui, |ui| {
                ui.strong("CPU #");
                ui.strong("Group");
                ui.strong("Utilization");
                ui.strong("Load Bar");
                ui.strong("Source");
                ui.end_row();

                for core in &frame.logical_cpus {
                    ui.monospace(format!("CPU {}", core.cpu_index));
                    ui.monospace(format!("Group {}", core.group));
                    ui.monospace(format_percent(core.utilization_percent.value));

                    // Compact load bar
                    let (bar_rect, _) =
                        ui.allocate_exact_size(Vec2::new(120.0, 16.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(bar_rect, CornerRadius::same(2), p.surface_raised);
                    if let Some(pct) = core.utilization_percent.value {
                        let fill_w = bar_rect.width() * (pct.clamp(0.0, 100.0) / 100.0);
                        if fill_w > 0.5 {
                            let fill_rect = Rect::from_min_size(
                                bar_rect.min,
                                Vec2::new(fill_w, bar_rect.height()),
                            );
                            ui.painter()
                                .rect_filled(fill_rect, CornerRadius::same(2), p.cpu);
                        }
                    }

                    ui.label(&core.utilization_percent.source);
                    ui.end_row();
                }
            });
    });
}

fn render_memory_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.heading("System Memory & Commit Budget");
    ui.label(
        egui::RichText::new(
            "Physical memory allocation and Windows commit limit / page file backing.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    egui::Grid::new("resources_memory_grid")
        .striped(true)
        .num_columns(6)
        .spacing([20.0, 10.0])
        .show(ui, |ui| {
            ui.strong("Metric");
            ui.strong("Used");
            ui.strong("Available");
            ui.strong("Total / Limit");
            ui.strong("Utilization");
            ui.strong("Source");
            ui.end_row();

            // Physical RAM
            let phys_used = frame.memory_used_bytes.value;
            let phys_avail = frame.memory_available_bytes.value;
            let phys_total = frame.memory_total_bytes.value;
            let phys_pct = match (phys_used, phys_total) {
                (Some(u), Some(t)) if t > 0 => Some((u as f64 * 100.0 / t as f64) as f32),
                _ => None,
            };

            ui.colored_label(p.memory, "Physical RAM");
            ui.monospace(phys_used.map_or_else(|| "Unavailable".into(), format_bytes));
            ui.monospace(phys_avail.map_or_else(|| "Unavailable".into(), format_bytes));
            ui.monospace(phys_total.map_or_else(|| "Unavailable".into(), format_bytes));
            ui.monospace(format_percent(phys_pct));
            ui.label(&frame.memory_used_bytes.source);
            ui.end_row();

            // Commit Limit
            let com_used = frame.commit_used_bytes.value;
            let com_total = frame.commit_total_bytes.value;
            let com_avail = match (com_used, com_total) {
                (Some(u), Some(t)) if t >= u => Some(t - u),
                _ => None,
            };
            let com_pct = match (com_used, com_total) {
                (Some(u), Some(t)) if t > 0 => Some((u as f64 * 100.0 / t as f64) as f32),
                _ => None,
            };

            ui.colored_label(p.primary, "Commit Charge");
            ui.monospace(com_used.map_or_else(|| "Unavailable".into(), format_bytes));
            ui.monospace(com_avail.map_or_else(|| "Unavailable".into(), format_bytes));
            ui.monospace(com_total.map_or_else(|| "Unavailable".into(), format_bytes));
            ui.monospace(format_percent(com_pct));
            ui.label(&frame.commit_used_bytes.source);
            ui.end_row();
        });
}

fn render_gpu_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.heading("GPU Adapters & Engines");
    ui.label(
        egui::RichText::new(
            "Physical graphics adapters discovered via DXGI and engine load from Windows PDH.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if frame.gpu_adapters.is_empty() {
        ui.label("No discrete or integrated GPU adapters discovered.");
    } else {
        ui.strong("Adapters");
        egui::Grid::new("resources_gpu_adapters_grid")
            .striped(true)
            .num_columns(6)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                ui.strong("Adapter");
                ui.strong("LUID");
                ui.strong("Busiest Engine");
                ui.strong("Dedicated VRAM (Used / Capacity)");
                ui.strong("Shared Memory (Used / Capacity)");
                ui.strong("Budget");
                ui.end_row();

                for adapter in &frame.gpu_adapters {
                    ui.label(&adapter.name);
                    ui.monospace(format!("0x{:x}", adapter.adapter_luid));
                    ui.monospace(format_percent(adapter.busiest_engine_percent.value));

                    let ded_str = match (
                        adapter.dedicated_used_bytes.value,
                        adapter.dedicated_total_bytes.value,
                    ) {
                        (Some(u), Some(t)) if t > 0 => {
                            format!("{} / {}", format_bytes(u), format_bytes(t))
                        }
                        (Some(u), _) => format_bytes(u),
                        _ => "—".into(),
                    };
                    ui.monospace(ded_str);

                    let shr_str = match (
                        adapter.shared_used_bytes.value,
                        adapter.shared_total_bytes.value,
                    ) {
                        (Some(u), Some(t)) if t > 0 => {
                            format!("{} / {}", format_bytes(u), format_bytes(t))
                        }
                        (Some(u), _) => format_bytes(u),
                        _ => "—".into(),
                    };
                    ui.monospace(shr_str);

                    let budget_str = adapter
                        .budget_bytes
                        .as_ref()
                        .and_then(|b| b.value)
                        .map_or_else(|| "—".into(), format_bytes);
                    ui.monospace(budget_str);
                    ui.end_row();
                }
            });
    }

    ui.add_space(14.0);

    if !frame.gpu_engines.is_empty() {
        ui.strong(format!("Active GPU Engines ({})", frame.gpu_engines.len()));
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .show(ui, |ui| {
                egui::Grid::new("resources_gpu_engines_grid")
                    .striped(true)
                    .num_columns(5)
                    .spacing([18.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("Adapter LUID");
                        ui.strong("Engine Type");
                        ui.strong("Index");
                        ui.strong("Process PID");
                        ui.strong("Utilization");
                        ui.end_row();

                        for eng in &frame.gpu_engines {
                            let luid_str = eng
                                .adapter_luid
                                .map(|l| format!("0x{l:x}"))
                                .unwrap_or_else(|| "Adapter".into());
                            ui.label(luid_str);
                            ui.label(&eng.engine_type);
                            ui.monospace(
                                eng.engine_id
                                    .map_or_else(|| "—".into(), |idx| idx.to_string()),
                            );
                            ui.monospace(
                                eng.process_pid
                                    .map_or_else(|| "System".into(), |pid| format!("PID {pid}")),
                            );
                            ui.monospace(format_percent(eng.utilization_percent.value));
                            ui.end_row();
                        }
                    });
            });
    }
}

fn render_storage_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.heading("Physical Disks & Block Storage");
    ui.label(
        egui::RichText::new(
            "Throughput, response time latency, and queue depth per physical disk.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if frame.disks.is_empty() {
        ui.label("No physical disk devices discovered in current telemetry frame.");
        return;
    }

    egui::Grid::new("resources_storage_grid")
        .striped(true)
        .num_columns(5)
        .spacing([20.0, 8.0])
        .show(ui, |ui| {
            ui.strong("Device Label");
            ui.strong("Read Rate");
            ui.strong("Write Rate");
            ui.strong("Avg Latency");
            ui.strong("Queue Depth");
            ui.end_row();

            for disk in &frame.disks {
                ui.label(&disk.label);
                ui.monospace(format_rate(disk.read_bytes_per_sec.value.map(|v| v as f32)));
                ui.monospace(format_rate(
                    disk.write_bytes_per_sec.value.map(|v| v as f32),
                ));

                let lat_str = disk
                    .latency_ms
                    .as_ref()
                    .and_then(|m| m.value)
                    .map_or_else(|| "—".into(), |v| format!("{v:.2} ms"));
                ui.monospace(lat_str);

                let queue_str = disk
                    .queue_depth
                    .as_ref()
                    .and_then(|m| m.value)
                    .map_or_else(|| "—".into(), |v| format!("{v:.1}"));
                ui.monospace(queue_str);
                ui.end_row();
            }
        });
}

fn render_network_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.heading("Network Adapters & Interfaces");
    ui.label(
        egui::RichText::new(
            "Network interface bandwidth, link connection status, and theoretical link speed.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if frame.network_interfaces.is_empty() {
        ui.label("No network adapters reported in current telemetry frame.");
        return;
    }

    egui::Grid::new("resources_network_grid")
        .striped(true)
        .num_columns(5)
        .spacing([20.0, 8.0])
        .show(ui, |ui| {
            ui.strong("Interface");
            ui.strong("Receive Rate");
            ui.strong("Transmit Rate");
            ui.strong("Link State");
            ui.strong("Link Speed");
            ui.end_row();

            for iface in &frame.network_interfaces {
                ui.label(&iface.label);
                ui.monospace(format_rate(
                    iface.read_bytes_per_sec.value.map(|v| v as f32),
                ));
                ui.monospace(format_rate(
                    iface.write_bytes_per_sec.value.map(|v| v as f32),
                ));

                let state_str = iface.link_state.as_deref().unwrap_or("Connected");
                ui.label(state_str);

                let speed_str = iface
                    .link_speed_bps
                    .as_ref()
                    .and_then(|m| m.value)
                    .map_or_else(
                        || "—".into(),
                        |bps| {
                            if bps >= 1_000_000_000 {
                                format!("{:.1} Gbps", bps as f64 / 1_000_000_000.0)
                            } else if bps >= 1_000_000 {
                                format!("{:.0} Mbps", bps as f64 / 1_000_000.0)
                            } else {
                                format!("{bps} bps")
                            }
                        },
                    );
                ui.monospace(speed_str);
                ui.end_row();
            }
        });
}

fn render_sensors_tab(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.heading("Hardware Sensors & Telemetry Providers");
    ui.label(
        egui::RichText::new(
            "Vendor hardware metrics (Intel IGCL, NVIDIA NVML, AMD ADLX) from isolated workers.",
        )
        .size(11.5)
        .color(p.secondary),
    );
    ui.add_space(8.0);

    if frame.sensors.is_empty() {
        ui.label("No active vendor sensors reported. Sensors require supported GPU hardware and driver runtimes.");
        ui.label(
            egui::RichText::new(
                "Check the Capabilities tab to see provider runtime discovery status.",
            )
            .size(11.5)
            .color(p.secondary),
        );
        return;
    }

    egui::Grid::new("resources_sensors_grid")
        .striped(true)
        .num_columns(5)
        .spacing([20.0, 8.0])
        .show(ui, |ui| {
            ui.strong("Hardware Scope");
            ui.strong("Sensor");
            ui.strong("Type");
            ui.strong("Reading");
            ui.strong("Provider");
            ui.end_row();

            for sensor in &frame.sensors {
                ui.label(&sensor.scope);
                ui.label(&sensor.name);
                ui.monospace(format!("{:?}", sensor.sensor_type));

                let val_str = sensor.value.value.map_or_else(
                    || "Unavailable".into(),
                    |v| format!("{v:.1} {}", sensor.value.unit),
                );
                ui.monospace(val_str);

                ui.label(&sensor.provider);
                ui.end_row();
            }
        });
}
