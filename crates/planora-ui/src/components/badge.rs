use crate::{state::AppState, theme::Palette};
use eframe::egui::{self, CornerRadius, Pos2, Stroke, Vec2};
use planora_core::AvailabilityReason;

pub fn render_status_badge(ui: &mut egui::Ui, state: &AppState, is_paused: bool, p: Palette) {
    let (bg, text_color, dot_color, label) = if is_paused {
        (
            p.surface_raised,
            p.warning,
            p.warning,
            "⏸ Display Paused".to_string(),
        )
    } else {
        match state {
            AppState::Live => (
                p.surface_raised,
                p.text,
                p.success,
                "Live Telemetry".to_string(),
            ),
            AppState::Stale(dur) => (
                p.surface_raised,
                p.warning,
                p.warning,
                format!("Stale ({:.1}s)", dur.as_secs_f32()),
            ),
            AppState::Connecting | AppState::Reconnecting => (
                p.surface_raised,
                p.secondary,
                p.secondary,
                "Connecting...".to_string(),
            ),
            AppState::AwaitingAuthorization => (
                p.surface_raised,
                p.error,
                p.error,
                "Needs Authorization".to_string(),
            ),
            AppState::IncompatibleVersion { helper_version } => (
                p.surface_raised,
                p.error,
                p.error,
                format!("Version Mismatch (v{helper_version})"),
            ),
            AppState::Failed(_) => (p.surface_raised, p.error, p.error, "Offline".to_string()),
            AppState::FirstLaunch => (
                p.surface_raised,
                p.secondary,
                p.secondary,
                "Setup Required".to_string(),
            ),
        }
    };

    let height = 22.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(140.0, height), egui::Sense::hover());

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(11), bg);
    painter.rect_stroke(
        rect,
        CornerRadius::same(11),
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );

    // Status dot
    let dot_center = Pos2::new(rect.min.x + 12.0, rect.center().y);
    painter.circle_filled(dot_center, 3.5, dot_color);

    // Text label
    let text_pos = Pos2::new(rect.min.x + 22.0, rect.center().y - 6.0);
    painter.text(
        text_pos,
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(11.5),
        text_color,
    );
}

pub fn render_compact_sidebar_status(
    ui: &mut egui::Ui,
    state: &AppState,
    is_paused: bool,
    p: Palette,
) {
    if is_paused {
        ui.colored_label(p.warning, "⏸ Display Paused");
        return;
    }
    match state {
        AppState::Live => {
            ui.colored_label(p.success, "● Live Telemetry");
        }
        AppState::Stale(elapsed) => {
            ui.colored_label(
                p.warning,
                format!("▲ Stale ({:.1}s ago)", elapsed.as_secs_f32()),
            );
        }
        AppState::Connecting | AppState::Reconnecting => {
            ui.colored_label(p.secondary, "◌ Connecting...");
        }
        AppState::AwaitingAuthorization => {
            ui.colored_label(p.error, "🔒 Needs Authorization");
        }
        AppState::IncompatibleVersion { helper_version } => {
            ui.colored_label(p.error, format!("✖ Version mismatch (v{helper_version})"));
        }
        AppState::Failed(_) => {
            ui.colored_label(p.error, "✖ Offline");
        }
        AppState::FirstLaunch => {
            ui.colored_label(p.secondary, "○ Setup required");
        }
    }
}

pub fn render_availability_badge(
    ui: &mut egui::Ui,
    reason: Option<&AvailabilityReason>,
    p: Palette,
) {
    match reason {
        None => {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(64.0, 18.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), p.surface_raised);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Available",
                egui::FontId::proportional(11.0),
                p.success,
            );
        }
        Some(r) => {
            let label = match r {
                AvailabilityReason::WarmingUp => "Warming up",
                AvailabilityReason::Unsupported(_) => "Unsupported",
                AvailabilityReason::PermissionDenied(_) => "Denied",
                AvailabilityReason::TemporarilyFailed(_) => "Failed",
                AvailabilityReason::Stale(_) => "Stale",
            };
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(76.0, 18.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), p.surface_raised);
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(4),
                Stroke::new(1.0, p.border),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                p.secondary,
            );
            resp.on_hover_text(format!("Reason: {r}"));
        }
    }
}
