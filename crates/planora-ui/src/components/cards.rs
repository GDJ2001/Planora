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
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), egui::Sense::hover());
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
    let available_w = ui.available_width();
    let is_wide = available_w >= 900.0;
    let gap = 16.0;
    let card_w = if is_wide {
        (available_w - 3.0 * gap) / 4.0
    } else {
        (available_w - gap) / 2.0
    };

    let card_frame =
        |ui: &mut egui::Ui, width: f32, add_contents: Box<dyn FnOnce(&mut egui::Ui)>| {
            egui::Frame::group(ui.style())
                .fill(p.surface)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(CornerRadius::same(CORNER_RADIUS as u8))
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.set_min_height(96.0);
                    add_contents(ui);
                });
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
                ui.label(
                    egui::RichText::new(mem_str)
                        .size(19.0)
                        .strong()
                        .color(p.text),
                );
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
                ui.label(
                    egui::RichText::new(format_rate(r_rate.zip(w_rate).map(|(r, w)| r.saturating_add(w) as f32)))
                        .size(19.0)
                        .strong()
                        .color(p.text),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "R: {} · W: {}",
                            format_rate(r_rate.map(|r| r as f32)),
                            format_rate(w_rate.map(|w| w as f32))
                        ))
                        .size(10.5)
                        .color(p.secondary),
                    );
                });
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
                ui.label(
                    egui::RichText::new(format_rate(rx.zip(tx).map(|(rx, tx)| rx.saturating_add(tx) as f32)))
                        .size(19.0)
                        .strong()
                        .color(p.text),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "↓: {} · ↑: {}",
                            format_rate(rx.map(|rx| rx as f32)),
                            format_rate(tx.map(|tx| tx as f32))
                        ))
                        .size(10.5)
                        .color(p.secondary),
                    );
                });
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

    if is_wide {
        ui.horizontal(|ui| {
            draw_card1_cpu(ui, card_w);
            ui.add_space(gap);
            draw_card2_mem(ui, card_w);
            ui.add_space(gap);
            draw_card3_disk(ui, card_w);
            ui.add_space(gap);
            draw_card4_net(ui, card_w);
        });
    } else {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                draw_card1_cpu(ui, card_w);
                ui.add_space(gap);
                draw_card2_mem(ui, card_w);
            });
            ui.add_space(gap);
            ui.horizontal(|ui| {
                draw_card3_disk(ui, card_w);
                ui.add_space(gap);
                draw_card4_net(ui, card_w);
            });
        });
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
