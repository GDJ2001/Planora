//! Cached OS preferences. No OS queries occur in the render callback.
use eframe::egui::{self, Color32};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemAppearance {
    pub text_scale: f32,
    pub animations: bool,
    pub preferred_dark: Option<bool>,
    /// High contrast foreground and background, when enabled.
    pub contrast: Option<(Color32, Color32)>,
}

#[cfg(target_os = "linux")]
fn setting_output(program: &str, args: &[&str], stop: &AtomicBool) -> Option<String> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    if !std::path::Path::new(program).is_file() {
        return None;
    }
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(800);
    loop {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let output = child.wait_with_output().ok()?;
                return String::from_utf8(output.stdout)
                    .ok()
                    .map(|s| s.trim().to_owned());
            }
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
}

#[cfg(target_os = "linux")]
fn portal_color(value: &str) -> Option<bool> {
    if value.contains("uint32 1") {
        Some(true)
    } else if value.contains("uint32 2") {
        Some(false)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn watch_linux(ctx: egui::Context, snapshot: Arc<Mutex<SystemAppearance>>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        let read_gnome = |schema: &str, key: &str| {
            setting_output("/usr/bin/gsettings", &["get", schema, key], &stop)
        };
        let portal = setting_output(
            "/usr/bin/gdbus",
            &[
                "call",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
                "--method",
                "org.freedesktop.portal.Settings.Read",
                "org.freedesktop.appearance",
                "color-scheme",
            ],
            &stop,
        )
        .and_then(|value| portal_color(&value));
        let gnome_color =
            read_gnome("org.gnome.desktop.interface", "color-scheme").and_then(|value| {
                if value.contains("prefer-dark") {
                    Some(true)
                } else if value.contains("prefer-light") {
                    Some(false)
                } else {
                    None
                }
            });
        let kde_color = std::env::var_os("HOME")
            .and_then(|home| {
                std::fs::read_to_string(std::path::PathBuf::from(home).join(".config/kdeglobals"))
                    .ok()
            })
            .and_then(|file| {
                file.lines()
                    .find_map(|line| line.strip_prefix("ColorScheme="))
                    .map(|scheme| scheme.to_ascii_lowercase().contains("dark"))
            });
        let text_scale = read_gnome("org.gnome.desktop.interface", "text-scaling-factor")
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(1.0)
            .clamp(1.0, 3.0);
        let animations = read_gnome("org.gnome.desktop.interface", "enable-animations")
            .and_then(|value| value.parse::<bool>().ok())
            .unwrap_or(true);
        let contrast_enabled = read_gnome("org.gnome.desktop.interface", "gtk-theme")
            .is_some_and(|value| value.to_ascii_lowercase().contains("highcontrast"));
        let next = SystemAppearance {
            text_scale,
            animations,
            preferred_dark: portal.or(gnome_color).or(kde_color),
            contrast: contrast_enabled.then_some((Color32::WHITE, Color32::BLACK)),
        };
        let mut current = snapshot.lock().unwrap_or_else(|poison| poison.into_inner());
        if *current != next {
            *current = next;
            ctx.request_repaint();
        }
        drop(current);
        if stop.load(Ordering::Acquire) {
            break;
        }
        std::thread::park_timeout(std::time::Duration::from_secs(2));
    }
}

impl Default for SystemAppearance {
    fn default() -> Self {
        Self {
            text_scale: 1.0,
            animations: true,
            preferred_dark: None,
            contrast: None,
        }
    }
}

#[derive(Default)]
pub struct AppearanceMonitor {
    snapshot: Arc<Mutex<SystemAppearance>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl AppearanceMonitor {
    pub fn start(ctx: egui::Context) -> Self {
        let mut monitor = Self::default();
        #[cfg(windows)]
        {
            let snapshot = monitor.snapshot.clone();
            let stop = monitor.stop.clone();
            monitor.worker = std::thread::Builder::new()
                .name("planora-appearance".into())
                .spawn(move || watch_windows(ctx, snapshot, stop))
                .ok();
        }
        #[cfg(target_os = "linux")]
        {
            let snapshot = monitor.snapshot.clone();
            let stop = monitor.stop.clone();
            monitor.worker = std::thread::Builder::new()
                .name("planora-appearance".into())
                .spawn(move || watch_linux(ctx, snapshot, stop))
                .ok();
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = (&mut monitor, ctx);
        monitor
    }

    pub fn snapshot(&self) -> SystemAppearance {
        *self.snapshot.lock().unwrap_or_else(|e| e.into_inner())
    }

    // Only used by isolated fixtures; normal windows read Windows settings.
    pub fn set_fixture_snapshot(&self, value: SystemAppearance) {
        *self.snapshot.lock().unwrap() = value;
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(|w| w.is_finished())
    }

    pub fn join(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for AppearanceMonitor {
    fn drop(&mut self) {
        self.request_stop();
        self.join();
    }
}

#[cfg(windows)]
fn watch_windows(
    ctx: egui::Context,
    snapshot: Arc<Mutex<SystemAppearance>>,
    stop: Arc<AtomicBool>,
) {
    use windows::{
        Foundation::TypedEventHandler,
        UI::ViewManagement::{AccessibilitySettings, UISettings},
        Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    };
    if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_err() {
        crate::startup::record("appearance: Windows Runtime initialization failed");
        return;
    }
    // Drop subscriptions and COM objects before balancing RoInitialize.
    {
        let settings = UISettings::new();
        let accessibility = AccessibilitySettings::new();
        if let (Ok(settings), Ok(accessibility)) = (settings, accessibility) {
            let owner = std::thread::current();
            let owner2 = owner.clone();
            let handler = TypedEventHandler::new(move |_, _| {
                owner2.unpark();
                Ok(())
            });
            let animation_owner = owner.clone();
            let animation_handler = TypedEventHandler::new(move |_, _| {
                animation_owner.unpark();
                Ok(())
            });
            let hc_handler = TypedEventHandler::new(move |_, _| {
                owner.unpark();
                Ok(())
            });
            let text = settings.TextScaleFactorChanged(&handler).ok();
            let colors = settings.ColorValuesChanged(&handler).ok();
            let animations = settings.AnimationsEnabledChanged(&animation_handler).ok();
            let contrast = accessibility.HighContrastChanged(&hc_handler).ok();
            loop {
                let high_contrast = accessibility.HighContrast().unwrap_or(false);
                let next = SystemAppearance {
                    text_scale: settings
                        .TextScaleFactor()
                        .ok()
                        .filter(|scale| scale.is_finite())
                        .unwrap_or(1.0)
                        .clamp(1.0, 3.0) as f32,
                    animations: settings.AnimationsEnabled().unwrap_or(false),
                    preferred_dark: None,
                    contrast: high_contrast.then(|| {
                        use windows_sys::Win32::Graphics::Gdi::{
                            COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor,
                        };
                        let color =
                            |v: u32| Color32::from_rgb(v as u8, (v >> 8) as u8, (v >> 16) as u8);
                        unsafe {
                            (
                                color(GetSysColor(COLOR_WINDOWTEXT)),
                                color(GetSysColor(COLOR_WINDOW)),
                            )
                        }
                    }),
                };
                let mut current = snapshot.lock().unwrap_or_else(|e| e.into_inner());
                if *current != next {
                    *current = next;
                    ctx.request_repaint();
                }
                drop(current);
                if stop.load(Ordering::Acquire) {
                    break;
                }
                // Event-driven in supported Windows builds; bounded fallback for older event interfaces.
                std::thread::park_timeout(std::time::Duration::from_secs(30));
                if stop.load(Ordering::Acquire) {
                    break;
                }
            }
            if let Some(token) = text {
                let _ = settings.RemoveTextScaleFactorChanged(token);
            }
            if let Some(token) = colors {
                let _ = settings.RemoveColorValuesChanged(token);
            }
            if let Some(token) = animations {
                let _ = settings.RemoveAnimationsEnabledChanged(token);
            }
            if let Some(token) = contrast {
                let _ = accessibility.RemoveHighContrastChanged(token);
            }
        } else {
            crate::startup::record(
                "appearance: Windows settings unavailable; using native theme and DPI",
            );
        }
    }
    unsafe {
        RoUninitialize();
    }
}

pub fn text_scale(ctx: &egui::Context) -> f32 {
    ctx.data(|data| data.get_temp::<f32>(egui::Id::unique("system_text_scale")))
        .unwrap_or(1.0)
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::portal_color;
    #[test]
    fn portal_color_does_not_guess_unknown_or_no_preference() {
        assert_eq!(portal_color("(<uint32 1>,)"), Some(true));
        assert_eq!(portal_color("(<uint32 2>,)"), Some(false));
        assert_eq!(portal_color("(<uint32 0>,)"), None);
        assert_eq!(portal_color("not a portal response"), None);
    }
}
