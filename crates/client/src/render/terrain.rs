//! Terrain rendering. The server sends terrain bands in every snapshot;
//! this module turns them into tinted quads in screen space.
//!
//! Each `TerrainKind` picks its own color (and, when we have art, its
//! own sprite/tiling). The world is Y-up so a band that fills `[0,
//! top_y]` in world coords lands at the bottom of the screen.

use ggez::glam::Vec2;
use ggez::graphics::{self, Canvas, Color, DrawParam};
use sim::{TerrainBand, TerrainKind};

use crate::render::camera::Camera;

fn color_for(kind: TerrainKind) -> Color {
    match kind {
        // Earthy brown — reads as ground without competing with the
        // ship/rock sprites for attention.
        TerrainKind::Ground => Color::new(0.36, 0.24, 0.14, 1.0),
    }
}

pub fn draw(canvas: &mut Canvas, camera: &Camera, bands: &[TerrainBand]) {
    let view = camera.view_size();
    for band in bands {
        let color = color_for(band.kind);
        // World-space band corners (Y-up): (0, 0) to (world_w, top_y).
        // Convert the world-space top-left of the band — which is at
        // (0, top_y) in Y-up — into screen-space; that's the rect's
        // top-left in screen-space (Y-down).
        let top_left = camera.world_to_screen(Vec2::new(0.0, band.top_y));
        let bottom_right = camera.world_to_screen(Vec2::new(view.x, 0.0));
        let w = bottom_right.x - top_left.x;
        let h = bottom_right.y - top_left.y;
        canvas.draw(
            &graphics::Quad,
            DrawParam::new()
                .dest(top_left)
                .scale([w, h])
                .color(color),
        );
    }
}
