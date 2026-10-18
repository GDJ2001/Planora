use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use planora_core::{
    BenchmarkResult, Capability, Command, ProcessIdentity, Response, TelemetryFrame, Topology,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Sender},
    },
    thread,
    time::{Duration, Instant},
};

#[allow(dead_code)]
mod tokens {
    use eframe::egui;
    include!(concat!(env!("OUT_DIR"), "/theme.rs"));
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn planora_register_daemon() -> i32;
    fn planora_unregister_daemon() -> i32;
}

#[derive(Default)]
struct Shared {
    connected: bool,
    error: Option<String>,
    frame: Option<Arc<TelemetryFrame>>,
    topology: Option<Arc<Topology>>,
    capabilities: Vec<Capability>,
    benchmark_running: bool,
    benchmark_result: Option<BenchmarkResult>,
    benchmark_error: Option<String>,
    affinity: Option<(ProcessIdentity, Vec<u16>)>,
}

fn worker(
    state: Arc<Mutex<Shared>>,
    rx: mpsc::Receiver<Command>,
    context: egui::Context,
    poll_ms: Arc<AtomicU64>,
) {
    let mut next = Instant::now();
    let mut next_topology = Instant::now();
    loop {
        while let Ok(command) = rx.try_recv() {
            let response = planora_helper::transact(&command);
            let mut shared = state.lock().unwrap();
            match response {
                Ok(Response::BenchmarkStatus {
                    running,
                    result,
                    error,
                }) => {
                    shared.benchmark_running = running;
                    shared.benchmark_result = result;
                    shared.benchmark_error = error;
                }
                Ok(Response::Affinity {
                    process,
                    logical_cpus,
                })
                | Ok(Response::AffinityChanged {
                    process,
                    logical_cpus,
                    ..
                }) => {
                    shared.affinity = Some((process, logical_cpus));
                    shared.error = None;
                }
                Ok(Response::Error { message, .. }) => shared.error = Some(message),
                Err(error) => {
                    shared.connected = false;
                    shared.error = Some(error.to_string());
                }
                _ => {}
            }
            context.request_repaint();
        }
        if Instant::now() >= next {
            let interval = poll_ms.load(Ordering::Relaxed).max(50);
            next = Instant::now() + Duration::from_millis(interval);
            match planora_helper::transact(&Command::Hello {
                version: planora_core::PROTOCOL_VERSION,
            }) {
                Ok(Response::Hello {
                    helper_authorized: true,
                    ..
                }) => {
                    if Instant::now() >= next_topology {
                        next_topology = Instant::now() + Duration::from_secs(5);
                        if let Ok(Response::Topology(value)) =
                            planora_helper::transact(&Command::Topology)
                        {
                            state.lock().unwrap().topology = Some(Arc::new(value));
                        }
                        if let Ok(Response::Capabilities(value)) =
                            planora_helper::transact(&Command::Capabilities)
                        {
                            state.lock().unwrap().capabilities = value;
                        }
                    }
                    if let Ok(Response::Snapshot(frame)) =
                        planora_helper::transact(&Command::Snapshot)
                    {
                        let mut shared = state.lock().unwrap();
                        shared.connected = true;
                        shared.error = None;
                        shared.frame = Some(Arc::from(frame));
                    }
                    if let Ok(Response::BenchmarkStatus {
                        running,
                        result,
                        error,
                    }) = planora_helper::transact(&Command::BenchmarkStatus)
                    {
                        let mut shared = state.lock().unwrap();
                        shared.benchmark_running = running;
                        shared.benchmark_result = result;
                        shared.benchmark_error = error;
                    }
                }
                Ok(_) => {
                    let mut shared = state.lock().unwrap();
                    shared.connected = false;
                    shared.frame = None;
                    shared.topology = None;
                    shared.capabilities.clear();
                    shared.error = Some("helper authorization failed".into());
                    next_topology = Instant::now();
                }
                Err(error) => {
                    let mut shared = state.lock().unwrap();
                    shared.connected = false;
                    shared.frame = None;
                    shared.topology = None;
                    shared.capabilities.clear();
                    shared.error = Some(error.to_string());
                    next_topology = Instant::now();
                }
            }
            context.request_repaint();
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Overview,
    Processes,
    Hardware,
    Benchmarks,
    Capabilities,
}

#[derive(PartialEq, Clone, Copy)]
enum Theme {
    System,
    Dark,
    Light,
}

#[derive(PartialEq, Clone, Copy)]
enum SortColumn {
    Cpu,
    Memory,
    Pid,
    Name,
}

#[derive(Clone, Debug, PartialEq)]
enum SelectedHardware {
    Core(u16),
    Cache(u8, u64),
    Gpu(u16),
}

fn theme_is_dark(theme: Theme, system: Option<egui::Theme>) -> bool {
    match theme {
        Theme::System => system != Some(egui::Theme::Light),
        Theme::Dark => true,
        Theme::Light => false,
    }
}

fn visible_processes<'a>(
    frame: &'a TelemetryFrame,
    query: &str,
    sort_column: SortColumn,
    descending: bool,
) -> Vec<&'a planora_core::ProcessRecord> {
    let query = query.to_lowercase();
    let mut rows: Vec<_> = frame
        .processes
        .iter()
        .filter(|p| {
            if query.is_empty() {
                true
            } else {
                p.name.to_lowercase().contains(&query)
                    || p.identity.pid.to_string().contains(&query)
            }
        })
        .collect();

    rows.sort_by(|a, b| {
        let ord = match sort_column {
            SortColumn::Cpu => match (a.cpu_percent.value, b.cpu_percent.value) {
                (Some(va), Some(vb)) => va.total_cmp(&vb),
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => a.identity.pid.cmp(&b.identity.pid),
            },
            SortColumn::Memory => match (a.memory_bytes.value, b.memory_bytes.value) {
                (Some(va), Some(vb)) => va.cmp(&vb),
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => a.identity.pid.cmp(&b.identity.pid),
            },
            SortColumn::Pid => a.identity.pid.cmp(&b.identity.pid),
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        };
        if descending { ord.reverse() } else { ord }
    });
    rows
}

fn selected_record<'a>(
    frame: &'a TelemetryFrame,
    selected: &ProcessIdentity,
) -> Option<&'a planora_core::ProcessRecord> {
    frame.processes.iter().find(|p| &p.identity == selected)
}

struct App {
    shared: Arc<Mutex<Shared>>,
    tx: Sender<Command>,
    page: Page,
    theme: Theme,
    search: String,
    selected: Option<ProcessIdentity>,
    sort_column: SortColumn,
    sort_descending: bool,
    cpu_history: VecDeque<f32>,
    mem_history: VecDeque<f32>,
    disk_history: VecDeque<f32>,
    net_history: VecDeque<f32>,
    last_sequence: u64,
    selected_hardware: Option<SelectedHardware>,
    affinity_input: String,
    matrix_input: String,
    poll_ms: Arc<AtomicU64>,
    canvas_pan: Vec2,
    canvas_zoom: f32,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = mpsc::channel();
        let shared2 = shared.clone();
        let context = cc.egui_ctx.clone();
        let poll_ms = Arc::new(AtomicU64::new(100));
        let poll_copy = poll_ms.clone();
        thread::spawn(move || worker(shared2, rx, context, poll_copy));
        Self {
            shared,
            tx,
            page: Page::Overview,
            theme: Theme::System,
            search: String::new(),
            selected: None,
            sort_column: SortColumn::Cpu,
            sort_descending: true,
            cpu_history: VecDeque::with_capacity(120),
            mem_history: VecDeque::with_capacity(120),
            disk_history: VecDeque::with_capacity(120),
            net_history: VecDeque::with_capacity(120),
            last_sequence: 0,
            selected_hardware: None,
            affinity_input: String::new(),
            matrix_input: "0,1".into(),
            poll_ms,
            canvas_pan: Vec2::ZERO,
            canvas_zoom: 1.0,
        }
    }

    fn palette(&self, ctx: &egui::Context) -> Palette {
        let dark = theme_is_dark(self.theme, ctx.system_theme());
        if dark {
            Palette {
                background: tokens::DARK_BACKGROUND,
                surface: tokens::DARK_SURFACE,
                surface_raised: tokens::DARK_SURFACE_RAISED,
                text: tokens::DARK_TEXT,
                secondary: tokens::DARK_TEXT_SECONDARY,
                border: tokens::DARK_BORDER_SUBTLE,
                primary: tokens::DARK_PRIMARY,
                accent: tokens::DARK_ACCENT,
                focus: tokens::DARK_FOCUS,
                cpu: tokens::DARK_CPU,
                memory: tokens::DARK_MEMORY,
                gpu: tokens::DARK_GPU,
                disk: tokens::DARK_DISK,
                network: tokens::DARK_NETWORK,
                success: tokens::DARK_SUCCESS,
                warning: tokens::DARK_WARNING,
                error: tokens::DARK_ERROR,
            }
        } else {
            Palette {
                background: tokens::LIGHT_BACKGROUND,
                surface: tokens::LIGHT_SURFACE,
                surface_raised: tokens::LIGHT_SURFACE_RAISED,
                text: tokens::LIGHT_TEXT,
                secondary: tokens::LIGHT_TEXT_SECONDARY,
                border: tokens::LIGHT_BORDER_SUBTLE,
                primary: tokens::LIGHT_PRIMARY,
                accent: tokens::LIGHT_ACCENT,
                focus: tokens::LIGHT_FOCUS,
                cpu: tokens::LIGHT_CPU,
                memory: tokens::LIGHT_MEMORY,
                gpu: tokens::LIGHT_GPU,
                disk: tokens::LIGHT_DISK,
                network: tokens::LIGHT_NETWORK,
                success: tokens::LIGHT_SUCCESS,
                warning: tokens::LIGHT_WARNING,
                error: tokens::LIGHT_ERROR,
            }
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let p = self.palette(ctx);
        let mut visuals = match self.theme {
            Theme::System => {
                if ctx.system_theme() == Some(egui::Theme::Light) {
                    egui::Visuals::light()
                } else {
                    egui::Visuals::dark()
                }
            }
            Theme::Dark => egui::Visuals::dark(),
            Theme::Light => egui::Visuals::light(),
        };
        visuals.panel_fill = p.background;
        visuals.window_fill = p.surface;
        visuals.override_text_color = Some(p.text);
        visuals.selection.bg_fill = p.accent;
        visuals.selection.stroke = Stroke::new(1.0, p.focus);
        ctx.set_visuals(visuals);
    }

    fn metric<T: std::fmt::Display>(m: &planora_core::Metric<T>) -> String {
        m.value
            .as_ref()
            .map(|v| {
                if m.unit.is_empty() {
                    v.to_string()
                } else {
                    format!("{v} {}", m.unit)
                }
            })
            .unwrap_or_else(|| {
                format!(
                    "Unavailable ({})",
                    m.unavailable_reason.as_deref().unwrap_or("unknown")
                )
            })
    }

    fn card(ui: &mut egui::Ui, label: &str, value: String, color: Color32) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(label);
            ui.colored_label(color, egui::RichText::new(value).size(20.0).strong());
        });
    }

    fn render_resource_chart(
        ui: &mut egui::Ui,
        title: &str,
        data: &VecDeque<f32>,
        max_val: f32,
        color: Color32,
        p: Palette,
        unit: &str,
    ) {
        ui.label(title);
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width().max(180.0), 90.0),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        painter.rect_filled(rect, 6.0, p.surface);
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );

        for frac in [0.25, 0.5, 0.75] {
            let y = egui::lerp(rect.bottom()..=rect.top(), frac);
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(1.0, p.border),
            );
        }

        if data.len() > 1 {
            let max_bound = max_val.max(0.001);
            let points: Vec<Pos2> = data
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let x =
                        rect.left() + rect.width() * (i as f32 / (data.len() - 1).max(1) as f32);
                    let norm = (v / max_bound).clamp(0.0, 1.0);
                    let y = rect.bottom() - rect.height() * norm;
                    Pos2::new(x, y)
                })
                .collect();

            let mut area = points.clone();
            area.push(Pos2::new(points.last().unwrap().x, rect.bottom()));
            area.push(Pos2::new(points[0].x, rect.bottom()));
            painter.add(egui::Shape::convex_polygon(
                area,
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28),
                Stroke::NONE,
            ));
            painter.add(egui::Shape::line(
                points,
                Stroke::new(tokens::CHART_LINE_WIDTH, color),
            ));
        }

        if let Some(&latest) = data.back() {
            painter.text(
                Pos2::new(rect.right() - 8.0, rect.top() + 6.0),
                egui::Align2::RIGHT_TOP,
                format!("{latest:.1} {unit}"),
                egui::FontId::proportional(11.0),
                p.secondary,
            );
        }
    }

    fn overview(&mut self, ui: &mut egui::Ui, frame: &TelemetryFrame, p: Palette) {
        ui.heading("Overview");
        ui.label("Live micro-architectural and system telemetry");
        ui.add_space(8.0);

        ui.horizontal_wrapped(|ui| {
            Self::card(ui, "CPU Total", Self::metric(&frame.cpu_percent), p.cpu);
            let mem = frame
                .memory_used_bytes
                .value
                .zip(frame.memory_total_bytes.value)
                .map(|(used, total)| {
                    format!(
                        "{:.1} / {:.1} GiB",
                        used as f64 / 1_073_741_824.0,
                        total as f64 / 1_073_741_824.0
                    )
                })
                .unwrap_or_else(|| "Unavailable".into());
            Self::card(ui, "Memory", mem, p.memory);
            Self::card(
                ui,
                "Disk Read",
                Self::metric(&frame.disk_read_bytes_per_sec),
                p.disk,
            );
            Self::card(
                ui,
                "Disk Write",
                Self::metric(&frame.disk_write_bytes_per_sec),
                p.disk,
            );
            Self::card(
                ui,
                "Network RX",
                Self::metric(&frame.network_rx_bytes_per_sec),
                p.network,
            );
            Self::card(
                ui,
                "Network TX",
                Self::metric(&frame.network_tx_bytes_per_sec),
                p.network,
            );
            Self::card(ui, "GPU Usage", Self::metric(&frame.gpu_percent), p.gpu);
            Self::card(
                ui,
                "Package Power",
                Self::metric(&frame.package_power_watts),
                p.accent,
            );
        });

        ui.add_space(16.0);
        ui.heading("Resource Trends");

        egui::Grid::new("overview_charts")
            .num_columns(2)
            .spacing([16.0, 12.0])
            .show(ui, |ui| {
                Self::render_resource_chart(
                    ui,
                    "CPU Utilization (%)",
                    &self.cpu_history,
                    100.0,
                    p.cpu,
                    p,
                    "%",
                );
                Self::render_resource_chart(
                    ui,
                    "Memory Used (%)",
                    &self.mem_history,
                    100.0,
                    p.memory,
                    p,
                    "%",
                );
                ui.end_row();

                let max_disk = self.disk_history.iter().copied().fold(1024.0f32, f32::max);
                Self::render_resource_chart(
                    ui,
                    "Disk Activity (KB/s)",
                    &self.disk_history,
                    max_disk,
                    p.disk,
                    p,
                    "KB/s",
                );

                let max_net = self.net_history.iter().copied().fold(1024.0f32, f32::max);
                Self::render_resource_chart(
                    ui,
                    "Network Traffic (KB/s)",
                    &self.net_history,
                    max_net,
                    p.network,
                    p,
                    "KB/s",
                );
                ui.end_row();
            });

        ui.add_space(12.0);
        ui.label(format!(
            "Collector: {} processes · {:.2} ms per sample · sequence #{} · timestamp {} ms",
            frame.processes.len(),
            frame.collection_duration_ms,
            frame.sequence,
            frame.sampled_at_ms
        ));
    }

    fn processes(
        &mut self,
        ui: &mut egui::Ui,
        frame: &TelemetryFrame,
        affinity: Option<&(ProcessIdentity, Vec<u16>)>,
        p: Palette,
    ) {
        ui.heading("Processes");
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Filter by name or PID...")
                    .desired_width(220.0),
            );
            if !self.search.is_empty() && ui.button("Clear").clicked() {
                self.search.clear();
            }
            ui.separator();
            ui.label("Sort by:");
            let sort_btn = |ui: &mut egui::Ui,
                            col: SortColumn,
                            label: &str,
                            current: SortColumn,
                            desc: bool| {
                let is_current = current == col;
                let text = if is_current {
                    format!("{label} {}", if desc { "▼" } else { "▲" })
                } else {
                    label.to_string()
                };
                if ui.button(text).clicked() {
                    Some(if is_current { !desc } else { true })
                } else {
                    None
                }
            };

            if let Some(desc) = sort_btn(
                ui,
                SortColumn::Cpu,
                "CPU",
                self.sort_column,
                self.sort_descending,
            ) {
                self.sort_column = SortColumn::Cpu;
                self.sort_descending = desc;
            }
            if let Some(desc) = sort_btn(
                ui,
                SortColumn::Memory,
                "Memory",
                self.sort_column,
                self.sort_descending,
            ) {
                self.sort_column = SortColumn::Memory;
                self.sort_descending = desc;
            }
            if let Some(desc) = sort_btn(
                ui,
                SortColumn::Pid,
                "PID",
                self.sort_column,
                self.sort_descending,
            ) {
                self.sort_column = SortColumn::Pid;
                self.sort_descending = desc;
            }
            if let Some(desc) = sort_btn(
                ui,
                SortColumn::Name,
                "Name",
                self.sort_column,
                self.sort_descending,
            ) {
                self.sort_column = SortColumn::Name;
                self.sort_descending = desc;
            }
        });

        let rows = visible_processes(frame, &self.search, self.sort_column, self.sort_descending);
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() * 0.55)
            .show(ui, |ui| {
                egui::Grid::new("process_table")
                    .striped(true)
                    .num_columns(5)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("Process Name");
                        ui.strong("PID");
                        ui.strong("CPU %");
                        ui.strong("Resident Memory");
                        ui.strong("GPU %");
                        ui.end_row();

                        for process in rows {
                            let is_selected = self.selected.as_ref() == Some(&process.identity);
                            let label = if process.name.is_empty() {
                                "<unnamed>"
                            } else {
                                &process.name
                            };

                            let resp = ui.selectable_label(is_selected, label);
                            if resp.clicked() {
                                self.selected = Some(process.identity.clone());
                                let _ = self.tx.send(Command::GetAffinity {
                                    process: process.identity.clone(),
                                });
                            }
                            ui.label(process.identity.pid.to_string());
                            ui.label(Self::metric(&process.cpu_percent));
                            ui.label(
                                process
                                    .memory_bytes
                                    .value
                                    .map(|v| format!("{:.1} MiB", v as f64 / 1_048_576.0))
                                    .unwrap_or_else(|| "Unavailable".into()),
                            );
                            ui.label(Self::metric(&process.gpu_percent));
                            ui.end_row();
                        }
                    });
            });

        if let Some(selected) = self.selected.as_ref() {
            ui.separator();
            ui.heading("Process Inspector");
            if let Some(process) = selected_record(frame, selected) {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&process.name).strong().size(16.0));
                    ui.label(format!("· PID {}", selected.pid));
                    ui.colored_label(
                        p.secondary,
                        format!("· {} #{}", selected.source, selected.started),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(format!("CPU: {}", Self::metric(&process.cpu_percent)));
                    ui.label(format!("Memory: {}", Self::metric(&process.memory_bytes)));
                    ui.label(format!("GPU: {}", Self::metric(&process.gpu_percent)));
                });

                let current_affinity = affinity
                    .filter(|(identity, _)| identity == selected)
                    .map(|(_, cpus)| cpus);

                ui.add_space(4.0);
                ui.label(format!(
                    "Current CPU Affinity Mask: {}",
                    current_affinity
                        .map(|v| format!("{v:?}"))
                        .unwrap_or_else(|| "Unavailable (querying...)".into())
                ));

                if cfg!(any(target_os = "linux", windows)) {
                    ui.horizontal(|ui| {
                        ui.label("Proposed CPU Set (comma-separated):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.affinity_input)
                                .hint_text("e.g. 0, 1, 2, 3")
                                .desired_width(160.0),
                        );
                        let can_apply =
                            current_affinity.is_some() && !self.affinity_input.trim().is_empty();
                        if ui
                            .add_enabled(can_apply, egui::Button::new("Apply Affinity"))
                            .clicked()
                            && let Ok(logical_cpus) = self
                                .affinity_input
                                .split(',')
                                .map(|x| x.trim().parse::<u16>())
                                .collect::<Result<Vec<_>, _>>()
                            && !logical_cpus.is_empty()
                        {
                            let _ = self.tx.send(Command::SetAffinity {
                                process: selected.clone(),
                                logical_cpus,
                            });
                        }
                    });
                }
            } else {
                ui.colored_label(
                    p.warning,
                    "This process ended or its PID was reused. Selection is retained for reference; actions are disabled.",
                );
            }
        }
    }

    fn hardware(&mut self, ui: &mut egui::Ui, topology: Option<&Topology>, p: Palette) {
        ui.heading("Hardware Topology Canvas");
        let Some(t) = topology else {
            ui.label("Topology unavailable");
            return;
        };

        ui.horizontal(|ui| {
            ui.label(format!("{} · {} · {}", t.vendor, t.model, t.architecture));
            ui.separator();
            ui.label(format!(
                "{} Cores · {} Caches · {} GPUs",
                t.cores.len(),
                t.caches.len(),
                t.gpus.len()
            ));
            ui.separator();
            if ui.button("Reset Zoom").clicked() {
                self.canvas_pan = Vec2::ZERO;
                self.canvas_zoom = 1.0;
            }
        });

        ui.add_space(8.0);
        let canvas_size = Vec2::new(ui.available_width(), 260.0);
        let (rect, response) = ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 8.0, p.surface);
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );

        if response.dragged() {
            self.canvas_pan += response.drag_delta();
        }

        let origin = rect.min + self.canvas_pan + Vec2::new(16.0, 16.0);
        let mut x_offset = 0.0f32;

        for core in &t.cores {
            let core_width = 110.0 * self.canvas_zoom;
            let core_height = 80.0 * self.canvas_zoom;
            let core_rect = Rect::from_min_size(
                origin + Vec2::new(x_offset, 0.0),
                Vec2::new(core_width, core_height),
            );

            let is_selected = self.selected_hardware == Some(SelectedHardware::Core(core.id));
            let fill = if is_selected {
                Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 40)
            } else {
                p.surface_raised
            };
            let stroke = if is_selected {
                Stroke::new(2.0, p.accent)
            } else {
                Stroke::new(1.0, p.border)
            };

            painter.rect_filled(core_rect, 6.0, fill);
            painter.rect_stroke(core_rect, 6.0, stroke, egui::StrokeKind::Inside);

            painter.text(
                core_rect.min + Vec2::new(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                format!("Core {}", core.id),
                egui::FontId::proportional(13.0 * self.canvas_zoom),
                p.text,
            );

            painter.text(
                core_rect.min + Vec2::new(8.0, 28.0),
                egui::Align2::LEFT_TOP,
                format!("CPUs: {:?}", core.logical_cpus),
                egui::FontId::proportional(11.0 * self.canvas_zoom),
                p.secondary,
            );

            if let Some(cls) = &core.class {
                painter.text(
                    core_rect.min + Vec2::new(8.0, 48.0),
                    egui::Align2::LEFT_TOP,
                    cls,
                    egui::FontId::proportional(9.0 * self.canvas_zoom),
                    p.primary,
                );
            }

            if response.clicked()
                && let Some(mouse_pos) = response.interact_pointer_pos()
                && core_rect.contains(mouse_pos)
            {
                self.selected_hardware = Some(SelectedHardware::Core(core.id));
            }

            x_offset += core_width + 12.0 * self.canvas_zoom;
        }

        let cache_y = 100.0 * self.canvas_zoom;
        let mut cache_x = 0.0f32;
        for cache in &t.caches {
            let cache_width = 120.0 * self.canvas_zoom;
            let cache_height = 50.0 * self.canvas_zoom;
            let cache_rect = Rect::from_min_size(
                origin + Vec2::new(cache_x, cache_y),
                Vec2::new(cache_width, cache_height),
            );

            let is_selected = self.selected_hardware
                == Some(SelectedHardware::Cache(cache.level, cache.size_bytes));
            let fill = if is_selected {
                Color32::from_rgba_unmultiplied(p.primary.r(), p.primary.g(), p.primary.b(), 40)
            } else {
                p.surface_raised
            };
            let stroke = if is_selected {
                Stroke::new(2.0, p.primary)
            } else {
                Stroke::new(1.0, p.border)
            };

            painter.rect_filled(cache_rect, 4.0, fill);
            painter.rect_stroke(cache_rect, 4.0, stroke, egui::StrokeKind::Inside);

            let size_str = if cache.size_bytes >= 1_048_576 {
                format!("{:.1} MiB", cache.size_bytes as f64 / 1_048_576.0)
            } else {
                format!("{} KiB", cache.size_bytes / 1024)
            };

            painter.text(
                cache_rect.min + Vec2::new(8.0, 6.0),
                egui::Align2::LEFT_TOP,
                format!("L{} Cache ({size_str})", cache.level),
                egui::FontId::proportional(12.0 * self.canvas_zoom),
                p.text,
            );

            painter.text(
                cache_rect.min + Vec2::new(8.0, 26.0),
                egui::Align2::LEFT_TOP,
                format!("Line: {} B", cache.line_bytes),
                egui::FontId::proportional(10.0 * self.canvas_zoom),
                p.secondary,
            );

            if response.clicked()
                && let Some(mouse_pos) = response.interact_pointer_pos()
                && cache_rect.contains(mouse_pos)
            {
                self.selected_hardware =
                    Some(SelectedHardware::Cache(cache.level, cache.size_bytes));
            }

            cache_x += cache_width + 10.0 * self.canvas_zoom;
        }

        let gpu_y = 170.0 * self.canvas_zoom;
        let mut gpu_x = 0.0f32;
        for gpu in &t.gpus {
            let gpu_width = 240.0 * self.canvas_zoom;
            let gpu_height = 50.0 * self.canvas_zoom;
            let gpu_rect = Rect::from_min_size(
                origin + Vec2::new(gpu_x, gpu_y),
                Vec2::new(gpu_width, gpu_height),
            );

            let is_selected = self.selected_hardware == Some(SelectedHardware::Gpu(gpu.id));
            let fill = if is_selected {
                Color32::from_rgba_unmultiplied(p.gpu.r(), p.gpu.g(), p.gpu.b(), 40)
            } else {
                p.surface_raised
            };
            let stroke = if is_selected {
                Stroke::new(2.0, p.gpu)
            } else {
                Stroke::new(1.0, p.border)
            };

            painter.rect_filled(gpu_rect, 4.0, fill);
            painter.rect_stroke(gpu_rect, 4.0, stroke, egui::StrokeKind::Inside);

            painter.text(
                gpu_rect.min + Vec2::new(8.0, 6.0),
                egui::Align2::LEFT_TOP,
                format!("GPU: {}", gpu.name),
                egui::FontId::proportional(12.0 * self.canvas_zoom),
                p.text,
            );
            painter.text(
                gpu_rect.min + Vec2::new(8.0, 26.0),
                egui::Align2::LEFT_TOP,
                &gpu.source,
                egui::FontId::proportional(10.0 * self.canvas_zoom),
                p.secondary,
            );

            if response.clicked()
                && let Some(mouse_pos) = response.interact_pointer_pos()
                && gpu_rect.contains(mouse_pos)
            {
                self.selected_hardware = Some(SelectedHardware::Gpu(gpu.id));
            }

            gpu_x += gpu_width + 12.0 * self.canvas_zoom;
        }

        ui.add_space(12.0);
        if let Some(selected) = &self.selected_hardware {
            ui.separator();
            ui.heading("Selected Hardware Component");
            match selected {
                SelectedHardware::Core(id) => {
                    if let Some(core) = t.cores.iter().find(|c| c.id == *id) {
                        ui.label(format!("Physical Core ID: {}", core.id));
                        ui.label(format!("Logical Processors: {:?}", core.logical_cpus));
                        ui.label(format!("Package ID: {:?}", core.package_id));
                        ui.label(format!("NUMA Node: {:?}", core.numa_node));
                        if let Some(cls) = &core.class {
                            ui.label(format!("Efficiency Class: {cls}"));
                        }
                    }
                }
                SelectedHardware::Cache(level, size) => {
                    if let Some(c) = t
                        .caches
                        .iter()
                        .find(|c| c.level == *level && c.size_bytes == *size)
                    {
                        ui.label(format!("Level: L{}", c.level));
                        ui.label(format!(
                            "Capacity: {} bytes ({} KiB)",
                            c.size_bytes,
                            c.size_bytes / 1024
                        ));
                        ui.label(format!("Line Size: {} bytes", c.line_bytes));
                        ui.label(format!("Shared By CPUs: {:?}", c.shared_by));
                        ui.label(format!("Source: {}", c.source));
                    }
                }
                SelectedHardware::Gpu(id) => {
                    if let Some(g) = t.gpus.iter().find(|g| g.id == *id) {
                        ui.label(format!("GPU Node ID: {}", g.id));
                        ui.label(format!("Name: {}", g.name));
                        ui.label(format!("Provider: {}", g.source));
                    }
                }
            }
        }
    }

    fn benchmarks(&mut self, ui: &mut egui::Ui, shared: &Shared, p: Palette) {
        ui.heading("Hardware Benchmarks");
        ui.label(
            "Explicit, cancellable micro-architectural probes. Line and page chase use at most 64 MiB and 12 seconds.",
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !shared.benchmark_running,
                    egui::Button::new("Run Line Probe"),
                )
                .clicked()
            {
                let _ = self.tx.send(Command::RunCacheProbe { page_mode: false });
            }
            if ui
                .add_enabled(
                    !shared.benchmark_running,
                    egui::Button::new("Run Page Probe"),
                )
                .clicked()
            {
                let _ = self.tx.send(Command::RunCacheProbe { page_mode: true });
            }
            if ui
                .add_enabled(shared.benchmark_running, egui::Button::new("Cancel"))
                .clicked()
            {
                let _ = self.tx.send(Command::CancelBenchmark);
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Logical CPUs for Coherency Matrix:");
            ui.add(
                egui::TextEdit::singleline(&mut self.matrix_input)
                    .hint_text("e.g. 0,1,2,3")
                    .desired_width(120.0),
            );
            if ui
                .add_enabled(
                    !shared.benchmark_running,
                    egui::Button::new("Run Core Matrix"),
                )
                .clicked()
                && let Ok(logical_cpus) = self
                    .matrix_input
                    .split(',')
                    .map(|x| x.trim().parse::<u16>())
                    .collect::<Result<Vec<_>, _>>()
            {
                let _ = self.tx.send(Command::RunCoreMatrix { logical_cpus });
            }
        });

        if shared.benchmark_running {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Probe executing in background thread...");
            });
        }

        if let Some(error) = &shared.benchmark_error {
            ui.add_space(8.0);
            ui.colored_label(p.error, format!("Benchmark Error: {error}"));
        }

        if let Some(result) = &shared.benchmark_result {
            ui.separator();
            ui.heading(format!("Results: {}", result.mode));
            ui.colored_label(p.secondary, &result.methodology);
            if !result.comparable_to_x86_cycles {
                ui.label(
                    "Note: Monotonic nanosecond timing is reported. Exact x86 cycle comparability requires RDTSCP/TSC calibration.",
                );
            }

            if !result.points.is_empty() {
                ui.add_space(8.0);
                ui.label("Latency Curve (Working Set Size vs Nanoseconds per Load):");

                let (chart_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width().max(320.0), 160.0),
                    egui::Sense::hover(),
                );
                let painter = ui.painter();
                painter.rect_filled(chart_rect, 6.0, p.surface);
                painter.rect_stroke(
                    chart_rect,
                    6.0,
                    Stroke::new(1.0, p.border),
                    egui::StrokeKind::Inside,
                );

                let max_lat = result
                    .points
                    .iter()
                    .map(|pt| pt.p99_ns)
                    .fold(10.0f64, f64::max) as f32;

                let pts: Vec<Pos2> = result
                    .points
                    .iter()
                    .enumerate()
                    .map(|(i, pt)| {
                        let x = chart_rect.left()
                            + chart_rect.width()
                                * (i as f32 / (result.points.len() - 1).max(1) as f32);
                        let y = chart_rect.bottom()
                            - chart_rect.height() * (pt.median_ns as f32 / max_lat).clamp(0.0, 1.0);
                        Pos2::new(x, y)
                    })
                    .collect();

                if pts.len() > 1 {
                    painter.add(egui::Shape::line(pts, Stroke::new(2.0, p.accent)));
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("benchmark_points")
                        .striped(true)
                        .num_columns(5)
                        .spacing([24.0, 4.0])
                        .show(ui, |ui| {
                            ui.strong("Working Set");
                            ui.strong("Min");
                            ui.strong("Median");
                            ui.strong("P95");
                            ui.strong("P99");
                            ui.end_row();

                            for pt in &result.points {
                                let kib = pt.working_set_bytes / 1024;
                                let size_str = if kib >= 1024 {
                                    format!("{:.1} MiB", kib as f64 / 1024.0)
                                } else {
                                    format!("{kib} KiB")
                                };
                                ui.label(size_str);
                                ui.label(format!("{:.2} ns", pt.min_ns));
                                ui.label(format!("{:.2} ns", pt.median_ns));
                                ui.label(format!("{:.2} ns", pt.p95_ns));
                                ui.label(format!("{:.2} ns", pt.p99_ns));
                                ui.end_row();
                            }
                        });
                });
            }

            if !result.core_pairs.is_empty() {
                ui.add_space(8.0);
                ui.heading("Inter-Core Coherency Latency Pairs");
                egui::Grid::new("core_pairs_table")
                    .striped(true)
                    .num_columns(3)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Core A");
                        ui.strong("Core B");
                        ui.strong("Estimated One-Way Latency");
                        ui.end_row();
                        for pair in &result.core_pairs {
                            ui.label(format!("CPU {}", pair.cpu_a));
                            ui.label(format!("CPU {}", pair.cpu_b));
                            ui.label(format!("{:.2} ns", pair.estimated_one_way_ns));
                            ui.end_row();
                        }
                    });
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    background: Color32,
    surface: Color32,
    surface_raised: Color32,
    text: Color32,
    secondary: Color32,
    border: Color32,
    primary: Color32,
    accent: Color32,
    focus: Color32,
    cpu: Color32,
    memory: Color32,
    gpu: Color32,
    disk: Color32,
    network: Color32,
    success: Color32,
    warning: Color32,
    error: Color32,
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.apply_theme(&ctx);
        let p = self.palette(&ctx);
        let snapshot = {
            let shared = self.shared.lock().unwrap();
            (
                shared.connected,
                shared.error.clone(),
                shared.frame.clone(),
                shared.topology.clone(),
                shared.capabilities.clone(),
                shared.benchmark_running,
                shared.benchmark_result.clone(),
                shared.benchmark_error.clone(),
                shared.affinity.clone(),
            )
        };

        if let Some(frame) = &snapshot.2
            && frame.sequence != self.last_sequence
        {
            self.last_sequence = frame.sequence;
            if let Some(cpu) = frame.cpu_percent.value {
                self.cpu_history.push_back(cpu);
                if self.cpu_history.len() > 120 {
                    self.cpu_history.pop_front();
                }
            }
            if let (Some(used), Some(total)) = (
                frame.memory_used_bytes.value,
                frame.memory_total_bytes.value,
            ) {
                let pct = if total > 0 {
                    (used as f32 / total as f32) * 100.0
                } else {
                    0.0
                };
                self.mem_history.push_back(pct);
                if self.mem_history.len() > 120 {
                    self.mem_history.pop_front();
                }
            }
            let disk_rate = frame
                .disk_read_bytes_per_sec
                .value
                .unwrap_or(0)
                .saturating_add(frame.disk_write_bytes_per_sec.value.unwrap_or(0))
                as f32
                / 1024.0;
            self.disk_history.push_back(disk_rate);
            if self.disk_history.len() > 120 {
                self.disk_history.pop_front();
            }

            let net_rate = frame
                .network_rx_bytes_per_sec
                .value
                .unwrap_or(0)
                .saturating_add(frame.network_tx_bytes_per_sec.value.unwrap_or(0))
                as f32
                / 1024.0;
            self.net_history.push_back(net_rate);
            if self.net_history.len() > 120 {
                self.net_history.pop_front();
            }
        }

        egui::Panel::left("nav").min_size(170.0).show(root, |ui| {
            ui.heading("◉ Planora");
            ui.small("Your PC, clearly.");
            ui.separator();

            for (page, label) in [
                (Page::Overview, "Overview"),
                (Page::Processes, "Processes"),
                (Page::Hardware, "Hardware"),
                (Page::Benchmarks, "Benchmarks"),
                (Page::Capabilities, "Capabilities"),
            ] {
                if ui.selectable_label(self.page == page, label).clicked() {
                    self.page = page;
                }
            }

            ui.separator();
            ui.label("Theme:");
            egui::ComboBox::from_id_salt("theme")
                .selected_text(match self.theme {
                    Theme::System => "System",
                    Theme::Dark => "Dark",
                    Theme::Light => "Light",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.theme, Theme::System, "System");
                    ui.selectable_value(&mut self.theme, Theme::Dark, "Dark");
                    ui.selectable_value(&mut self.theme, Theme::Light, "Light");
                });

            ui.add_space(8.0);
            ui.label("Poll Interval:");
            let current = self.poll_ms.load(Ordering::Relaxed);
            egui::ComboBox::from_id_salt("poll")
                .selected_text(format!("{current} ms"))
                .show_ui(ui, |ui| {
                    for interval in [50, 100, 200, 500, 1000] {
                        if ui
                            .selectable_label(current == interval, format!("{interval} ms"))
                            .clicked()
                        {
                            self.poll_ms.store(interval, Ordering::Relaxed);
                        }
                    }
                });
        });

        egui::CentralPanel::default().show(root, |ui| {
            if !snapshot.0 {
                ui.heading("Helper Authorization Required");
                ui.label(
                    "Planora requires an authorized local privileged helper before monitoring starts.",
                );
                ui.add_space(8.0);

                #[cfg(windows)]
                if ui
                    .button("Authorize Windows Helper (Administrator Approval)")
                    .clicked()
                {
                    let script = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|p| p.join("Authorize-Planora.ps1")));
                    if let Some(script) = script {
                        let result = std::process::Command::new("powershell.exe")
                            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                            .arg(script)
                            .spawn();
                        if let Err(error) = result {
                            self.shared.lock().unwrap().error = Some(error.to_string());
                        }
                    }
                }

                #[cfg(target_os = "macos")]
                if ui.button("Request macOS Daemon Approval").clicked()
                    && unsafe { planora_register_daemon() } != 0
                {
                    self.shared.lock().unwrap().error =
                        Some("Registration failed; check Console.app for details.".into());
                }

                #[cfg(target_os = "linux")]
                if ui.button("Authorize Linux Helper (Administrator Approval)").clicked()
                    && let Err(error) = std::process::Command::new(
                        "/usr/libexec/planora/authorize-user",
                    )
                    .spawn()
                {
                    self.shared.lock().unwrap().error = Some(error.to_string());
                }
                #[cfg(target_os = "linux")]
                ui.label("After approval, sign out and back in so group membership takes effect.");

                if let Some(error) = snapshot.1 {
                    ui.add_space(8.0);
                    ui.colored_label(p.error, error);
                }
                return;
            }

            let Some(frame) = snapshot.2.as_ref() else {
                ui.spinner();
                return;
            };

            match self.page {
                Page::Overview => self.overview(ui, frame, p),
                Page::Processes => self.processes(ui, frame, snapshot.8.as_ref(), p),
                Page::Hardware => self.hardware(ui, snapshot.3.as_deref(), p),
                Page::Benchmarks => self.benchmarks(
                    ui,
                    &Shared {
                        benchmark_running: snapshot.5,
                        benchmark_result: snapshot.6,
                        benchmark_error: snapshot.7,
                        ..Shared::default()
                    },
                    p,
                ),
                Page::Capabilities => {
                    ui.heading("System Capabilities & Availability");
                    ui.label("Granular platform counter and hardware feature status:");
                    ui.add_space(8.0);
                    egui::Grid::new("capabilities_table")
                        .striped(true)
                        .num_columns(3)
                        .spacing([24.0, 6.0])
                        .show(ui, |ui| {
                            ui.strong("Capability");
                            ui.strong("Status");
                            ui.strong("Reason / Details");
                            ui.end_row();

                            for c in snapshot.4 {
                                ui.label(&c.name);
                                if c.available {
                                    ui.colored_label(p.success, "✓ Available");
                                    ui.label("Supported on this platform");
                                } else {
                                    ui.colored_label(p.error, "— Unavailable");
                                    ui.colored_label(
                                        p.secondary,
                                        c.reason.as_deref().unwrap_or("Hardware/OS feature unsupported"),
                                    );
                                }
                                ui.end_row();
                            }
                        });
                }
            }
        });
    }
}

fn main() -> eframe::Result {
    #[cfg(target_os = "macos")]
    if std::env::args().any(|arg| arg == "--unregister-helper") {
        let result = unsafe { planora_unregister_daemon() };
        if result != 0 {
            eprintln!("helper unregistration failed: {result}");
        }
        return Ok(());
    }

    if std::env::args().any(|arg| arg == "--probe-helper") {
        match planora_helper::transact(&Command::Snapshot) {
            Ok(Response::Snapshot(frame)) => {
                println!(
                    "Planora helper: {} processes; collector {:.2} ms; sample {}",
                    frame.processes.len(),
                    frame.collection_duration_ms,
                    frame.sequence
                );
                return Ok(());
            }
            Ok(other) => {
                eprintln!("unexpected helper response: {other:?}");
                std::process::exit(1);
            }
            Err(error) => {
                eprintln!("helper unavailable: {error}");
                std::process::exit(1);
            }
        }
    }

    let bytes = include_bytes!("../../../assets/brand/platforms/windows/planora-256.png");
    let png = image::load_from_memory(bytes)
        .expect("bundled icon")
        .into_rgba8();
    let icon = egui::IconData {
        rgba: png.as_raw().clone(),
        width: png.width(),
        height: png.height(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Planora")
            .with_inner_size([1180.0, 760.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "Planora",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use planora_core::{Metric, PROTOCOL_VERSION, ProcessRecord};

    fn frame() -> TelemetryFrame {
        let m = Metric::unavailable("bytes", "fixture", 1, "missing");
        let make = |pid, started, name: &str, cpu| ProcessRecord {
            identity: ProcessIdentity {
                pid,
                started,
                source: "fixture".into(),
            },
            name: name.into(),
            cpu_percent: cpu,
            memory_bytes: Metric::available(1024, "bytes", "fixture", 1),
            disk_read_bytes_per_sec: m.clone(),
            disk_write_bytes_per_sec: m.clone(),
            gpu_percent: Metric::unavailable("%", "fixture", 1, "missing"),
            affinity: Metric::unavailable("cpus", "fixture", 1, "missing"),
        };
        TelemetryFrame {
            protocol_version: PROTOCOL_VERSION,
            sequence: 1,
            sampled_at_ms: 1,
            collection_duration_ms: 0.1,
            cpu_percent: Metric::unavailable("%", "fixture", 1, "missing"),
            memory_used_bytes: m.clone(),
            memory_total_bytes: m.clone(),
            disk_read_bytes_per_sec: m.clone(),
            disk_write_bytes_per_sec: m.clone(),
            network_rx_bytes_per_sec: m.clone(),
            network_tx_bytes_per_sec: m.clone(),
            gpu_percent: Metric::unavailable("%", "fixture", 1, "missing"),
            package_power_watts: Metric::unavailable("W", "fixture", 1, "missing"),
            processes: vec![
                make(4, 1, "Alpha", Metric::available(20.0, "%", "fixture", 1)),
                make(
                    4,
                    2,
                    "Alpha",
                    Metric::unavailable("%", "fixture", 1, "missing"),
                ),
                make(7, 1, "Beta", Metric::available(5.0, "%", "fixture", 1)),
            ],
        }
    }

    #[test]
    fn filtering_and_missing_cpu_sort() {
        let f = frame();
        let rows = visible_processes(&f, "alpha", SortColumn::Cpu, true);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].identity.started, 1);
        assert!(rows[1].cpu_percent.value.is_none());
    }

    #[test]
    fn sorting_by_name_and_pid() {
        let f = frame();
        let rows = visible_processes(&f, "", SortColumn::Name, false);
        assert_eq!(rows[0].name, "Alpha");
        assert_eq!(rows[2].name, "Beta");

        let rows_pid = visible_processes(&f, "", SortColumn::Pid, true);
        assert_eq!(rows_pid[0].identity.pid, 7);
    }

    #[test]
    fn selection_rejects_reused_pid() {
        let f = frame();
        let old = ProcessIdentity {
            pid: 4,
            started: 3,
            source: "fixture".into(),
        };
        assert!(selected_record(&f, &old).is_none());
    }

    #[test]
    fn theme_override_wins() {
        assert!(!theme_is_dark(Theme::Light, Some(egui::Theme::Dark)));
        assert!(theme_is_dark(Theme::Dark, Some(egui::Theme::Light)));
    }
}
