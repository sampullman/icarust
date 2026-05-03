use crate::util::{Point2, Vector2};

pub struct Camera {
    world_size: Vector2,
    view_size: Vector2,
    pixels_per_unit: Vector2,
    view_center: Point2,
    y_limits: Option<(f32, f32)>,
    wrap_x: bool,
}

impl Camera {
    pub fn new(screen_width: u32, screen_height: u32, view_width: f32, view_height: f32) -> Self {
        let world_size = Vector2::new(screen_width as f32, screen_height as f32);
        let view_size = Vector2::new(view_width, view_height);
        Camera {
            world_size,
            view_size,
            pixels_per_unit: world_size / view_size,
            view_center: Point2::new(view_width / 2.0, view_height / 2.0),
            y_limits: None,
            wrap_x: false,
        }
    }

    pub fn set_y_limits(&mut self, limits: (f32, f32)) {
        self.y_limits = Some(limits);
    }

    pub fn set_drawable_size(&mut self, width: f32, height: f32) {
        self.world_size = Vector2::new(width, height);
        self.pixels_per_unit = self.world_size / self.view_size;
    }

    pub fn set_x_wrap(&mut self, wrap: bool) {
        self.wrap_x = wrap;
    }

    pub fn move_by(&mut self, by: Vector2) {
        let to = self.view_center + by;
        self.move_to(to);
    }

    pub fn move_to(&mut self, to: Point2) {
        self.view_center = to;
        if let Some(y_limits) = self.y_limits {
            if self.view_center.y > y_limits.1 {
                self.view_center.y = y_limits.1;
            } else if self.view_center.y < y_limits.0 {
                self.view_center.y = y_limits.0;
            }
        }
    }

    pub fn world_width(&self) -> f32 {
        self.world_size.x
    }

    pub fn world_height(&self) -> f32 {
        self.world_size.y
    }

    /// Translates a world point to a screen point
    pub fn world_to_screen_coords(&self, from: Point2) -> Point2 {
        let view_offset = from - self.view_center;
        let view_scale = view_offset * self.pixels_per_unit;
        let x = view_scale.x + self.world_size.x / 2.0;
        let y = self.world_size.y - (view_scale.y + self.world_size.y / 2.0);
        Point2::new(x, y)
    }

    pub fn static_world_to_screen_coords(&self, from: Point2) -> Point2 {
        let y = self.world_size.y - from.y;
        Point2::new(from.x, y)
    }

    pub fn screen_to_world_coords(&self, from: (i32, i32)) -> Point2 {
        let (sx, sy) = from;
        let sx = sx as f32;
        let sy = sy as f32;
        let flipped_x = sx - (self.world_size.x / 2.0);
        let flipped_y = -sy + self.world_size.y / 2.0;
        let screen_coords = Vector2::new(flipped_x, flipped_y);
        let units_per_pixel = self.view_size / self.world_size;
        let view_scale = screen_coords * units_per_pixel;
        self.view_center + view_scale
    }

    pub fn location(&self) -> Point2 {
        self.view_center
    }
}
