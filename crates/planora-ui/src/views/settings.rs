use crate::{
    components::header::render_page_header,
    formatting::format_bytes,
    history::HistoryRange,
    state::{AppState, BUILD_INFO, DisplayPause, UserSettings},
    theme::{CORNER_RADIUS, Density, Palette, Theme},
};
use eframe::egui::{self, Color32, CornerRadius, Stroke, Vec2};
use planora_core::{Command, DiagnosticEntry};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc::SyncSender,
};

#[derive(Default)]
pub struct SettingsViewState {
    pub show_confirm_modal: bool,
    pub preview_files: Vec<(String, u64)>,
    pub status_message: Option<(bool, String)>,
}

fn is_reparse_point_or_symlink(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

pub fn render_settings_view(
    ui: &mut egui::Ui,
    settings: &mut UserSettings,
    settings_error: &mut Option<String>,
    state: &mut SettingsViewState,
    poll_ms: &Arc<AtomicU64>,
    effective_ms: Option<u64>,
    diagnostic_logs: &[DiagnosticEntry],
    app_state: &AppState,
    pause_state: &mut DisplayPause,
    tx: &SyncSender<Command>,
    p: Palette,
) {
    let current_poll = poll_ms.load(Ordering::Relaxed);

    // 1. Page Header
    render_page_header(
        ui,
        "Settings & Preferences",
        "Interface themes, measurement cadence, diagnostic logs, and platform build information",
        app_state,
        pause_state,
        current_poll,
        p,
    );

    if let Some(error) = settings_error.as_deref() {
        ui.colored_label(p.error, error);
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        // Group 1: Appearance & Display
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.strong("Appearance & Motion");
                ui.add_space(8.0);

                // Theme Mode with visual preview swatches
                ui.horizontal(|ui| {
                    ui.label("Theme Mode:");
                    ui.add_space(8.0);

                    for (t, label, preview_bg, preview_fg) in [
                        (Theme::System, "System Auto", p.surface_raised, p.text),
                        (Theme::Dark, "Dark (Planora Deep)", Color32::from_rgb(11, 18, 32), Color32::from_rgb(244, 247, 251)),
                        (Theme::Light, "Light (Planora Clean)", Color32::from_rgb(244, 247, 251), Color32::from_rgb(20, 33, 61)),
                    ] {
                        let is_active = settings.theme == t;
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(140.0, 32.0), egui::Sense::click());
                        let painter = ui.painter();
                        painter.rect_filled(rect, CornerRadius::same(4), preview_bg);
                        painter.rect_stroke(
                            rect,
                            CornerRadius::same(4),
                            Stroke::new(if is_active { 2.0 } else { 1.0 }, if is_active { p.primary } else { p.border }),
                            egui::StrokeKind::Inside,
                        );
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            egui::FontId::proportional(12.0),
                            preview_fg,
                        );

                        if resp.clicked() {
                            settings.theme = t;
                            persist(settings, settings_error);
                        }
                    }
                });

                ui.add_space(10.0);

                // Density Selection
                ui.horizontal(|ui| {
                    ui.label("Display Density:");
                    ui.add_space(8.0);
                    let mut changed = false;
                    changed |= ui.selectable_value(&mut settings.density, Density::Compact, "Compact").changed();
                    changed |= ui.selectable_value(&mut settings.density, Density::Normal, "Normal").changed();
                    changed |= ui.selectable_value(&mut settings.density, Density::Spacious, "Spacious").changed();
                    if changed {
                        persist(settings, settings_error);
                    }
                });

                ui.add_space(10.0);

                // Reduced Motion Toggle
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut settings.reduced_motion, "Reduced Motion (Disable continuous animations and transitions)").changed() {
                        persist(settings, settings_error);
                    }
                });
            });

        ui.add_space(14.0);

        // Group 2: Telemetry Cadence & History
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.strong("Telemetry & Historical Buffers");
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Sampling Poll Interval:");
                    ui.add_space(8.0);
                    for ms in [100, 250, 500, 1_000, 2_000] {
                        let is_sel = current_poll == ms;
                        if ui.selectable_label(is_sel, format!("{ms} ms")).clicked() {
                            poll_ms.store(ms, Ordering::Relaxed);
                            settings.poll_interval_ms = ms;
                            persist(settings, settings_error);
                        }
                    }
                });

                ui.label(format!(
                    "Requested: {current_poll} ms · Effective: {}",
                    effective_ms.map_or_else(|| "Waiting for helper".to_string(), |ms| format!("{ms} ms"))
                ));

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Default History Time Window:");
                    ui.add_space(8.0);
                    let mut changed = false;
                    changed |= ui.selectable_value(&mut settings.default_history_range, HistoryRange::Sec30, "30 seconds").changed();
                    changed |= ui.selectable_value(&mut settings.default_history_range, HistoryRange::Sec60, "60 seconds (default)").changed();
                    changed |= ui.selectable_value(&mut settings.default_history_range, HistoryRange::Min5, "5 minutes").changed();
                    changed |= ui.selectable_value(&mut settings.default_history_range, HistoryRange::Min15, "15 minutes").changed();
                    if changed {
                        persist(settings, settings_error);
                    }
                });
            });

        ui.add_space(14.0);

        // Group 3: Diagnostics & Operation Log Buffer
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Helper Operation Logs (256 Event Ring Buffer)");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Copy Logs to Clipboard").clicked() {
                            let text = diagnostic_logs
                                .iter()
                                .map(|e| format!("[{} ms] op:{} {:?} - {}", e.timestamp_ms, e.operation_id, e.code, e.message))
                                .collect::<Vec<_>>()
                                .join("\n");
                            ui.copy_text(text);
                        }
                        if ui.button("Fetch Diagnostic Logs").clicked() {
                            let _ = tx.send(Command::DiagnosticLogs);
                        }
                    });
                });

                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    if diagnostic_logs.is_empty() {
                        ui.label(egui::RichText::new("No diagnostic events fetched yet. Click 'Fetch Diagnostic Logs' to inspect.").color(p.secondary));
                    } else {
                        for entry in diagnostic_logs {
                            ui.label(
                                egui::RichText::new(format!(
                                    "[{} ms] op:{} {:?} - {}",
                                    entry.timestamp_ms, entry.operation_id, entry.code, entry.message
                                ))
                                .monospace()
                                .size(11.0),
                            );
                        }
                    }
                });
            });

        ui.add_space(14.0);

        // Group 4: Local Data & Personal Privacy
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.strong("Local Data & Personal Privacy");
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Planora preserves your personal preferences and recording sessions by default.")
                        .size(11.5)
                        .color(p.secondary),
                );
                ui.label(
                    egui::RichText::new("You can explicitly inspect and purge all current-user configuration and stored data.")
                        .size(11.5)
                        .color(p.secondary),
                );
                ui.add_space(8.0);

                if let Some((is_err, msg)) = &state.status_message {
                    ui.colored_label(if *is_err { p.error } else { p.success }, msg);
                    ui.add_space(4.0);
                }

                if !state.show_confirm_modal {
                    if ui.button("Inspect & Delete Local Planora Data...").clicked() {
                        state.status_message = None;
                        if let Some(dir) = UserSettings::file_path().and_then(|p| p.parent().map(|p| p.to_path_buf())) {
                            if is_reparse_point_or_symlink(&dir) {
                                state.status_message = Some((
                                    true,
                                    "Security check failed: Planora data folder is a symlink or junction point.".into(),
                                ));
                            } else if dir.is_dir() {
                                let mut files = Vec::new();
                                if let Ok(entries) = std::fs::read_dir(&dir) {
                                    for entry in entries.flatten() {
                                        let name = entry.file_name().to_string_lossy().into_owned();
                                        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                                        files.push((name, size));
                                    }
                                }
                                state.preview_files = files;
                                state.show_confirm_modal = true;
                            } else {
                                state.status_message = Some((false, "No local Planora data directory exists on disk.".into()));
                            }
                        }
                    }
                } else {
                    egui::Frame::group(ui.style())
                        .fill(p.surface_raised)
                        .stroke(Stroke::new(1.0, p.error))
                        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.colored_label(p.error, "Confirm Permanent Data Deletion");
                            ui.label("The following files in your current-user Planora folder will be permanently deleted:");
                            ui.add_space(4.0);

                            let total_bytes: u64 = state.preview_files.iter().map(|(_, sz)| sz).sum();
                            for (fname, sz) in &state.preview_files {
                                ui.label(format!("• {fname} ({})", format_bytes(*sz)));
                            }
                            ui.add_space(4.0);
                            ui.strong(format!("Total Size: {}", format_bytes(total_bytes)));
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                if ui.button(egui::RichText::new("Yes, Delete My Planora Data").color(p.error)).clicked() {
                                    if let Some(dir) = UserSettings::file_path().and_then(|p| p.parent().map(|p| p.to_path_buf())) {
                                        if is_reparse_point_or_symlink(&dir) {
                                            state.status_message = Some((
                                                true,
                                                "Deletion rejected: data directory is a symlink or reparse point.".into(),
                                            ));
                                        } else {
                                            match std::fs::remove_dir_all(&dir) {
                                                Ok(()) => {
                                                    *settings = UserSettings::default();
                                                    state.status_message = Some((
                                                        false,
                                                        "Local Planora data deleted successfully. Settings reset to defaults.".into(),
                                                    ));
                                                }
                                                Err(e) => {
                                                    state.status_message = Some((
                                                        true,
                                                        format!("Failed to delete data folder: {e}"),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                    state.show_confirm_modal = false;
                                }

                                if ui.button("Cancel").clicked() {
                                    state.show_confirm_modal = false;
                                }
                            });
                        });
                }
            });

        ui.add_space(14.0);

        // Group 5: Build & System Information
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.strong("Build Identity & Platform Verification");
                ui.add_space(8.0);

                egui::Grid::new("build_info_grid")
                    .striped(true)
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Application Version:");
                        ui.label(egui::RichText::new(BUILD_INFO.version).monospace());
                        ui.end_row();

                        ui.label("Telemetry IPC Protocol:");
                        ui.label(egui::RichText::new(format!("Protocol v{}", BUILD_INFO.protocol_version)).monospace());
                        ui.end_row();

                        ui.label("Git Source Commit:");
                        ui.label(egui::RichText::new(BUILD_INFO.git_commit).monospace());
                        ui.end_row();

                        ui.label("Target Platform:");
                        ui.label(egui::RichText::new(BUILD_INFO.target).monospace());
                        ui.end_row();

                        ui.label("Compiler Profile:");
                        ui.label(egui::RichText::new(BUILD_INFO.profile).monospace());
                        ui.end_row();

                        ui.label("Rust Toolchain:");
                        ui.label(egui::RichText::new("Rust 1.95.0 (Edition 2024)").monospace());
                        ui.end_row();
                    });
            });

        ui.add_space(20.0);
    });
}

fn persist(settings: &UserSettings, error: &mut Option<String>) {
    *error = settings
        .save()
        .err()
        .map(|e| format!("Could not save preferences: {e}"));
}
