use crate::assets::{Asset, AssetManager, Text};
use crate::actors::Drawable;
use crate::render::camera::Camera;
use ggez::{Context, GameResult};
use crate::util::{Point2};

#[derive(Debug)]
struct BaseWidget<T: Asset> {
    pub asset: T,
    pub pos: Point2,
    pub facing: f32,
}

pub trait Widget {

    fn position(&self) -> Point2;
    fn set_position(&mut self, pos: Point2);
    fn facing(&self) -> f32;
    fn set_facing(&mut self, facing: f32);
    fn width(&self, ctx: &mut Context) -> u32;
    fn height(&self, ctx: &mut Context) -> u32;
    fn half_width(&self, ctx: &mut Context) -> f32;
    fn half_height(&self, ctx: &mut Context) -> f32;
}

#[derive(Debug, Widget, Drawable)]
pub struct TextWidget {
    base: BaseWidget<Text>
}

impl TextWidget {

    pub fn new(ctx: &mut Context, am: &mut AssetManager, font_size: f32) -> GameResult<TextWidget> {
        Ok(TextWidget {
            base: BaseWidget {
                asset: am.make_text(ctx, "", "/DejaVuSerif.ttf", font_size)?,
                pos: Point2::origin(),
                facing: 0.0,
            }
        })
    }

    pub fn set_text(&mut self, ctx: &mut Context, am: &mut AssetManager, text: &str, size: f32) {
        am.update_text(ctx, &mut self.base.asset, text, size)
    }

}
