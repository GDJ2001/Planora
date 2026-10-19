use crate::{
    components::header::render_page_header,
    formatting::format_bytes,
    state::{AppState, DisplayPause, HardwareViewMode},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Pos2, Rect, Stroke, Vec2};
use planora_core::Topology;

pub struct HardwareViewState {
    pub view_mode: HardwareViewMode,
    pub canvas_pan: Vec2,
    pub canvas_zoom: f32,
    pub selected_core_id: Option<u16>,
}

impl Default for HardwareViewState {
    fn default() -> Self {
        Self {
            view_mode: HardwareViewMode::Canvas,
            canvas_pan: Vec2::ZERO,
            canvas_zoom: 1.0,
            selected_core_id: None,
        }
    }
}

pub fn render_hardware_view(
    ui: &mut egui::Ui,
    topology: Option<&Topology>,
    state: &mut HardwareViewState,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    p: Palette,
) {
    // 1. Page Header
    render_page_header(
        ui,
        "Hardware Topology",
        "Substantiated processor cores, cache hierarchy, and compute node architecture",
        app_state,
        pause_state,
        poll_interval_ms,
        p,
    );

    let Some(t) = topology else {
        ui.label("Hardware topology discovery is loading or unavailable on this platform...");
        return;
    };

    // 2. Toolbar: View mode toggle & Canvas controls
    ui.horizontal(|ui| {
        ui.selectable_value(
            &mut state.view_mode,
            HardwareViewMode::Canvas,
            "Silicon Canvas",
        );
        ui.selectable_value(
            &mut state.view_mode,
            HardwareViewMode::Tree,
            "Accessible Tree",
        );

        if state.view_mode == HardwareViewMode::Canvas {
            ui.add_space(16.0);
            if ui.small_button("Zoom In (+)").clicked() {
                state.canvas_zoom = (state.canvas_zoom * 1.2).min(3.0);
            }
            if ui.small_button("Zoom Out (-)").clicked() {
                state.canvas_zoom = (state.canvas_zoom / 1.2).max(0.4);
            }
            if ui.small_button("Reset View").clicked() {
                state.canvas_pan = Vec2::ZERO;
                state.canvas_zoom = 1.0;
            }
        }
    });

    ui.add_space(12.0);

    match state.view_mode {
        HardwareViewMode::Tree => {
            render_tree_view(ui, t, p);
        }
        HardwareViewMode::Canvas => {
            render_canvas_view(ui, t, state, p);
        }
    }
}

fn render_tree_view(ui: &mut egui::Ui, t: &Topology, p: Palette) {
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.heading("Processor Packages & Cores");
            ui.add_space(6.0);
            let mut package_ids: Vec<u16> = t.cores.iter().filter_map(|c| c.package_id).collect();
            package_ids.sort_unstable();
            package_ids.dedup();
            if package_ids.is_empty() {
                package_ids.push(0);
            }
            for pkg_id in package_ids {
                ui.strong(format!("Package #{pkg_id}"));
                for core in t
                    .cores
                    .iter()
                    .filter(|c| c.package_id.unwrap_or(0) == pkg_id)
                {
                    ui.label(format!(
                        "  └── Core #{}: Logical CPUs {:?}",
                        core.id, core.logical_cpus
                    ));
                }
            }

            ui.add_space(14.0);
            ui.heading("Cache Hierarchy");
            ui.add_space(6.0);
            for cache in &t.caches {
                let size_s = format_bytes(cache.size_bytes);
                let line_s = cache
                    .line_bytes
                    .map(|l| format!("{l} B"))
                    .unwrap_or_else(|| "unknown".into());
                ui.label(format!(
                    "Cache #{} (L{} {}): {} · Line size: {} · Shared by CPUs: {:?}",
                    cache.id, cache.level, cache.cache_type, size_s, line_s, cache.shared_by
                ));
            }

            if !t.gpus.is_empty() {
                ui.add_space(14.0);
                ui.heading("GPU Compute Nodes");
                ui.add_space(6.0);
                for gpu in &t.gpus {
                    ui.label(format!("GPU #{}: {} ({})", gpu.id, gpu.name, gpu.source));
                }
            }
        });
}

fn render_canvas_view(ui: &mut egui::Ui, t: &Topology, state: &mut HardwareViewState, p: Palette) {
    let canvas_h = 360.0;
    let (response, painter) = ui.allocate_painter(
        Vec2::new(ui.available_width(), canvas_h),
        egui::Sense::click_and_drag(),
    );

    state.canvas_pan += response.drag_delta();

    let canvas_rect = response.rect;
    painter.rect_filled(
        canvas_rect,
        CornerRadius::same(CORNER_RADIUS as u8),
        p.surface_raised,
    );
    painter.rect_stroke(
        canvas_rect,
        CornerRadius::same(CORNER_RADIUS as u8),
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );

    let zoom = state.canvas_zoom;
    let num_cores = t.cores.len().max(1);
    let cols = if num_cores <= 4 {
        num_cores
    } else if num_cores <= 8 {
        4
    } else {
        8
    };
    let rows = num_cores.div_ceil(cols);

    let core_w = 124.0 * zoom;
    let core_h = 72.0 * zoom;
    let gap_x = 14.0 * zoom;
    let gap_y = 14.0 * zoom;

    let grid_w = cols as f32 * core_w + (cols.saturating_sub(1)) as f32 * gap_x;
    let grid_h = rows as f32 * core_h + (rows.saturating_sub(1)) as f32 * gap_y;

    let base_origin_x = canvas_rect.min.x + ((canvas_rect.width() - grid_w) / 2.0).max(24.0);
    let base_origin_y = canvas_rect.min.y + ((canvas_rect.height() - grid_h) / 2.0).max(36.0);
    let origin = Pos2::new(base_origin_x, base_origin_y) + state.canvas_pan;

    // Package boundary
    let pkg_pad = 14.0 * zoom;
    let pkg_rect = Rect::from_min_size(
        origin - Vec2::splat(pkg_pad),
        Vec2::new(grid_w + 2.0 * pkg_pad, grid_h + 2.0 * pkg_pad),
    );
    painter.rect_filled(
        pkg_rect,
        CornerRadius::same(6),
        p.surface.linear_multiply(0.5),
    );
    painter.rect_stroke(
        pkg_rect,
        CornerRadius::same(6),
        Stroke::new(1.0, p.border_control),
        egui::StrokeKind::Inside,
    );
    let pkg_id_str = t
        .cores
        .first()
        .and_then(|c| c.package_id)
        .map(|id| format!("Package #{id} · "))
        .unwrap_or_default();
    let pkg_name = format!("{}{} {}", pkg_id_str, t.vendor, t.model)
        .trim()
        .to_string();
    let pkg_label = if pkg_name.is_empty() {
        "Processor Package".to_string()
    } else {
        pkg_name
    };
    painter.text(
        pkg_rect.left_top() + Vec2::new(8.0, 4.0),
        egui::Align2::LEFT_TOP,
        pkg_label,
        egui::FontId::proportional(11.5 * zoom),
        p.secondary,
    );

    // Canvas Legend in top right
    let legend_x = canvas_rect.max.x - 12.0;
    let legend_y = canvas_rect.min.y + 10.0;
    painter.text(
        Pos2::new(legend_x, legend_y),
        egui::Align2::RIGHT_TOP,
        "Drag to pan · Click core to inspect cache hierarchy",
        egui::FontId::proportional(11.0),
        p.secondary,
    );

    let click_pos = if response.clicked() {
        response.interact_pointer_pos()
    } else {
        None
    };

    for (i, core) in t.cores.iter().enumerate() {
        let col_idx = (i % cols) as f32;
        let row_idx = (i / cols) as f32;
        let core_pos = origin + Vec2::new(col_idx * (core_w + gap_x), row_idx * (core_h + gap_y));
        let core_rect = Rect::from_min_size(core_pos, Vec2::new(core_w, core_h));

        if let Some(pos) = click_pos
            && core_rect.contains(pos)
        {
            state.selected_core_id = Some(core.id);
        }

        let is_selected = state.selected_core_id == Some(core.id);
        let fill = if is_selected { p.selected } else { p.surface };
        let stroke = if is_selected {
            Stroke::new(2.0, p.focus)
        } else {
            Stroke::new(1.0, p.cpu)
        };

        painter.rect_filled(core_rect, CornerRadius::same(4), fill);
        painter.rect_stroke(
            core_rect,
            CornerRadius::same(4),
            stroke,
            egui::StrokeKind::Inside,
        );

        painter.text(
            core_rect.center_top() + Vec2::new(0.0, 6.0 * zoom),
            egui::Align2::CENTER_TOP,
            format!("Core #{}", core.id),
            egui::FontId::proportional(13.0 * zoom),
            p.text,
        );

        let cpu_str = if core.logical_cpus.len() == 1 {
            format!("CPU {}", core.logical_cpus[0])
        } else {
            format!("CPUs {:?}", core.logical_cpus)
        };
        painter.text(
            core_rect.center_bottom() - Vec2::new(0.0, 8.0 * zoom),
            egui::Align2::CENTER_BOTTOM,
            cpu_str,
            egui::FontId::monospace(11.5 * zoom),
            p.secondary,
        );
    }

    // Detail Inspector below canvas
    ui.add_space(14.0);
    if let Some(core_id) = state.selected_core_id
        && let Some(core) = t.cores.iter().find(|c| c.id == core_id)
    {
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.strong(format!("Selected Core #{}", core.id));
                ui.label(format!("Logical Processors: {:?}", core.logical_cpus));
                let sharing_caches: Vec<_> = t
                    .caches
                    .iter()
                    .filter(|c| {
                        core.logical_cpus
                            .iter()
                            .any(|cpu| c.shared_by.contains(cpu))
                    })
                    .collect();
                if !sharing_caches.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Associated Caches:")
                            .size(11.5)
                            .color(p.secondary),
                    );
                    for c in sharing_caches {
                        let size = format_bytes(c.size_bytes);
                        ui.label(format!(
                            "• L{} {} ({}): Shared by {:?}",
                            c.level, c.cache_type, size, c.shared_by
                        ));
                    }
                }
            });
    }
}
