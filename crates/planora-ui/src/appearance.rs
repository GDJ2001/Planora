//! Cached, event-driven OS preferences. No OS queries occur in the render callback.
use eframe::egui::{self, Color32};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemAppearance {
    pub text_scale: f32,
    pub animations: bool,
    /// Windows high contrast foreground and background, when enabled.
    pub contrast: Option<(Color32, Color32)>,
}

impl Default for SystemAppearance {
    fn default() -> Self {
        Self {
            text_scale: 1.0,
            animations: true,
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
        #[cfg(not(windows))]
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
