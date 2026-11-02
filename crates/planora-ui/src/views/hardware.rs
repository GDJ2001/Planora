use crate::{
    components::header::render_page_header,
    formatting::format_bytes,
    state::{AppState, DisplayPause, HardwareViewMode},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Pos2, Rect, Stroke, Vec2};
use planora_core::Topology;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

const CORE_W: f32 = 124.0;
const CORE_H: f32 = 72.0;
const CORE_GAP: f32 = 14.0;
const PACKAGE_PAD: f32 = 16.0;
const PACKAGE_HEADER_H: f32 = 32.0;
const PACKAGE_GAP: f32 = 20.0;

struct PackageGroup {
    id: Option<u16>,
    core_indices: Vec<usize>,
    columns: usize,
    width: f32,
    height: f32,
}

fn package_groups(t: &Topology) -> Vec<PackageGroup> {
    let mut by_package: BTreeMap<Option<u16>, Vec<usize>> = BTreeMap::new();
    for (index, core) in t.cores.iter().enumerate() {
        by_package.entry(core.package_id).or_default().push(index);
    }
    by_package
        .into_iter()
        .map(|(id, core_indices)| {
            let count = core_indices.len();
            let columns = if count <= 4 {
                count
            } else if count <= 8 {
                4
            } else {
                8
            };
            let rows = count.div_ceil(columns);
            PackageGroup {
                id,
                core_indices,
                columns,
                width: columns as f32 * CORE_W
                    + columns.saturating_sub(1) as f32 * CORE_GAP
                    + 2.0 * PACKAGE_PAD,
                height: rows as f32 * CORE_H
                    + rows.saturating_sub(1) as f32 * CORE_GAP
                    + 2.0 * PACKAGE_PAD
                    + PACKAGE_HEADER_H,
            }
        })
        .collect()
}

fn canvas_height(groups: &[PackageGroup]) -> f32 {
    let total_height = groups.iter().map(|group| group.height).sum::<f32>()
        + groups.len().saturating_sub(1) as f32 * PACKAGE_GAP;
    (total_height * 0.8 + 56.0).clamp(360.0, 1600.0)
}

fn fit_zoom(content_size: Vec2, canvas_size: Vec2) -> f32 {
    if content_size.x <= 0.0 || content_size.y <= 0.0 {
        return 1.0;
    }
    let available = canvas_size - Vec2::new(48.0, 56.0);
    (available.x.max(100.0) / content_size.x)
        .min(available.y.max(100.0) / content_size.y)
        .clamp(0.1, 1.75)
}

pub struct HardwareViewState {
    pub view_mode: HardwareViewMode,
    pub canvas_pan: Vec2,
    pub canvas_zoom: f32,
    pub selected_core_id: Option<u16>,
    pub initial_fit_done: bool,
    pub fitted_layout_signature: Option<u64>,
    pub rescan_hardware_requested: bool,
    pub export_compat_report_requested: bool,
    pub status_message: Option<(bool, String)>,
}

impl Default for HardwareViewState {
    fn default() -> Self {
        Self {
            view_mode: HardwareViewMode::Canvas,
            canvas_pan: Vec2::ZERO,
            canvas_zoom: 1.0,
            selected_core_id: None,
            initial_fit_done: false,
            fitted_layout_signature: None,
            rescan_hardware_requested: false,
            export_compat_report_requested: false,
            status_message: None,
        }
    }
}

fn sync_topology_state(state: &mut HardwareViewState, topology: &Topology) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    topology.generation.hash(&mut hasher);
    topology.vendor.hash(&mut hasher);
    topology.model.hash(&mut hasher);
    for core in &topology.cores {
        core.id.hash(&mut hasher);
        core.package_id.hash(&mut hasher);
        core.logical_cpus.hash(&mut hasher);
    }
    let signature = hasher.finish();
    if state.fitted_layout_signature != Some(signature) {
        state.initial_fit_done = false;
        state.fitted_layout_signature = Some(signature);
        if state
            .selected_core_id
            .is_some_and(|id| !topology.cores.iter().any(|core| core.id == id))
        {
            state.selected_core_id = None;
        }
    }
}

fn core_utilization(
    core: &planora_core::CoreDescriptor,
    frame: Option<&planora_core::TelemetryFrame>,
) -> Option<f32> {
    let f = frame?;
    let mut sum = 0.0f32;
    if core.logical_cpus.is_empty() {
        return None;
    }
    for &cpu_idx in &core.logical_cpus {
        let mut matches = f.logical_cpus.iter().filter(|cpu| cpu.cpu_index == cpu_idx);
        let cpu = matches.next()?;
        // CoreDescriptor does not carry a Windows processor-group number.
        // Duplicate local indices across groups cannot be mapped safely.
        if matches.next().is_some() {
            return None;
        }
        sum += cpu.utilization_percent.value?;
    }
    Some(sum / core.logical_cpus.len() as f32)
}

pub fn render_hardware_view(
    ui: &mut egui::Ui,
    topology: Option<&Topology>,
    frame: Option<&planora_core::TelemetryFrame>,
    inventory: Option<&planora_core::HardwareInventory>,
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
    sync_topology_state(state, t);

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
                state.initial_fit_done = false; // Trigger recalculation of fit zoom
            }
        }
    });

    ui.horizontal(|ui| {
        if ui.button("Rescan Hardware").clicked() {
            state.rescan_hardware_requested = true;
        }
        if ui.button("Export Compatibility Report").clicked() {
            state.export_compat_report_requested = true;
        }
    });
    if let Some((is_error, msg)) = &state.status_message {
        let color = if *is_error { p.error } else { p.success };
        ui.colored_label(color, msg);
    }
    ui.add_space(12.0);

    match state.view_mode {
        HardwareViewMode::Tree => {
            render_tree_view(ui, t, frame, inventory, state, p);
        }
        HardwareViewMode::Canvas => {
            render_canvas_view(ui, t, frame, inventory, state, p);
            if let Some(inv) = inventory {
                ui.add_space(14.0);
                render_inventory_section(ui, inv, p);
            }
        }
    }
}

fn render_tree_view(
    ui: &mut egui::Ui,
    t: &Topology,
    frame: Option<&planora_core::TelemetryFrame>,
    inventory: Option<&planora_core::HardwareInventory>,
    state: &mut HardwareViewState,
    p: Palette,
) {
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.heading("Processor Packages & Cores");
            ui.add_space(6.0);
            let groups = package_groups(t);
            if groups.is_empty() {
                ui.label("No processor cores were reported by the platform.");
            }
            for group in groups {
                if let Some(id) = group.id {
                    ui.strong(format!("Package #{id}"));
                } else {
                    ui.strong("Cores with unknown package identity");
                }
                for index in group.core_indices {
                    let core = &t.cores[index];
                    let util_opt = core_utilization(core, frame);
                    let util_str =
                        util_opt.map_or_else(String::new, |u| format!(" · Utilization: {u:.1}%"));
                    let is_selected = state.selected_core_id == Some(core.id);
                    let text = format!(
                        "  └── Core #{}: Logical CPUs {:?}{}",
                        core.id, core.logical_cpus, util_str
                    );
                    if ui.selectable_label(is_selected, text).clicked() {
                        state.selected_core_id = Some(core.id);
                    }
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

    render_detail_inspector(ui, t, frame, state, p);
    if let Some(inv) = inventory {
        ui.add_space(14.0);
        render_inventory_section(ui, inv, p);
    }
}

fn render_canvas_view(
    ui: &mut egui::Ui,
    t: &Topology,
    frame: Option<&planora_core::TelemetryFrame>,
    inventory: Option<&planora_core::HardwareInventory>,
    state: &mut HardwareViewState,
    p: Palette,
) {
    let groups = package_groups(t);
    let unscaled_w = groups.iter().map(|group| group.width).fold(0.0, f32::max);
    let unscaled_h = groups.iter().map(|group| group.height).sum::<f32>()
        + groups.len().saturating_sub(1) as f32 * PACKAGE_GAP;
    let canvas_h = canvas_height(&groups);
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

    // Auto-fit graph on first load or Reset View
    if !state.initial_fit_done {
        state.canvas_zoom = fit_zoom(Vec2::new(unscaled_w, unscaled_h), canvas_rect.size());
        state.canvas_pan = Vec2::ZERO;
        state.initial_fit_done = true;
    }

    let zoom = state.canvas_zoom;
    let painter = painter.with_clip_rect(canvas_rect);
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

    if groups.is_empty() {
        painter.text(
            canvas_rect.center(),
            egui::Align2::CENTER_CENTER,
            "No processor cores were reported by the platform",
            egui::FontId::proportional(14.0),
            p.secondary,
        );
    }

    let mut group_y = canvas_rect.min.y + (canvas_rect.height() - unscaled_h * zoom) / 2.0;
    for group in &groups {
        let pkg_origin = Pos2::new(
            canvas_rect.min.x + (canvas_rect.width() - group.width * zoom) / 2.0,
            group_y,
        ) + state.canvas_pan;
        let pkg_rect = Rect::from_min_size(
            pkg_origin,
            Vec2::new(group.width * zoom, group.height * zoom),
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

        let group_name = match group.id {
            Some(id) => format!("Package #{id}"),
            None => "Cores with unknown package identity".to_string(),
        };
        let model = format!("{} {}", t.vendor, t.model).trim().to_string();
        let full_label = if model.is_empty() {
            group_name
        } else {
            format!("{group_name} · {model}")
        };
        let header_rect = Rect::from_min_size(
            pkg_rect.min,
            Vec2::new(pkg_rect.width(), PACKAGE_HEADER_H * zoom),
        );
        painter.with_clip_rect(header_rect.shrink(2.0)).text(
            header_rect.left_center() + Vec2::new(12.0 * zoom, 0.0),
            egui::Align2::LEFT_CENTER,
            &full_label,
            egui::FontId::proportional((12.5 * zoom).clamp(9.0, 18.0)),
            p.text,
        );
        if response
            .hover_pos()
            .is_some_and(|pos| pkg_rect.contains(pos))
        {
            let _ = response.clone().on_hover_text(full_label);
        }

        let grid_origin =
            pkg_rect.min + Vec2::new(PACKAGE_PAD * zoom, (PACKAGE_HEADER_H + PACKAGE_PAD) * zoom);
        for (slot, index) in group.core_indices.iter().enumerate() {
            let core = &t.cores[*index];
            let core_pos = grid_origin
                + Vec2::new(
                    (slot % group.columns) as f32 * (CORE_W + CORE_GAP) * zoom,
                    (slot / group.columns) as f32 * (CORE_H + CORE_GAP) * zoom,
                );
            let core_rect = Rect::from_min_size(core_pos, Vec2::new(CORE_W * zoom, CORE_H * zoom));

            if let Some(pos) = click_pos
                && core_rect.contains(pos)
            {
                state.selected_core_id = Some(core.id);
            }

            let core_util = core_utilization(core, frame);
            let is_selected = state.selected_core_id == Some(core.id);
            let fill = if is_selected {
                p.selected
            } else if let Some(util) = core_util {
                p.cpu
                    .gamma_multiply(0.18 + 0.65 * (util.clamp(0.0, 100.0) / 100.0))
            } else {
                p.surface
            };
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

            let core_painter = painter.with_clip_rect(core_rect.shrink(2.0));
            core_painter.text(
                core_rect.center_top() + Vec2::new(0.0, 6.0 * zoom),
                egui::Align2::CENTER_TOP,
                format!("Core #{}", core.id),
                egui::FontId::proportional(13.0 * zoom),
                p.text,
            );
            if let Some(util) = core_util {
                core_painter.text(
                    core_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{util:.0}%"),
                    egui::FontId::monospace(13.0 * zoom),
                    p.text,
                );
            }
            let cpu_str = if core.logical_cpus.len() == 1 {
                format!("CPU {}", core.logical_cpus[0])
            } else {
                format!("CPUs {:?}", core.logical_cpus)
            };
            core_painter.text(
                core_rect.center_bottom() - Vec2::new(0.0, 8.0 * zoom),
                egui::Align2::CENTER_BOTTOM,
                cpu_str,
                egui::FontId::monospace(11.5 * zoom),
                p.secondary,
            );
        }
        group_y += (group.height + PACKAGE_GAP) * zoom;
    }

    render_detail_inspector(ui, t, frame, state, p);
    if let Some(inv) = inventory {
        ui.add_space(14.0);
        render_inventory_section(ui, inv, p);
    }
}

fn render_detail_inspector(
    ui: &mut egui::Ui,
    t: &Topology,
    frame: Option<&planora_core::TelemetryFrame>,
    state: &HardwareViewState,
    p: Palette,
) {
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
                if let Some(util) = core_utilization(core, frame) {
                    ui.label(format!("Current Utilization: {util:.1}%"));
                }
                if let Some(cls) = &core.class {
                    ui.label(format!("Architecture Class: {cls}"));
                }
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

fn render_inventory_section(ui: &mut egui::Ui, inv: &planora_core::HardwareInventory, p: Palette) {
    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.heading("Discovered Hardware Inventory");
            ui.add_space(8.0);

            // Motherboard
            ui.strong("Motherboard & BIOS");
            let mb = &inv.motherboard;
            let mfg = if mb.manufacturer.is_empty() {
                "Unknown manufacturer"
            } else {
                &mb.manufacturer
            };
            let model = if mb.product_name.is_empty() {
                "Unknown model"
            } else {
                &mb.product_name
            };
            let ver = &mb.version;
            let bios_v = if mb.bios_vendor.is_empty() {
                "Unknown vendor"
            } else {
                &mb.bios_vendor
            };
            let bios_ver = if mb.bios_version.is_empty() {
                "Unknown version"
            } else {
                &mb.bios_version
            };
            let bios_date = &mb.bios_release_date;
            ui.label(format!("  └── Board: {mfg} {model} {ver}"));
            ui.label(format!("  └── BIOS: {bios_v} {bios_ver} {bios_date}"));
            ui.add_space(8.0);

            // Memory
            ui.strong("Physical Memory (SMBIOS)");
            let total_ram_bytes = inv.memory.total_physical_bytes;
            let slots_used = inv.memory.modules.len();
            ui.label(format!(
                "  └── Installed: {} ({} slots populated)",
                format_bytes(total_ram_bytes),
                slots_used
            ));
            for module in &inv.memory.modules {
                let loc = &module.slot_locator;
                let cap = format_bytes(module.capacity_bytes);
                let speed = if module.speed_mts > 0 {
                    format!("· {} MT/s", module.speed_mts)
                } else {
                    String::new()
                };
                let mfg = &module.manufacturer;
                let part = &module.part_number;
                ui.label(format!("      └── {loc}: {cap} {speed} {mfg} {part}"));
            }
            ui.add_space(8.0);

            // Storage
            if !inv.storage.is_empty() {
                ui.strong("Physical Storage");
                for disk in &inv.storage {
                    let model = if disk.model.is_empty() {
                        "Unknown Drive"
                    } else {
                        &disk.model
                    };
                    let bus = &disk.bus_type;
                    let media = &disk.media_type;
                    let cap = format_bytes(disk.capacity_bytes);
                    let path = &disk.device_id;
                    ui.label(format!("  └── {model} ({bus} {media}) {cap} · {path}"));
                }
                ui.add_space(8.0);
            }

            // Network
            if !inv.network.is_empty() {
                ui.strong("Network Interfaces");
                for net in &inv.network {
                    let speed = net
                        .link_speed_bps
                        .map(|s| format!("{} Mbps", s / 1_000_000))
                        .unwrap_or_default();
                    let desc = &net.description;
                    ui.label(format!("  └── {} ({}): {}", net.name, desc, speed));
                }
                ui.add_space(8.0);
            }

            // GPUs
            if !inv.gpus.is_empty() {
                ui.strong("Graphics Adapters");
                for gpu in &inv.gpus {
                    let pci = format!("{:04x}:{:04x}", gpu.pci_vendor_id, gpu.pci_device_id);
                    let drv = gpu.driver_version.as_deref().unwrap_or("N/A");
                    let vram = format_bytes(gpu.dedicated_vram_bytes);
                    ui.label(format!(
                        "  └── {} · PCI: {pci} · Dedicated VRAM: {vram} · Driver: {drv}",
                        gpu.name
                    ));
                }
                ui.add_space(8.0);
            }

            // CPU Features
            if !inv.cpu.features.is_empty() {
                ui.strong("Processor Features");
                ui.label(format!("  └── Features: {}", inv.cpu.features.join(", ")));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use planora_core::{CoreDescriptor, LogicalCpuSample, Metric};

    fn topology(package_ids: &[Option<u16>]) -> Topology {
        Topology {
            generation: 1,
            vendor: "Test Vendor".into(),
            model: "A long processor model name for the header clipping test".into(),
            architecture: "x86_64".into(),
            cores: package_ids
                .iter()
                .enumerate()
                .map(|(index, package_id)| CoreDescriptor {
                    id: index as u16,
                    package_id: *package_id,
                    numa_node: None,
                    logical_cpus: vec![index as u16],
                    class: None,
                })
                .collect(),
            caches: vec![],
            gpus: vec![],
            notes: vec![],
        }
    }

    #[test]
    fn test_package_header_does_not_overlap_core_nodes() {
        for zoom in [0.5, 0.8, 1.0, 1.5, 2.0] {
            let header_bottom = PACKAGE_HEADER_H * zoom;
            let core_top = (PACKAGE_HEADER_H + PACKAGE_PAD) * zoom;
            assert!(core_top - header_bottom >= 8.0 * zoom);
        }
    }

    #[test]
    fn multi_package_layout_preserves_discovered_identity_and_fits() {
        let mut ids = vec![Some(0); 32];
        ids.extend(vec![Some(2); 32]);
        ids.push(None);
        let t = topology(&ids);
        let groups = package_groups(&t);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].id, None);
        assert_eq!(groups[1].id, Some(0));
        assert_eq!(groups[2].id, Some(2));
        assert_eq!(
            groups.iter().map(|g| g.core_indices.len()).sum::<usize>(),
            65
        );
        assert_eq!(groups[0].core_indices, vec![64]);

        let width = groups.iter().map(|g| g.width).fold(0.0, f32::max);
        let height =
            groups.iter().map(|g| g.height).sum::<f32>() + (groups.len() - 1) as f32 * PACKAGE_GAP;
        for canvas_width in [760.0, 900.0, 1180.0] {
            let canvas_size = Vec2::new(canvas_width, canvas_height(&groups));
            let zoom = fit_zoom(Vec2::new(width, height), canvas_size);
            assert!(width * zoom <= canvas_size.x - 48.0 + 0.01);
            assert!(height * zoom <= canvas_size.y - 56.0 + 0.01);
        }
    }

    #[test]
    fn topology_change_refits_and_discards_missing_core_selection() {
        let mut state = HardwareViewState {
            selected_core_id: Some(7),
            initial_fit_done: true,
            ..Default::default()
        };
        sync_topology_state(&mut state, &topology(&[Some(0); 8]));
        state.initial_fit_done = true;
        // A restarted helper can reuse the same generation number.
        let next = topology(&[Some(0), Some(0)]);
        sync_topology_state(&mut state, &next);
        assert!(!state.initial_fit_done);
        assert!(state.fitted_layout_signature.is_some());
        assert_eq!(state.selected_core_id, None);
    }

    #[test]
    fn ambiguous_processor_group_addressing_has_no_utilization_tint() {
        let mut frame = crate::smoke_test::test_frame();
        frame.logical_cpus = [0, 1]
            .map(|group| LogicalCpuSample {
                cpu_index: 0,
                group,
                utilization_percent: Metric::available(40.0, "%", "fixture", 1000),
            })
            .to_vec();
        let core = CoreDescriptor {
            id: 0,
            package_id: Some(0),
            numa_node: None,
            logical_cpus: vec![0],
            class: None,
        };
        assert_eq!(core_utilization(&core, Some(&frame)), None);
    }
}
