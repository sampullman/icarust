//! Sky background — cream backdrop plus a deterministic field of puffy
//! white clouds that follow the camera (and the world's X-wrap).
//!
//! Clouds aren't simulation entities; they're picked client-side from a
//! seeded RNG so every player sees the same skyline without having to
//! ship cloud positions over the wire. Each cloud is drawn as a small
//! stack of overlapping circles, which gives the "pillowy" silhouette
//! seen in the Luftrauser-style reference art.

use ggez::glam::Vec2;
use ggez::graphics::{self, Canvas, Color, DrawMode, DrawParam, InstanceArray, Mesh, MeshBuilder};
use ggez::{Context, GameResult};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::render::camera::Camera;

/// Cream sky tone — main scene background.
pub const SKY_COLOR: Color = Color::new(0.94, 0.90, 0.68, 1.0);

/// A pre-built cloud "stamp" with a fixed shape. We pick from a handful
/// at build time so the sky doesn't look like the same cloud xeroxed
/// twenty times. Each stamp also owns a per-frame `InstanceArray` so
/// every cloud sharing the stamp lands in one draw call.
struct CloudStamp {
    mesh: Mesh,
    /// Approximate world-space half-width of the stamp. Used to decide
    /// whether a placement is visible.
    half_width: f32,
    /// Reused per frame: cleared, repopulated with one `DrawParam` per
    /// visible cloud (across wrap copies), then drawn as one batched call.
    instances: InstanceArray,
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
            cloud_stamp(ctx, &[(0.0, 0.0, 32.0), (28.0, -4.0, 24.0), (-26.0, 2.0, 22.0), (12.0, -16.0, 18.0)])?,
            cloud_stamp(ctx, &[(0.0, 0.0, 40.0), (34.0, 6.0, 26.0), (-30.0, 4.0, 24.0), (-8.0, -18.0, 22.0), (18.0, -14.0, 18.0)])?,
            cloud_stamp(ctx, &[(0.0, 0.0, 22.0), (18.0, -2.0, 18.0), (-18.0, 0.0, 16.0)])?,
            cloud_stamp(ctx, &[(0.0, 0.0, 30.0), (24.0, 2.0, 22.0), (-24.0, -2.0, 20.0), (4.0, -14.0, 16.0), (44.0, 0.0, 18.0)])?,
        ];

        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0xC10D_C10D_u64);
        // Cloud band: the bottom sits where pilots typically spawn so even
        // grounded play sees clouds overhead; the top sits just under the
        // world ceiling. `min(540, world.y)` pins the bottom to a fixed
        // altitude even after the world gets taller, otherwise the lower
        // band would drift up and out of the spawn view.
        let cloud_bottom = world_size.y.min(540.0) * 0.5;
        let cloud_top = world_size.y * 0.95;
        let band_height = (cloud_top - cloud_bottom).max(1.0);
        // Density: ~one cloud per (280 wide × 360 tall) world cell. Scaling
        // with the cloud band's vertical extent keeps the sky from feeling
        // empty after we extend the world upward.
        let area_cells = (world_size.x / 280.0) * (band_height / 360.0);
        let count = (area_cells.round() as usize).max(8);
        let mut clouds = Vec::with_capacity(count);
        for _ in 0..count {
            let stamp = rng.gen_range(0..stamps.len());
            // Y-up coords: higher values draw visually higher on screen.
            let y = cloud_bottom + rng.gen::<f32>() * band_height;
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
    /// entities so they paint on top. Clouds sharing a stamp are batched
    /// into one draw call apiece, so the worst-case is 4 cloud draws
    /// regardless of cloud count.
    pub fn draw(&mut self, canvas: &mut Canvas, camera: &Camera) {
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
        for stamp in self.stamps.iter_mut() {
            stamp.instances.clear();
        }
        for cloud in &self.clouds {
            let stamp = &mut self.stamps[cloud.stamp];
            let half = stamp.half_width * cloud.scale;
            for cand in camera
                .world_x_offsets_for(cloud.pos.x, half)
                .into_iter()
                .flatten()
            {
                let screen_pos = camera.world_to_screen(Vec2::new(cand, cloud.pos.y));
                stamp.instances.push(
                    DrawParam::new()
                        .dest(screen_pos)
                        .scale([
                            scale_to_screen * cloud.scale,
                            scale_to_screen * cloud.scale,
                        ])
                        .color(Color::WHITE),
                );
            }
        }
        for stamp in &self.stamps {
            if stamp.instances.instances().is_empty() {
                continue;
            }
            canvas.draw_instanced_mesh(stamp.mesh.clone(), &stamp.instances, DrawParam::default());
        }
    }
}

/// Build a cloud stamp from a list of `(dx, dy, radius)` triples. Each
/// circle is drawn in screen-space (Y-down), so a `dy < 0` lobe sits
/// above the cloud's anchor on screen. Allocates a fresh `InstanceArray`
/// the renderer reuses across frames to batch all clouds sharing the
/// stamp into one draw call.
fn cloud_stamp(ctx: &mut Context, circles: &[(f32, f32, f32)]) -> GameResult<CloudStamp> {
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
        instances: InstanceArray::new(ctx, None),
    })
}
