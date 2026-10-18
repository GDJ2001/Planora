use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke, Vec2};
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
pub mod tokens {
    use eframe::egui;
    include!(concat!(env!("OUT_DIR"), "/theme.rs"));
}

pub const SIDEBAR_WIDTH: f32 = 208.0;
pub const PAGE_PADDING: f32 = 24.0;
pub const PANEL_GAP: f32 = 16.0;
pub const CORNER_RADIUS: f32 = 8.0;
pub const CORNER_RADIUS_SM: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Density {
    Compact,
    #[default]
    Normal,
    Spacious,
}

impl Density {
    pub fn item_spacing(&self) -> Vec2 {
        match self {
            Self::Compact => Vec2::new(6.0, 4.0),
            Self::Normal => Vec2::new(8.0, 6.0),
            Self::Spacious => Vec2::new(12.0, 10.0),
        }
    }

    pub fn row_height(&self) -> f32 {
        match self {
            Self::Compact => 24.0,
            Self::Normal => 30.0,
            Self::Spacious => 38.0,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Normal => "Normal",
            Self::Spacious => "Spacious",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub is_dark: bool,
    pub background: Color32,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub border: Color32,
    pub border_control: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub primary: Color32,
    pub accent: Color32,
    pub focus: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub cpu: Color32,
    pub memory: Color32,
    pub gpu: Color32,
    pub disk: Color32,
    pub network: Color32,
}

pub fn theme_is_dark(theme: Theme, system: Option<egui::Theme>) -> bool {
    match theme {
        Theme::System => system != Some(egui::Theme::Light),
        Theme::Dark => true,
        Theme::Light => false,
    }
}

pub fn get_palette(is_dark: bool) -> Palette {
    if is_dark {
        Palette {
            is_dark: true,
            background: tokens::DARK_BACKGROUND,
            surface: tokens::DARK_SURFACE,
            surface_raised: Color32::from_rgb(25, 38, 59), // #19263B
            text: tokens::DARK_TEXT,
            secondary: tokens::DARK_TEXT_SECONDARY,
            border: tokens::DARK_BORDER_SUBTLE,
            border_control: Color32::from_rgb(82, 98, 122), // #52627A
            hover: Color32::from_rgb(25, 38, 59),
            selected: tokens::DARK_SELECTED,
            primary: tokens::DARK_PRIMARY,
            accent: tokens::DARK_ACCENT,
            focus: tokens::DARK_FOCUS,
            success: Color32::from_rgb(52, 211, 153), // #34D399
            warning: Color32::from_rgb(251, 191, 36), // #FBBF24
            error: Color32::from_rgb(248, 113, 113),  // #F87171
            cpu: tokens::DARK_CPU,
            memory: tokens::DARK_MEMORY,
            gpu: tokens::DARK_GPU,
            disk: tokens::DARK_DISK,
            network: tokens::DARK_NETWORK,
        }
    } else {
        Palette {
            is_dark: false,
            background: tokens::LIGHT_BACKGROUND,
            surface: tokens::LIGHT_SURFACE,
            surface_raised: Color32::from_rgb(233, 239, 247), // #E9EFF7
            text: tokens::LIGHT_TEXT,
            secondary: tokens::LIGHT_TEXT_SECONDARY,
            border: tokens::LIGHT_BORDER_SUBTLE,
            border_control: Color32::from_rgb(167, 180, 200), // #A7B4C8
            hover: Color32::from_rgb(233, 239, 247),
            selected: tokens::LIGHT_SELECTED,
            primary: tokens::LIGHT_PRIMARY,
            accent: tokens::LIGHT_ACCENT,
            focus: tokens::LIGHT_FOCUS,
            success: Color32::from_rgb(4, 120, 87), // #047857
            warning: Color32::from_rgb(180, 83, 9), // #B45309
            error: Color32::from_rgb(185, 28, 28),  // #B91C1C
            cpu: tokens::LIGHT_CPU,
            memory: tokens::LIGHT_MEMORY,
            gpu: tokens::LIGHT_GPU,
            disk: tokens::LIGHT_DISK,
            network: tokens::LIGHT_NETWORK,
        }
    }
}

pub fn apply_egui_theme(ctx: &egui::Context, is_dark: bool, density: Density) {
    let p = get_palette(is_dark);
    let mut visuals = if is_dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    visuals.override_text_color = Some(p.text);
    visuals.panel_fill = p.background;
    visuals.window_fill = p.surface;
    visuals.extreme_bg_color = p.background;

    // Restrained borders and 8px corners
    visuals.widgets.noninteractive.bg_fill = p.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(CORNER_RADIUS as u8);

    visuals.widgets.inactive.bg_fill = p.surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(CORNER_RADIUS as u8);

    visuals.widgets.hovered.bg_fill = p.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.focus);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(CORNER_RADIUS as u8);

    visuals.widgets.active.bg_fill = p.selected;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, p.primary);
    visuals.widgets.active.corner_radius = CornerRadius::same(CORNER_RADIUS as u8);

    visuals.widgets.open.bg_fill = p.surface_raised;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, p.border_control);
    visuals.widgets.open.corner_radius = CornerRadius::same(CORNER_RADIUS as u8);

    visuals.selection.bg_fill = p.selected;
    visuals.selection.stroke = Stroke::new(1.0, p.primary);

    ctx.set_visuals(visuals);

    let egui_theme = if is_dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.style_mut_of(egui_theme, |style| {
        style.spacing.item_spacing = density.item_spacing();
        style.spacing.window_margin = Margin::same(CORNER_RADIUS as i8);
    });
}
