use crate::{
    components::badge::render_status_badge,
    state::{AppState, DisplayPause},
    theme::Palette,
};
use eframe::egui::{self, Vec2};

fn header_text(ui: &mut egui::Ui, title: &str, subtitle: &str, p: Palette) {
    ui.add(egui::Label::new(egui::RichText::new(title).heading()).wrap());
    ui.add(
        egui::Label::new(
            egui::RichText::new(subtitle)
                .color(p.secondary)
                .size(12.0 * crate::appearance::text_scale(ui.ctx())),
        )
        .wrap(),
    );
}

fn header_controls(
    ui: &mut egui::Ui,
    state: &AppState,
    pause: &mut DisplayPause,
    interval: u64,
    p: Palette,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(format!("{interval} ms"))
                .color(p.secondary)
                .monospace(),
        );
        if ui
            .add_enabled(
                *state != AppState::Replay,
                egui::Button::new(if pause.is_paused { "Resume" } else { "Pause" }),
            )
            .clicked()
        {
            pause.is_paused = !pause.is_paused;
        }
        render_status_badge(ui, state, pause.is_paused, p);
    });
}

pub fn render_page_header(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    p: Palette,
) {
    let scale = crate::appearance::text_scale(ui.ctx()).max(1.0);
    let controls_width = 300.0 * scale;
    let available = ui.available_width();
    if available < controls_width + 300.0 * scale {
        header_text(ui, title, subtitle, p);
        header_controls(ui, app_state, pause_state, poll_interval_ms, p);
    } else {
        ui.horizontal_top(|ui| {
            let left_width = available - controls_width - ui.spacing().item_spacing.x;
            ui.allocate_ui_with_layout(
                Vec2::new(left_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(left_width);
                    header_text(ui, title, subtitle, p);
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(controls_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    header_controls(ui, app_state, pause_state, poll_interval_ms, p);
                },
            );
        });
    }
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(12.0);
}
