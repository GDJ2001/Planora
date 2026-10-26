use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

pub fn draw_overview(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let pad = r.width() * 0.15;
    let mid_x = r.center().x;
    let mid_y = r.center().y;
    // 4 mini cards
    p.rect_stroke(
        Rect::from_min_max(
            r.min + Vec2::new(pad, pad),
            Pos2::new(mid_x - 1.5, mid_y - 1.5),
        ),
        1.5,
        s,
        eframe::egui::StrokeKind::Inside,
    );
    p.rect_stroke(
        Rect::from_min_max(
            Pos2::new(mid_x + 1.5, r.min.y + pad),
            Pos2::new(r.max.x - pad, mid_y - 1.5),
        ),
        1.5,
        s,
        eframe::egui::StrokeKind::Inside,
    );
    p.rect_stroke(
        Rect::from_min_max(
            Pos2::new(r.min.x + pad, mid_y + 1.5),
            Pos2::new(mid_x - 1.5, r.max.y - pad),
        ),
        1.5,
        s,
        eframe::egui::StrokeKind::Inside,
    );
    p.rect_stroke(
        Rect::from_min_max(
            Pos2::new(mid_x + 1.5, mid_y + 1.5),
            r.max - Vec2::new(pad, pad),
        ),
        1.5,
        s,
        eframe::egui::StrokeKind::Inside,
    );
}

pub fn draw_processes(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let pad = r.width() * 0.15;
    let y1 = r.min.y + r.height() * 0.28;
    let y2 = r.min.y + r.height() * 0.50;
    let y3 = r.min.y + r.height() * 0.72;

    for y in [y1, y2, y3] {
        p.circle_filled(Pos2::new(r.min.x + pad + 1.5, y), 1.5, c);
        p.line_segment(
            [
                Pos2::new(r.min.x + pad + 7.0, y),
                Pos2::new(r.max.x - pad, y),
            ],
            s,
        );
    }
}

pub fn draw_hardware(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let pad = r.width() * 0.22;
    let inner = Rect::from_min_max(r.min + Vec2::new(pad, pad), r.max - Vec2::new(pad, pad));
    p.rect_stroke(inner, 2.0, s, eframe::egui::StrokeKind::Inside);
    // Pins
    for f in [0.35, 0.65] {
        let x = inner.min.x + inner.width() * f;
        p.line_segment([Pos2::new(x, r.min.y + 1.0), Pos2::new(x, inner.min.y)], s);
        p.line_segment([Pos2::new(x, inner.max.y), Pos2::new(x, r.max.y - 1.0)], s);
        let y = inner.min.y + inner.height() * f;
        p.line_segment([Pos2::new(r.min.x + 1.0, y), Pos2::new(inner.min.x, y)], s);
        p.line_segment([Pos2::new(inner.max.x, y), Pos2::new(r.max.x - 1.0, y)], s);
    }
}

pub fn draw_benchmarks(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let center = r.center() + Vec2::new(0.0, r.height() * 0.15);
    let radius = r.width() * 0.35;
    // Speedometer arc approximation
    let steps = 8;
    let mut pts = Vec::new();
    for i in 0..=steps {
        let angle = std::f32::consts::PI * (0.85 + (i as f32 / steps as f32) * 1.3);
        pts.push(center + Vec2::new(angle.cos() * radius, angle.sin() * radius));
    }
    for i in 0..pts.len() - 1 {
        p.line_segment([pts[i], pts[i + 1]], s);
    }
    // Needle
    p.line_segment(
        [center, center + Vec2::new(radius * 0.5, -radius * 0.6)],
        Stroke::new(2.0, c),
    );
    p.circle_filled(center, 2.0, c);
}

pub fn draw_recordings(p: &Painter, r: Rect, c: Color32) {
    let center = r.center();
    let radius = r.width().min(r.height()) * 0.36;
    p.circle_stroke(center, radius, Stroke::new(1.5, c));
    p.circle_filled(center, radius * 0.35, c);
}

pub fn draw_capabilities(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    // Shield shape
    let top = r.min.y + r.height() * 0.15;
    let mid_y = r.min.y + r.height() * 0.55;
    let bot_y = r.max.y - r.height() * 0.15;
    let left = r.min.x + r.width() * 0.18;
    let right = r.max.x - r.width() * 0.18;
    let mid_x = r.center().x;

    let pts = [
        Pos2::new(left, top),
        Pos2::new(right, top),
        Pos2::new(right, mid_y),
        Pos2::new(mid_x, bot_y),
        Pos2::new(left, mid_y),
        Pos2::new(left, top),
    ];
    for i in 0..pts.len() - 1 {
        p.line_segment([pts[i], pts[i + 1]], s);
    }
    // Mini check
    p.line_segment(
        [
            Pos2::new(mid_x - 3.0, mid_y - 1.0),
            Pos2::new(mid_x - 0.5, mid_y + 2.0),
        ],
        s,
    );
    p.line_segment(
        [
            Pos2::new(mid_x - 0.5, mid_y + 2.0),
            Pos2::new(mid_x + 4.0, mid_y - 3.0),
        ],
        s,
    );
}

pub fn draw_settings(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let center = r.center();
    let r_inner = r.width() * 0.18;
    let r_outer = r.width() * 0.35;
    p.circle_stroke(center, r_inner, s);

    for i in 0..6 {
        let angle = (i as f32) * std::f32::consts::PI / 3.0;
        let p1 = center + Vec2::new(angle.cos() * (r_inner + 1.0), angle.sin() * (r_inner + 1.0));
        let p2 = center + Vec2::new(angle.cos() * r_outer, angle.sin() * r_outer);
        p.line_segment([p1, p2], Stroke::new(2.5, c));
    }
}

pub fn draw_cpu(p: &Painter, r: Rect, c: Color32) {
    draw_hardware(p, r, c);
}

pub fn draw_memory(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let pad_x = r.width() * 0.12;
    let pad_y = r.height() * 0.28;
    let rect = Rect::from_min_max(
        r.min + Vec2::new(pad_x, pad_y),
        r.max - Vec2::new(pad_x, pad_y),
    );
    p.rect_stroke(rect, 1.5, s, eframe::egui::StrokeKind::Inside);
    // 3 memory chips inside
    let w = rect.width() / 4.5;
    for i in 0..3 {
        let x = rect.min.x + (i as f32 + 0.6) * w * 1.3;
        let chip = Rect::from_min_size(
            Pos2::new(x, rect.min.y + 2.0),
            Vec2::new(w, rect.height() - 4.0),
        );
        p.rect_filled(chip, 1.0, c);
    }
}

pub fn draw_disk(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let pad_x = r.width() * 0.15;
    let w = r.width() - pad_x * 2.0;
    // Two stacked disc platters
    for y_off in [0.25, 0.55] {
        let rect = Rect::from_min_size(
            Pos2::new(r.min.x + pad_x, r.min.y + r.height() * y_off),
            Vec2::new(w, r.height() * 0.22),
        );
        p.rect_stroke(rect, 3.0, s, eframe::egui::StrokeKind::Inside);
        p.circle_filled(Pos2::new(rect.max.x - 5.0, rect.center().y), 1.5, c);
    }
}

pub fn draw_network(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let pad = r.width() * 0.2;
    let x_up = r.min.x + pad + 3.0;
    let x_down = r.max.x - pad - 3.0;

    // Up arrow
    p.line_segment(
        [
            Pos2::new(x_up, r.max.y - pad),
            Pos2::new(x_up, r.min.y + pad),
        ],
        s,
    );
    p.line_segment(
        [
            Pos2::new(x_up - 3.0, r.min.y + pad + 3.0),
            Pos2::new(x_up, r.min.y + pad),
        ],
        s,
    );
    p.line_segment(
        [
            Pos2::new(x_up + 3.0, r.min.y + pad + 3.0),
            Pos2::new(x_up, r.min.y + pad),
        ],
        s,
    );

    // Down arrow
    p.line_segment(
        [
            Pos2::new(x_down, r.min.y + pad),
            Pos2::new(x_down, r.max.y - pad),
        ],
        s,
    );
    p.line_segment(
        [
            Pos2::new(x_down - 3.0, r.max.y - pad - 3.0),
            Pos2::new(x_down, r.max.y - pad),
        ],
        s,
    );
    p.line_segment(
        [
            Pos2::new(x_down + 3.0, r.max.y - pad - 3.0),
            Pos2::new(x_down, r.max.y - pad),
        ],
        s,
    );
}

pub fn draw_gpu(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let pad = r.width() * 0.15;
    let body = Rect::from_min_max(
        r.min + Vec2::new(pad, pad + 2.0),
        r.max - Vec2::new(pad, pad + 2.0),
    );
    p.rect_stroke(body, 2.0, s, eframe::egui::StrokeKind::Inside);
    // Fan circle
    p.circle_stroke(body.center(), body.height() * 0.35, s);
}

pub fn draw_pause(p: &Painter, r: Rect, c: Color32) {
    let w = 2.5;
    let h = r.height() * 0.6;
    let y = r.center().y - h / 2.0;
    p.rect_filled(
        Rect::from_min_size(Pos2::new(r.center().x - 4.0, y), Vec2::new(w, h)),
        1.0,
        c,
    );
    p.rect_filled(
        Rect::from_min_size(Pos2::new(r.center().x + 1.5, y), Vec2::new(w, h)),
        1.0,
        c,
    );
}

pub fn draw_play(p: &Painter, r: Rect, c: Color32) {
    let h = r.height() * 0.6;
    let w = r.width() * 0.5;
    let x = r.center().x - w / 2.0;
    let y = r.center().y;
    let pts = [
        Pos2::new(x, y - h / 2.0),
        Pos2::new(x + w, y),
        Pos2::new(x, y + h / 2.0),
    ];
    p.add(eframe::egui::Shape::convex_polygon(
        pts.to_vec(),
        c,
        Stroke::NONE,
    ));
}

pub fn draw_search(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let center = r.center() - Vec2::new(2.0, 2.0);
    let radius = r.width() * 0.25;
    p.circle_stroke(center, radius, s);
    let p1 = center + Vec2::new(radius * 0.7, radius * 0.7);
    let p2 = r.max - Vec2::new(r.width() * 0.2, r.height() * 0.2);
    p.line_segment([p1, p2], Stroke::new(2.0, c));
}
