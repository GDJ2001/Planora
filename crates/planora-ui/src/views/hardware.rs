use crate::{
    components::header::render_page_header,
    formatting::format_bytes,
    state::{AppState, DisplayPause, HardwareViewMode},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Rect, Stroke, Vec2};
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
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.heading("Processor Packages & Cores");
                ui.add_space(6.0);
                let mut package_ids: Vec<u16> =
                    t.cores.iter().filter_map(|c| c.package_id).collect();
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

    let origin = canvas_rect.min + state.canvas_pan + Vec2::new(24.0, 24.0);
    let zoom = state.canvas_zoom;

    // Render cores in grid
    let mut x_off = 0.0f32;
    let mut y_off = 0.0f32;
    let core_w = 120.0 * zoom;
    let core_h = 70.0 * zoom;

    let click_pos = if response.clicked() {
        response.interact_pointer_pos()
    } else {
        None
    };

    for (i, core) in t.cores.iter().enumerate() {
        let core_rect =
            Rect::from_min_size(origin + Vec2::new(x_off, y_off), Vec2::new(core_w, core_h));

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
            egui::FontId::proportional(12.5 * zoom),
            p.text,
        );

        painter.text(
            core_rect.center_bottom() - Vec2::new(0.0, 8.0 * zoom),
            egui::Align2::CENTER_BOTTOM,
            format!("{:?}", core.logical_cpus),
            egui::FontId::monospace(10.5 * zoom),
            p.secondary,
        );

        x_off += core_w + 14.0 * zoom;
        if (i + 1) % 4 == 0 {
            x_off = 0.0;
            y_off += core_h + 16.0 * zoom;
        }
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
