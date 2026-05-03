use crate::assets::{AssetManager, TextAsset};
use crate::render::camera::Camera;
use crate::util::Point2;
use ggez::graphics::{Canvas, Color, DrawParam};
use ggez::{Context, GameResult};

pub struct TextWidget {
    pub asset: TextAsset,
    pub pos: Point2,
    pub size: f32,
}

impl TextWidget {
    pub fn new(ctx: &mut Context, am: &mut AssetManager, font_size: f32) -> GameResult<TextWidget> {
        let font = am.ensure_default_font(ctx)?;
        Ok(TextWidget {
            asset: TextAsset::new(font, "", font_size),
            pos: Point2::ZERO,
            size: font_size,
        })
    }

    pub fn set_text(
        &mut self,
        _ctx: &mut Context,
        _am: &mut AssetManager,
        text: &str,
        size: f32,
    ) {
        self.asset.set_text(text, size);
        self.size = size;
    }

    pub fn set_position(&mut self, pos: Point2) {
        self.pos = pos;
    }

    pub fn width(&self, ctx: &Context) -> f32 {
        self.asset.measure(ctx).0
    }

    pub fn height(&self, ctx: &Context) -> f32 {
        self.asset.measure(ctx).1
    }

    pub fn half_width(&self, ctx: &Context) -> f32 {
        self.width(ctx) / 2.0
    }

    pub fn half_height(&self, ctx: &Context) -> f32 {
        self.height(ctx) / 2.0
    }

    pub fn draw(&self, canvas: &mut Canvas, camera: &Camera) {
        let screen_pos = camera.static_world_to_screen_coords(self.pos);
        canvas.draw(
            &self.asset.text,
            DrawParam::new().dest(screen_pos).color(Color::WHITE),
        );
    }
}
