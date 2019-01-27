
use ggez::{audio, Context, GameResult, graphics};
use ggez::graphics::{Image, Font};
use crate::render::camera::{CameraDraw};
use std::collections::HashMap;
use std::rc::Rc;

pub type SoundId = usize;

pub struct AssetManager {
    image_cache: HashMap<String, Rc<Image>>,
    font_cache: HashMap<String, Rc<Font>>,
    sound_cache: Vec<Rc<audio::Source>>,
}

impl AssetManager {

    pub fn new() -> Self {
        AssetManager {
            image_cache: HashMap::new(),
            font_cache: HashMap::new(),
            sound_cache: Vec::new(), }
    }

    pub fn add_sound(&mut self, ctx: &mut Context, path: &str) -> SoundId {
        self.sound_cache.push(Rc::new(audio::Source::new(ctx, path).unwrap()));
        self.sound_cache.len() - 1
    }

    pub fn get_sound(&self, id: SoundId) -> Rc<audio::Source> {
        self.sound_cache[id].clone()
    }

    pub fn get_image(&mut self, ctx: &mut Context, file: &str) -> Rc<Image> {
        {
            if let Some(image) = self.image_cache.get(file) {
                return image.clone()
            }
        }

        let new_image = Rc::new(Image::new(ctx, file).unwrap());
        self.image_cache.insert(file.to_string(), new_image.clone());
        new_image
    }

    pub fn make_sprite(&mut self, ctx: &mut Context, file: &str) -> Sprite {
        Sprite { image: self.get_image(ctx, file) }
    }

    pub fn get_font(&mut self, ctx: &mut Context, key: &str) -> GameResult<Rc<Font>> {
        {
            if let Some(font) = self.font_cache.get(key) {
                return Ok(font.clone())
            }
        }

        let new_font = Rc::new(Font::new(ctx, "/DejaVuSerif.ttf", 18)?);
        self.font_cache.insert(key.to_string(), new_font.clone());
        Ok(new_font.clone())
    }

    pub fn make_text(&mut self, ctx: &mut Context, text: &str, file: &str, size: u32) -> GameResult<Text> {

        let key = format!("{}_{}", file, size);
        let font = self.get_font(ctx, &key)?;
        Ok(Text { text: graphics::Text::new(ctx, text, &font)?, font_key: key })
    }

    pub fn update_text(&mut self, ctx: &mut Context, text: &mut Text, new_str: &str) {
        let font = self.get_font(ctx, &text.font_key).unwrap();
        text.text = graphics::Text::new(ctx, new_str, &font).unwrap()

    }
}

pub trait Asset {
    fn drawable(&self) -> &CameraDraw;

    fn width(&self) -> u32;
    fn height(&self) -> u32;

    fn half_width(&self) -> f32 {
        (self.width() as f32) / 2.0
    }
    fn half_height(&self) -> f32 {
        (self.height() as f32) / 2.0
    }
    fn is_static(&self) -> bool {
        false
    }
}

#[derive(Debug)]
pub struct Sprite {
    image: Rc<Image>,
}

impl Asset for Sprite {

    fn drawable(&self) -> &CameraDraw {
        &*self.image
    }
    
    fn width(&self) -> u32 {
        self.image.width()
    }
    fn height(&self) -> u32 {
        self.image.height()
    }
}

#[derive(Debug)]
pub struct Text {
    pub text: graphics::Text,
    pub font_key: String,
}

impl Asset for Text {

    fn drawable(&self) -> &CameraDraw {
        &self.text
    }
    fn is_static(&self) -> bool {
        true
    }
    fn width(&self) -> u32 {
        self.text.width()
    }
    fn height(&self) -> u32 {
        self.text.height()
    }
}
