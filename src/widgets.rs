
#[derive(Debug)]
pub struct BaseWidget<T: Asset> {
    pub asset: T,
    pub pos: Point2,
    pub facing: f32,
}

trait Widget {

    fn draw(&self, ctx: &mut Context, world_coords: (u32, u32));
}

#[derive(Widget)]
struct TextWidget {
    base: BaseWidget
}