//! Application-owned window geometry. Eframe's legacy window record is deliberately
//! kept separate from Planora preferences, so a bad record cannot shrink startup.
use eframe::egui::{self, Pos2, Vec2};
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};

pub const DEFAULT_SIZE: [f32; 2] = [1180.0, 790.0];
pub const MIN_SIZE: [f32; 2] = [1024.0, 640.0];
const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub version: u32,
    pub position: Option<[f32; 2]>,
    pub normal_size: [f32; 2],
    pub maximized: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            version: VERSION,
            position: None,
            normal_size: DEFAULT_SIZE,
            maximized: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WorkArea {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl WorkArea {
    fn size(self) -> [f32; 2] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }
}

impl Placement {
    pub fn validate(self, work: WorkArea) -> Self {
        let size = work.size();
        let min = [MIN_SIZE[0].min(size[0]), MIN_SIZE[1].min(size[1])];
        let mut result = self;
        if self.version != VERSION
            || size.iter().any(|v| !v.is_finite() || *v < 320.0)
            || self.normal_size.iter().any(|v| !v.is_finite())
            || self.normal_size[0] < min[0]
            || self.normal_size[1] < min[1]
        {
            result = Self::default();
        }
        result.normal_size = [
            result.normal_size[0].clamp(min[0], size[0].max(min[0])),
            result.normal_size[1].clamp(min[1], size[1].max(min[1])),
        ];
        result.position = result.position.filter(|p| {
            p.iter().all(|v| v.is_finite())
                && p[0] + result.normal_size[0] > work.min[0] + 80.0
                && p[1] + result.normal_size[1] > work.min[1] + 40.0
                && p[0] < work.max[0] - 80.0
                && p[1] < work.max[1] - 40.0
        });
        result
    }

    pub fn viewport(self, work: WorkArea) -> egui::ViewportBuilder {
        let valid = self.validate(work);
        let size = valid.normal_size;
        let mut builder = egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size([
                MIN_SIZE[0].min(work.size()[0]),
                MIN_SIZE[1].min(work.size()[1]),
            ])
            .with_maximized(valid.maximized)
            .with_clamp_size_to_monitor_size(true)
            .with_title("Planora");
        if let Some(position) = valid.position {
            builder = builder.with_position(Pos2::from(position));
        }
        builder
    }

    pub fn observe(&mut self, viewport: &egui::ViewportInfo, work: WorkArea) {
        if viewport.minimized == Some(true) || viewport.fullscreen == Some(true) {
            return;
        }
        if viewport.maximized == Some(true) {
            self.maximized = true;
            return;
        }
        let Some(inner) = viewport.inner_rect else {
            return;
        };
        let size = inner.size();
        let proposed = Self {
            version: VERSION,
            position: viewport.outer_rect.map(|r| [r.min.x, r.min.y]),
            normal_size: [size.x, size.y],
            maximized: false,
        };
        // Reject transient first frames and minimize animations rather than
        // overwriting the last known good normal rectangle.
        if proposed.normal_size[0] >= MIN_SIZE[0].min(work.size()[0])
            && proposed.normal_size[1] >= MIN_SIZE[1].min(work.size()[1])
            && proposed.normal_size.iter().all(|v| v.is_finite())
        {
            *self = proposed.validate(work);
        }
    }

    pub fn expected_size(self) -> Vec2 {
        self.normal_size.into()
    }
}

pub fn path() -> Option<PathBuf> {
    super::state::UserSettings::file_path().map(|p| p.with_file_name("window-placement.json"))
}

pub fn load(work: WorkArea) -> Placement {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|data| serde_json::from_str::<Placement>(&data).ok())
        .unwrap_or_default()
        .validate(work)
}

pub fn save(placement: &Placement) -> io::Result<()> {
    let target = path()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "settings directory unavailable"))?;
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid placement path"))?;
    std::fs::create_dir_all(parent)?;
    let temp = target.with_file_name(format!("window-placement.{}.tmp", std::process::id()));
    let result = (|| -> io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(placement).map_err(io::Error::other)?)?;
        file.sync_all()?;
        super::state::replace_settings_file(&temp, &target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(windows)]
pub fn current_work_area() -> WorkArea {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::{
            HiDpi::GetDpiForSystem,
            WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW},
        },
    };
    let mut rect = RECT::default();
    let ok =
        unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rect as *mut _ as *mut _, 0) } != 0;
    let scale = (unsafe { GetDpiForSystem() }.max(96) as f32) / 96.0;
    if ok && rect.right > rect.left && rect.bottom > rect.top {
        WorkArea {
            min: [rect.left as f32 / scale, rect.top as f32 / scale],
            max: [rect.right as f32 / scale, rect.bottom as f32 / scale],
        }
    } else {
        WorkArea {
            min: [0.0, 0.0],
            max: [1180.0, 790.0],
        }
    }
}

#[cfg(not(windows))]
pub fn current_work_area() -> WorkArea {
    WorkArea {
        min: [0.0, 0.0],
        max: [1920.0, 1080.0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn work() -> WorkArea {
        WorkArea {
            min: [0.0, 0.0],
            max: [1536.0, 824.0],
        }
    }

    #[test]
    fn rejects_tiny_corrupt_and_offscreen_geometry() {
        let tiny = Placement {
            normal_size: [70.0, 90.0],
            ..Placement::default()
        };
        assert_eq!(tiny.validate(work()).normal_size, DEFAULT_SIZE);
        let bad = Placement {
            normal_size: [f32::NAN, 640.0],
            ..Placement::default()
        };
        assert_eq!(bad.validate(work()).normal_size, DEFAULT_SIZE);
        let offscreen = Placement {
            position: Some([6000.0, 6000.0]),
            ..Placement::default()
        };
        assert_eq!(offscreen.validate(work()).position, None);
    }

    #[test]
    fn smaller_screen_clamps_size_without_losing_maximized_intent() {
        let area = WorkArea {
            min: [0.0, 0.0],
            max: [900.0, 600.0],
        };
        let saved = Placement {
            maximized: true,
            ..Placement::default()
        }
        .validate(area);
        assert_eq!(saved.normal_size, [900.0, 600.0]);
        assert!(saved.maximized);
    }
}
