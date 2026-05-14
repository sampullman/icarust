//! Sky background — cream backdrop plus a deterministic field of puffy
//! white clouds that follow the camera (and the world's X-wrap).
//!
//! Clouds aren't simulation entities; they're picked client-side from a
//! seeded RNG so every player sees the same skyline without having to
//! ship cloud positions over the wire. Each cloud is drawn as a small
//! stack of overlapping circles, which gives the "pillowy" silhouette
//! seen in the Luftrauser-style reference art.

use ggez::glam::Vec2;
use ggez::graphics::{self, Canvas, Color, DrawMode, DrawParam, Mesh, MeshBuilder};
use ggez::{Context, GameResult};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::render::camera::Camera;

/// Cream sky tone — main scene background.
pub const SKY_COLOR: Color = Color::new(0.94, 0.90, 0.68, 1.0);

/// A pre-built cloud "stamp" with a fixed shape. We pick from a handful
/// at build time so the sky doesn't look like the same cloud xeroxed
/// twenty times.
struct CloudStamp {
    mesh: Mesh,
    /// Approximate world-space half-width of the stamp. Used to decide
    /// whether a placement is visible.
    half_width: f32,
}

struct Cloud {
    /// World-space anchor point (Y-up). The stamp is centered here.
    pos: Vec2,
    /// Which stamp this cloud uses.
    stamp: usize,
    /// Per-cloud horizontal scale jitter (1.0 = base size).
    scale: f32,
    /// Drift speed in world units / s (X axis only). Positive drifts right.
    drift: f32,
}

pub struct Sky {
    stamps: Vec<CloudStamp>,
    clouds: Vec<Cloud>,
}

impl Sky {
    /// Build a sky for a world of size `world_size`, seeded from `seed`.
    /// Two seeds equal → identical cloudscape, which means snapshots from
    /// the same server look the same on every connected client.
    pub fn build(ctx: &mut Context, world_size: Vec2, seed: u64) -> GameResult<Self> {
        let stamps = vec![
            cloud_mesh(ctx, &[(0.0, 0.0, 32.0), (28.0, -4.0, 24.0), (-26.0, 2.0, 22.0), (12.0, -16.0, 18.0)])?,
            cloud_mesh(ctx, &[(0.0, 0.0, 40.0), (34.0, 6.0, 26.0), (-30.0, 4.0, 24.0), (-8.0, -18.0, 22.0), (18.0, -14.0, 18.0)])?,
            cloud_mesh(ctx, &[(0.0, 0.0, 22.0), (18.0, -2.0, 18.0), (-18.0, 0.0, 16.0)])?,
            cloud_mesh(ctx, &[(0.0, 0.0, 30.0), (24.0, 2.0, 22.0), (-24.0, -2.0, 20.0), (4.0, -14.0, 16.0), (44.0, 0.0, 18.0)])?,
        ];

        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0xC10D_C10D_u64);
        // Roughly one cloud per ~280 world units, so a 3200-wide world
        // carries ~11 clouds — enough density without paving over the sky.
        let count = ((world_size.x / 280.0).round() as usize).max(8);
        let mut clouds = Vec::with_capacity(count);
        for _ in 0..count {
            let stamp = rng.gen_range(0..stamps.len());
            // Y range: roughly the top 60% of the world. Y-up coords, so
            // the higher values are visually higher on screen.
            let y = world_size.y * (0.55 + rng.gen::<f32>() * 0.40);
            let x = rng.gen::<f32>() * world_size.x;
            let scale = 0.85 + rng.gen::<f32>() * 0.55;
            // Tiny lateral drift so the sky isn't completely static. Half
            // the clouds drift right, half left.
            let drift = (rng.gen::<f32>() - 0.5) * 14.0;
            clouds.push(Cloud {
                pos: Vec2::new(x, y),
                stamp,
                scale,
                drift,
            });
        }

        Ok(Sky { stamps, clouds })
    }

    /// Slide each cloud horizontally on its own drift speed. Wraps with
    /// the world so clouds reappear on the opposite seam — mirrors the
    /// sim's X-wrap behaviour for entities.
    pub fn update(&mut self, dt: f32, world_size: Vec2) {
        for c in &mut self.clouds {
            c.pos.x += c.drift * dt;
            if c.pos.x < 0.0 {
                c.pos.x += world_size.x;
            } else if c.pos.x >= world_size.x {
                c.pos.x -= world_size.x;
            }
        }
    }

    /// Draw the cream sky band + every visible cloud. Call before any
    /// entities so they paint on top.
    pub fn draw(&self, canvas: &mut Canvas, camera: &Camera) {
        // Sky fill across the whole screen. The world doesn't actually
        // need to know about this — the canvas was cleared with SKY_COLOR
        // already in `lib.rs`. We still paint a sky quad here so the
        // letterbox bars stay the same neutral tone as the play area.
        let screen = camera.screen_size();
        canvas.draw(
            &graphics::Quad,
            DrawParam::new()
                .dest(Vec2::ZERO)
                .scale([screen.x, screen.y])
                .color(SKY_COLOR),
        );

        let scale_to_screen = camera.scale();
        for cloud in &self.clouds {
            let stamp = &self.stamps[cloud.stamp];
            let half = stamp.half_width * cloud.scale;
            for cand in camera.world_x_offsets_for(cloud.pos.x, half).into_iter().flatten() {
                let screen_pos = camera.world_to_screen(Vec2::new(cand, cloud.pos.y));
                canvas.draw(
                    &stamp.mesh,
                    DrawParam::new()
                        .dest(screen_pos)
                        .scale([scale_to_screen * cloud.scale, scale_to_screen * cloud.scale])
                        .color(Color::WHITE),
                );
            }
        }
    }
}

/// Build a cloud mesh from a list of `(dx, dy, radius)` triples. Each
/// circle is drawn in screen-space (Y-down), so a `dy < 0` lobe sits
/// above the cloud's anchor on screen.
fn cloud_mesh(ctx: &mut Context, circles: &[(f32, f32, f32)]) -> GameResult<CloudStamp> {
    let mut mb = MeshBuilder::new();
    let mut max_extent: f32 = 0.0;
    for (dx, dy, r) in circles {
        // Tolerance ~0.5 px gives smooth-looking puffs without ballooning
        // vertex counts.
        mb.circle(DrawMode::fill(), Vec2::new(*dx, *dy), *r, 0.5, Color::WHITE)?;
        max_extent = max_extent.max(dx.abs() + r);
    }
    let data = mb.build();
    Ok(CloudStamp {
        mesh: Mesh::from_data(ctx, data),
        half_width: max_extent,
    })
}
