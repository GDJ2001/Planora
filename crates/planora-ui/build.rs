use std::{env, fs, path::PathBuf};

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=../../assets/brand/platforms/windows/planora.ico");
        println!("cargo:rerun-if-changed=src/planora.manifest");
        winresource::WindowsResource::new()
            .set_icon("../../assets/brand/platforms/windows/planora.ico")
            .set_manifest_file("src/planora.manifest")
            .set("FileDescription", "Planora hardware visualizer")
            .set("ProductName", "Planora")
            .compile()
            .expect("failed to embed Planora Windows icon and manifest");
    }
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/macos_authorize.m")
            .flag("-fobjc-arc")
            .compile("planora_authorize");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=ServiceManagement");
    }

    let path = PathBuf::from("../../assets/design-tokens.json");
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rerun-if-changed=../../assets/brand/ui");

    // Rebuild the displayed identity when switching branches or advancing HEAD.
    for git_path in ["HEAD", "refs", "packed-refs"] {
        if let Ok(output) = std::process::Command::new("git")
            .args(["rev-parse", "--git-path", git_path])
            .output()
            && output.status.success()
        {
            println!(
                "cargo:rerun-if-changed={}",
                String::from_utf8_lossy(&output.stdout).trim()
            );
        }
    }
    if let Ok(reference) = std::process::Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        && reference.status.success()
    {
        let reference = String::from_utf8_lossy(&reference.stdout);
        if let Ok(path) = std::process::Command::new("git")
            .args(["rev-parse", "--git-path", reference.trim()])
            .output()
            && path.status.success()
        {
            println!(
                "cargo:rerun-if-changed={}",
                String::from_utf8_lossy(&path.stdout).trim()
            );
        }
    }

    let git_commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "release".to_string());
    println!("cargo:rustc-env=PLANORA_GIT_COMMIT={}", git_commit);
    let rustc_cmd = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let rustc_version = std::process::Command::new(rustc_cmd)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "rustc 1.99.1".to_string());
    println!("cargo:rustc-env=PLANORA_RUSTC_VERSION={}", rustc_version);
    println!(
        "cargo:rustc-env=PLANORA_TARGET={}",
        env::var("TARGET").unwrap_or_else(|_| "unknown".to_string())
    );
    println!(
        "cargo:rustc-env=PLANORA_PROFILE={}",
        env::var("PROFILE").unwrap_or_else(|_| "debug".to_string())
    );

    let root: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut generated = String::new();

    // Component metrics
    let icon_size = root["component"]["icon"]["size"].as_f64().unwrap_or(24.0);
    let icon_stroke = root["component"]["icon"]["strokeWidth"]
        .as_f64()
        .unwrap_or(1.75);
    let chart_line_width = root["component"]["chart"]["lineWidth"]
        .as_f64()
        .unwrap_or(2.0);
    let chart_point_radius = root["component"]["chart"]["pointRadius"]
        .as_f64()
        .unwrap_or(3.0);

    generated.push_str(&format!("pub const ICON_SIZE: f32 = {icon_size:.1};\n"));
    generated.push_str(&format!(
        "pub const ICON_STROKE_WIDTH: f32 = {icon_stroke:.2};\n"
    ));
    generated.push_str(&format!(
        "pub const CHART_LINE_WIDTH: f32 = {chart_line_width:.1};\n"
    ));
    generated.push_str(&format!(
        "pub const CHART_POINT_RADIUS: f32 = {chart_point_radius:.1};\n\n"
    ));

    // Semantic colors for dark and light themes
    for theme in ["dark", "light"] {
        let section = &root["semantic"][theme];
        for key in [
            "background",
            "surface",
            "surfaceRaised",
            "text",
            "textSecondary",
            "borderSubtle",
            "borderControl",
            "hover",
            "pressed",
            "selected",
            "primary",
            "primaryHover",
            "accent",
            "focus",
            "success",
            "warning",
            "error",
            "cpu",
            "memory",
            "gpu",
            "disk",
            "network",
        ] {
            let raw = if ["cpu", "memory", "gpu", "disk", "network"].contains(&key) {
                &section["resource"][key]
            } else {
                &section[key]
            };
            let reference = match raw.as_str() {
                Some(r) => r,
                None => continue,
            };
            let mut token_path = &reference[1..reference.len() - 1];
            let mut value = &root;
            while let Some((part, rest)) = token_path.split_once('.') {
                value = &value[part];
                token_path = rest;
            }
            value = &value[token_path];
            let hex = value.as_str().unwrap().trim_start_matches('#');
            let bytes = (0..3)
                .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let mut identifier = String::new();
            for ch in key.chars() {
                if ch.is_uppercase() {
                    identifier.push('_');
                }
                identifier.push(ch.to_ascii_uppercase());
            }
            generated.push_str(&format!(
                "pub const {}_{}: egui::Color32 = egui::Color32::from_rgb({}, {}, {});\n",
                theme.to_uppercase(),
                identifier,
                bytes[0],
                bytes[1],
                bytes[2]
            ));
        }
        generated.push('\n');
    }

    // Embed approved brand UI icons from assets/brand/ui
    let icons = [
        ("overview", "planora-overview.svg"),
        ("processes", "planora-processes.svg"),
        ("cpu", "planora-cpu.svg"),
        ("memory", "planora-memory.svg"),
        ("gpu", "planora-gpu.svg"),
        ("disk", "planora-disk.svg"),
        ("network", "planora-network.svg"),
        ("users", "planora-users.svg"),
        ("settings", "planora-settings.svg"),
        ("search", "planora-search.svg"),
        ("refresh", "planora-refresh.svg"),
        ("pause", "planora-pause.svg"),
        ("check", "planora-check.svg"),
        ("warning", "planora-warning.svg"),
        ("error", "planora-error.svg"),
        ("chevron_down", "planora-chevron-down.svg"),
        ("chevron_left", "planora-chevron-left.svg"),
        ("chevron_right", "planora-chevron-right.svg"),
    ];

    let ui_icons_dir = PathBuf::from("../../assets/brand/ui");
    for (name, filename) in icons {
        let content = fs::read_to_string(ui_icons_dir.join(filename)).unwrap_or_default();
        generated.push_str(&format!(
            "pub const ICON_{}_SVG: &str = r#\"{content}\"#;\n",
            name.to_ascii_uppercase()
        ));
    }

    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("theme.rs"),
        generated,
    )
    .unwrap();
}
