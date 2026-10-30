//! Native charts use the same observed samples as the table alternatives.
use crate::formatting::{format_bytes, format_rate};
use crate::history::{ChartPoint, HistoryRange, TimeSeries};
use crate::theme::Palette;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use std::{
    hash::{Hash, Hasher},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueScale {
    Auto,
    Percent { full: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AxisScale {
    pub max: f32,
    pub step: f32,
    pub ticks: Vec<f32>,
    pub divisor: f32,
    pub unit_label: String,
    pub decimals: usize,
}

/// Calculate round ticks, human unit labels, and adaptive bounds.
pub fn axis_scale(observed_max: f32, unit: &str, scale: ValueScale) -> AxisScale {
    match scale {
        ValueScale::Percent { full } => {
            if full {
                AxisScale {
                    max: 100.0,
                    step: 25.0,
                    ticks: vec![0.0, 25.0, 50.0, 75.0, 100.0],
                    divisor: 1.0,
                    unit_label: "%".to_string(),
                    decimals: 0,
                }
            } else {
                let req = (observed_max * 1.08).max(1.0);
                if req <= 10.0 {
                    AxisScale {
                        max: 10.0,
                        step: 2.5,
                        ticks: vec![0.0, 2.5, 5.0, 7.5, 10.0],
                        divisor: 1.0,
                        unit_label: "%".to_string(),
                        decimals: 1,
                    }
                } else if req <= 25.0 {
                    AxisScale {
                        max: 25.0,
                        step: 5.0,
                        ticks: vec![0.0, 5.0, 10.0, 15.0, 20.0, 25.0],
                        divisor: 1.0,
                        unit_label: "%".to_string(),
                        decimals: 0,
                    }
                } else if req <= 50.0 {
                    AxisScale {
                        max: 50.0,
                        step: 10.0,
                        ticks: vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0],
                        divisor: 1.0,
                        unit_label: "%".to_string(),
                        decimals: 0,
                    }
                } else {
                    AxisScale {
                        max: 100.0,
                        step: 25.0,
                        ticks: vec![0.0, 25.0, 50.0, 75.0, 100.0],
                        divisor: 1.0,
                        unit_label: "%".to_string(),
                        decimals: 0,
                    }
                }
            }
        }
        ValueScale::Auto => {
            let is_rate = unit == "B/s";
            let is_bytes = unit == "bytes";
            if is_rate || is_bytes {
                let req = (observed_max * 1.08).max(1.0);
                let (divisor, prefix) = if req >= 1_073_741_824.0 {
                    (1_073_741_824.0, "GiB")
                } else if req >= 1_048_576.0 {
                    (1_048_576.0, "MiB")
                } else if req >= 1024.0 {
                    (1024.0, "KiB")
                } else {
                    (1.0, if is_rate { "B" } else { "bytes" })
                };
                let unit_label = if is_rate {
                    format!("{prefix}/s")
                } else {
                    prefix.to_string()
                };
                let scaled_val = req / divisor;
                let (step, num_ticks, decimals) = calculate_nice_step(scaled_val);
                let ticks: Vec<f32> = (0..=num_ticks).map(|i| i as f32 * step).collect();
                let max = step * num_ticks as f32 * divisor;
                AxisScale {
                    max,
                    step,
                    ticks,
                    divisor,
                    unit_label,
                    decimals,
                }
            } else {
                let req = (observed_max * 1.08).max(1.0);
                let (step, num_ticks, decimals) = calculate_nice_step(req);
                let ticks: Vec<f32> = (0..=num_ticks).map(|i| i as f32 * step).collect();
                let max = step * num_ticks as f32;
                AxisScale {
                    max,
                    step,
                    ticks,
                    divisor: 1.0,
                    unit_label: unit.to_string(),
                    decimals,
                }
            }
        }
    }
}

fn calculate_nice_step(target: f32) -> (f32, usize, usize) {
    let rough_step = (target / 4.0).max(0.001);
    let magnitude = 10f32.powf(rough_step.log10().floor());
    let normalized = rough_step / magnitude;
    let (step_mult, decimals) = if normalized <= 1.0 {
        (1.0, if magnitude < 1.0 { 1 } else { 0 })
    } else if normalized <= 2.0 {
        (2.0, if magnitude < 1.0 { 1 } else { 0 })
    } else if normalized <= 2.5 {
        (2.5, if magnitude <= 1.0 { 1 } else { 0 })
    } else if normalized <= 5.0 {
        (5.0, if magnitude < 1.0 { 1 } else { 0 })
    } else {
        (10.0, if magnitude < 0.1 { 1 } else { 0 })
    };
    let step = step_mult * magnitude;
    let num_ticks = ((target / step).ceil() as usize).clamp(4, 5);
    (step, num_ticks, decimals)
}

/// Fritsch–Carlson monotone cubic interpolation.
/// On each interval the curve stays strictly within its endpoints' bounds (no overshoot).
pub fn monotone_path(points: &[Pos2], step_px: f32) -> Vec<Pos2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let n = points.len();
    let mut dx = Vec::with_capacity(n - 1);
    let mut dy = Vec::with_capacity(n - 1);
    let mut slope = Vec::with_capacity(n - 1);

    for i in 0..n - 1 {
        let delta_x = points[i + 1].x - points[i].x;
        let delta_y = points[i + 1].y - points[i].y;
        dx.push(delta_x);
        dy.push(delta_y);
        slope.push(if delta_x.abs() > 1e-6 {
            delta_y / delta_x
        } else {
            0.0
        });
    }

    // Initial tangents via averaging
    let mut m = Vec::with_capacity(n);
    m.push(slope[0]);
    for i in 1..n - 1 {
        if slope[i - 1] * slope[i] <= 0.0 {
            m.push(0.0);
        } else {
            m.push((slope[i - 1] + slope[i]) / 2.0);
        }
    }
    m.push(slope[n - 2]);

    // Fritsch–Carlson constraint adjustments
    for i in 0..n - 1 {
        if slope[i].abs() < 1e-6 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let alpha = m[i] / slope[i];
            let beta = m[i + 1] / slope[i];
            if alpha < 0.0 {
                m[i] = 0.0;
            }
            if beta < 0.0 {
                m[i + 1] = 0.0;
            }
            let hyp = alpha * alpha + beta * beta;
            if hyp > 9.0 {
                let tau = 3.0 / hyp.sqrt();
                m[i] = tau * alpha * slope[i];
                m[i + 1] = tau * beta * slope[i];
            }
        }
    }

    let mut result = Vec::new();
    let step = step_px.max(2.0);
    for i in 0..n - 1 {
        let p0 = points[i];
        let p1 = points[i + 1];
        let h = dx[i];
        let m0 = m[i];
        let m1 = m[i + 1];
        let num_steps = ((p1.x - p0.x).abs() / step).ceil().max(1.0) as usize;

        for step_idx in 0..num_steps {
            let t = step_idx as f32 / num_steps as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            let x = p0.x + t * h;
            let y = h00 * p0.y + h10 * h * m0 + h01 * p1.y + h11 * h * m1;
            result.push(Pos2::new(x, y));
        }
    }
    if let Some(last) = points.last() {
        result.push(*last);
    }
    result
}

/// One triangle-strip mesh from the curve to the baseline: eliminates per-quad anti-aliasing seams.
pub fn area_mesh(path: &[Pos2], baseline: f32, top: Color32, bottom: Color32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    if path.len() < 2 {
        return mesh;
    }
    mesh.vertices.reserve(path.len() * 2);
    mesh.indices.reserve((path.len() - 1) * 6);

    for &p in path {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: p,
            uv: egui::epaint::WHITE_UV,
            color: top,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(p.x, baseline),
            uv: egui::epaint::WHITE_UV,
            color: bottom,
        });
    }

    for i in 0..(path.len() - 1) as u32 {
        let tl = i * 2;
        let bl = i * 2 + 1;
        let tr = (i + 1) * 2;
        let br = (i + 1) * 2 + 1;

        mesh.indices.push(tl);
        mesh.indices.push(bl);
        mesh.indices.push(tr);

        mesh.indices.push(bl);
        mesh.indices.push(br);
        mesh.indices.push(tr);
    }

    mesh
}

/// Faint min..max range band mesh drawn behind the line.
pub fn band_mesh(upper: &[Pos2], lower: &[Pos2], color: Color32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let count = upper.len().min(lower.len());
    if count < 2 {
        return mesh;
    }
    mesh.vertices.reserve(count * 2);
    mesh.indices.reserve((count - 1) * 6);

    for i in 0..count {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: upper[i],
            uv: egui::epaint::WHITE_UV,
            color,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: lower[i],
            uv: egui::epaint::WHITE_UV,
            color,
        });
    }

    for i in 0..(count - 1) as u32 {
        let tl = i * 2;
        let bl = i * 2 + 1;
        let tr = (i + 1) * 2;
        let br = (i + 1) * 2 + 1;

        mesh.indices.push(tl);
        mesh.indices.push(bl);
        mesh.indices.push(tr);

        mesh.indices.push(bl);
        mesh.indices.push(br);
        mesh.indices.push(tr);
    }

    mesh
}

/// Arc-length based dashing: guarantees uniform dash and gap lengths along the entire path.
pub fn dashed_path_shapes(
    path: &[Pos2],
    stroke: Stroke,
    dash_len: f32,
    gap_len: f32,
) -> Vec<Shape> {
    let mut shapes = Vec::new();
    if path.len() < 2 {
        return shapes;
    }
    let mut distance_in_phase = 0.0f32;
    let mut in_dash = true;

    for window in path.windows(2) {
        let mut p_start = window[0];
        let p_end = window[1];
        let delta = p_end - p_start;
        let mut seg_len = delta.length();
        if seg_len < 1e-4 {
            continue;
        }
        let dir = delta / seg_len;

        while seg_len > 0.0 {
            let phase_limit = if in_dash { dash_len } else { gap_len };
            let remaining_phase = phase_limit - distance_in_phase;

            if seg_len <= remaining_phase {
                if in_dash {
                    shapes.push(Shape::line_segment([p_start, p_end], stroke));
                }
                distance_in_phase += seg_len;
                seg_len = 0.0;
            } else {
                let step = remaining_phase;
                let p_next = p_start + dir * step;
                if in_dash {
                    shapes.push(Shape::line_segment([p_start, p_next], stroke));
                }
                p_start = p_next;
                seg_len -= step;
                in_dash = !in_dash;
                distance_in_phase = 0.0;
            }
        }
    }
    shapes
}

pub struct PlotSeries<'a> {
    pub name: &'a str,
    pub samples: &'a TimeSeries,
    pub color: Color32,
    pub dashed: bool,
}

#[derive(Default)]
pub struct PlotState {
    pub cursor: Option<f32>,
    pub pinned: bool,
    axis_max: f32,
    below_half_since: Option<Instant>,
    cache_key: Option<PlotCacheKey>,
    cache_now: Option<Instant>,
    cached_segments: Vec<Vec<Vec<ChartPoint>>>,
    pending_key: Option<PlotCacheKey>,
    pending_result: Option<Receiver<PreparedPlot>>,
}

#[derive(Clone, PartialEq)]
struct PlotCacheKey {
    range: HistoryRange,
    width: u32,
    scale_bits: u32,
    gap_ms: u64,
    series: Vec<(u64, u64, u64)>,
}

impl PlotCacheKey {
    fn same_layout(&self, other: &Self) -> bool {
        self.range == other.range
            && self.width == other.width
            && self.scale_bits == other.scale_bits
            && self.gap_ms == other.gap_ms
            && self.series.len() == other.series.len()
            && self
                .series
                .iter()
                .zip(&other.series)
                .all(|(a, b)| a.0 == b.0)
    }
}

struct PlotJob {
    series: Vec<TimeSeries>,
    now: Instant,
    range: HistoryRange,
    width: usize,
    gap: Duration,
    result: SyncSender<PreparedPlot>,
}

struct PreparedPlot {
    now: Instant,
    segments: Vec<Vec<Vec<ChartPoint>>>,
    duration_ms: f32,
}

struct PlotPreparationWorker {
    input: Option<SyncSender<PlotJob>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PlotPreparationWorker {
    fn start() -> Self {
        let (input, jobs) = mpsc::sync_channel::<PlotJob>(8);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("planora-chart-prep".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    let job = match jobs.recv_timeout(Duration::from_millis(100)) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let started = Instant::now();
                    let segments = job
                        .series
                        .iter()
                        .map(|series| series.chart_segments(job.now, job.range, job.gap, job.width))
                        .collect();
                    let _ = job.result.try_send(PreparedPlot {
                        now: job.now,
                        segments,
                        duration_ms: started.elapsed().as_secs_f32() * 1000.0,
                    });
                }
            })
            .expect("chart preparation worker");
        Self {
            input: Some(input),
            stop,
            thread: Some(thread),
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.input.take();
    }

    fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn join(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

static PLOT_WORKER: OnceLock<Mutex<Option<PlotPreparationWorker>>> = OnceLock::new();

fn prepare_plot_async(
    series: Vec<TimeSeries>,
    now: Instant,
    range: HistoryRange,
    width: usize,
    gap: Duration,
) -> Option<Receiver<PreparedPlot>> {
    let worker = PLOT_WORKER.get_or_init(|| Mutex::new(None));
    let mut worker = worker.lock().unwrap();
    let worker = worker.get_or_insert_with(PlotPreparationWorker::start);
    let (result, rx) = mpsc::sync_channel(1);
    let job = PlotJob {
        series,
        now,
        range,
        width,
        gap,
        result,
    };
    if worker.input.as_ref()?.try_send(job).is_ok() {
        Some(rx)
    } else {
        None
    }
}

pub fn stop_preparation_worker() {
    if let Some(worker) = PLOT_WORKER.get()
        && let Some(worker) = worker.lock().unwrap().as_mut()
    {
        worker.stop();
    }
}

pub fn preparation_worker_finished() -> bool {
    PLOT_WORKER.get().is_none_or(|worker| {
        worker
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(PlotPreparationWorker::is_finished)
    })
}

pub fn join_preparation_worker() {
    if let Some(worker) = PLOT_WORKER.get()
        && let Some(mut worker) = worker.lock().unwrap().take()
    {
        worker.join();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GapPolicy {
    Standard,
    CpuUtilization,
}

impl GapPolicy {
    fn gap_threshold(self) -> Duration {
        match self {
            Self::Standard => Duration::from_secs(3),
            // CPU history is aggregated into one-second observations. A
            // missing second must remain visible instead of bridging it.
            Self::CpuUtilization => Duration::from_millis(2200),
        }
    }
}

/// Paint a bounded, gap-aware trend using observed interval averages.
pub fn time_plot(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    series: &[PlotSeries<'_>],
    range: HistoryRange,
    now: Instant,
    unit: &str,
    scale: ValueScale,
    height: f32,
    state: &mut PlotState,
    p: Palette,
) {
    draw_time_plot(
        ui,
        id,
        series,
        range,
        now,
        unit,
        scale,
        height,
        state,
        p,
        GapPolicy::Standard,
    );
}

/// Show observed CPU utilization as a conventional time-series line. Missing
/// intervals remain blank, and the line is not smoothed into extra extrema.
pub fn cpu_utilization_plot(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    series: &[PlotSeries<'_>],
    range: HistoryRange,
    now: Instant,
    scale: ValueScale,
    height: f32,
    state: &mut PlotState,
    p: Palette,
) {
    draw_time_plot(
        ui,
        id,
        series,
        range,
        now,
        "%",
        scale,
        height,
        state,
        p,
        GapPolicy::CpuUtilization,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_time_plot(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    series: &[PlotSeries<'_>],
    range: HistoryRange,
    now: Instant,
    unit: &str,
    scale: ValueScale,
    height: f32,
    state: &mut PlotState,
    p: Palette,
    gap_policy: GapPolicy,
) {
    let chart_id = ui.make_persistent_id(id);
    let width = ui.available_width().max(140.0);
    let height = height.max(180.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let response = ui.interact(rect, chart_id, Sense::click_and_drag());
    let text_scale = crate::appearance::text_scale(ui.ctx());

    // Check cached samples
    let est_canvas_width = (rect.width() - 60.0).max(40.0);
    let key = PlotCacheKey {
        range,
        width: est_canvas_width.round() as u32,
        scale_bits: ui.ctx().pixels_per_point().to_bits(),
        gap_ms: gap_policy.gap_threshold().as_millis() as u64,
        series: series
            .iter()
            .map(|item| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                item.name.hash(&mut hasher);
                (
                    hasher.finish(),
                    item.samples.generation,
                    item.samples
                        .samples
                        .back()
                        .map_or(0, |sample| sample.timestamp_ms),
                )
            })
            .collect(),
    };
    let layout_changed = state
        .cache_key
        .as_ref()
        .is_none_or(|old| !old.same_layout(&key));
    if layout_changed {
        state.cached_segments.clear();
        state.cache_key = None;
        if state
            .pending_key
            .as_ref()
            .is_some_and(|old| !old.same_layout(&key))
        {
            state.pending_key = None;
            state.pending_result = None;
        }
    }
    if let Some(receiver) = &state.pending_result {
        match receiver.try_recv() {
            Ok(prepared) => {
                if let Some(pending) = state.pending_key.take()
                    && pending.same_layout(&key)
                {
                    state.cached_segments = prepared.segments;
                    state.cache_key = Some(pending);
                    state.cache_now = Some(prepared.now);
                    crate::CHART_PREP_RING.with(|r| r.borrow_mut().record(prepared.duration_ms));
                }
                state.pending_result = None;
            }
            Err(TryRecvError::Disconnected) => {
                state.pending_result = None;
                state.pending_key = None;
            }
            Err(TryRecvError::Empty) => ui.ctx().request_repaint_after(Duration::from_millis(50)),
        }
    }
    let refresh_due = state
        .cache_key
        .as_ref()
        .is_none_or(|old| !old.same_layout(&key))
        || state.cache_now.is_none_or(|cached_at| {
            now < cached_at || now.saturating_duration_since(cached_at) >= Duration::from_secs(1)
        });
    if refresh_due && state.pending_result.is_none() {
        let total_samples: usize = series.iter().map(|item| item.samples.samples.len()).sum();
        if total_samples <= 128 {
            let started = Instant::now();
            let gap = gap_policy.gap_threshold();
            state.cached_segments = series
                .iter()
                .map(|item| {
                    item.samples
                        .chart_segments(now, range, gap, est_canvas_width as usize)
                })
                .collect();
            state.cache_key = Some(key);
            state.cache_now = Some(now);
            crate::CHART_PREP_RING.with(|r| {
                r.borrow_mut()
                    .record(started.elapsed().as_secs_f32() * 1000.0)
            });
        } else if let Some(receiver) = prepare_plot_async(
            series.iter().map(|item| item.samples.clone()).collect(),
            now,
            range,
            est_canvas_width as usize,
            gap_policy.gap_threshold(),
        ) {
            state.pending_key = Some(key);
            state.pending_result = Some(receiver);
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        } else {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }
    let segments = &state.cached_segments;

    let observed_max = segments
        .iter()
        .flatten()
        .flatten()
        .fold(0.0f32, |max, p| max.max(p.max));

    // Dynamic axis hysteresis
    let target = observed_max;
    if target > state.axis_max {
        state.axis_max = target;
        state.below_half_since = None;
    } else if target < state.axis_max * 0.5 {
        let since = state.below_half_since.get_or_insert(now);
        if now.saturating_duration_since(*since) >= Duration::from_secs(10) {
            state.axis_max = target;
            state.below_half_since = None;
        }
    } else {
        state.below_half_since = None;
    }

    let axis = axis_scale(state.axis_max, unit, scale);
    let ymax = axis.max.max(1.0);

    // Dynamic gutter calculation
    let font_id = egui::FontId::proportional(11.0 * text_scale);
    let max_tick_label_width = axis
        .ticks
        .iter()
        .map(|&tick| {
            let s = format!("{:.prec$}", tick, prec = axis.decimals);
            ui.painter()
                .layout_no_wrap(s, font_id.clone(), p.secondary)
                .size()
                .x
        })
        .fold(24.0f32, f32::max);
    let left_gutter = (max_tick_label_width + 12.0).max(36.0 * text_scale);
    let header_height = 18.0 * text_scale;
    let bottom_gutter = 26.0 * text_scale;

    let canvas = Rect::from_min_max(
        Pos2::new(rect.left() + left_gutter, rect.top() + header_height + 4.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - bottom_gutter),
    );

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.surface);
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );

    // Horizontal grid lines and tick labels
    if unit == "%" && axis.max >= 99.0 {
        let minor_stroke = Stroke::new(1.0, p.border.gamma_multiply(0.45));
        for tens in 1..10 {
            // Major ticks 25, 50, 75, 100 are drawn below
            if tens == 2 || tens == 5 || tens == 7 || tens == 10 {
                continue;
            }
            let fraction = (tens as f32 * 10.0) / 100.0;
            let y = canvas.bottom() - fraction * canvas.height();
            painter.line_segment(
                [Pos2::new(canvas.left(), y), Pos2::new(canvas.right(), y)],
                minor_stroke,
            );
        }
    }

    for &tick in &axis.ticks {
        let fraction = (tick / (axis.max / axis.divisor).max(0.001)).clamp(0.0, 1.0);
        let y = canvas.bottom() - fraction * canvas.height();
        painter.line_segment(
            [Pos2::new(canvas.left(), y), Pos2::new(canvas.right(), y)],
            Stroke::new(1.0, p.border),
        );
        let tick_str = format!("{:.prec$}", tick, prec = axis.decimals);
        painter.text(
            Pos2::new(canvas.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            tick_str,
            font_id.clone(),
            p.secondary,
        );
    }

    // Vertical grid lines and time labels
    let range_dur = range.duration().as_secs();
    for col in 0..=4 {
        let x = canvas.left() + canvas.width() * col as f32 / 4.0;
        painter.line_segment(
            [Pos2::new(x, canvas.top()), Pos2::new(x, canvas.bottom())],
            Stroke::new(1.0, p.border),
        );
        let (label_text, align) = if col == 4 {
            ("Now".to_string(), egui::Align2::RIGHT_CENTER)
        } else if col == 0 {
            (format!("{} ago", range.label()), egui::Align2::LEFT_CENTER)
        } else {
            let secs_ago = (range_dur as f32 * (4 - col) as f32 / 4.0).round() as u64;
            let text = if secs_ago < 60 {
                format!("{secs_ago}s")
            } else if secs_ago.is_multiple_of(60) {
                format!("{}m", secs_ago / 60)
            } else {
                format!("{}m {}s", secs_ago / 60, secs_ago % 60)
            };
            (text, egui::Align2::CENTER_CENTER)
        };
        painter.text(
            Pos2::new(x, rect.bottom() - 12.0),
            align,
            label_text,
            font_id.clone(),
            p.secondary,
        );
    }

    // Dedicated header row: unit caption on the left
    painter.text(
        Pos2::new(canvas.left(), rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        &axis.unit_label,
        font_id.clone(),
        p.secondary,
    );

    let to_point = |x: f32, y: f32| {
        Pos2::new(
            canvas.left() + x * canvas.width(),
            canvas.bottom() - (y / ymax).clamp(0.0, 1.0) * canvas.height(),
        )
    };

    let canvas_painter = ui.painter_at(canvas.expand(1.0));

    // Show one direct trend per measurement. A single series gets a restrained
    // area fill; paired and comparison charts remain readable as separate lines.
    for (item, item_segments) in series.iter().zip(segments) {
        for segment in item_segments {
            if segment.is_empty() {
                continue;
            }
            if segment.len() == 1 {
                canvas_painter.circle_filled(
                    to_point(segment[0].x, segment[0].mean),
                    3.5,
                    item.color,
                );
                continue;
            }

            let path: Vec<Pos2> = segment.iter().map(|p| to_point(p.x, p.mean)).collect();
            if series.len() == 1 {
                let fill_top = if p.is_dark { 0.16 } else { 0.12 };
                let fill = area_mesh(
                    &path,
                    canvas.bottom(),
                    item.color.gamma_multiply(fill_top),
                    item.color.gamma_multiply(0.02),
                );
                canvas_painter.add(fill);
            }
            if !item.dashed {
                canvas_painter.add(Shape::line(path, Stroke::new(1.75, item.color)));
            } else {
                let dashed_shapes =
                    dashed_path_shapes(&path, Stroke::new(1.75, item.color), 6.0, 4.0);
                for shape in dashed_shapes {
                    canvas_painter.add(shape);
                }
            }
        }
    }

    // Interaction & Cursor
    if response.clicked() {
        response.request_focus();
        state.pinned = !state.pinned;
        if !state.pinned {
            state.cursor = None;
        } else if state.cursor.is_none() {
            state.cursor = response
                .interact_pointer_pos()
                .map(|pos| ((pos.x - canvas.left()) / canvas.width()).clamp(0.0, 1.0));
        }
    }
    if response.hovered() && !state.pinned {
        state.cursor = response
            .hover_pos()
            .map(|pos| ((pos.x - canvas.left()) / canvas.width()).clamp(0.0, 1.0));
    }

    if response.has_focus() {
        let mut observed_x: Vec<f32> = segments.iter().flatten().flatten().map(|p| p.x).collect();
        observed_x.sort_by(f32::total_cmp);
        observed_x.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        ui.input(|input| {
            if input.key_pressed(egui::Key::Escape) {
                state.pinned = false;
                state.cursor = None;
            } else if input.key_pressed(egui::Key::Home) {
                state.cursor = observed_x.first().copied();
                state.pinned = true;
            } else if input.key_pressed(egui::Key::End) {
                state.cursor = observed_x.last().copied();
                state.pinned = true;
            } else if input.key_pressed(egui::Key::ArrowLeft) {
                if let Some(target) = observed_x
                    .iter()
                    .rev()
                    .find(|&&x| x < state.cursor.unwrap_or(1.01) - 1e-4)
                    .copied()
                {
                    state.cursor = Some(target);
                } else if state.cursor.is_none() {
                    state.cursor = observed_x.last().copied();
                }
                state.pinned = true;
            } else if input.key_pressed(egui::Key::ArrowRight) {
                if let Some(target) = observed_x
                    .iter()
                    .find(|&&x| x > state.cursor.unwrap_or(-0.01) + 1e-4)
                    .copied()
                {
                    state.cursor = Some(target);
                } else if state.cursor.is_none() {
                    state.cursor = observed_x.first().copied();
                }
                state.pinned = true;
            }
        });
    }

    if let Some(cursor_norm_x) = state.cursor {
        // Find nearest plotted ChartPoint to snap cursor
        let tolerance_x = (24.0 / canvas.width()).clamp(0.02, 0.08);
        let mut nearest_point: Option<ChartPoint> = None;
        let mut min_dist = f32::MAX;
        for p in segments.iter().flatten().flatten() {
            let dist = (p.x - cursor_norm_x).abs();
            if dist < min_dist && dist <= tolerance_x {
                min_dist = dist;
                nearest_point = Some(*p);
            }
        }

        let snapped_x = nearest_point.map_or(cursor_norm_x, |p| p.x);
        let screen_x = canvas.left() + snapped_x * canvas.width();

        painter.line_segment(
            [
                Pos2::new(screen_x, canvas.top()),
                Pos2::new(screen_x, canvas.bottom()),
            ],
            Stroke::new(1.0, p.focus),
        );

        let mut details = Vec::new();
        let mut concise = Vec::new();
        let mut missing = Vec::new();
        let tolerance = Duration::from_secs_f32(
            (range.duration().as_secs_f32() / canvas.width() * 1.5).clamp(1.5, 2.5),
        );

        for (idx, item) in series.iter().enumerate() {
            if let Some((sample, age)) = item
                .samples
                .query_sample_near(now, range, snapped_x, tolerance)
            {
                let reading = sample.value.map_or_else(
                    || {
                        sample
                            .unavailable_reason
                            .as_ref()
                            .map_or_else(|| "Unavailable".to_owned(), ToString::to_string)
                    },
                    |v| format_plot_value(v, unit),
                );
                details.push((item.name, reading.clone(), sample, age));
                concise.push(format!("{}: {}", item.name, reading));

                // Dot marker on the snapped point
                if let Some(pt) = segments.get(idx).and_then(|segs| {
                    segs.iter()
                        .flatten()
                        .find(|p| (p.x - snapped_x).abs() < 1e-4)
                }) {
                    painter.circle_filled(to_point(pt.x, pt.mean), 3.0, item.color);
                    painter.circle_stroke(to_point(pt.x, pt.mean), 4.5, Stroke::new(1.0, p.focus));
                }
            } else {
                missing.push(item.name);
                concise.push(format!("{}: Unavailable", item.name));
            }
        }

        if !details.is_empty() || !missing.is_empty() {
            response.on_hover_ui_at_pointer(|ui| {
                ui.set_max_width(280.0);
                for (name, reading, sample, age) in &details {
                    ui.horizontal(|ui| {
                        ui.strong(*name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.strong(reading);
                        });
                    });
                    ui.label(
                        egui::RichText::new(format!(
                            "Sample {} ms Unix · {} ago · {}",
                            sample.timestamp_ms,
                            format_age(*age),
                            sample.source
                        ))
                        .small(),
                    );
                    if let (Some(min), Some(max)) = (sample.minimum, sample.maximum) {
                        ui.label(
                            egui::RichText::new(format!(
                                "Interval average: {} · Minimum: {} · Maximum: {}",
                                sample.value.map_or_else(
                                    || "Unavailable".into(),
                                    |value| format_plot_value(value, unit)
                                ),
                                format_plot_value(min, unit),
                                format_plot_value(max, unit)
                            ))
                            .small(),
                        );
                    }
                    if let Some(reason) = &sample.unavailable_reason {
                        ui.label(egui::RichText::new(reason.to_string()).small());
                    }
                }
                for name in &missing {
                    ui.label(format!("{name}: Unavailable at this time"));
                }
            });

            let readout_summary = if state.pinned {
                format!("Pinned · {}", concise.join(" · "))
            } else {
                concise.join(" · ")
            };
            // Paint readout in the dedicated header area above the canvas (never over data)
            painter.text(
                Pos2::new(canvas.right(), rect.top() + 6.0),
                egui::Align2::RIGHT_TOP,
                readout_summary,
                font_id,
                p.focus,
            );
        }
    }
}

fn format_age(age: Duration) -> String {
    if age.as_secs() < 60 {
        format!("{}s", age.as_secs())
    } else {
        format!("{}m {}s", age.as_secs() / 60, age.as_secs() % 60)
    }
}

fn format_plot_value(value: f32, unit: &str) -> String {
    match unit {
        "%" => format!("{value:.1}%"),
        "B/s" => format_rate(Some(value)),
        "bytes" => format_bytes(value.max(0.0) as u64),
        _ => format!("{value:.1} {unit}"),
    }
}

pub fn sparkline(ui: &mut egui::Ui, series: &TimeSeries, now: Instant, color: Color32, p: Palette) {
    sparkline_sized(ui, series, now, color, p, Vec2::new(68.0, 28.0));
}

pub fn sparkline_sized(
    ui: &mut egui::Ui,
    series: &TimeSeries,
    now: Instant,
    color: Color32,
    p: Palette,
    size: Vec2,
) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, p.surface_raised);
    let plot_rect = rect.shrink2(Vec2::new(1.0, 1.5));
    let segments = series.chart_segments(
        now,
        HistoryRange::Sec60,
        Duration::from_secs(3),
        size.x.max(1.0) as usize,
    );
    let observed_max = segments
        .iter()
        .flatten()
        .fold(0.0f32, |acc, point| acc.max(point.max));
    // Sparklines have no shared axis. Scale each metric from zero using its
    // actual observed peak, including byte rates far above 100 B/s.
    let ymax = sparkline_ceiling(observed_max);

    for segment in segments {
        let points: Vec<Pos2> = segment
            .iter()
            .map(|p| {
                Pos2::new(
                    plot_rect.left() + p.x * plot_rect.width(),
                    plot_rect.bottom() - (p.mean / ymax).clamp(0.0, 1.0) * plot_rect.height(),
                )
            })
            .collect();
        if points.len() >= 2 {
            let fill_mesh = area_mesh(
                &points,
                plot_rect.bottom(),
                color.gamma_multiply(0.12),
                color.gamma_multiply(0.02),
            );
            painter.add(fill_mesh);
            painter.add(Shape::line(points, Stroke::new(1.4, color)));
        } else if let Some(point) = points.first() {
            painter.circle_filled(*point, 2.0, color);
        }
    }
}

fn sparkline_ceiling(observed_max: f32) -> f32 {
    (observed_max * 1.08).max(1.0)
}

pub fn capacity_bar(
    ui: &mut egui::Ui,
    used: Option<u64>,
    total: Option<u64>,
    color: Color32,
    p: Palette,
) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(1.0), 14.0),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, p.surface_raised);
    if let (Some(used), Some(total)) = (used, total)
        && total > 0
    {
        let fraction = (used as f64 / total as f64).clamp(0.0, 1.0) as f32;
        let fill = Rect::from_min_size(rect.min, Vec2::new(rect.width() * fraction, rect.height()));
        painter.rect_filled(fill.intersect(rect), 4.0, color);
    } else if total == Some(0) {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Shared memory (UMA)",
            egui::FontId::proportional(10.0 * crate::appearance::text_scale(ui.ctx())),
            p.secondary,
        );
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Unavailable",
            egui::FontId::proportional(10.0 * crate::appearance::text_scale(ui.ctx())),
            p.secondary,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn background_preparation_preserves_dense_history_gaps_and_spikes() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        for second in 0..900 {
            let value = if second == 300 {
                None
            } else if second == 600 {
                Some(99.0)
            } else {
                Some(10.0)
            };
            series.push(
                now - Duration::from_secs(899 - second),
                (second + 1) * 1000,
                value,
            );
        }
        let expected = series.chart_segments(now, HistoryRange::Min15, Duration::from_secs(3), 600);
        let mut worker = PlotPreparationWorker::start();
        let (sender, receiver) = mpsc::sync_channel(1);
        worker
            .input
            .as_ref()
            .unwrap()
            .send(PlotJob {
                series: vec![series],
                now,
                range: HistoryRange::Min15,
                width: 600,
                gap: Duration::from_secs(3),
                result: sender,
            })
            .unwrap();
        let result = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(result.segments, vec![expected]);
        assert!(result.segments[0].len() >= 2);
        assert!(
            result.segments[0]
                .iter()
                .flatten()
                .any(|point| point.max >= 99.0)
        );
        worker.join();
    }

    #[test]
    fn cpu_plot_breaks_a_missing_one_second_observation() {
        let now = Instant::now();
        let mut series = TimeSeries::new();
        series.push(now - Duration::from_millis(4000), 1_000, Some(21.0));
        series.push(now - Duration::from_millis(3000), 2_000, Some(33.0));
        series.push(now - Duration::from_millis(500), 4_000, Some(44.0));
        let segments = series.chart_segments(
            now,
            HistoryRange::Sec60,
            GapPolicy::CpuUtilization.gap_threshold(),
            0,
        );
        assert_eq!(
            segments.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(segments[0][0].mean, 21.0);
        assert_eq!(segments[1][0].mean, 44.0);
    }

    #[test]
    fn single_trend_has_one_fill_and_paired_trends_have_none() {
        let now = Instant::now();
        let mut first = TimeSeries::new();
        let mut second = TimeSeries::new();
        for offset in 0..3 {
            let time = now - Duration::from_secs(3 - offset);
            first.push(
                time,
                1_000 + offset * 1_000,
                Some((offset * 10 + 15) as f32),
            );
            second.push(time, 1_000 + offset * 1_000, Some((offset * 5 + 8) as f32));
        }
        let p = crate::theme::get_palette(true);
        let mesh_count = |paired: bool| {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(640.0, 360.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let mut plot = PlotState::default();
                    let mut series = vec![PlotSeries {
                        name: "Read",
                        samples: &first,
                        color: p.cpu,
                        dashed: false,
                    }];
                    if paired {
                        series.push(PlotSeries {
                            name: "Write",
                            samples: &second,
                            color: p.disk,
                            dashed: true,
                        });
                    }
                    time_plot(
                        ui,
                        "chart-style-test",
                        &series,
                        HistoryRange::Sec30,
                        now,
                        "%",
                        ValueScale::Percent { full: true },
                        240.0,
                        &mut plot,
                        p,
                    );
                },
            );
            let count = output
                .shapes
                .iter()
                .filter(|shape| matches!(shape.shape, egui::Shape::Mesh(_)))
                .count();
            output.textures_delta.clear();
            count
        };
        assert_eq!(mesh_count(false), 1);
        assert_eq!(mesh_count(true), 0);
    }

    #[test]
    fn sparkline_rate_scale_does_not_cap_at_percent_range() {
        assert!(sparkline_ceiling(1_000_000.0) > 1_000_000.0);
        assert_eq!(sparkline_ceiling(0.0), 1.0);
    }

    #[test]
    fn monotone_path_never_overshoots_neighbouring_readings() {
        let raw = vec![
            Pos2::new(0.0, 10.0),
            Pos2::new(50.0, 10.0),  // flat run
            Pos2::new(100.0, 80.0), // rising
            Pos2::new(150.0, 20.0), // falling
            Pos2::new(200.0, 20.0), // flat run
            Pos2::new(250.0, 95.0), // spike
            Pos2::new(300.0, 5.0),  // sharp drop
        ];
        let smoothed = monotone_path(&raw, 2.0);
        assert!(smoothed.len() > raw.len());
        for w in smoothed.windows(2) {
            assert!(w[1].x >= w[0].x, "x must never go backwards");
        }
        for i in 0..raw.len() - 1 {
            let (x0, x1) = (raw[i].x, raw[i + 1].x);
            let y_min = raw[i].y.min(raw[i + 1].y) - 1e-3;
            let y_max = raw[i].y.max(raw[i + 1].y) + 1e-3;
            for p in &smoothed {
                if p.x >= x0 && p.x <= x1 {
                    assert!(
                        p.y >= y_min && p.y <= y_max,
                        "point at x={} y={} overshoots interval [{}, {}]",
                        p.x,
                        p.y,
                        y_min,
                        y_max
                    );
                }
            }
        }
    }

    #[test]
    fn area_fill_is_a_single_mesh_without_seams() {
        let path = vec![
            Pos2::new(0.0, 50.0),
            Pos2::new(20.0, 30.0),
            Pos2::new(40.0, 70.0),
            Pos2::new(60.0, 20.0),
        ];
        let baseline = 100.0;
        let mesh = area_mesh(&path, baseline, Color32::RED, Color32::TRANSPARENT);
        assert_eq!(mesh.vertices.len(), path.len() * 2);
        assert_eq!(mesh.indices.len(), (path.len() - 1) * 6);
    }

    #[test]
    fn axis_ticks_are_round_and_consistent() {
        // Percent Fit mode for 9.7% observed max -> 25% ceiling with 5% steps
        let p_scale = axis_scale(9.7, "%", ValueScale::Percent { full: false });
        assert_eq!(p_scale.max, 25.0);
        assert_eq!(p_scale.ticks, vec![0.0, 5.0, 10.0, 15.0, 20.0, 25.0]);

        // Percent Full mode -> 100% ceiling with 25% steps
        let full_scale = axis_scale(9.7, "%", ValueScale::Percent { full: true });
        assert_eq!(full_scale.max, 100.0);
        assert_eq!(full_scale.ticks, vec![0.0, 25.0, 50.0, 75.0, 100.0]);

        // Network/Storage rate in KiB/s
        let net_scale = axis_scale(168_000.0, "B/s", ValueScale::Auto);
        assert_eq!(net_scale.unit_label, "KiB/s");
        assert_eq!(net_scale.ticks, vec![0.0, 50.0, 100.0, 150.0, 200.0]);
        assert_eq!(net_scale.decimals, 0);

        // Small rate in B/s
        let b_scale = axis_scale(500.0, "B/s", ValueScale::Auto);
        assert_eq!(b_scale.unit_label, "B/s");
        assert!(b_scale.ticks.last().unwrap() >= &500.0);
    }

    #[test]
    fn chart_readouts_use_human_units() {
        assert_eq!(format_plot_value(9.13, "%"), "9.1%");
        assert_eq!(format_plot_value(2048.0, "B/s"), "2.0 KiB/s");
    }
}
