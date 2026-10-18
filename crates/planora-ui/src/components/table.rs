use crate::{
    formatting::{format_bytes, format_percent, format_rate},
    theme::Palette,
};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};
use planora_core::{ProcessIdentity, ProcessRecord};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortColumn {
    Name,
    Pid,
    #[default]
    Cpu,
    Memory,
    DiskRead,
    DiskWrite,
    Gpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumnsConfig {
    pub show_pid: bool,
    pub show_cpu: bool,
    pub show_memory: bool,
    pub show_disk_read: bool,
    pub show_disk_write: bool,
    pub show_gpu: bool,
}

impl Default for TableColumnsConfig {
    fn default() -> Self {
        Self {
            show_pid: true,
            show_cpu: true,
            show_memory: true,
            show_disk_read: true,
            show_disk_write: true,
            show_gpu: false,
        }
    }
}

pub fn sort_processes<'a>(
    processes: &'a [ProcessRecord],
    filter_query: &str,
    sort_column: SortColumn,
    sort_descending: bool,
) -> Vec<&'a ProcessRecord> {
    let q = filter_query.trim().to_lowercase();
    let mut rows: Vec<&ProcessRecord> = processes
        .iter()
        .filter(|p| {
            if q.is_empty() {
                true
            } else {
                p.name.to_lowercase().contains(&q) || p.identity.pid.to_string().contains(&q)
            }
        })
        .collect();

    let compare_optional = |a: Option<f32>, b: Option<f32>| match (a, b) {
        (Some(va), Some(vb)) => {
            if sort_descending {
                vb.partial_cmp(&va).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
            }
        }
        (Some(_), None) => std::cmp::Ordering::Less, // Available always beats Unavailable
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    };

    rows.sort_by(|a, b| {
        let primary = match sort_column {
            SortColumn::Name => {
                if sort_descending {
                    b.name.cmp(&a.name)
                } else {
                    a.name.cmp(&b.name)
                }
            }
            SortColumn::Pid => {
                if sort_descending {
                    b.identity.pid.cmp(&a.identity.pid)
                } else {
                    a.identity.pid.cmp(&b.identity.pid)
                }
            }
            SortColumn::Cpu => compare_optional(a.cpu_percent.value, b.cpu_percent.value),
            SortColumn::Memory => compare_optional(
                a.memory_bytes.value.map(|v| v as f32),
                b.memory_bytes.value.map(|v| v as f32),
            ),
            SortColumn::DiskRead => compare_optional(
                a.disk_read_bytes_per_sec.value.map(|v| v as f32),
                b.disk_read_bytes_per_sec.value.map(|v| v as f32),
            ),
            SortColumn::DiskWrite => compare_optional(
                a.disk_write_bytes_per_sec.value.map(|v| v as f32),
                b.disk_write_bytes_per_sec.value.map(|v| v as f32),
            ),
            SortColumn::Gpu => compare_optional(a.gpu_percent.value, b.gpu_percent.value),
        };

        if primary == std::cmp::Ordering::Equal {
            // Deterministic secondary tie-breaker by PID
            a.identity.pid.cmp(&b.identity.pid)
        } else {
            primary
        }
    });

    rows
}

pub struct ProcessTableState {
    pub sort_column: SortColumn,
    pub sort_descending: bool,
    pub columns: TableColumnsConfig,
    pub search_query: String,
}

impl Default for ProcessTableState {
    fn default() -> Self {
        Self {
            sort_column: SortColumn::Cpu,
            sort_descending: true,
            columns: TableColumnsConfig::default(),
            search_query: String::new(),
        }
    }
}

pub fn render_process_table(
    ui: &mut egui::Ui,
    rows: &[&ProcessRecord],
    selected_identity: &mut Option<ProcessIdentity>,
    state: &mut ProcessTableState,
    row_height: f32,
    p: Palette,
) {
    let col_pid_w = 64.0;
    let col_cpu_w = 110.0;
    let col_mem_w = 110.0;
    let col_disk_r_w = 95.0;
    let col_disk_w_w = 95.0;
    let col_gpu_w = 80.0;

    // Remaining width is for Process Name
    let mut fixed_w = 0.0;
    if state.columns.show_pid {
        fixed_w += col_pid_w;
    }
    if state.columns.show_cpu {
        fixed_w += col_cpu_w;
    }
    if state.columns.show_memory {
        fixed_w += col_mem_w;
    }
    if state.columns.show_disk_read {
        fixed_w += col_disk_r_w;
    }
    if state.columns.show_disk_write {
        fixed_w += col_disk_w_w;
    }
    if state.columns.show_gpu {
        fixed_w += col_gpu_w;
    }

    let col_name_w = (ui.available_width() - fixed_w - 24.0).max(140.0);

    // Keyboard navigation (Up/Down)
    let mut sel_idx = None;
    if let Some(cur_id) = selected_identity.as_ref() {
        for (idx, r) in rows.iter().enumerate() {
            if &r.identity == cur_id {
                sel_idx = Some(idx);
                break;
            }
        }
    }

    if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
        let next = match sel_idx {
            Some(i) => (i + 1).min(rows.len().saturating_sub(1)),
            None => 0,
        };
        if let Some(r) = rows.get(next) {
            *selected_identity = Some(r.identity.clone());
        }
    } else if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
        let prev = match sel_idx {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        if let Some(r) = rows.get(prev) {
            *selected_identity = Some(r.identity.clone());
        }
    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        *selected_identity = None;
    }

    // Unified Header row
    let header_bg = p.surface_raised;
    egui::Frame::default()
        .fill(header_bg)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Name header
                let (rect_name, resp_name) =
                    ui.allocate_exact_size(Vec2::new(col_name_w, 20.0), egui::Sense::click());
                let name_arrow = if state.sort_column == SortColumn::Name {
                    if state.sort_descending {
                        " ▼"
                    } else {
                        " ▲"
                    }
                } else {
                    ""
                };
                ui.painter().text(
                    Pos2::new(rect_name.min.x, rect_name.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("Process Name{name_arrow}"),
                    egui::FontId::proportional(12.0),
                    p.text,
                );
                if resp_name.clicked() {
                    if state.sort_column == SortColumn::Name {
                        state.sort_descending = !state.sort_descending;
                    } else {
                        state.sort_column = SortColumn::Name;
                        state.sort_descending = false;
                    }
                }

                // PID header
                if state.columns.show_pid {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(col_pid_w, 20.0), egui::Sense::click());
                    let arrow = if state.sort_column == SortColumn::Pid {
                        if state.sort_descending {
                            " ▼"
                        } else {
                            " ▲"
                        }
                    } else {
                        ""
                    };
                    ui.painter().text(
                        Pos2::new(rect.max.x - 4.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format!("PID{arrow}"),
                        egui::FontId::proportional(12.0),
                        p.text,
                    );
                    if resp.clicked() {
                        if state.sort_column == SortColumn::Pid {
                            state.sort_descending = !state.sort_descending;
                        } else {
                            state.sort_column = SortColumn::Pid;
                            state.sort_descending = true;
                        }
                    }
                }

                // CPU header
                if state.columns.show_cpu {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(col_cpu_w, 20.0), egui::Sense::click());
                    let arrow = if state.sort_column == SortColumn::Cpu {
                        if state.sort_descending {
                            " ▼"
                        } else {
                            " ▲"
                        }
                    } else {
                        ""
                    };
                    ui.painter().text(
                        Pos2::new(rect.max.x - 4.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format!("CPU{arrow}"),
                        egui::FontId::proportional(12.0),
                        p.text,
                    );
                    if resp.clicked() {
                        if state.sort_column == SortColumn::Cpu {
                            state.sort_descending = !state.sort_descending;
                        } else {
                            state.sort_column = SortColumn::Cpu;
                            state.sort_descending = true;
                        }
                    }
                }

                // Memory header
                if state.columns.show_memory {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(col_mem_w, 20.0), egui::Sense::click());
                    let arrow = if state.sort_column == SortColumn::Memory {
                        if state.sort_descending {
                            " ▼"
                        } else {
                            " ▲"
                        }
                    } else {
                        ""
                    };
                    ui.painter().text(
                        Pos2::new(rect.max.x - 4.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format!("Memory{arrow}"),
                        egui::FontId::proportional(12.0),
                        p.text,
                    );
                    if resp.clicked() {
                        if state.sort_column == SortColumn::Memory {
                            state.sort_descending = !state.sort_descending;
                        } else {
                            state.sort_column = SortColumn::Memory;
                            state.sort_descending = true;
                        }
                    }
                }

                // Disk Read header
                if state.columns.show_disk_read {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(col_disk_r_w, 20.0), egui::Sense::click());
                    let arrow = if state.sort_column == SortColumn::DiskRead {
                        if state.sort_descending {
                            " ▼"
                        } else {
                            " ▲"
                        }
                    } else {
                        ""
                    };
                    ui.painter().text(
                        Pos2::new(rect.max.x - 4.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format!("Disk R{arrow}"),
                        egui::FontId::proportional(12.0),
                        p.text,
                    );
                    if resp.clicked() {
                        if state.sort_column == SortColumn::DiskRead {
                            state.sort_descending = !state.sort_descending;
                        } else {
                            state.sort_column = SortColumn::DiskRead;
                            state.sort_descending = true;
                        }
                    }
                }

                // Disk Write header
                if state.columns.show_disk_write {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(col_disk_w_w, 20.0), egui::Sense::click());
                    let arrow = if state.sort_column == SortColumn::DiskWrite {
                        if state.sort_descending {
                            " ▼"
                        } else {
                            " ▲"
                        }
                    } else {
                        ""
                    };
                    ui.painter().text(
                        Pos2::new(rect.max.x - 4.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format!("Disk W{arrow}"),
                        egui::FontId::proportional(12.0),
                        p.text,
                    );
                    if resp.clicked() {
                        if state.sort_column == SortColumn::DiskWrite {
                            state.sort_descending = !state.sort_descending;
                        } else {
                            state.sort_column = SortColumn::DiskWrite;
                            state.sort_descending = true;
                        }
                    }
                }

                // GPU header
                if state.columns.show_gpu {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(col_gpu_w, 20.0), egui::Sense::click());
                    let arrow = if state.sort_column == SortColumn::Gpu {
                        if state.sort_descending {
                            " ▼"
                        } else {
                            " ▲"
                        }
                    } else {
                        ""
                    };
                    ui.painter().text(
                        Pos2::new(rect.max.x - 4.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format!("GPU{arrow}"),
                        egui::FontId::proportional(12.0),
                        p.text,
                    );
                    if resp.clicked() {
                        if state.sort_column == SortColumn::Gpu {
                            state.sort_descending = !state.sort_descending;
                        } else {
                            state.sort_column = SortColumn::Gpu;
                            state.sort_descending = true;
                        }
                    }
                }
            });
        });

    ui.separator();

    // Virtualized Table Rows
    let total_rows = rows.len();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, total_rows, |ui, row_range| {
            for idx in row_range {
                let process = rows[idx];
                let is_selected = selected_identity.as_ref() == Some(&process.identity);
                let row_bg = if is_selected {
                    p.selected
                } else if idx % 2 == 1 {
                    p.surface_raised.linear_multiply(0.4)
                } else {
                    Color32::TRANSPARENT
                };

                let row_rect = egui::Frame::default()
                    .fill(row_bg)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Process Name (clickable)
                            let (name_rect, name_resp) = ui.allocate_exact_size(
                                Vec2::new(col_name_w, row_height - 6.0),
                                egui::Sense::click(),
                            );
                            let display_name = if process.name.is_empty() {
                                "<unnamed>"
                            } else {
                                &process.name
                            };
                            // Clip long executable names to their own column.
                            // Hover reveals the full name without overlapping numeric cells.
                            ui.painter_at(name_rect).text(
                                Pos2::new(name_rect.min.x, name_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                display_name,
                                egui::FontId::proportional(12.5),
                                if is_selected { p.focus } else { p.text },
                            );

                            if name_resp.clicked() {
                                *selected_identity = Some(process.identity.clone());
                            }
                            name_resp.on_hover_text(display_name);

                            // PID
                            if state.columns.show_pid {
                                let (pid_rect, pid_resp) = ui.allocate_exact_size(
                                    Vec2::new(col_pid_w, row_height - 6.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().text(
                                    Pos2::new(pid_rect.max.x - 4.0, pid_rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    process.identity.pid.to_string(),
                                    egui::FontId::monospace(11.5),
                                    p.secondary,
                                );
                                if pid_resp.clicked() {
                                    *selected_identity = Some(process.identity.clone());
                                }
                            }

                            // CPU with subtle in-cell bar
                            if state.columns.show_cpu {
                                let (cpu_rect, cpu_resp) = ui.allocate_exact_size(
                                    Vec2::new(col_cpu_w, row_height - 6.0),
                                    egui::Sense::click(),
                                );
                                if let Some(cpu_pct) = process.cpu_percent.value {
                                    // Subtle in-cell bar behind number
                                    let bar_w = (cpu_rect.width() - 8.0)
                                        * (cpu_pct.clamp(0.0, 100.0) / 100.0);
                                    if bar_w > 1.0 {
                                        let bar_rect = Rect::from_min_size(
                                            Pos2::new(
                                                cpu_rect.max.x - 4.0 - bar_w,
                                                cpu_rect.min.y + 2.0,
                                            ),
                                            Vec2::new(bar_w, cpu_rect.height() - 4.0),
                                        );
                                        ui.painter().rect_filled(
                                            bar_rect,
                                            CornerRadius::same(2),
                                            p.cpu.linear_multiply(0.20),
                                        );
                                    }
                                }
                                ui.painter().text(
                                    Pos2::new(cpu_rect.max.x - 4.0, cpu_rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    format_percent(process.cpu_percent.value),
                                    egui::FontId::monospace(11.5),
                                    p.text,
                                );
                                if cpu_resp.clicked() {
                                    *selected_identity = Some(process.identity.clone());
                                }
                            }

                            // Memory with subtle in-cell bar
                            if state.columns.show_memory {
                                let (mem_rect, mem_resp) = ui.allocate_exact_size(
                                    Vec2::new(col_mem_w, row_height - 6.0),
                                    egui::Sense::click(),
                                );
                                if let Some(mem_bytes) = process.memory_bytes.value {
                                    // Subtle in-cell bar
                                    let max_ref = (1024.0 * 1024.0 * 1024.0 * 4.0) as f32; // 4 GiB scale
                                    let bar_w = (mem_rect.width() - 8.0)
                                        * ((mem_bytes as f32) / max_ref).clamp(0.0, 1.0);
                                    if bar_w > 1.0 {
                                        let bar_rect = Rect::from_min_size(
                                            Pos2::new(
                                                mem_rect.max.x - 4.0 - bar_w,
                                                mem_rect.min.y + 2.0,
                                            ),
                                            Vec2::new(bar_w, mem_rect.height() - 4.0),
                                        );
                                        ui.painter().rect_filled(
                                            bar_rect,
                                            CornerRadius::same(2),
                                            p.memory.linear_multiply(0.20),
                                        );
                                    }
                                }
                                let mem_text = match process.memory_bytes.value {
                                    Some(b) => format_bytes(b),
                                    None => "Unavailable".into(),
                                };
                                ui.painter().text(
                                    Pos2::new(mem_rect.max.x - 4.0, mem_rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    mem_text,
                                    egui::FontId::monospace(11.5),
                                    p.text,
                                );
                                if mem_resp.clicked() {
                                    *selected_identity = Some(process.identity.clone());
                                }
                            }

                            // Disk Read
                            if state.columns.show_disk_read {
                                let (dr_rect, dr_resp) = ui.allocate_exact_size(
                                    Vec2::new(col_disk_r_w, row_height - 6.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().text(
                                    Pos2::new(dr_rect.max.x - 4.0, dr_rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    format_rate(
                                        process.disk_read_bytes_per_sec.value.map(|v| v as f32),
                                    ),
                                    egui::FontId::monospace(11.0),
                                    p.secondary,
                                );
                                if dr_resp.clicked() {
                                    *selected_identity = Some(process.identity.clone());
                                }
                            }

                            // Disk Write
                            if state.columns.show_disk_write {
                                let (dw_rect, dw_resp) = ui.allocate_exact_size(
                                    Vec2::new(col_disk_w_w, row_height - 6.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().text(
                                    Pos2::new(dw_rect.max.x - 4.0, dw_rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    format_rate(
                                        process.disk_write_bytes_per_sec.value.map(|v| v as f32),
                                    ),
                                    egui::FontId::monospace(11.0),
                                    p.secondary,
                                );
                                if dw_resp.clicked() {
                                    *selected_identity = Some(process.identity.clone());
                                }
                            }

                            // GPU
                            if state.columns.show_gpu {
                                let (gpu_rect, gpu_resp) = ui.allocate_exact_size(
                                    Vec2::new(col_gpu_w, row_height - 6.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().text(
                                    Pos2::new(gpu_rect.max.x - 4.0, gpu_rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    format_percent(process.gpu_percent.value),
                                    egui::FontId::monospace(11.0),
                                    p.secondary,
                                );
                                if gpu_resp.clicked() {
                                    *selected_identity = Some(process.identity.clone());
                                }
                            }
                        });
                    });

                if is_selected {
                    ui.painter().rect_stroke(
                        row_rect.response.rect,
                        CornerRadius::same(2),
                        Stroke::new(1.0, p.focus),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        });
}
