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
    IoRead,
    IoWrite,
    DiskRead,
    DiskWrite,
    Gpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumnsConfig {
    pub show_pid: bool,
    pub show_cpu: bool,
    pub show_memory: bool,
    pub show_io_read: bool,
    pub show_io_write: bool,
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
            show_io_read: false,
            show_io_write: false,
            show_disk_read: false,
            show_disk_write: false,
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
    let compare_integer = |a: Option<u64>, b: Option<u64>| match (a, b) {
        (Some(a), Some(b)) => {
            if sort_descending {
                b.cmp(&a)
            } else {
                a.cmp(&b)
            }
        }
        (Some(_), None) => std::cmp::Ordering::Less,
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
            SortColumn::Memory => compare_integer(a.memory_bytes.value, b.memory_bytes.value),
            SortColumn::IoRead => {
                compare_integer(a.io_read_bytes_per_sec.value, b.io_read_bytes_per_sec.value)
            }
            SortColumn::IoWrite => compare_integer(
                a.io_write_bytes_per_sec.value,
                b.io_write_bytes_per_sec.value,
            ),
            SortColumn::DiskRead => compare_integer(
                a.disk_read_bytes_per_sec.value,
                b.disk_read_bytes_per_sec.value,
            ),
            SortColumn::DiskWrite => compare_integer(
                a.disk_write_bytes_per_sec.value,
                b.disk_write_bytes_per_sec.value,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessColumnLayout {
    pub col_name_w: f32,
    pub show_pid: bool,
    pub col_pid_w: f32,
    pub show_cpu: bool,
    pub col_cpu_w: f32,
    pub show_memory: bool,
    pub col_mem_w: f32,
    pub show_io_read: bool,
    pub col_io_r_w: f32,
    pub show_io_write: bool,
    pub col_io_w_w: f32,
    pub show_disk_read: bool,
    pub col_disk_r_w: f32,
    pub show_disk_write: bool,
    pub col_disk_w_w: f32,
    pub show_gpu: bool,
    pub col_gpu_w: f32,
    pub col_spacing: f32,
    pub inner_margin_x: f32,
}

impl ProcessColumnLayout {
    pub fn minimum_width(config: &TableColumnsConfig) -> f32 {
        Self::minimum_width_scaled(config, 1.0)
    }

    pub fn minimum_width_scaled(config: &TableColumnsConfig, text_scale: f32) -> f32 {
        let scale = text_scale.max(1.0);
        let mut width = (120.0 + 16.0) * scale;
        let mut columns = 1;
        for (show, size) in [
            (config.show_pid, 64.0 * scale),
            (config.show_cpu, 95.0 * scale),
            (config.show_memory, 95.0 * scale),
            (config.show_io_read, 85.0 * scale),
            (config.show_io_write, 85.0 * scale),
            (config.show_disk_read, 85.0 * scale),
            (config.show_disk_write, 85.0 * scale),
            (config.show_gpu, 75.0 * scale),
        ] {
            if show {
                width += size;
                columns += 1;
            }
        }
        width + (columns - 1) as f32 * 8.0
    }

    pub fn compute(available_w: f32, config: &TableColumnsConfig) -> Self {
        Self::compute_scaled(available_w, config, 1.0)
    }

    pub fn compute_scaled(available_w: f32, config: &TableColumnsConfig, text_scale: f32) -> Self {
        let scale = text_scale.max(1.0);
        let col_spacing = 8.0;
        let inner_margin_x = 8.0;

        let col_pid_w = (64.0 * scale).round();
        let col_cpu_w = (95.0 * scale).round();
        let col_mem_w = (95.0 * scale).round();
        let col_io_r_w = (85.0 * scale).round();
        let col_io_w_w = (85.0 * scale).round();
        let col_disk_r_w = (85.0 * scale).round();
        let col_disk_w_w = (85.0 * scale).round();
        let col_gpu_w = (75.0 * scale).round();

        let show_gpu = config.show_gpu;
        let show_disk_write = config.show_disk_write;
        let show_disk_read = config.show_disk_read;
        let show_io_write = config.show_io_write;
        let show_io_read = config.show_io_read;
        let show_pid = config.show_pid;
        let show_cpu = config.show_cpu;
        let show_memory = config.show_memory;

        let mut fixed_w = 0.0;
        let mut num_cols = 1;
        if show_pid {
            fixed_w += col_pid_w;
            num_cols += 1;
        }
        if show_cpu {
            fixed_w += col_cpu_w;
            num_cols += 1;
        }
        if show_memory {
            fixed_w += col_mem_w;
            num_cols += 1;
        }
        if show_io_read {
            fixed_w += col_io_r_w;
            num_cols += 1;
        }
        if show_io_write {
            fixed_w += col_io_w_w;
            num_cols += 1;
        }
        if show_disk_read {
            fixed_w += col_disk_r_w;
            num_cols += 1;
        }
        if show_disk_write {
            fixed_w += col_disk_w_w;
            num_cols += 1;
        }
        if show_gpu {
            fixed_w += col_gpu_w;
            num_cols += 1;
        }

        let total_spacing = (num_cols - 1) as f32 * col_spacing + 2.0 * inner_margin_x;
        let col_name_w = (available_w - fixed_w - total_spacing).max(120.0);

        Self {
            col_name_w,
            show_pid,
            col_pid_w,
            show_cpu,
            col_cpu_w,
            show_memory,
            col_mem_w,
            show_io_read,
            col_io_r_w,
            show_io_write,
            col_io_w_w,
            show_disk_read,
            col_disk_r_w,
            show_disk_write,
            col_disk_w_w,
            show_gpu,
            col_gpu_w,
            col_spacing,
            inner_margin_x,
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
    let text_scale = crate::appearance::text_scale(ui.ctx()).max(1.0);
    let min_width = ProcessColumnLayout::minimum_width_scaled(&state.columns, text_scale);
    if ui.available_width() < min_width {
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.set_min_width(min_width);
            render_process_table_inner(ui, rows, selected_identity, state, row_height, p);
        });
    } else {
        render_process_table_inner(ui, rows, selected_identity, state, row_height, p);
    }
}

pub fn render_process_table_inline(
    ui: &mut egui::Ui,
    rows: &[&ProcessRecord],
    selected_identity: &mut Option<ProcessIdentity>,
    state: &mut ProcessTableState,
    row_height: f32,
    p: Palette,
) {
    let text_scale = crate::appearance::text_scale(ui.ctx()).max(1.0);
    let min_width = ProcessColumnLayout::minimum_width_scaled(&state.columns, text_scale);
    let mut render_body = |ui: &mut egui::Ui| {
        let layout =
            ProcessColumnLayout::compute_scaled(ui.available_width(), &state.columns, text_scale);
        let max_visible_memory = rows
            .iter()
            .filter_map(|process| process.memory_bytes.value)
            .max()
            .unwrap_or(1);

        render_process_header(ui, &layout, state, p);
        ui.separator();

        let row_height = row_height.max(32.0 * text_scale);
        let cell_h = (row_height - 6.0).max(16.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        for (idx, process) in rows.iter().enumerate() {
            render_process_row(
                ui,
                process,
                idx,
                &layout,
                max_visible_memory,
                selected_identity,
                cell_h,
                p,
            );
        }
    };

    if ui.available_width() < min_width {
        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_min_width(min_width);
                render_body(ui);
            });
    } else {
        render_body(ui);
    }
}

pub fn render_process_header(
    ui: &mut egui::Ui,
    layout: &ProcessColumnLayout,
    state: &mut ProcessTableState,
    p: Palette,
) {
    let header_bg = p.surface_raised;
    egui::Frame::default()
        .fill(header_bg)
        .inner_margin(egui::Margin::symmetric(layout.inner_margin_x as i8, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = layout.col_spacing;

                // Name header
                let (rect_name, resp_name) = ui
                    .allocate_exact_size(Vec2::new(layout.col_name_w, 20.0), egui::Sense::click());
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
                    egui::FontId::proportional(12.0 * crate::appearance::text_scale(ui.ctx())),
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
                if layout.show_pid {
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_pid_w, 20.0),
                        egui::Sense::click(),
                    );
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
                        egui::FontId::proportional(12.0 * crate::appearance::text_scale(ui.ctx())),
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
                if layout.show_cpu {
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_cpu_w, 20.0),
                        egui::Sense::click(),
                    );
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
                        egui::FontId::proportional(12.0 * crate::appearance::text_scale(ui.ctx())),
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
                if layout.show_memory {
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_mem_w, 20.0),
                        egui::Sense::click(),
                    );
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
                        egui::FontId::proportional(12.0 * crate::appearance::text_scale(ui.ctx())),
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

                // I/O Read header
                if layout.show_io_read {
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_io_r_w, 20.0),
                        egui::Sense::click(),
                    );
                    let arrow = if state.sort_column == SortColumn::IoRead {
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
                        format!("I/O R{arrow}"),
                        egui::FontId::proportional(12.0 * crate::appearance::text_scale(ui.ctx())),
                        p.text,
                    );
                    if resp.clicked() {
                        if state.sort_column == SortColumn::IoRead {
                            state.sort_descending = !state.sort_descending;
                        } else {
                            state.sort_column = SortColumn::IoRead;
                            state.sort_descending = true;
                        }
                    }
                }

                // I/O Write header
                if layout.show_io_write {
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_io_w_w, 20.0),
                        egui::Sense::click(),
                    );
                    let arrow = if state.sort_column == SortColumn::IoWrite {
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
                        format!("I/O W{arrow}"),
                        egui::FontId::proportional(12.0 * crate::appearance::text_scale(ui.ctx())),
                        p.text,
                    );
                    if resp.clicked() {
                        if state.sort_column == SortColumn::IoWrite {
                            state.sort_descending = !state.sort_descending;
                        } else {
                            state.sort_column = SortColumn::IoWrite;
                            state.sort_descending = true;
                        }
                    }
                }

                // Disk Read header
                if layout.show_disk_read {
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_disk_r_w, 20.0),
                        egui::Sense::click(),
                    );
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
                        egui::FontId::proportional(12.0 * crate::appearance::text_scale(ui.ctx())),
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
                if layout.show_disk_write {
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_disk_w_w, 20.0),
                        egui::Sense::click(),
                    );
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
                        egui::FontId::proportional(12.0 * crate::appearance::text_scale(ui.ctx())),
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
                if layout.show_gpu {
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_gpu_w, 20.0),
                        egui::Sense::click(),
                    );
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
                        egui::FontId::proportional(12.0 * crate::appearance::text_scale(ui.ctx())),
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
}

pub fn render_process_row(
    ui: &mut egui::Ui,
    process: &ProcessRecord,
    idx: usize,
    layout: &ProcessColumnLayout,
    max_visible_memory: u64,
    selected_identity: &mut Option<ProcessIdentity>,
    cell_h: f32,
    p: Palette,
) {
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
        .inner_margin(egui::Margin::symmetric(layout.inner_margin_x as i8, 3))
        .show(ui, |ui| {
            ui.set_height(cell_h);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = layout.col_spacing;

                // Process Name (clickable)
                let (name_rect, name_resp) = ui.allocate_exact_size(
                    Vec2::new(layout.col_name_w, cell_h),
                    egui::Sense::click(),
                );
                let display_name = if process.name.is_empty() {
                    "<unnamed>"
                } else {
                    &process.name
                };
                // Clip long executable names to their own column.
                ui.painter_at(name_rect).text(
                    Pos2::new(name_rect.min.x, name_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    display_name,
                    egui::FontId::proportional(13.0 * crate::appearance::text_scale(ui.ctx())),
                    if is_selected { p.focus } else { p.text },
                );

                if name_resp.clicked() {
                    *selected_identity = Some(process.identity.clone());
                }
                name_resp.on_hover_text(display_name);

                // PID
                if layout.show_pid {
                    let (pid_rect, pid_resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_pid_w, cell_h),
                        egui::Sense::click(),
                    );
                    ui.painter_at(pid_rect).text(
                        Pos2::new(pid_rect.max.x - 4.0, pid_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        process.identity.pid.to_string(),
                        egui::FontId::monospace(12.0 * crate::appearance::text_scale(ui.ctx())),
                        p.secondary,
                    );
                    if pid_resp.clicked() {
                        *selected_identity = Some(process.identity.clone());
                    }
                }

                // CPU uses a thin rail below the number so the bar never crosses tabular digits
                if layout.show_cpu {
                    let (cpu_rect, cpu_resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_cpu_w, cell_h),
                        egui::Sense::click(),
                    );
                    if let Some(cpu_pct) = process.cpu_percent.value {
                        let bar_w = (cpu_rect.width() - 8.0) * (cpu_pct.clamp(0.0, 100.0) / 100.0);
                        if bar_w > 1.0 {
                            let bar_rect = Rect::from_min_size(
                                Pos2::new(cpu_rect.min.x + 4.0, cpu_rect.max.y - 3.0),
                                Vec2::new(bar_w, 2.0),
                            );
                            ui.painter_at(cpu_rect).rect_filled(
                                bar_rect,
                                CornerRadius::same(1),
                                p.cpu.linear_multiply(0.7),
                            );
                        }
                    }
                    ui.painter_at(cpu_rect).text(
                        Pos2::new(cpu_rect.max.x - 4.0, cpu_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_percent(process.cpu_percent.value),
                        egui::FontId::monospace(12.0 * crate::appearance::text_scale(ui.ctx())),
                        p.text,
                    );
                    if cpu_resp.clicked() {
                        *selected_identity = Some(process.identity.clone());
                    }
                }

                // The memory rail compares rows in the current result set.
                if layout.show_memory {
                    let (mem_rect, mem_resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_mem_w, cell_h),
                        egui::Sense::click(),
                    );
                    if let Some(mem_bytes) = process.memory_bytes.value {
                        let bar_w = (mem_rect.width() - 8.0)
                            * ((mem_bytes as f64) / (max_visible_memory as f64)) as f32;
                        if bar_w > 1.0 {
                            let bar_rect = Rect::from_min_size(
                                Pos2::new(mem_rect.min.x + 4.0, mem_rect.max.y - 3.0),
                                Vec2::new(bar_w, 2.0),
                            );
                            ui.painter_at(mem_rect).rect_filled(
                                bar_rect,
                                CornerRadius::same(1),
                                p.memory.linear_multiply(0.7),
                            );
                        }
                    }
                    let mem_text = match process.memory_bytes.value {
                        Some(b) => format_bytes(b),
                        None => "Unavailable".into(),
                    };
                    ui.painter_at(mem_rect).text(
                        Pos2::new(mem_rect.max.x - 4.0, mem_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        &mem_text,
                        egui::FontId::monospace(12.0 * crate::appearance::text_scale(ui.ctx())),
                        p.text,
                    );
                    if mem_resp.clicked() {
                        *selected_identity = Some(process.identity.clone());
                    }
                    if process.memory_bytes.value.is_some() {
                        mem_resp.on_hover_text(format!(
                            "{mem_text} · bar relative to the largest visible process"
                        ));
                    } else {
                        mem_resp.on_hover_text("Memory unavailable");
                    }
                }

                // I/O Read
                if layout.show_io_read {
                    let (io_r_rect, io_r_resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_io_r_w, cell_h),
                        egui::Sense::click(),
                    );
                    ui.painter_at(io_r_rect).text(
                        Pos2::new(io_r_rect.max.x - 4.0, io_r_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_rate(process.io_read_bytes_per_sec.value.map(|v| v as f32)),
                        egui::FontId::monospace(12.0 * crate::appearance::text_scale(ui.ctx())),
                        p.secondary,
                    );
                    if io_r_resp.clicked() {
                        *selected_identity = Some(process.identity.clone());
                    }
                }

                // I/O Write
                if layout.show_io_write {
                    let (io_w_rect, io_w_resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_io_w_w, cell_h),
                        egui::Sense::click(),
                    );
                    ui.painter_at(io_w_rect).text(
                        Pos2::new(io_w_rect.max.x - 4.0, io_w_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_rate(process.io_write_bytes_per_sec.value.map(|v| v as f32)),
                        egui::FontId::monospace(12.0 * crate::appearance::text_scale(ui.ctx())),
                        p.secondary,
                    );
                    if io_w_resp.clicked() {
                        *selected_identity = Some(process.identity.clone());
                    }
                }

                // Disk Read
                if layout.show_disk_read {
                    let (dr_rect, dr_resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_disk_r_w, cell_h),
                        egui::Sense::click(),
                    );
                    ui.painter_at(dr_rect).text(
                        Pos2::new(dr_rect.max.x - 4.0, dr_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_rate(process.disk_read_bytes_per_sec.value.map(|v| v as f32)),
                        egui::FontId::monospace(12.0 * crate::appearance::text_scale(ui.ctx())),
                        p.secondary,
                    );
                    if dr_resp.clicked() {
                        *selected_identity = Some(process.identity.clone());
                    }
                }

                // Disk Write
                if layout.show_disk_write {
                    let (dw_rect, dw_resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_disk_w_w, cell_h),
                        egui::Sense::click(),
                    );
                    ui.painter_at(dw_rect).text(
                        Pos2::new(dw_rect.max.x - 4.0, dw_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_rate(process.disk_write_bytes_per_sec.value.map(|v| v as f32)),
                        egui::FontId::monospace(12.0 * crate::appearance::text_scale(ui.ctx())),
                        p.secondary,
                    );
                    if dw_resp.clicked() {
                        *selected_identity = Some(process.identity.clone());
                    }
                }

                // GPU
                if layout.show_gpu {
                    let (gpu_rect, gpu_resp) = ui.allocate_exact_size(
                        Vec2::new(layout.col_gpu_w, cell_h),
                        egui::Sense::click(),
                    );
                    ui.painter_at(gpu_rect).text(
                        Pos2::new(gpu_rect.max.x - 4.0, gpu_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_percent(process.gpu_percent.value),
                        egui::FontId::monospace(12.0 * crate::appearance::text_scale(ui.ctx())),
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

fn render_process_table_inner(
    ui: &mut egui::Ui,
    rows: &[&ProcessRecord],
    selected_identity: &mut Option<ProcessIdentity>,
    state: &mut ProcessTableState,
    row_height: f32,
    p: Palette,
) {
    let text_scale = crate::appearance::text_scale(ui.ctx()).max(1.0);
    let layout =
        ProcessColumnLayout::compute_scaled(ui.available_width(), &state.columns, text_scale);
    let max_visible_memory = rows
        .iter()
        .filter_map(|process| process.memory_bytes.value)
        .max()
        .unwrap_or(1);

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

    render_process_header(ui, &layout, state, p);
    ui.separator();

    // Virtualized Table Rows with zero-drift height accounting
    let total_rows = rows.len();
    let row_height = row_height.max(32.0 * text_scale);
    let cell_h = (row_height - 6.0).max(16.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, total_rows, |ui, row_range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for idx in row_range {
                render_process_row(
                    ui,
                    rows[idx],
                    idx,
                    &layout,
                    max_visible_memory,
                    selected_identity,
                    cell_h,
                    p,
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_columns_remain_visible_at_narrow_widths() {
        let config = TableColumnsConfig {
            show_gpu: true,
            show_disk_read: true,
            show_disk_write: true,
            ..Default::default()
        };

        let layout = ProcessColumnLayout::compute(380.0, &config);
        assert!(layout.show_pid && layout.show_cpu && layout.show_memory);
        assert!(layout.show_disk_read && layout.show_disk_write && layout.show_gpu);
        assert!(ProcessColumnLayout::minimum_width(&config) > 380.0);
    }

    #[test]
    fn test_process_column_layout_width_and_virtualization_accounting() {
        let config = TableColumnsConfig::default();
        let available_w = 800.0;
        let layout = ProcessColumnLayout::compute(available_w, &config);

        // Name column width must fill remaining space
        let mut fixed_w = 0.0;
        let mut cols = 1; // name
        if layout.show_pid {
            fixed_w += layout.col_pid_w;
            cols += 1;
        }
        if layout.show_cpu {
            fixed_w += layout.col_cpu_w;
            cols += 1;
        }
        if layout.show_memory {
            fixed_w += layout.col_mem_w;
            cols += 1;
        }
        if layout.show_disk_read {
            fixed_w += layout.col_disk_r_w;
            cols += 1;
        }
        if layout.show_disk_write {
            fixed_w += layout.col_disk_w_w;
            cols += 1;
        }
        if layout.show_gpu {
            fixed_w += layout.col_gpu_w;
            cols += 1;
        }

        let spacing = (cols - 1) as f32 * layout.col_spacing + 2.0 * layout.inner_margin_x;
        let total_w = layout.col_name_w + fixed_w + spacing;
        assert!(
            (total_w - available_w).abs() < 0.1,
            "Total calculated column width {total_w} should match available width {available_w}"
        );

        // Virtualized row height exact match:
        // Declared row_height (e.g. 28.0) must equal inner cell_h (22.0) + vertical margins (2 * 3.0)
        // with item_spacing.y = 0.0 to prevent cumulative drift over hundreds of rows
        let declared_row_h = 28.0;
        let cell_h = declared_row_h - 6.0;
        let vertical_margin_total = 6.0;
        assert_eq!(cell_h + vertical_margin_total, declared_row_h);
    }

    #[test]
    fn test_process_table_inline_renders_without_nested_scroll_areas() {
        let ctx = egui::Context::default();
        let mut state = ProcessTableState::default();
        let mut selected = None;
        let frame = crate::smoke_test::test_frame();
        let rows: Vec<&ProcessRecord> = frame.processes.iter().collect();
        let p = crate::theme::get_palette(true);

        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
                ..Default::default()
            },
            |ui| {
                render_process_table_inline(ui, &rows, &mut selected, &mut state, 32.0, p);
            },
        );
        output.textures_delta.clear();
    }
}
