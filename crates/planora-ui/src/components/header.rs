use crate::{
    components::badge::render_status_badge,
    state::{AppState, DisplayPause},
    theme::Palette,
};
use eframe::egui;

pub fn render_page_header(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    poll_interval_ms: u64,
    p: Palette,
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.add_space(2.0);
            ui.heading(title);
            ui.add_space(2.0);
            ui.label(egui::RichText::new(subtitle).color(p.secondary).size(12.5));
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Status badge
            render_status_badge(ui, app_state, pause_state.is_paused, p);

            ui.add_space(8.0);

            // Pause / Resume toggle button
            let pause_btn_text = if pause_state.is_paused {
                "Resume Display ▶"
            } else {
                "Pause Display ⏸"
            };
            if ui.button(pause_btn_text).clicked() {
                pause_state.is_paused = !pause_state.is_paused;
            }

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("{poll_interval_ms} ms"))
                    .color(p.secondary)
                    .monospace()
                    .size(11.5),
            );
        });
    });

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(12.0);
}
