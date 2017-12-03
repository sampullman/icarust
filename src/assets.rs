
use ggez::Context;
use ggez::graphics::{Image, Point2};
use std::collections::HashMap;
use util;
use std::rc::Rc;

pub struct AssetManager {
    image_cache: HashMap<String, Rc<Image>>,
}

impl AssetManager {

    pub fn new() -> Self {
        AssetManager { image_cache: HashMap::new() }
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
        util::draw_image(ctx, &*self.image, position, facing, world_coords);
    }
}