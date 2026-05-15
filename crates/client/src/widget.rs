use crate::assets::{AssetManager, TextAsset};
use crate::render::camera::Point2;
use ggez::graphics::{Canvas, Color, DrawParam};
use ggez::{Context, GameResult};

/// HUD text positioned in screen-pixel coordinates.
pub struct TextWidget {
    pub asset: TextAsset,
    pub pos: Point2,
    pub size: f32,
}

impl TextWidget {
    pub fn new(_ctx: &mut Context, am: &mut AssetManager, font_size: f32) -> GameResult<TextWidget> {
        let font = am.ensure_default_font(_ctx)?;
        Ok(TextWidget {
            asset: TextAsset::new(font, "", font_size),
            pos: Point2::ZERO,
            size: font_size,
        })
    }

    pub fn set_text(&mut self, text: &str, size: f32) {
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

    pub fn draw(&self, canvas: &mut Canvas) {
        self.draw_with(canvas, Color::WHITE);
    }

    pub fn draw_with(&self, canvas: &mut Canvas, color: Color) {
        canvas.draw(&self.asset.text, DrawParam::new().dest(self.pos).color(color));
    }
}
