use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions, Vec2};

const LOGO_DARK_BYTES: &[u8] =
    include_bytes!("../../../../assets/brand/png/planora-horizontal-dark.png");
const LOGO_LIGHT_BYTES: &[u8] =
    include_bytes!("../../../../assets/brand/png/planora-horizontal-light.png");
const MARK_BYTES: &[u8] = include_bytes!("../../../../assets/brand/png/planora-mark.png");

#[derive(Default)]
pub struct LogoCache {
    dark_texture: Option<TextureHandle>,
    light_texture: Option<TextureHandle>,
    mark_texture: Option<TextureHandle>,
}

impl LogoCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn load_image(bytes: &[u8]) -> Option<ColorImage> {
        let mut img = image::load_from_memory(bytes).ok()?.to_rgba8();
        if img.width() > 1024 {
            let target_w = 1024;
            let target_h = (img.height() as f32 * (1024.0 / img.width() as f32)) as u32;
            img = image::imageops::resize(
                &img,
                target_w,
                target_h,
                image::imageops::FilterType::Lanczos3,
            );
        }
        let size = [img.width() as usize, img.height() as usize];
        let pixels = img.into_raw();
        Some(ColorImage::from_rgba_unmultiplied(size, &pixels))
    }

    pub fn render(&mut self, ui: &mut egui::Ui, is_dark: bool, target_width: f32) {
        let texture = if is_dark {
            if self.dark_texture.is_none()
                && let Some(ci) = Self::load_image(LOGO_DARK_BYTES)
            {
                self.dark_texture = Some(ui.ctx().load_texture(
                    "planora_logo_dark",
                    ci,
                    TextureOptions::LINEAR,
                ));
            }
            self.dark_texture.as_ref()
        } else {
            if self.light_texture.is_none()
                && let Some(ci) = Self::load_image(LOGO_LIGHT_BYTES)
            {
                self.light_texture = Some(ui.ctx().load_texture(
                    "planora_logo_light",
                    ci,
                    TextureOptions::LINEAR,
                ));
            }
            self.light_texture.as_ref()
        };

        if let Some(tex) = texture {
            let aspect = tex.size()[1] as f32 / tex.size()[0] as f32;
            let target_height = target_width * aspect;
            ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::new(target_width, target_height)));
        } else {
            // Fallback text if image decoding fails
            ui.heading("◉ Planora");
        }
    }

    pub fn render_mark(&mut self, ui: &mut egui::Ui, size: f32) {
        if self.mark_texture.is_none()
            && let Some(image) = Self::load_image(MARK_BYTES)
        {
            self.mark_texture = Some(ui.ctx().load_texture(
                "planora_mark",
                image,
                TextureOptions::LINEAR,
            ));
        }
        if let Some(texture) = &self.mark_texture {
            ui.add(egui::Image::new(texture).fit_to_exact_size(Vec2::splat(size)));
        }
    }
}
