//! Terrain — the surfaces that fill the bottom (and eventually other
//! parts) of the world.
//!
//! A terrain layout is a stack of horizontal bands. Today there is one
//! kind (`Ground`) and one band; the structure is set up so future
//! levels can introduce water, mountains, etc. by adding new
//! `TerrainKind` variants and stacking different bands per level.
//!
//! Each band carries a [`GroundProfile`]: a heightmap of evenly-spaced
//! samples across the full world width. Segments between adjacent
//! samples are linear, so silhouettes are made of angled lines — easy
//! to render and easy to collide against without rounded-arc math. The
//! profile wraps with the world (sample N == sample 0) so the X seam
//! never shows as a step.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::util::Vec2;

/// Lower bound on any ground sample (Y-up). Defines the deepest a valley
/// can go. Kept positive so the world's bottom edge isn't above the
/// terrain.
pub const GROUND_MIN_HEIGHT: f32 = 18.0;
/// Upper bound on any ground sample (Y-up). Bounds hill peaks so flight
/// space stays generous and the camera's vertical clamp behaves.
pub const GROUND_MAX_HEIGHT: f32 = 130.0;

/// Default sample spacing along the X axis (world units) for a
/// procedurally-generated profile. Smaller = finer hills, more wire
/// bytes and more renderer geometry; larger = blockier, cheaper. 128
/// gives 25 segments on the 3200-wide world — coarse enough that the
/// terrain mesh stays small, fine enough that hills still read as
/// smooth curves rather than triangles.
pub const PROFILE_SPACING: f32 = 128.0;

/// Maximum |height delta| between two adjacent samples after generation.
/// With `PROFILE_SPACING = 128`, this caps the slope at ~0.59 (≈ 30°)
/// so hills stay walkable for ground vehicles and the per-x circle-vs-
/// height collision approximation stays accurate.
pub const MAX_SLOPE_DELTA: f32 = 75.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerrainKind {
    Ground,
}

/// Heightmap describing the top surface of a single terrain band across
/// the full world width. Heights are sampled at evenly-spaced X positions
/// starting at `x = 0`. The X axis is toroidal: the implicit final sample
/// at `x = world_width` equals `heights[0]`, so wraps don't introduce a
/// step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundProfile {
    /// Total X extent the heights cover. Must match `World::config.world_size.x`.
    pub world_width: f32,
    /// Distance in world units between adjacent samples.
    pub spacing: f32,
    /// Surface height (Y-up) at each sample. `heights[i]` lives at
    /// `x = i * spacing`. Wraps: the value at `x = world_width` is
    /// `heights[0]`.
    pub heights: Vec<f32>,
}

impl GroundProfile {
    /// A perfectly flat profile of height `h` spanning `world_width`.
    /// Used by tests that don't care about hills.
    pub fn flat(world_width: f32, h: f32) -> Self {
        let n = ((world_width / PROFILE_SPACING).round() as usize).max(2);
        Self {
            world_width,
            spacing: world_width / n as f32,
            heights: vec![h; n],
        }
    }

    /// Linearly interpolated surface height at world-X `x`. Accepts any
    /// real `x`; values are wrapped into `[0, world_width)`.
    pub fn height_at(&self, x: f32) -> f32 {
        let n = self.heights.len();
        if n == 0 || self.world_width <= 0.0 || self.spacing <= 0.0 {
            return 0.0;
        }
        let xw = x.rem_euclid(self.world_width);
        let p = xw / self.spacing;
        let i0 = (p.floor() as i64).rem_euclid(n as i64) as usize;
        let i1 = (i0 + 1) % n;
        let t = p - p.floor();
        self.heights[i0] * (1.0 - t) + self.heights[i1] * t
    }

    /// Largest height across every sample. Used to size the camera's
    /// vertical clamp and the off-screen "anything above this is sky"
    /// fast paths.
    pub fn max_height(&self) -> f32 {
        self.heights.iter().copied().fold(0.0_f32, f32::max)
    }

    /// Smallest height across every sample. Used by the camera so it
    /// can follow the pilot into valleys without leaving the floor.
    pub fn min_height(&self) -> f32 {
        self.heights.iter().copied().fold(f32::INFINITY, f32::min)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainBand {
    pub kind: TerrainKind,
    pub profile: GroundProfile,
}

/// Build the default terrain for a fresh world: a single `Ground` band
/// with a procedurally-generated hilly profile seeded from `seed`.
pub fn default_terrain(world_width: f32, seed: u64) -> Vec<TerrainBand> {
    vec![TerrainBand {
        kind: TerrainKind::Ground,
        profile: generate_ground_profile(world_width, seed),
    }]
}

/// Generate a deterministic hilly ground profile. Samples are built from
/// a small sum of low-integer-frequency sines (so the profile is exactly
/// periodic over `world_width`), with seeded random phases per harmonic
/// for variety. A post-pass smooths any leftover too-steep transitions
/// down to `MAX_SLOPE_DELTA`.
pub fn generate_ground_profile(world_width: f32, seed: u64) -> GroundProfile {
    use rand::Rng;
    use std::f32::consts::TAU;

    let n = ((world_width / PROFILE_SPACING).round() as usize).max(4);
    let spacing = world_width / n as f32;
    // Distinct namespace from the world's RNG so terrain shape is stable
    // regardless of how many sim ticks have run when it's built.
    let mut rng = ChaCha8Rng::seed_from_u64(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));

    let center = (GROUND_MIN_HEIGHT + GROUND_MAX_HEIGHT) * 0.5;
    let amp = (GROUND_MAX_HEIGHT - GROUND_MIN_HEIGHT) * 0.5;
    // Three harmonics with integer frequencies so each sine has an
    // integer number of cycles across the world — guarantees seam
    // continuity. Amplitudes decay so the lowest frequency dominates
    // the silhouette and the higher frequencies just add bumpiness.
    let harmonics: [(f32, f32, f32); 3] = [
        (3.0, 0.55, rng.gen::<f32>() * TAU),
        (5.0, 0.27, rng.gen::<f32>() * TAU),
        (7.0, 0.18, rng.gen::<f32>() * TAU),
    ];

    let mut heights = Vec::with_capacity(n);
    for i in 0..n {
        let t = (i as f32 / n as f32) * TAU;
        let mut h = 0.0_f32;
        for (w, a, phi) in harmonics {
            h += a * (w * t + phi).sin();
        }
        let raw = center + amp * h;
        heights.push(raw.clamp(GROUND_MIN_HEIGHT, GROUND_MAX_HEIGHT));
    }

    smooth_slopes(&mut heights);
    GroundProfile { world_width, spacing, heights }
}

/// Walk the profile and gently flatten any pair of adjacent samples
/// whose height delta exceeds `MAX_SLOPE_DELTA`. Two passes (forward +
/// backward) converge quickly for inputs that are already mostly within
/// bound thanks to the harmonic clamp above.
fn smooth_slopes(heights: &mut [f32]) {
    let n = heights.len();
    if n < 2 {
        return;
    }
    let cap = MAX_SLOPE_DELTA;
    for _ in 0..2 {
        for i in 0..n {
            let j = (i + 1) % n;
            let delta = heights[j] - heights[i];
            if delta.abs() > cap {
                let mid = (heights[i] + heights[j]) * 0.5;
                let half = cap * 0.5;
                let (lo, hi) = if delta >= 0.0 {
                    (mid - half, mid + half)
                } else {
                    (mid + half, mid - half)
                };
                heights[i] = lo.clamp(GROUND_MIN_HEIGHT, GROUND_MAX_HEIGHT);
                heights[j] = hi.clamp(GROUND_MIN_HEIGHT, GROUND_MAX_HEIGHT);
            }
        }
    }
}

/// World-Y of the ground surface at horizontal position `x`. Picks the
/// tallest `Ground` band's height at `x`; non-Ground bands (future water,
/// mountains, ...) are ignored here so callers asking "where can a tank
/// roll?" still get the dirt level.
pub fn ground_surface_at(x: f32, bands: &[TerrainBand]) -> f32 {
    bands
        .iter()
        .filter(|b| b.kind == TerrainKind::Ground)
        .map(|b| b.profile.height_at(x))
        .fold(0.0_f32, f32::max)
}

/// Whether the given X is passable for a ground vehicle (a tank). Today
/// always `true`; future water / mountain bands should return `false` at
/// the X-ranges they cover so tanks stop at their edges.
pub fn passable_for_ground_vehicle(_x: f32, _bands: &[TerrainBand]) -> bool {
    true
}

/// Highest top-surface point across every band — the upper bound flying
/// objects need to clear. Computed once per call from sample maxes.
pub fn surface_y(bands: &[TerrainBand]) -> f32 {
    bands.iter().map(|b| b.profile.max_height()).fold(0.0_f32, f32::max)
}

/// Lowest top-surface point across every band — the deepest a valley
/// reaches. Used by the camera to set the vertical clamp so the player
/// can dive into valleys without being blocked by a max-altitude floor.
pub fn min_surface_y(bands: &[TerrainBand]) -> f32 {
    let mut min = f32::INFINITY;
    for b in bands {
        min = min.min(b.profile.min_height());
    }
    if min.is_finite() {
        min
    } else {
        0.0
    }
}

/// Local top surface at world-X `x` — `max(profile.height_at(x))` across
/// every band. What a shot or low-flying enemy should bounce off of.
pub fn surface_y_at(x: f32, bands: &[TerrainBand]) -> f32 {
    bands.iter().map(|b| b.profile.height_at(x)).fold(0.0_f32, f32::max)
}

/// If a circle at `(pos, bbox)` overlaps any terrain band, return the
/// topmost overlapping band's kind. Each band is tested with
/// [`circle_dips_into_band`], which samples the circle's underside at
/// a few X positions to catch glancing impacts on angled segments
/// without an analytical segment-vs-circle solve.
pub fn terrain_hit(pos: Vec2, bbox: f32, bands: &[TerrainBand]) -> Option<TerrainKind> {
    let mut best: Option<&TerrainBand> = None;
    for band in bands {
        if !circle_dips_into_band(pos, bbox, band) {
            continue;
        }
        match best {
            Some(b) if b.profile.height_at(pos.x) >= band.profile.height_at(pos.x) => {}
            _ => best = Some(band),
        }
    }
    best.map(|b| b.kind)
}

/// Does a circle of radius `bbox` at `pos` dip into `band`'s top
/// surface? We sample five X positions across the circle's footprint
/// and at each compare the circle's underside (a function of `dx`) to
/// the local ground height. Cheap and accurate for the slopes we allow
/// (≤ `MAX_SLOPE_DELTA` per sample).
fn circle_dips_into_band(pos: Vec2, bbox: f32, band: &TerrainBand) -> bool {
    const SAMPLES: i32 = 5;
    let half = (SAMPLES - 1) / 2;
    let step = bbox / half as f32;
    for i in -half..=half {
        let dx = i as f32 * step;
        let underside_dy = (bbox * bbox - dx * dx).max(0.0).sqrt();
        let underside = pos.y - underside_dy;
        let h = band.profile.height_at(pos.x + dx);
        if underside <= h {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_W: f32 = 3200.0;

    fn flat_bands(h: f32) -> Vec<TerrainBand> {
        vec![TerrainBand {
            kind: TerrainKind::Ground,
            profile: GroundProfile::flat(TEST_W, h),
        }]
    }

    #[test]
    fn flat_profile_interpolates_to_same_height() {
        let p = GroundProfile::flat(TEST_W, 30.0);
        assert!((p.height_at(0.0) - 30.0).abs() < 1e-4);
        assert!((p.height_at(100.0) - 30.0).abs() < 1e-4);
        assert!((p.height_at(TEST_W - 0.1) - 30.0).abs() < 1e-4);
    }

    #[test]
    fn no_hit_when_circle_is_well_above_band() {
        let bands = flat_bands(26.0);
        // Center well above ground; player bbox 12 → bottom at 100.
        assert!(terrain_hit(Vec2::new(640.0, 112.0), 12.0, &bands).is_none());
    }

    #[test]
    fn hit_when_circle_dips_into_band() {
        let bands = flat_bands(26.0);
        // Center just above the surface so the circle's bottom sinks in.
        let pos = Vec2::new(640.0, 26.0 + 11.5);
        assert_eq!(terrain_hit(pos, 12.0, &bands), Some(TerrainKind::Ground));
    }

    #[test]
    fn generated_profile_is_deterministic_for_seed() {
        let a = generate_ground_profile(TEST_W, 42);
        let b = generate_ground_profile(TEST_W, 42);
        assert_eq!(a.heights, b.heights);
        let c = generate_ground_profile(TEST_W, 43);
        assert_ne!(a.heights, c.heights, "different seed should diverge");
    }

    #[test]
    fn generated_profile_wraps_continuously() {
        let p = generate_ground_profile(TEST_W, 7);
        let h0 = p.height_at(0.0);
        let h_just_before = p.height_at(TEST_W - 1e-2);
        // Must close the loop within one MAX_SLOPE_DELTA per sample.
        assert!(
            (h0 - h_just_before).abs() < MAX_SLOPE_DELTA + 1.0,
            "seam discontinuity: {h0} vs {h_just_before}",
        );
    }

    #[test]
    fn generated_profile_respects_bounds() {
        let p = generate_ground_profile(TEST_W, 123);
        for h in &p.heights {
            assert!(
                *h >= GROUND_MIN_HEIGHT && *h <= GROUND_MAX_HEIGHT,
                "out-of-bounds height {h}",
            );
        }
    }

    #[test]
    fn slopes_are_bounded_after_smoothing() {
        let p = generate_ground_profile(TEST_W, 31415);
        for i in 0..p.heights.len() {
            let j = (i + 1) % p.heights.len();
            let delta = (p.heights[j] - p.heights[i]).abs();
            assert!(
                delta <= MAX_SLOPE_DELTA + 1e-3,
                "slope {delta} between samples {i}, {j} exceeds cap",
            );
        }
    }

    #[test]
    fn hilly_profile_lifts_collision_on_peaks() {
        // Park a player just above ground level in a flat world — no hit.
        // Now run the same player position with a hill that peaks at
        // their footprint and confirm they crash.
        let mut bumpy = GroundProfile::flat(TEST_W, 20.0);
        let center = bumpy.heights.len() / 2;
        bumpy.heights[center] = 120.0;
        bumpy.heights[center - 1] = 100.0;
        bumpy.heights[center + 1] = 100.0;
        let bands = vec![TerrainBand {
            kind: TerrainKind::Ground,
            profile: bumpy,
        }];
        let x = (center as f32) * PROFILE_SPACING;
        // Well above the original flat ground but inside the hill peak.
        let pos = Vec2::new(x, 100.0);
        assert_eq!(terrain_hit(pos, 12.0, &bands), Some(TerrainKind::Ground));
    }
}
