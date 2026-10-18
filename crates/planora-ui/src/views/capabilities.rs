use crate::{
    components::header::render_page_header,
    state::{AppState, DisplayPause},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, CornerRadius, Stroke, Vec2};
use planora_core::Capability;

pub struct CapabilitiesViewState {
    pub filter: CapabilitiesFilter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CapabilitiesFilter {
    #[default]
    All,
    AvailableOnly,
    UnavailableOnly,
}

impl Default for CapabilitiesViewState {
    fn default() -> Self {
        Self {
            filter: CapabilitiesFilter::All,
        }
    }
}

pub fn render_capabilities_view(
    ui: &mut egui::Ui,
    capabilities: &[Capability],
    state: &mut CapabilitiesViewState,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    p: Palette,
) {
    // 1. Page Header
    render_page_header(
        ui,
        "System Capabilities",
        "Substantiated hardware and OS telemetry providers and sensor availability",
        app_state,
        pause_state,
        poll_interval_ms,
        p,
    );

    // 2. Filter Toolbar
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.selectable_value(&mut state.filter, CapabilitiesFilter::All, "All");
        ui.selectable_value(
            &mut state.filter,
            CapabilitiesFilter::AvailableOnly,
            "Available Only",
        );
        ui.selectable_value(
            &mut state.filter,
            CapabilitiesFilter::UnavailableOnly,
            "Unavailable Only",
        );
    });

    ui.add_space(12.0);

    let filtered: Vec<&Capability> = capabilities
        .iter()
        .filter(|cap| match state.filter {
            CapabilitiesFilter::All => true,
            CapabilitiesFilter::AvailableOnly => cap.available,
            CapabilitiesFilter::UnavailableOnly => !cap.available,
        })
        .collect();

    egui::Frame::group(ui.style())
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            if filtered.is_empty() {
                ui.label(
                    egui::RichText::new("No capabilities match the selected filter.")
                        .color(p.secondary),
                );
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("capabilities_grid")
                    .striped(true)
                    .num_columns(3)
                    .spacing([24.0, 10.0])
                    .show(ui, |ui| {
                        ui.strong("Capability");
                        ui.strong("Status");
                        ui.strong("Provider / Reason Details");
                        ui.end_row();

                        for cap in filtered {
                            ui.label(&cap.name);

                            if cap.available {
                                let (rect, _) = ui.allocate_exact_size(
                                    Vec2::new(64.0, 18.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    CornerRadius::same(4),
                                    p.surface_raised,
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Available",
                                    egui::FontId::proportional(11.0),
                                    p.success,
                                );
                            } else {
                                let (rect, resp) = ui.allocate_exact_size(
                                    Vec2::new(76.0, 18.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    CornerRadius::same(4),
                                    p.surface_raised,
                                );
                                ui.painter().rect_stroke(
                                    rect,
                                    CornerRadius::same(4),
                                    Stroke::new(1.0, p.border),
                                    egui::StrokeKind::Inside,
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Unavailable",
                                    egui::FontId::proportional(11.0),
                                    p.secondary,
                                );
                                if let Some(r) = &cap.reason {
                                    resp.on_hover_text(r);
                                }
                            }

                            match &cap.reason {
                                Some(r) => {
                                    ui.label(
                                        egui::RichText::new(r.as_str())
                                            .size(12.0)
                                            .color(p.secondary),
                                    );
                                }
                                None => {
                                    ui.label(
                                        egui::RichText::new("Substantiated by OS / Hardware")
                                            .size(12.0)
                                            .color(p.success),
                                    );
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        });
}
