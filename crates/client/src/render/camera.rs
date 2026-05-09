//! Camera that maps the fixed-size, Y-up world into the screen.
//!
//! The world is `view_size` wide and tall (1280 × 540 in production) and
//! always centered on the screen. The scale is uniform — `min(screen.x /
//! view.x, screen.y / view.y)` — so resizing or going fullscreen
//! letterboxes instead of stretching. Y is flipped on the way out
//! because the world is Y-up but the screen is Y-down.

use ggez::glam::Vec2;

pub type Point2 = Vec2;

pub struct Camera {
    view_size: Vec2,
    screen_size: Vec2,
    /// Pixels per world unit. Uniform so the world stays square.
    scale: f32,
}

impl Camera {
    pub fn new(screen_w: f32, screen_h: f32, view_w: f32, view_h: f32) -> Self {
        let mut c = Camera {
            view_size: Vec2::new(view_w, view_h),
            screen_size: Vec2::new(screen_w, screen_h),
            scale: 1.0,
        };
        c.recompute_scale();
        c
    }

    pub fn set_screen_size(&mut self, w: f32, h: f32) {
        self.screen_size = Vec2::new(w, h);
        self.recompute_scale();
    }

    fn recompute_scale(&mut self) {
        let sx = self.screen_size.x / self.view_size.x;
        let sy = self.screen_size.y / self.view_size.y;
        self.scale = sx.min(sy);
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn view_size(&self) -> Vec2 {
        self.view_size
    }

    pub fn screen_size(&self) -> Vec2 {
        self.screen_size
    }

    /// World (Y-up) point → screen (Y-down) pixel position. The view is
    /// centered in the screen, so anything outside `view_size` lands in
    /// the letterbox bars and won't be visible.
    pub fn world_to_screen(&self, world: Point2) -> Point2 {
        let center = self.view_size * 0.5;
        let s = self.scale;
        Point2::new(
            (world.x - center.x) * s + self.screen_size.x * 0.5,
            (center.y - world.y) * s + self.screen_size.y * 0.5,
        )
    }
}
