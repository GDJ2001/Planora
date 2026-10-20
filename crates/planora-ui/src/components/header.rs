use crate::{
    components::badge::render_status_badge,
    state::{AppState, DisplayPause},
    theme::Palette,
};
use eframe::egui::{self, Vec2};

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
        let right_width = 260.0;
        let left_width = (ui.available_width() - right_width - 12.0).max(180.0);

        ui.allocate_ui_with_layout(
            Vec2::new(left_width, ui.available_height()),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.add_space(2.0);
                ui.heading(title);
                ui.add_space(2.0);
                ui.label(egui::RichText::new(subtitle).color(p.secondary).size(12.0));
            },
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            // Status badge (rightmost)
            render_status_badge(ui, app_state, pause_state.is_paused, p);

            // Pause / Resume toggle button
            let pause_btn_text = if pause_state.is_paused {
                "Resume"
            } else {
                "Pause"
            };
            if ui
                .add_enabled(
                    *app_state != AppState::Replay,
                    egui::Button::new(pause_btn_text),
                )
                .clicked()
            {
                pause_state.is_paused = !pause_state.is_paused;
            }

            ui.label(
                egui::RichText::new(format!("{poll_interval_ms} ms"))
                    .color(p.secondary)
                    .monospace()
                    .size(11.0),
            );
        });
    });

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(12.0);
}
