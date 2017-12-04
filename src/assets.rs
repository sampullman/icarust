
use ggez::{Context, GameResult, graphics};
use ggez::graphics::{Image, Font, Point2};
use std::collections::HashMap;
use util;
use std::rc::Rc;

pub struct AssetManager {
    image_cache: HashMap<String, Rc<Image>>,
    font_cache: HashMap<String, Rc<Font>>,
}

impl AssetManager {

    pub fn new() -> Self {
        AssetManager {
            image_cache: HashMap::new(),
            font_cache: HashMap::new(), }
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
    fn draw(&self, ctx: &mut Context, world_coords: (u32, u32), position: Point2, facing: f32);
}

#[derive(Debug)]
pub struct Sprite {
    image: Rc<Image>,
}

impl Asset for Sprite {

    fn draw(&self, ctx: &mut Context, world_coords: (u32, u32), position: Point2, facing: f32) {
        util::draw_image(ctx, &*self.image, position, facing, world_coords).unwrap();
    }
}

#[derive(Debug)]
pub struct Text {
    pub text: graphics::Text,
    pub font_key: String,
}

impl Asset for Text {

    fn draw(&self, ctx: &mut Context, world_coords: (u32, u32), position: Point2, facing: f32) {
        util::draw_image(ctx, &self.text, position, facing, world_coords).unwrap();
    }
}
