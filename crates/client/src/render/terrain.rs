//! Terrain rendering.
//!
//! The server sends a `Vec<TerrainBand>` in every snapshot; each band
//! carries a `GroundProfile` heightmap. We turn that into a *single*
//! batched mesh covering one world-width of terrain — soil polygon,
//! horizon stripe, grass tufts, and pebbles, all in one
//! `MeshBuilder` with vertex colors baked in. At draw time we issue
//! one `canvas.draw` per visible wrap copy, instead of one per
//! decoration. On a 3200-wide world that's typically 2 draw calls
//! per frame for the whole ground layer.
//!
//! The mesh is built in world coords with Y flipped to screen-down,
//! so a `dest = world_to_screen(Vec2::new(dx, 0))` per wrap copy
//! places it correctly. Tuft rotation (slope-aligned) is baked into
//! the vertex positions at build time so the GPU never sees a
//! per-tuft rotation matrix.
//!
//! New `TerrainKind`s plug in here by adding a row to `fill_color_for`
//! and (optionally) a per-kind decoration pass inside
//! `append_terrain_band`.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawMode, DrawParam, Mesh, MeshBuilder};
use ggez::{Context, GameResult};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sim::terrain::{GroundProfile, TerrainBand, TerrainKind};

use crate::render::camera::Camera;

/// Approximate spacing (world units) between grass tufts / pebbles.
/// Tightening this packs more decorations per world but they're all
/// baked into one batched mesh, so the cost is purely build-time
/// geometry, not per-frame draws.
const TUFT_SPACING: f32 = 32.0;
/// Seeds the tuft placement RNG. XOR'd with the world's first surface
/// height so two different terrains don't produce identical tufting.
const TUFT_SEED_BASE: u64 = 0xA17E_C0FF_EE3A_DA15;
/// Seeds the dirt-patch placement RNG. Distinct from the tuft seed so
/// the two passes can be tuned independently without one accidentally
/// reflowing the other.
const PATCH_SEED_BASE: u64 = 0xD12D_B0DD_B17A_F00D;
/// Approximate spacing (world units) between dirt patches. Smaller than
/// `TUFT_SPACING` so the patches cover the soil more densely — they're
/// the main thing keeping the ground from looking like a flat brown
/// stripe.
const PATCH_SPACING: f32 = 14.0;
/// How far below the surface a patch may sit. Patches sample a depth in
/// `[0, PATCH_MAX_DEPTH]`, with most weight near the top so the visible
/// "skin" of the soil is the part that looks textured. Anything deeper
/// would be hidden by the camera most of the time.
const PATCH_MAX_DEPTH: f32 = 36.0;
/// How far below world Y=0 the soil polygon's bottom edge sits. We
/// over-extend so the camera diving into a valley never sees the
/// mesh's open bottom. Stored in screen-down (Y-flipped) coords, so
/// this is positive.
const FLOOR_DEPTH: f32 = 80.0;
/// Horizon stripe thickness (world units).
const HORIZON_STRIPE: f32 = 2.0;

fn fill_color_for(kind: TerrainKind) -> Color {
    match kind {
        // Warm dusty tan — main soil body.
        TerrainKind::Ground => Color::new(0.66, 0.50, 0.36, 1.0),
    }
}

fn edge_color_for(kind: TerrainKind) -> Color {
    match kind {
        // Darker maroon edge that reads as the horizon line.
        TerrainKind::Ground => Color::new(0.42, 0.20, 0.20, 1.0),
    }
}

/// Tint of the grass blades sitting on top of the soil. Slightly
/// green-shifted brown so it reads as dry-grass aesthetic rather than
/// lush lawn — matches the dusty palette of the rest of the world.
const GRASS_COLOR: Color = Color::new(0.46, 0.52, 0.26, 1.0);
/// Tint of dirt pebbles — a step darker than the soil fill so they
/// pop against it.
const PEBBLE_COLOR: Color = Color::new(0.42, 0.30, 0.20, 1.0);

/// Palette used by the dirt-patch decoration pass. Each entry is a
/// small (rgb) offset added to the soil fill — keeping the patches in
/// the same dusty family while still reading as separate clods. The
/// first entry is "darker than soil" (most common); the others are a
/// touch redder or lighter to break up regularity. Alpha is baked into
/// the mesh, so the values double as draw-time tints.
const DIRT_PATCH_DELTAS: &[(f32, f32, f32)] = &[
    (-0.10, -0.10, -0.08),
    (-0.06, -0.04, -0.02),
    (0.05, 0.03, 0.0),
    (-0.14, -0.16, -0.14),
];

/// Two flavors of tuft. Sampled with weighted random choice at build
/// time so the surface gets a mix.
#[derive(Debug, Clone, Copy)]
enum TuftStyle {
    Grass,
    Pebble,
}

/// One concrete tuft placement along the surface (world coords, Y-up).
#[derive(Debug, Clone, Copy)]
struct Tuft {
    /// World X anchor.
    x: f32,
    /// Surface Y at the anchor — pre-computed so build time doesn't
    /// have to interpolate the profile for each tuft.
    y: f32,
    /// Local rotation aligning the tuft with the slope at its anchor.
    /// `0` = upright; positive = tilted right on screen.
    rotation: f32,
    /// Uniform scale jitter (1.0 = base size).
    scale: f32,
    style: TuftStyle,
}

/// Cached terrain renderer. Rebuilds the mesh only when the band
/// signature changes — today that's "once, ever," but the indirection
/// means a future server that swaps terrain per level works without
/// special-casing.
pub struct TerrainRenderer {
    /// Hash of the bands that produced the cached mesh. Cheap
    /// signature so `sync` can short-circuit when nothing changed.
    signature: u64,
    /// Single batched mesh containing every renderable piece of the
    /// ground layer. Vertex colors are baked in so the draw call uses
    /// a plain white tint.
    mesh: Option<Mesh>,
    /// World width covered by the cached profile — needed at draw
    /// time to pick wrap-mirrored copies.
    world_width: f32,
}

impl TerrainRenderer {
    /// Builds an empty renderer with no cached mesh. First call to
    /// [`sync`] populates it from the server's terrain.
    pub fn empty() -> Self {
        Self {
            signature: 0,
            mesh: None,
            world_width: 0.0,
        }
    }

    /// Refresh the cached mesh if `bands` differs from the last sync.
    /// Cheap when bands are unchanged (just hashes the heights), so it
    /// is safe to call on every snapshot.
    pub fn sync(&mut self, ctx: &mut Context, bands: &[TerrainBand]) {
        let sig = signature(bands);
        if sig == self.signature && self.mesh.is_some() {
            return;
        }
        if let Err(e) = self.rebuild(ctx, bands) {
            tracing::warn!(error = ?e, "terrain renderer rebuild failed");
        } else {
            self.signature = sig;
        }
    }

    fn rebuild(&mut self, ctx: &mut Context, bands: &[TerrainBand]) -> GameResult<()> {
        self.mesh = None;
        self.world_width = 0.0;

        let mut mb = MeshBuilder::new();
        for band in bands {
            self.world_width = self.world_width.max(band.profile.world_width);
            append_terrain_band(&mut mb, band)?;
        }
        if self.world_width > 0.0 {
            self.mesh = Some(Mesh::from_data(ctx, mb.build()));
        }
        Ok(())
    }

    /// Draw the cached terrain mesh once per visible wrap copy. Called
    /// after the sky and before the entities, so flying objects pass
    /// in front of the ground.
    pub fn draw(&self, canvas: &mut Canvas, camera: &Camera) {
        let Some(mesh) = &self.mesh else {
            return;
        };
        let scale = camera.scale();
        for dx in copies_for(camera.center().x, self.world_width, camera.view_size().x) {
            let origin = camera.world_to_screen(Vec2::new(dx, 0.0));
            canvas.draw(
                mesh,
                DrawParam::new()
                    .dest(origin)
                    .scale([scale, scale])
                    .color(Color::WHITE),
            );
        }
    }
}

/// Append every piece of a single terrain band to `mb`: filled soil
/// polygon, horizon stripe, and the band's tuft decorations. Vertex
/// colors come from `fill_color_for` / `edge_color_for` / the tuft
/// palette, so the resulting mesh can be drawn with a plain white
/// tint.
fn append_terrain_band(mb: &mut MeshBuilder, band: &TerrainBand) -> GameResult<()> {
    match band.kind {
        TerrainKind::Ground => {
            append_ground_polygon(mb, &band.profile, band.kind)?;
            // Dirt patches go *before* the horizon stripe and tufts so
            // the surface line + grass cover any patch that pokes up
            // through the very top of the soil — keeps the silhouette
            // crisp while still letting some patches kiss the surface.
            let patches = build_dirt_patches(&band.profile, fill_color_for(band.kind));
            for patch in &patches {
                append_dirt_patch(mb, patch)?;
            }
            append_horizon_stripe(mb, &band.profile, band.kind)?;
            let tufts = build_tufts(&band.profile);
            for tuft in &tufts {
                append_tuft(mb, tuft)?;
            }
        }
    }
    Ok(())
}

/// Walk the top-edge of the profile L→R then seal with two bottom
/// corners below the world floor. Earcut triangulation in
/// `MeshBuilder::polygon` handles the non-convex top edge correctly.
fn append_ground_polygon(
    mb: &mut MeshBuilder,
    profile: &GroundProfile,
    kind: TerrainKind,
) -> GameResult<()> {
    let n = profile.heights.len();
    if n == 0 {
        return Ok(());
    }
    let mut poly: Vec<Vec2> = Vec::with_capacity(n + 3);
    for i in 0..=n {
        let xi = (i as f32) * profile.spacing;
        let h = profile.heights[i % n];
        poly.push(Vec2::new(xi, -h));
    }
    poly.push(Vec2::new(profile.world_width, FLOOR_DEPTH));
    poly.push(Vec2::new(0.0, FLOOR_DEPTH));
    mb.polygon(DrawMode::fill(), &poly, fill_color_for(kind))?;
    Ok(())
}

/// Stripe along the profile's top edge using one thin quad per
/// segment. Per-segment quads (rather than a polyline stroke) give us
/// stable thickness regardless of how ggez handles stroke joins.
fn append_horizon_stripe(
    mb: &mut MeshBuilder,
    profile: &GroundProfile,
    kind: TerrainKind,
) -> GameResult<()> {
    let n = profile.heights.len();
    if n == 0 {
        return Ok(());
    }
    let color = edge_color_for(kind);
    let half_t = HORIZON_STRIPE * 0.5;
    for i in 0..n {
        let xa = (i as f32) * profile.spacing;
        let xb = ((i + 1) as f32) * profile.spacing;
        let a = Vec2::new(xa, -profile.heights[i]);
        let b = Vec2::new(xb, -profile.heights[(i + 1) % n]);
        let dir = b - a;
        let len = dir.length();
        if len < 1e-3 {
            continue;
        }
        let nrm = Vec2::new(-dir.y / len, dir.x / len);
        let off = nrm * half_t;
        let quad = [a - off, b - off, b + off, a + off];
        mb.polygon(DrawMode::fill(), &quad, color)?;
    }
    Ok(())
}

/// Build a list of tufts anchored to the surface of `profile`. Spacing
/// is roughly `TUFT_SPACING` with seeded jitter; each tuft picks a
/// style and a slope-aligned rotation.
fn build_tufts(profile: &GroundProfile) -> Vec<Tuft> {
    let seed = TUFT_SEED_BASE
        ^ profile.heights.first().copied().unwrap_or(0.0).to_bits() as u64
        ^ (profile.heights.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    let w = profile.world_width;
    if w <= 0.0 {
        return Vec::new();
    }
    let approx_count = (w / TUFT_SPACING).round() as usize;
    let mut tufts = Vec::with_capacity(approx_count);
    let mut x = 0.0_f32;
    while x < w {
        let jitter: f32 = rng.gen::<f32>() * TUFT_SPACING * 0.4;
        let here = (x + jitter).min(w - 1.0);
        let y = profile.height_at(here);
        // Slope at this point, used to rotate the tuft so it stands
        // perpendicular to the ground rather than straight up.
        let probe = 4.0_f32.min(profile.spacing * 0.5);
        let slope = (profile.height_at(here + probe) - profile.height_at(here - probe))
            / (2.0 * probe);
        let rotation = slope.atan();
        let scale = 0.85 + rng.gen::<f32>() * 0.35;
        // 75% grass, 25% pebble. Mixing them sells "dusty soil with
        // scrubby growth" without leaning entirely on one stamp.
        let style = if rng.gen::<f32>() < 0.75 {
            TuftStyle::Grass
        } else {
            TuftStyle::Pebble
        };
        tufts.push(Tuft {
            x: here,
            y,
            rotation,
            scale,
            style,
        });
        x += TUFT_SPACING;
    }
    tufts
}

/// Local-space points (screen-down, anchor at origin) of each polygon
/// that makes up a grass tuft. `-y` is "up out of the ground." Vertex
/// order goes CCW in screen-down (which is CW in world-up), matching
/// ggez's expected winding.
const GRASS_BLADES: &[&[Vec2]] = &[
    // Central blade.
    &[
        Vec2::new(-1.2, 0.0),
        Vec2::new(1.2, 0.0),
        Vec2::new(0.0, -6.5),
    ],
    // Left lean.
    &[
        Vec2::new(-2.5, 0.0),
        Vec2::new(-0.5, 0.0),
        Vec2::new(-3.6, -4.6),
    ],
    // Right lean.
    &[
        Vec2::new(0.5, 0.0),
        Vec2::new(2.5, 0.0),
        Vec2::new(3.6, -4.4),
    ],
];

/// Pebble silhouette — a flat oval-ish polygon resting on the ground.
const PEBBLE: &[Vec2] = &[
    Vec2::new(-3.0, 0.0),
    Vec2::new(-1.8, -2.0),
    Vec2::new(1.8, -2.2),
    Vec2::new(3.2, -0.4),
    Vec2::new(1.4, 0.4),
    Vec2::new(-1.6, 0.3),
];

/// Bake one tuft into the batched mesh: rotate + scale its local
/// vertices, translate to the band's flipped anchor, and add as a
/// polygon with the tuft's color baked in. No per-draw rotation or
/// scale needs to follow at render time.
fn append_tuft(mb: &mut MeshBuilder, tuft: &Tuft) -> GameResult<()> {
    let (sin_r, cos_r) = tuft.rotation.sin_cos();
    let scale = tuft.scale;
    let anchor = Vec2::new(tuft.x, -tuft.y);

    let (polys, color): (&[&[Vec2]], Color) = match tuft.style {
        TuftStyle::Grass => (GRASS_BLADES, GRASS_COLOR),
        TuftStyle::Pebble => (&[PEBBLE], PEBBLE_COLOR),
    };

    // Reused buffer per polygon — avoids alloc-per-tuft.
    let mut transformed: Vec<Vec2> = Vec::with_capacity(8);
    for points in polys {
        transformed.clear();
        for p in points.iter() {
            let sx = p.x * scale;
            let sy = p.y * scale;
            // Screen-space rotation (CCW math, CW visual): standard
            // 2x2 rotation matrix matches `DrawParam::rotation`.
            transformed.push(Vec2::new(
                anchor.x + cos_r * sx - sin_r * sy,
                anchor.y + sin_r * sx + cos_r * sy,
            ));
        }
        mb.polygon(DrawMode::fill(), &transformed, color)?;
    }
    Ok(())
}

/// One dirt patch — a small, randomly-shaped polygon sitting in or just
/// below the soil surface. Built once at mesh-bake time; the resulting
/// polygon has vertex colors baked in so the draw call uses a plain
/// white tint like everything else in the batched mesh.
#[derive(Debug, Clone)]
struct DirtPatch {
    /// World position of the patch center. Y-up.
    center: Vec2,
    /// Local-space polygon vertices around the origin (already scaled
    /// and rotated). Rendered as a single concave-allowed polygon.
    points: Vec<Vec2>,
    color: Color,
}

/// Scatter dirt patches across `profile`. Each patch picks a tint from
/// `DIRT_PATCH_DELTAS` relative to `soil`, a random depth below the
/// surface, and an irregular 6-sided silhouette. Deterministic in the
/// profile's heightmap so two clients render identical terrain.
fn build_dirt_patches(profile: &GroundProfile, soil: Color) -> Vec<DirtPatch> {
    let seed = PATCH_SEED_BASE
        ^ profile.heights.first().copied().unwrap_or(0.0).to_bits() as u64
        ^ (profile.heights.len() as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    let w = profile.world_width;
    if w <= 0.0 {
        return Vec::new();
    }
    let approx_count = (w / PATCH_SPACING).round() as usize;
    let mut patches = Vec::with_capacity(approx_count);
    let mut x = 0.0_f32;
    while x < w {
        let jitter: f32 = (rng.gen::<f32>() - 0.5) * PATCH_SPACING * 0.9;
        let here = (x + jitter).clamp(0.0, w - 0.5);
        // Bias depth toward the top of the soil so most patches read on
        // the visible "skin." `t^2` skews the uniform random toward 0
        // (near-surface) while still allowing the occasional deeper clod.
        let t: f32 = rng.gen();
        let depth = t * t * PATCH_MAX_DEPTH;
        let surface_y = profile.height_at(here);
        let center = Vec2::new(here, surface_y - depth);

        let delta = DIRT_PATCH_DELTAS[rng.gen_range(0..DIRT_PATCH_DELTAS.len())];
        let color = Color::new(
            (soil.r + delta.0).clamp(0.0, 1.0),
            (soil.g + delta.1).clamp(0.0, 1.0),
            (soil.b + delta.2).clamp(0.0, 1.0),
            1.0,
        );

        let radius = 2.4 + rng.gen::<f32>() * 3.2;
        let rotation = rng.gen::<f32>() * std::f32::consts::TAU;
        let points = build_patch_polygon(&mut rng, radius, rotation);

        patches.push(DirtPatch { center, points, color });
        x += PATCH_SPACING;
    }
    patches
}

/// Produce an irregular 6-vertex blob around the origin with the given
/// average `radius` and base `rotation`. Per-vertex radius jitter keeps
/// the silhouette from looking like a stamped circle.
fn build_patch_polygon(rng: &mut ChaCha8Rng, radius: f32, rotation: f32) -> Vec<Vec2> {
    const SIDES: usize = 6;
    let mut out = Vec::with_capacity(SIDES);
    let step = std::f32::consts::TAU / SIDES as f32;
    for i in 0..SIDES {
        let angle = rotation + step * i as f32;
        let r = radius * (0.65 + rng.gen::<f32>() * 0.55);
        // World is Y-up but the soil polygon was authored Y-down (see
        // `append_ground_polygon` flipping with `-h`); patches are baked
        // into the same coordinate space so we flip Y here too.
        out.push(Vec2::new(angle.cos() * r, -angle.sin() * r));
    }
    out
}

/// Translate `patch` into mesh-space (Y-down) and append as a filled
/// polygon with the per-patch tint baked into the vertex colors.
fn append_dirt_patch(mb: &mut MeshBuilder, patch: &DirtPatch) -> GameResult<()> {
    let anchor = Vec2::new(patch.center.x, -patch.center.y);
    let transformed: Vec<Vec2> = patch
        .points
        .iter()
        .map(|p| Vec2::new(anchor.x + p.x, anchor.y + p.y))
        .collect();
    mb.polygon(DrawMode::fill(), &transformed, patch.color)?;
    Ok(())
}

/// Cheap order-sensitive hash of every band's heightmap. Used to
/// detect when the renderer's cache is stale without comparing whole
/// `Vec<f32>`s.
fn signature(bands: &[TerrainBand]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mix = |h: &mut u64, x: u64| {
        *h ^= x;
        *h = h.wrapping_mul(0x0000_0100_0000_01B3);
    };
    for b in bands {
        mix(&mut h, b.kind as u64);
        mix(&mut h, b.profile.heights.len() as u64);
        mix(&mut h, b.profile.world_width.to_bits() as u64);
        mix(&mut h, b.profile.spacing.to_bits() as u64);
        for s in &b.profile.heights {
            mix(&mut h, s.to_bits() as u64);
        }
    }
    h
}

/// World-X offsets of wrap copies whose horizontal extent overlaps the
/// viewport. With `view_width < world_width`, at most two of the three
/// candidates land in the viewport.
fn copies_for(center_x: f32, world_w: f32, view_w: f32) -> Vec<f32> {
    if world_w <= 0.0 {
        return vec![];
    }
    let half_view = view_w * 0.5 + 32.0;
    let mut out = Vec::with_capacity(3);
    for dx in [-world_w, 0.0, world_w] {
        let lo = dx;
        let hi = dx + world_w;
        if hi >= center_x - half_view && lo <= center_x + half_view {
            out.push(dx);
        }
    }
    out
}
