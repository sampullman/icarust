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

fn fill_color_for(kind: TerrainKind) -> Color {
    match kind {
        // Warm dusty tan — matches the Luftrauser reference.
        TerrainKind::Ground => Color::new(0.66, 0.50, 0.36, 1.0),
    }
}

fn edge_color_for(kind: TerrainKind) -> Color {
    match kind {
        // Darker maroon edge that reads as the horizon line.
        TerrainKind::Ground => Color::new(0.42, 0.20, 0.20, 1.0),
    }
}

pub fn draw(canvas: &mut Canvas, camera: &Camera, bands: &[TerrainBand]) {
    let screen = camera.screen_size();
    for band in bands {
        let fill = fill_color_for(band.kind);
        let edge = edge_color_for(band.kind);
        // Top of band in screen-space. We draw the band across the full
        // screen width so the letterbox bars stay covered too — the play
        // area's left/right edges shouldn't show sky through the floor.
        let top = camera.world_to_screen(Vec2::new(camera.center().x, band.top_y));
        let y_top = top.y;
        let h = (screen.y - y_top).max(0.0);
        canvas.draw(
            &graphics::Quad,
            DrawParam::new()
                .dest(Vec2::new(0.0, y_top))
                .scale([screen.x, h])
                .color(fill),
        );
        // Thin horizon stripe so the boundary reads cleanly. ~2 px tall
        // independent of screen scale — close enough.
        let stripe_h = 2.0_f32.max(2.0 * camera.scale() * 0.5);
        canvas.draw(
            &graphics::Quad,
            DrawParam::new()
                .dest(Vec2::new(0.0, y_top - stripe_h * 0.5))
                .scale([screen.x, stripe_h])
                .color(edge),
        );
    }
}
