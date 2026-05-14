//! Camera that maps the wide, Y-up world into a smaller scrolling view.
//!
//! The world is `WORLD_WIDTH × WORLD_HEIGHT` (3200 × 540 in production)
//! but only a `VIEW_WIDTH × VIEW_HEIGHT` chunk (1280 × 540) is drawn at
//! a time. The camera tracks the local player horizontally so flying
//! left/right scrolls the world instead of warping around. Vertical scroll
//! is disabled — the view fully covers the world's Y range.
//!
//! The world's X axis is toroidal (`sim::util::wrap_coord`), so an entity
//! near the seam may need to be drawn twice; `world_x_offsets_for` returns
//! up to three candidate offsets to try.

use ggez::glam::Vec2;

pub type Point2 = Vec2;

pub struct Camera {
    /// Total world extent in world units. X is toroidal at this width.
    world_size: Vec2,
    /// Visible chunk in world units (the camera's "lens"). The screen is
    /// letterboxed to match this aspect ratio.
    view_size: Vec2,
    /// Backing surface size in screen pixels.
    screen_size: Vec2,
    /// Camera center in world coords (Y-up).
    center: Vec2,
    /// Screen pixels per world unit. Uniform — the world stays square.
    scale: f32,
}

impl Camera {
    pub fn new(screen_w: f32, screen_h: f32, world_w: f32, world_h: f32, view_w: f32, view_h: f32) -> Self {
        let mut c = Camera {
            world_size: Vec2::new(world_w, world_h),
            view_size: Vec2::new(view_w, view_h),
            screen_size: Vec2::new(screen_w, screen_h),
            center: Vec2::new(world_w * 0.5, world_h * 0.5),
            scale: 1.0,
        };
        c.recompute_scale();
        c
    }

    pub fn set_screen_size(&mut self, w: f32, h: f32) {
        self.screen_size = Vec2::new(w, h);
        self.recompute_scale();
    }

    /// Smoothly slide the camera toward `target` (world coords). `t` is
    /// the per-frame interpolation factor — call with `dt * follow_rate`.
    /// X-wrap is handled by picking the shortest-arc target so crossing
    /// the seam doesn't jerk the camera across the whole world.
    pub fn follow(&mut self, target: Vec2, t: f32) {
        let t = t.clamp(0.0, 1.0);
        let world_w = self.world_size.x;
        // Shortest X delta accounting for wrap.
        let mut dx = target.x - self.center.x;
        let half = world_w * 0.5;
        if dx > half {
            dx -= world_w;
        } else if dx < -half {
            dx += world_w;
        }
        let new_x = self.center.x + dx * t;
        // Vertical center stays at world midpoint — we don't scroll Y.
        let new_y = self.world_size.y * 0.5;
        // Keep the camera center inside [0, world_w) so world_to_screen
        // arithmetic stays in a sane range. The actual seam handling lives
        // in `world_x_offsets_for` below.
        let wrapped = if new_x < 0.0 {
            new_x + world_w
        } else if new_x >= world_w {
            new_x - world_w
        } else {
            new_x
        };
        self.center = Vec2::new(wrapped, new_y);
    }

    /// Snap the camera directly to `target` (no easing). Used on the first
    /// snapshot to avoid a slow pan in from world center.
    pub fn snap_to(&mut self, target: Vec2) {
        let world_w = self.world_size.x;
        let x = if target.x < 0.0 {
            target.x + world_w
        } else if target.x >= world_w {
            target.x - world_w
        } else {
            target.x
        };
        self.center = Vec2::new(x, self.world_size.y * 0.5);
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

    pub fn world_size(&self) -> Vec2 {
        self.world_size
    }

    pub fn screen_size(&self) -> Vec2 {
        self.screen_size
    }

    pub fn center(&self) -> Vec2 {
        self.center
    }

    /// Up to three signed X offsets at which an entity at `world_x` may
    /// land within the visible viewport, given X-wrap. The caller plugs
    /// each into `world_to_screen` and skips draws that fall off the
    /// screen edge.
    pub fn world_x_offsets_for(&self, world_x: f32, sprite_half: f32) -> [Option<f32>; 3] {
        let world_w = self.world_size.x;
        // Visible half-width in world units, plus a margin so we don't pop
        // sprites in/out at the edge.
        let half_view = self.view_size.x * 0.5 + sprite_half + 4.0;
        let mut out = [None; 3];
        for (i, dx) in [-world_w, 0.0, world_w].iter().enumerate() {
            let candidate = world_x + *dx;
            let delta = candidate - self.center.x;
            if delta.abs() <= half_view {
                out[i] = Some(candidate);
            }
        }
        out
    }

    /// World (Y-up) point → screen (Y-down) pixel position, taking the
    /// camera's center into account. Use `world_x_offsets_for` if the
    /// caller needs to draw across a wrap seam.
    pub fn world_to_screen(&self, world: Point2) -> Point2 {
        let s = self.scale;
        Point2::new(
            (world.x - self.center.x) * s + self.screen_size.x * 0.5,
            (self.center.y - world.y) * s + self.screen_size.y * 0.5,
        )
    }
}
