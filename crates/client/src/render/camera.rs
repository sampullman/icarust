//! Camera that maps the wide, Y-up world into a smaller scrolling view.
//!
//! The world is `WORLD_WIDTH × WORLD_HEIGHT` (3200 × 1080 in production)
//! but only a `VIEW_WIDTH × VIEW_HEIGHT` chunk (1280 × 540) is drawn at
//! a time. The camera tracks the local player both horizontally and
//! vertically so flying around scrolls the world. X scroll wraps with
//! the seam; Y scroll is clamped so the bottom of the view never dips
//! more than `BELOW_GROUND_VIEW_RATIO` of a screen height below the
//! ground line, and the top never reads past the world ceiling.
//!
//! The world's X axis is toroidal (`sim::util::wrap_coord`), so an entity
//! near the seam may need to be drawn twice; `world_x_offsets_for` returns
//! up to three candidate offsets to try.

use ggez::glam::Vec2;

pub type Point2 = Vec2;

/// Maximum fraction of the view height that may show empty space below the
/// ground line when the player dives to the floor. Bumping this up lets the
/// pilot peek further under the horizon when crashing; lowering it pins
/// the ground closer to the bottom of the screen.
pub const BELOW_GROUND_VIEW_RATIO: f32 = 0.20;

pub struct Camera {
    /// Total world extent in world units. X is toroidal at this width;
    /// Y is a hard `[0, world_size.y]` box.
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
    /// World Y of the ground surface; used to clamp how far the camera
    /// can scroll down. Updated via `set_ground_y` when terrain changes.
    ground_y: f32,
    /// Cached `(min_y, max_y)` for the camera center, derived from
    /// `ground_y`, `view_size.y`, and `world_size.y`. Recomputed when
    /// any of those change.
    y_range: (f32, f32),
}

impl Camera {
    pub fn new(
        screen_w: f32,
        screen_h: f32,
        world_w: f32,
        world_h: f32,
        view_w: f32,
        view_h: f32,
        ground_y: f32,
    ) -> Self {
        let mut c = Camera {
            world_size: Vec2::new(world_w, world_h),
            view_size: Vec2::new(view_w, view_h),
            screen_size: Vec2::new(screen_w, screen_h),
            center: Vec2::new(world_w * 0.5, view_h * 0.5),
            scale: 1.0,
            ground_y,
            y_range: (0.0, 0.0),
        };
        c.recompute_scale();
        c.recompute_y_range();
        c
    }

    pub fn set_screen_size(&mut self, w: f32, h: f32) {
        self.screen_size = Vec2::new(w, h);
        self.recompute_scale();
    }

    /// Update the floor reference. Recomputes the vertical scroll bounds.
    /// Today the server never changes terrain, but the camera reads this
    /// from each snapshot so future per-level terrain swaps "just work".
    pub fn set_ground_y(&mut self, ground_y: f32) {
        if (self.ground_y - ground_y).abs() < f32::EPSILON {
            return;
        }
        self.ground_y = ground_y;
        self.recompute_y_range();
        // Re-clamp the current center so a tightening floor doesn't leave
        // the camera outside the new bounds.
        self.center.y = self.clamp_center_y(self.center.y);
    }

    /// Smoothly slide the camera toward `target` (world coords). `t` is
    /// the per-frame interpolation factor — call with `dt * follow_rate`.
    /// X uses the shortest-arc wrap so crossing the seam doesn't jerk
    /// the camera across the whole world. Y is unwrapped but clamped to
    /// keep ground/sky in their expected screen positions.
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
        // Vertical: ease toward the clamped target. Clamping `target.y`
        // before lerping makes the camera ease *toward the bound* rather
        // than perpetually overshooting and re-clamping each frame.
        let target_y = self.clamp_center_y(target.y);
        let new_y = self.center.y + (target_y - self.center.y) * t;
        let wrapped_x = if new_x < 0.0 {
            new_x + world_w
        } else if new_x >= world_w {
            new_x - world_w
        } else {
            new_x
        };
        self.center = Vec2::new(wrapped_x, new_y);
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
        let y = self.clamp_center_y(target.y);
        self.center = Vec2::new(x, y);
    }

    fn recompute_scale(&mut self) {
        let sx = self.screen_size.x / self.view_size.x;
        let sy = self.screen_size.y / self.view_size.y;
        self.scale = sx.min(sy);
    }

    /// Compute `(min_y, max_y)` for the camera center. The bottom bound
    /// caps how far below ground the viewport may peek; the top bound
    /// keeps the viewport entirely inside the world ceiling. If the world
    /// is shorter than the view, the range collapses and clamping just
    /// pins the camera at world center.
    fn recompute_y_range(&mut self) {
        let view_half = self.view_size.y * 0.5;
        let min_y = self.ground_y + view_half - self.view_size.y * BELOW_GROUND_VIEW_RATIO;
        let max_y = self.world_size.y - view_half;
        // Guard against a degenerate world (`world_h < view_h`): in that
        // case there's nowhere to scroll, so just park at the midpoint.
        let (min_y, max_y) = if max_y < min_y {
            let mid = (min_y + max_y) * 0.5;
            (mid, mid)
        } else {
            (min_y, max_y)
        };
        self.y_range = (min_y, max_y);
    }

    fn clamp_center_y(&self, y: f32) -> f32 {
        y.clamp(self.y_range.0, self.y_range.1)
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

    /// Vertical scroll bounds for the camera center, mostly for tests.
    #[allow(dead_code)]
    pub fn y_range(&self) -> (f32, f32) {
        self.y_range
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_camera() -> Camera {
        // Match the production constants: 1280x540 view, 3200x1080 world,
        // ground at y=26. Screen size matches view 1:1 so scale=1.
        Camera::new(1280.0, 540.0, 3200.0, 1080.0, 1280.0, 540.0, 26.0)
    }

    #[test]
    fn camera_y_range_clamps_to_20pct_below_ground() {
        let cam = make_camera();
        let (min_y, max_y) = cam.y_range();
        // Bottom of the view at min center is `min_y - view/2`. With
        // ground at 26 and view 540, 20% of the view (108) may show
        // below the ground.
        assert!(
            (min_y - (26.0 + 540.0 * 0.5 - 540.0 * 0.20)).abs() < 1e-3,
            "min camera y wrong: {min_y}"
        );
        // Top bound keeps view inside world ceiling.
        assert!((max_y - (1080.0 - 270.0)).abs() < 1e-3, "max camera y wrong: {max_y}");
    }

    #[test]
    fn follow_clamps_to_ground_floor() {
        let mut cam = make_camera();
        // Aim the camera at the ground; even fully eased it can't sink
        // below `min_y`.
        cam.snap_to(Vec2::new(1600.0, 26.0));
        let (min_y, _) = cam.y_range();
        assert!(
            (cam.center().y - min_y).abs() < 1e-3,
            "expected camera pinned to min_y={min_y}, got {}",
            cam.center().y
        );
    }

    #[test]
    fn follow_clamps_to_world_ceiling() {
        let mut cam = make_camera();
        cam.snap_to(Vec2::new(1600.0, 5000.0));
        let (_, max_y) = cam.y_range();
        assert!(
            (cam.center().y - max_y).abs() < 1e-3,
            "expected camera pinned to max_y={max_y}, got {}",
            cam.center().y
        );
    }

    #[test]
    fn snap_to_within_range_is_identity() {
        let mut cam = make_camera();
        cam.snap_to(Vec2::new(800.0, 400.0));
        assert!((cam.center().y - 400.0).abs() < 1e-3);
    }
}
