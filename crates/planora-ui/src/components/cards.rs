use crate::{
    components::icons,
    formatting::{format_memory_ratio, format_percent, format_rate},
    history::{HistoryRange, ResourceHistories, TimeSeries},
    theme::{CORNER_RADIUS, Palette},
};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};
use planora_core::TelemetryFrame;
use std::time::{Duration, Instant};

fn draw_mini_bar(ui: &mut egui::Ui, pct: f32, color: Color32, bg_track: Color32) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), bg_track);
    let fill_w = (rect.width() * (pct.clamp(0.0, 100.0) / 100.0)).max(0.0);
    if fill_w > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        painter.rect_filled(fill_rect, CornerRadius::same(3), color);
    }
}

fn draw_sparklines_two_series(
    ui: &mut egui::Ui,
    s1: &TimeSeries,
    c1: Color32,
    s2: &TimeSeries,
    c2: Color32,
    now: Instant,
    range: HistoryRange,
    bg: Color32,
) {
    let w = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 24.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), bg);

    let segs1 = s1.downsampled_segments(now, range, Duration::from_secs(3), rect.width() as usize);
    let segs2 = s2.downsampled_segments(now, range, Duration::from_secs(3), rect.width() as usize);

    // Calculate max across both
    let mut max_val = 1024.0f32; // minimum 1 KB/s baseline
    for seg in &segs1 {
        for (_, val) in seg {
            if *val > max_val {
                max_val = *val;
            }
        }
    }
    for seg in &segs2 {
        for (_, val) in seg {
            if *val > max_val {
                max_val = *val;
            }
        }
    }

    let plot_segs = |segs: &[Vec<(f32, f32)>], stroke: Stroke| {
        for seg in segs {
            if seg.len() < 2 {
                continue;
            }
            let pts: Vec<Pos2> = seg
                .iter()
                .map(|(norm_x, val)| {
                    let x = rect.min.x + norm_x * rect.width();
                    let norm_y = (val / max_val).clamp(0.0, 1.0);
                    let y = rect.max.y - norm_y * (rect.height() - 2.0) - 1.0;
                    Pos2::new(x, y)
                })
                .collect();
            painter.add(egui::Shape::line(pts, stroke));
        }
    };

    plot_segs(&segs1, Stroke::new(1.5, c1));
    plot_segs(&segs2, Stroke::new(1.2, c2));
}

pub fn render_summary_cards(
    ui: &mut egui::Ui,
    frame: &TelemetryFrame,
    histories: &ResourceHistories,
    now: Instant,
    p: Palette,
) {
    let available_w = (ui.available_width() - 4.0).max(100.0);
    let gap = 16.0;

    let columns = calculate_columns(available_w);
    let card_w = calculate_card_width(available_w, gap, columns);

    let card_frame =
        |ui: &mut egui::Ui, width: f32, add_contents: Box<dyn FnOnce(&mut egui::Ui)>| {
            ui.allocate_ui_with_layout(
                Vec2::new(width, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::Frame::group(ui.style())
                        .fill(p.surface)
                        .stroke(Stroke::new(1.0, p.border))
                        .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            let inner_w = (width - 26.0).max(60.0);
                            ui.set_max_width(inner_w);
                            ui.set_width(inner_w);
                            ui.set_min_height(96.0);
                            add_contents(ui);
                        });
                },
            );
        };

    let draw_card1_cpu = |ui: &mut egui::Ui, w: f32| {
        card_frame(
            ui,
            w,
            Box::new(|ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
                    icons::draw_cpu(ui.painter(), rect, p.cpu);
                    ui.strong("CPU");
                });
                ui.add_space(4.0);
                let cpu_val = frame.cpu_percent.value;
                ui.label(
                    egui::RichText::new(format_percent(cpu_val))
                        .size(22.0)
                        .strong()
                        .color(p.text),
                );
                ui.add_space(6.0);
                draw_mini_bar(ui, cpu_val.unwrap_or(0.0), p.cpu, p.surface_raised);
            }),
        );
    };

    let draw_card2_mem = |ui: &mut egui::Ui, w: f32| {
        card_frame(
            ui,
            w,
            Box::new(|ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
                    icons::draw_memory(ui.painter(), rect, p.memory);
                    ui.strong("Memory");
                });
                ui.add_space(4.0);
                let (mem_str, mem_pct) = format_memory_ratio(
                    frame.memory_used_bytes.value,
                    frame.memory_total_bytes.value,
                );
                let mem_full = format!("{mem_str} ({:.1}%)", mem_pct.unwrap_or(0.0));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&mem_str)
                            .size(18.0)
                            .strong()
                            .color(p.text),
                    )
                    .truncate(),
                )
                .on_hover_text(mem_full);
                ui.add_space(6.0);
                draw_mini_bar(ui, mem_pct.unwrap_or(0.0), p.memory, p.surface_raised);
            }),
        );
    };

    let draw_card3_disk = |ui: &mut egui::Ui, w: f32| {
        card_frame(
            ui,
            w,
            Box::new(|ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
                    icons::draw_disk(ui.painter(), rect, p.disk);
                    ui.strong("Disk");
                });
                ui.add_space(2.0);
                let r_rate = frame.disk_read_bytes_per_sec.value;
                let w_rate = frame.disk_write_bytes_per_sec.value;
                let total_rate_str =
                    format_rate(r_rate.zip(w_rate).map(|(r, w)| r.saturating_add(w) as f32));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&total_rate_str)
                            .size(19.0)
                            .strong()
                            .color(p.text),
                    )
                    .truncate(),
                )
                .on_hover_text(format!("Combined Disk Rate: {total_rate_str}"));

                let sub_text = format!(
                    "R: {} · W: {}",
                    format_rate(r_rate.map(|r| r as f32)),
                    format_rate(w_rate.map(|w| w as f32))
                );
                ui.add(
                    egui::Label::new(egui::RichText::new(&sub_text).size(11.5).color(p.secondary))
                        .truncate(),
                )
                .on_hover_text(&sub_text);

                ui.add_space(4.0);
                draw_sparklines_two_series(
                    ui,
                    &histories.disk_read,
                    p.disk,
                    &histories.disk_write,
                    p.accent,
                    now,
                    HistoryRange::Sec60,
                    p.surface_raised,
                );
            }),
        );
    };

    let draw_card4_net = |ui: &mut egui::Ui, w: f32| {
        card_frame(
            ui,
            w,
            Box::new(|ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(18.0, 18.0), egui::Sense::hover());
                    icons::draw_network(ui.painter(), rect, p.network);
                    ui.strong("Network");
                });
                ui.add_space(2.0);
                let rx = frame.network_rx_bytes_per_sec.value;
                let tx = frame.network_tx_bytes_per_sec.value;
                let total_net_str =
                    format_rate(rx.zip(tx).map(|(rx, tx)| rx.saturating_add(tx) as f32));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&total_net_str)
                            .size(19.0)
                            .strong()
                            .color(p.text),
                    )
                    .truncate(),
                )
                .on_hover_text(format!("Combined Network Rate: {total_net_str}"));

                let sub_text = format!(
                    "↓: {} · ↑: {}",
                    format_rate(rx.map(|rx| rx as f32)),
                    format_rate(tx.map(|tx| tx as f32))
                );
                ui.add(
                    egui::Label::new(egui::RichText::new(&sub_text).size(11.5).color(p.secondary))
                        .truncate(),
                )
                .on_hover_text(&sub_text);

                ui.add_space(4.0);
                draw_sparklines_two_series(
                    ui,
                    &histories.net_rx,
                    p.network,
                    &histories.net_tx,
                    p.focus,
                    now,
                    HistoryRange::Sec60,
                    p.surface_raised,
                );
            }),
        );
    };

    match columns {
        4 => {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                draw_card1_cpu(ui, card_w);
                draw_card2_mem(ui, card_w);
                draw_card3_disk(ui, card_w);
                draw_card4_net(ui, card_w);
            });
        }
        2 => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = gap;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    draw_card1_cpu(ui, card_w);
                    draw_card2_mem(ui, card_w);
                });
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    draw_card3_disk(ui, card_w);
                    draw_card4_net(ui, card_w);
                });
            });
        }
        _ => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = gap;
                draw_card1_cpu(ui, card_w);
                draw_card2_mem(ui, card_w);
                draw_card3_disk(ui, card_w);
                draw_card4_net(ui, card_w);
            });
        }
    }
}

pub fn render_secondary_resource_panel(ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
    ui.horizontal(|ui| {
        // GPU Panel
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(16.0, 16.0), egui::Sense::hover());
                    icons::draw_gpu(ui.painter(), rect, p.gpu);
                    ui.strong("GPU:");
                    match &frame.gpu_percent.value {
                        Some(val) => {
                            ui.label(
                                egui::RichText::new(format!("{val:.1}%"))
                                    .strong()
                                    .color(p.gpu),
                            );
                        }
                        None => {
                            let resp = ui.label(
                                egui::RichText::new("Unavailable")
                                    .color(p.secondary)
                                    .size(12.0),
                            );
                            if let Some(r) = &frame.gpu_percent.unavailable_reason {
                                resp.on_hover_text(format!("GPU metrics unavailable: {r:?}"));
                            }
                        }
                    }
                });
            });

        ui.add_space(12.0);

        // Package Power Panel
        egui::Frame::group(ui.style())
            .fill(p.surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Package Power:");
                    match &frame.package_power_watts.value {
                        Some(val) => {
                            ui.label(
                                egui::RichText::new(format!("{val:.1} W"))
                                    .strong()
                                    .color(p.warning),
                            );
                        }
                        None => {
                            let resp = ui.label(
                                egui::RichText::new("Unavailable")
                                    .color(p.secondary)
                                    .size(12.0),
                            );
                            if let Some(r) = &frame.package_power_watts.unavailable_reason {
                                resp.on_hover_text(format!("Power metrics unavailable: {r:?}"));
                            }
                        }
                    }
                });
            });
    });
}

pub fn calculate_columns(available_w: f32) -> usize {
    if available_w >= 1060.0 {
        4
    } else if available_w >= 520.0 {
        2
    } else {
        1
    }
}

pub fn calculate_card_width(available_w: f32, gap: f32, columns: usize) -> f32 {
    let stroke_safety = 2.0;
    match columns {
        4 => (((available_w - 3.0 * gap) / 4.0) - stroke_safety)
            .floor()
            .max(100.0),
        2 => (((available_w - gap) / 2.0) - stroke_safety)
            .floor()
            .max(100.0),
        _ => (available_w - stroke_safety).floor().max(100.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_column_breakpoints() {
        assert_eq!(calculate_columns(1280.0), 4);
        assert_eq!(calculate_columns(1060.0), 4);
        assert_eq!(calculate_columns(1059.0), 2);
        assert_eq!(calculate_columns(800.0), 2);
        assert_eq!(calculate_columns(520.0), 2);
        assert_eq!(calculate_columns(519.0), 1);
        assert_eq!(calculate_columns(320.0), 1);
    }

    #[test]
    fn test_card_width_containment_at_target_resolutions() {
        // Target window widths: 1024, 1180, 1366, 1536, 1920
        // Sidebar is 208px, root central panel padding is 2 * 24px = 48px
        // Usable content width = window_width - 208.0 - 48.0
        let gap = 16.0;
        let window_widths = [1024.0, 1180.0, 1366.0, 1536.0, 1920.0];

        for &win_w in &window_widths {
            let usable_w = win_w - 208.0 - 48.0;
            let cols = calculate_columns(usable_w);
            let card_w = calculate_card_width(usable_w, gap, cols);

            // Total width of rendered cards and gaps
            let total_row_w = (cols as f32 * card_w) + ((cols - 1) as f32 * gap);

            // Must fit within available content width without horizontal overflow
            assert!(
                total_row_w <= usable_w + 0.1,
                "Window {win_w}px (usable {usable_w}px): cards row width {total_row_w}px exceeds usable width"
            );

            // Inner width is width - 30.0 (14px frame margin on each side + 2px stroke)
            let inner_w = card_w - 30.0;
            assert!(
                inner_w >= 60.0,
                "Inner card width must remain at least 60px"
            );
        }
    }
}
