//! Terrain — the surfaces that fill the bottom (and eventually other parts)
//! of the world. A terrain layout is just a stack of horizontal bands. For
//! now there is one kind (`Ground`) and one band; the structure is set up
//! so future levels can introduce water, hills, etc. by adding new
//! `TerrainKind` variants and stacking different bands per level.
//!
//! Bands span the full world width. Each band records its `top_y` (the
//! highest world-Y the band reaches, in Y-up coordinates). The band
//! extends from `0.0` up to `top_y`. Multi-band layouts (e.g. water
//! sitting on top of seabed) can be modelled by ordering bands by
//! ascending `top_y` and reading the topmost band an entity overlaps.

use serde::{Deserialize, Serialize};

use crate::util::Vec2;

/// Default ground band thickness. Small enough that the play area is still
/// the dominant region of the screen, large enough to read visually as
/// "the floor."
pub const GROUND_HEIGHT: f32 = 26.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerrainKind {
    Ground,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TerrainBand {
    pub kind: TerrainKind,
    /// World-Y of the band's top surface (Y-up). The band fills `[0, top_y]`.
    pub top_y: f32,
}

/// The default terrain layout: a single ground band at the bottom.
pub fn default_terrain() -> Vec<TerrainBand> {
    vec![TerrainBand {
        kind: TerrainKind::Ground,
        top_y: GROUND_HEIGHT,
    }]
}

/// World-Y of the ground surface at horizontal position `x`. Today the
/// terrain is flat so `x` is ignored, but the signature already accepts
/// it so future hill / mountain bands can return a per-x height without
/// every caller having to switch helper. Returns `0.0` when no Ground
/// band is present.
pub fn ground_surface_at(_x: f32, bands: &[TerrainBand]) -> f32 {
    bands
        .iter()
        .filter(|b| b.kind == TerrainKind::Ground)
        .map(|b| b.top_y)
        .fold(0.0_f32, f32::max)
}

/// Whether the given X is passable for a ground vehicle (a tank). Today
/// the ground is uniformly solid so this is always `true`; once water /
/// mountain bands exist they should return `false` at the X-ranges they
/// cover so tanks stop at their edges instead of rolling through.
pub fn passable_for_ground_vehicle(_x: f32, _bands: &[TerrainBand]) -> bool {
    true
}

/// Highest `top_y` across all bands — the upper surface that flying
/// objects (shots, enemies) bounce off of. Players already crash on
/// any contact via `terrain_hit`; this is for the rebound path. Returns
/// `0.0` when no bands are present so the world bottom is still the
/// effective floor.
pub fn surface_y(bands: &[TerrainBand]) -> f32 {
    bands.iter().map(|b| b.top_y).fold(0.0_f32, f32::max)
}

/// If a circle of `(pos, bbox)` overlaps any terrain band, return the
/// topmost overlapping band's kind. Topmost wins so layered bands behave
/// the way you'd expect — splash the water before crunching the seabed.
pub fn terrain_hit(pos: Vec2, bbox: f32, bands: &[TerrainBand]) -> Option<TerrainKind> {
    let bottom = pos.y - bbox;
    let mut best: Option<&TerrainBand> = None;
    for band in bands {
        if bottom <= band.top_y {
            match best {
                Some(b) if b.top_y >= band.top_y => {}
                _ => best = Some(band),
            }
        }
    }
    best.map(|b| b.kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_hit_when_above_band() {
        let bands = default_terrain();
        // Center well above ground; player bbox 12 → bottom at 100.
        assert!(terrain_hit(Vec2::new(640.0, 112.0), 12.0, &bands).is_none());
    }

    #[test]
    fn hit_when_circle_dips_into_band() {
        let bands = default_terrain();
        // Center such that bottom of circle is exactly on top_y - epsilon.
        let pos = Vec2::new(640.0, GROUND_HEIGHT + 11.5);
        assert_eq!(terrain_hit(pos, 12.0, &bands), Some(TerrainKind::Ground));
    }
}
