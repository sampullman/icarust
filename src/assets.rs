
use ggez::{audio, Context, GameResult, graphics};
use ggez::graphics::{Image, Font};
use ncollide2d::world::CollisionObjectHandle;
use crate::render::camera::{CameraDraw};
use std::collections::HashMap;
use std::rc::Rc;

pub type SoundId = usize;

pub struct AssetManager {
    image_cache: HashMap<String, Rc<Image>>,
    font_cache: HashMap<String, Font>,
    sound_cache: Vec<Rc<audio::Source>>,
    id_gen: usize,
}

impl AssetManager {

    pub fn new() -> Self {
        AssetManager {
            image_cache: HashMap::new(),
            font_cache: HashMap::new(),
            sound_cache: Vec::new(),
            id_gen: 0,
        }
    }

    pub fn next_physics_id(&mut self) -> CollisionObjectHandle {
        self.id_gen += 1;
        CollisionObjectHandle(self.id_gen)
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

    pub fn get_font(&mut self, ctx: &mut Context, key: &str) -> GameResult<Font> {
        {
            if let Some(font) = self.font_cache.get(key) {
                return Ok(font.clone())
            }
        }

        let new_font = Font::new(ctx, "/DejaVuSerif.ttf")?;
        self.font_cache.insert(key.to_string(), new_font.clone());
        Ok(new_font.clone())
    }

    pub fn make_text(&mut self, ctx: &mut Context, text: &str, file: &str, size: f32) -> GameResult<Text> {

        let key = format!("{}", file);
        let font = self.get_font(ctx, &key)?;
        Ok(Text { text: graphics::Text::new((text, font, size)), font_key: key })
    }

    pub fn update_text(&mut self, ctx: &mut Context, text: &mut Text, new_str: &str, size: f32) {
        let font = self.get_font(ctx, &text.font_key).unwrap();
        text.text = graphics::Text::new((new_str, font, size))
    }
}

pub trait Asset {
    fn drawable(&self) -> &dyn CameraDraw;

    fn width(&self, ctx: &mut Context) -> u32;
    fn height(&self, ctx: &mut Context) -> u32;

    fn half_width(&self, ctx: &mut Context) -> f32 {
        (self.width(ctx) as f32) / 2.0
    }
    fn half_height(&self, ctx: &mut Context) -> f32 {
        (self.height(ctx) as f32) / 2.0
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

    fn drawable(&self) -> &dyn CameraDraw {
        &*self.image
    }
    
    fn width(&self, ctx: &mut Context) -> u32 {
        self.image.width().into()
    }
    fn height(&self, ctx: &mut Context) -> u32 {
        self.image.height().into()
    }
}

#[derive(Debug)]
pub struct Text {
    pub text: graphics::Text,
    pub font_key: String,
}

impl Asset for Text {

    fn drawable(&self) -> &dyn CameraDraw {
        &self.text
    }
    fn is_static(&self) -> bool {
        true
    }
    fn width(&self, ctx: &mut Context) -> u32 {
        self.text.width(ctx)
    }
    fn height(&self, ctx: &mut Context) -> u32 {
        self.text.height(ctx)
    }
}
