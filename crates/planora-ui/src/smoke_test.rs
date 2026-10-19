use crate::{
    App,
    state::{AppState, DisplayPause, Page, Shared, UserSettings},
    theme::{Density, Theme, get_palette},
};
use eframe::egui::{
    self, Color32, Pos2, Rect, Vec2,
    epaint::{ClippedShape, ColorMode, Shape},
};
use planora_core::{
    AvailabilityReason, CacheDescriptor, CacheType, Capability, CoreDescriptor, Metric,
    PROTOCOL_VERSION, ProcessIdentity, ProcessRecord, TelemetryFrame, Topology,
};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, atomic::AtomicU64, mpsc},
    time::Instant,
};

pub fn test_topology() -> Topology {
    Topology {
        generation: 1,
        vendor: "Intel".into(),
        model: "Core i7-12700H".into(),
        architecture: "x86_64".into(),
        cores: vec![
            CoreDescriptor {
                id: 0,
                package_id: Some(0),
                numa_node: Some(0),
                logical_cpus: vec![0, 1],
                class: Some("Performance".into()),
            },
            CoreDescriptor {
                id: 1,
                package_id: Some(0),
                numa_node: Some(0),
                logical_cpus: vec![2, 3],
                class: Some("Performance".into()),
            },
            CoreDescriptor {
                id: 2,
                package_id: Some(0),
                numa_node: Some(0),
                logical_cpus: vec![4, 5],
                class: Some("Performance".into()),
            },
            CoreDescriptor {
                id: 3,
                package_id: Some(0),
                numa_node: Some(0),
                logical_cpus: vec![6, 7],
                class: Some("Performance".into()),
            },
            CoreDescriptor {
                id: 4,
                package_id: Some(0),
                numa_node: Some(0),
                logical_cpus: vec![8],
                class: Some("Efficient".into()),
            },
            CoreDescriptor {
                id: 5,
                package_id: Some(0),
                numa_node: Some(0),
                logical_cpus: vec![9],
                class: Some("Efficient".into()),
            },
        ],
        caches: vec![
            CacheDescriptor {
                id: 0,
                level: 1,
                cache_type: CacheType::Data,
                size_bytes: 48 * 1024,
                line_bytes: Some(64),
                shared_by: vec![0, 1],
                source: "CPUID".into(),
            },
            CacheDescriptor {
                id: 1,
                level: 2,
                cache_type: CacheType::Unified,
                size_bytes: 1280 * 1024,
                line_bytes: Some(64),
                shared_by: vec![0, 1],
                source: "CPUID".into(),
            },
            CacheDescriptor {
                id: 2,
                level: 3,
                cache_type: CacheType::Unified,
                size_bytes: 24 * 1024 * 1024,
                line_bytes: Some(64),
                shared_by: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
                source: "CPUID".into(),
            },
        ],
        gpus: vec![planora_core::GpuDescriptor {
            id: 0,
            name: "Intel Iris Xe Graphics".into(),
            source: "DXGI".into(),
        }],
        notes: Vec::new(),
    }
}

pub fn test_frame() -> TelemetryFrame {
    let make_proc = |pid, name: &str, cpu: f32, mem_mb: u64| ProcessRecord {
        identity: ProcessIdentity {
            pid,
            started: 100,
            sequence_number: None,
            source: planora_core::IdentitySource::WindowsProcessFiletime,
        },
        name: name.into(),
        cpu_percent: Metric::available(cpu, "%", "mock", 1000),
        memory_bytes: Metric::available(mem_mb * 1024 * 1024, "bytes", "mock", 1000),
        disk_read_bytes_per_sec: Metric::available(1024 * 100, "B/s", "mock", 1000),
        disk_write_bytes_per_sec: Metric::available(1024 * 50, "B/s", "mock", 1000),
        gpu_percent: Metric::available(2.5, "%", "mock", 1000),
        affinity: Metric::unavailable(
            "cpus",
            "mock",
            1000,
            AvailabilityReason::Unsupported("none".into()),
        ),
    };

    TelemetryFrame {
        protocol_version: PROTOCOL_VERSION,
        session_id: 1,
        topology_generation: 1,
        sequence: 42,
        sampled_at_ms: 1000,
        collection_duration_ms: 2.1,
        cpu_percent: Metric::available(34.8, "%", "mock", 1000),
        memory_used_bytes: Metric::available(11_811_160_064, "bytes", "mock", 1000), // 11.0 GiB
        memory_total_bytes: Metric::available(17_179_869_184, "bytes", "mock", 1000), // 16.0 GiB
        disk_read_bytes_per_sec: Metric::available(4_500_000, "B/s", "mock", 1000),
        disk_write_bytes_per_sec: Metric::available(1_200_000, "B/s", "mock", 1000),
        network_rx_bytes_per_sec: Metric::available(850_000, "B/s", "mock", 1000),
        network_tx_bytes_per_sec: Metric::available(120_000, "B/s", "mock", 1000),
        gpu_percent: Metric::unavailable(
            "%",
            "mock",
            1000,
            AvailabilityReason::Unsupported("GPU sensor uninitialized".into()),
        ),
        package_power_watts: Metric::available(28.4, "W", "mock", 1000),
        processes: vec![
            make_proc(4, "System", 1.2, 12),
            make_proc(1042, "chrome.exe", 14.5, 1420),
            make_proc(3280, "Code.exe", 8.4, 980),
            make_proc(5504, "planora.exe", 2.1, 85),
            make_proc(8912, "rust-analyzer.exe", 5.6, 620),
            make_proc(12040, "explorer.exe", 0.8, 140),
            make_proc(14200, "dwm.exe", 2.2, 95),
            make_proc(18400, "slack.exe", 0.5, 410),
        ],
    }
}

pub fn create_test_app(theme: Theme, page: Page) -> App {
    let (tx, _rx) = mpsc::sync_channel(64);
    let settings = UserSettings {
        theme,
        density: Density::Normal,
        ..Default::default()
    };

    let frame = test_frame();
    let topo = test_topology();

    let shared = Arc::new(Mutex::new(Shared {
        state: Some(AppState::Live),
        last_frame_received: Some(Instant::now()),
        frame: Some(Arc::new(frame)),
        topology: Some(topo),
        capabilities: vec![
            Capability {
                name: "System CPU Utilization".into(),
                available: true,
                reason: None,
            },
            Capability {
                name: "Memory Total & In-Use".into(),
                available: true,
                reason: None,
            },
            Capability {
                name: "Disk IO Throughput".into(),
                available: true,
                reason: None,
            },
            Capability {
                name: "Network Bandwidth Rates".into(),
                available: true,
                reason: None,
            },
            Capability {
                name: "GPU Utilization & VRAM".into(),
                available: false,
                reason: Some("No supported hardware GPU telemetry adapter discovered".into()),
            },
            Capability {
                name: "Processor Package Power".into(),
                available: true,
                reason: None,
            },
        ],
        benchmark_state: Some(planora_core::BenchmarkState::Idle),
        diagnostic_logs: Vec::new(),
        affinity: None,
        action_error: None,
    }));

    App {
        shared,
        tx,
        poll_ms: Arc::new(AtomicU64::new(100)),
        settings,
        page,
        selected: None,
        display_pause: DisplayPause::default(),
        logo_cache: crate::components::logo::LogoCache::new(),
        histories: crate::history::ResourceHistories::default(),
        overview_state: crate::views::overview::OverviewViewState::default(),
        processes_state: crate::views::processes::ProcessesViewState::default(),
        hardware_state: crate::views::hardware::HardwareViewState::default(),
        benchmarks_state: crate::views::benchmarks::BenchmarksViewState::default(),
        capabilities_state: crate::views::capabilities::CapabilitiesViewState::default(),
    }
}

pub fn render_to_svg(app: &mut App, width: f32, height: f32) -> String {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(1.0);
    let is_dark = app.settings.theme == Theme::Dark;
    crate::theme::apply_egui_theme(&ctx, is_dark, app.settings.density);
    let p = get_palette(is_dark);

    let raw_input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height))),
        max_texture_side: Some(4096),
        ..Default::default()
    };

    // Warm-up run 1 (generates layout)
    let mut out1 = ctx.run_ui(raw_input.clone(), |ui| {
        app.render_ui(ui);
    });
    out1.textures_delta.clear();

    // Run 2 (produces final stabilized positions)
    let mut full_output = ctx.run_ui(raw_input, |ui| {
        app.render_ui(ui);
    });
    let svg = shapes_to_svg(width, height, p.background, &full_output.shapes);
    full_output.textures_delta.clear();
    svg
}

fn color_to_css(c: Color32) -> String {
    if c.a() == 255 {
        format!("rgb({}, {}, {})", c.r(), c.g(), c.b())
    } else {
        format!(
            "rgba({}, {}, {}, {:.3})",
            c.r(),
            c.g(),
            c.b(),
            c.a() as f32 / 255.0
        )
    }
}

fn color_mode_to_css(cm: &ColorMode) -> String {
    match cm {
        ColorMode::Solid(c) => color_to_css(*c),
        ColorMode::UV(_) => "rgb(255, 255, 255)".into(),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn shapes_to_svg(width: f32, height: f32, bg: Color32, shapes: &[ClippedShape]) -> String {
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  <style>
    text {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }}
    .mono {{ font-family: "JetBrains Mono", Consolas, monospace; }}
  </style>
  <rect width="100%" height="100%" fill="{}"/>
"#,
        color_to_css(bg)
    );

    fn recurse_shape(shape: &Shape, out: &mut String) {
        match shape {
            Shape::Rect(r) => {
                let rx = r.corner_radius.nw;
                let fill = color_to_css(r.fill);
                let stroke_w = r.stroke.width;
                let stroke_c = color_to_css(r.stroke.color);
                let w = r.rect.width();
                let h = r.rect.height();
                if w > 0.0 && h > 0.0 {
                    out.push_str(&format!(
                        r#"  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="{fill}" stroke="{stroke_c}" stroke-width="{stroke_w}"/>"#,
                        r.rect.min.x, r.rect.min.y, w, h, rx
                    ));
                    out.push('\n');
                }
            }
            Shape::Text(t) => {
                let raw_text = t.galley.text();
                if !raw_text.trim().is_empty() {
                    let esc = xml_escape(raw_text);
                    let color = color_to_css(t.fallback_color);
                    let font_size = t
                        .galley
                        .rows
                        .first()
                        .map(|r| r.rect().height())
                        .unwrap_or(13.0);
                    let is_mono = t
                        .galley
                        .job
                        .sections
                        .first()
                        .map(|s| s.format.font_id.family == egui::FontFamily::Monospace)
                        .unwrap_or(false);
                    let class_attr = if is_mono { r#" class="mono""# } else { "" };
                    let y = t.pos.y + font_size * 0.82;
                    out.push_str(&format!(
                        r#"  <text x="{:.1}" y="{:.1}" fill="{color}" font-size="{:.1}"{class_attr}>{esc}</text>"#,
                        t.pos.x, y, font_size
                    ));
                    out.push('\n');
                }
            }
            Shape::LineSegment { points, stroke } => {
                let c = color_to_css(stroke.color);
                out.push_str(&format!(
                    r#"  <line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{c}" stroke-width="{:.1}"/>"#,
                    points[0].x, points[0].y, points[1].x, points[1].y, stroke.width
                ));
                out.push('\n');
            }
            Shape::Path(p) if p.points.len() >= 2 => {
                let pts: Vec<String> = p
                    .points
                    .iter()
                    .map(|pt| format!("{:.1},{:.1}", pt.x, pt.y))
                    .collect();
                let stroke_c = color_mode_to_css(&p.stroke.color);
                let fill = color_to_css(p.fill);
                out.push_str(&format!(
                        r#"  <polyline points="{}" stroke="{stroke_c}" stroke-width="{:.1}" fill="{fill}"/>"#,
                        pts.join(" "),
                        p.stroke.width
                    ));
                out.push('\n');
            }
            Shape::Vec(shapes) => {
                for inner in shapes {
                    recurse_shape(inner, out);
                }
            }
            _ => {}
        }
    }

    for clipped in shapes {
        recurse_shape(&clipped.shape, &mut svg);
    }

    svg.push_str("</svg>\n");
    svg
}

pub fn run_smoke_fast_check() -> Result<(), String> {
    let resolutions = [
        (1024, 640),
        (1180, 790),
        (1366, 768),
        (1536, 864),
        (1920, 1080),
    ];
    let views = [
        (Page::Overview, "overview"),
        (Page::Processes, "processes"),
        (Page::Hardware, "hardware"),
        (Page::Benchmarks, "benchmarks"),
        (Page::Capabilities, "capabilities"),
        (Page::Settings, "settings"),
    ];

    for &(w, h) in &resolutions {
        let mut app_dark = create_test_app(Theme::Dark, Page::Overview);
        let svg_dark = render_to_svg(&mut app_dark, w as f32, h as f32);

        let mut app_light = create_test_app(Theme::Light, Page::Overview);
        let svg_light = render_to_svg(&mut app_light, w as f32, h as f32);

        if svg_dark.len() < 1000 {
            return Err(format!(
                "Dark SVG for {w}x{h} too small: {} bytes",
                svg_dark.len()
            ));
        }
        if svg_light.len() < 1000 {
            return Err(format!(
                "Light SVG for {w}x{h} too small: {} bytes",
                svg_light.len()
            ));
        }
        if svg_dark == svg_light {
            return Err(format!(
                "Dark and Light SVGs for {w}x{h} must not be identical"
            ));
        }
    }

    for (page, view_name) in views {
        let mut app_dark = create_test_app(Theme::Dark, page);
        let svg_dark = render_to_svg(&mut app_dark, 1180.0, 790.0);

        let mut app_light = create_test_app(Theme::Light, page);
        let svg_light = render_to_svg(&mut app_light, 1180.0, 790.0);

        if svg_dark == svg_light {
            return Err(format!(
                "Dark and Light SVGs for {view_name} must not be identical"
            ));
        }
    }

    Ok(())
}

pub fn assert_image_validity(path: &Path, expected_w: u32, expected_h: u32) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    if bytes.len() < 5000 {
        return Err(format!(
            "Image {} is suspiciously small ({} bytes)",
            path.display(),
            bytes.len()
        ));
    }
    let img = image::load_from_memory(&bytes)
        .map_err(|e| format!("Failed to decode PNG {}: {e}", path.display()))?;
    if img.width() != expected_w || img.height() != expected_h {
        return Err(format!(
            "Image {} dimensions mismatch: expected {}x{}, got {}x{}",
            path.display(),
            expected_w,
            expected_h,
            img.width(),
            img.height()
        ));
    }

    let mut has_non_black = false;
    let rgb = img.to_rgb8();
    for pixel in rgb.pixels().step_by(10) {
        if pixel.0[0] > 10 || pixel.0[1] > 10 || pixel.0[2] > 10 {
            has_non_black = true;
            break;
        }
    }
    if !has_non_black {
        return Err(format!(
            "Image {} appears to be entirely black",
            path.display()
        ));
    }
    Ok(())
}

pub fn run_smoke_suite(out_dir: &Path, capture_png: bool) -> Result<(), String> {
    let _ = fs::create_dir_all(out_dir);

    let resolutions = [
        (1024, 640),
        (1180, 790),
        (1366, 768),
        (1536, 864),
        (1920, 1080),
    ];
    let themes = [Theme::Dark, Theme::Light];

    println!("=== Running Visual Smoke Test Suite ===");

    for theme in themes {
        let theme_name = match theme {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
            _ => "System",
        };

        for &(w, h) in &resolutions {
            let mut app = create_test_app(theme, Page::Overview);
            let svg = render_to_svg(&mut app, w as f32, h as f32);

            let svg_filename = format!("smoke_{theme_name}_{w}x{h}.svg");
            let svg_path = out_dir.join(&svg_filename);
            let _ = fs::write(&svg_path, &svg);

            println!("Rendered SVG: {svg_filename}");

            if capture_png {
                let edge_path = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
                if Path::new(edge_path).exists() {
                    let png_filename = format!("smoke_{theme_name}_{w}x{h}.png");
                    let png_path = out_dir.join(&png_filename);

                    let abs_svg_path = if svg_path.is_absolute() {
                        svg_path.clone()
                    } else {
                        std::env::current_dir().unwrap_or_default().join(&svg_path)
                    };
                    let abs_png_path = if png_path.is_absolute() {
                        png_path.clone()
                    } else {
                        std::env::current_dir().unwrap_or_default().join(&png_path)
                    };

                    let url = format!("file:///{}", abs_svg_path.display()).replace('\\', "/");
                    let screenshot_arg = format!("--screenshot={}", abs_png_path.display());

                    let status = std::process::Command::new(edge_path)
                        .args([
                            "--headless",
                            "--disable-gpu",
                            &format!("--window-size={w},{h}"),
                            &screenshot_arg,
                            &url,
                        ])
                        .status();

                    if status.map(|s| s.success()).unwrap_or(false) && png_path.exists() {
                        assert_image_validity(&png_path, w as u32, h as u32)?;
                        println!("  └── Captured & Verified PNG: {png_filename}");
                    }
                }
            }
        }
    }

    // View-specific screenshots at 1180x790 (Processes, Hardware, Benchmarks, Capabilities, Settings)
    let views = [
        (Page::Processes, "processes"),
        (Page::Hardware, "hardware"),
        (Page::Benchmarks, "benchmarks"),
        (Page::Capabilities, "capabilities"),
        (Page::Settings, "settings"),
    ];

    for (page, view_name) in views {
        for theme in [Theme::Dark, Theme::Light] {
            let theme_name = match theme {
                Theme::Dark => "Dark",
                _ => "Light",
            };
            let mut app = create_test_app(theme, page);
            let svg = render_to_svg(&mut app, 1180.0, 790.0);
            let svg_filename = format!("smoke_{view_name}_{theme_name}_1180x790.svg");
            let svg_path = out_dir.join(&svg_filename);
            let _ = fs::write(&svg_path, &svg);

            if capture_png {
                let edge_path = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
                if Path::new(edge_path).exists() {
                    let png_filename = format!("smoke_{view_name}_{theme_name}_1180x790.png");
                    let png_path = out_dir.join(&png_filename);

                    let abs_svg_path = if svg_path.is_absolute() {
                        svg_path.clone()
                    } else {
                        std::env::current_dir().unwrap_or_default().join(&svg_path)
                    };
                    let abs_png_path = if png_path.is_absolute() {
                        png_path.clone()
                    } else {
                        std::env::current_dir().unwrap_or_default().join(&png_path)
                    };

                    let url = format!("file:///{}", abs_svg_path.display()).replace('\\', "/");
                    let screenshot_arg = format!("--screenshot={}", abs_png_path.display());

                    let status = std::process::Command::new(edge_path)
                        .args([
                            "--headless",
                            "--disable-gpu",
                            "--window-size=1180,790",
                            &screenshot_arg,
                            &url,
                        ])
                        .status();

                    if status.map(|s| s.success()).unwrap_or(false) && png_path.exists() {
                        assert_image_validity(&png_path, 1180, 790)?;
                        println!("Rendered & Verified view PNG: {png_filename}");
                    }
                }
            }
        }
    }

    println!("=== Smoke Suite Completed Successfully! ===");
    Ok(())
}
