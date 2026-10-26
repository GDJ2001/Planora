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
        let text_scale = crate::appearance::text_scale(ui.ctx()).max(1.0);
        let right_width = 260.0 * text_scale;
        let left_width = (ui.available_width() - right_width - 12.0).max(60.0);

        ui.allocate_ui_with_layout(
            Vec2::new(left_width, ui.available_height()),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.set_max_width(left_width);
                ui.add_space(2.0);
                ui.add(egui::Label::new(egui::RichText::new(title).heading()).truncate());
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(subtitle)
                            .color(p.secondary)
                            .size(12.0 * text_scale),
                    )
                    .truncate(),
                );
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
                    .size(11.0 * crate::appearance::text_scale(ui.ctx())),
            );
        });
    });

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(12.0);
}
