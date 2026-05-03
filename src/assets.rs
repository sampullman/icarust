use ggez::audio::{self, SoundSource};
use ggez::graphics::{self, FontData, Image};
use ggez::{Context, GameResult};
use std::collections::HashMap;

pub type SoundId = usize;

const DEFAULT_FONT: &str = "DejaVuSerif";
const DEFAULT_FONT_PATH: &str = "/DejaVuSerif.ttf";

pub struct AssetManager {
    image_cache: HashMap<String, Image>,
    sound_cache: Vec<audio::Source>,
    fonts_loaded: bool,
}

impl AssetManager {
    pub fn new() -> Self {
        AssetManager {
            image_cache: HashMap::new(),
            sound_cache: Vec::new(),
            fonts_loaded: false,
        }
    }

    pub fn add_sound(&mut self, ctx: &mut Context, path: &str) -> SoundId {
        self.sound_cache
            .push(audio::Source::new(ctx, path).expect("failed to load sound"));
        self.sound_cache.len() - 1
    }

    pub fn play_sound(&self, id: SoundId) {
        self.sound_cache[id].play();
    }

    pub fn get_image(&mut self, ctx: &mut Context, file: &str) -> Image {
        if let Some(image) = self.image_cache.get(file) {
            return image.clone();
        }
        let new_image = Image::from_path(ctx, file).expect("failed to load image");
        self.image_cache.insert(file.to_string(), new_image.clone());
        new_image
    }

    pub fn make_sprite(&mut self, ctx: &mut Context, file: &str) -> Sprite {
        Sprite {
            image: self.get_image(ctx, file),
        }
    }

    pub fn ensure_default_font(&mut self, ctx: &mut Context) -> GameResult<&'static str> {
        if !self.fonts_loaded {
            let font = FontData::from_path(ctx, DEFAULT_FONT_PATH)?;
            ctx.gfx.add_font(DEFAULT_FONT, font);
            self.fonts_loaded = true;
        }
        Ok(DEFAULT_FONT)
    }
}

#[derive(Debug, Clone)]
pub struct Sprite {
    pub image: Image,
}

impl Sprite {
    pub fn width(&self) -> f32 {
        self.image.width() as f32
    }
    pub fn height(&self) -> f32 {
        self.image.height() as f32
    }
    pub fn half_width(&self) -> f32 {
        self.width() / 2.0
    }
    pub fn half_height(&self) -> f32 {
        self.height() / 2.0
    }
}

pub struct TextAsset {
    pub text: graphics::Text,
}

impl TextAsset {
    pub fn new(font: &str, contents: &str, size: f32) -> Self {
        let mut text = graphics::Text::new(contents);
        text.set_font(font).set_scale(size);
        TextAsset { text }
    }

    pub fn set_text(&mut self, contents: &str, size: f32) {
        self.text.clear();
        self.text.add(contents);
        self.text.set_scale(size);
    }

    pub fn measure(&self, ctx: &Context) -> (f32, f32) {
        let bounds = self
            .text
            .measure(ctx)
            .unwrap_or(ggez::glam::Vec2::splat(1.0).into());
        (bounds.x, bounds.y)
    }
}
