use rand::RngCore;
use serde::{Deserialize, Serialize};

pub type Vec2 = glam::Vec2;

/// Unit vector for `angle` radians.
///
/// World space is Y-up. The convention is `(sin, cos)` so `facing == 0`
/// points along +Y. Increasing `angle` rotates clockwise on screen,
/// matching ggez's `DrawParam::rotation` direction.
pub fn vec_from_angle(angle: f32) -> Vec2 {
    Vec2::new(angle.sin(), angle.cos())
}

/// Returns a uniformly-random unit-direction vector scaled by a
/// uniformly-random magnitude in `[0, max_magnitude)`.
pub fn random_vec(rng: &mut impl RngCore, max_magnitude: f32) -> Vec2 {
    let angle = rand_unit(rng) * 2.0 * std::f32::consts::PI;
    let mag = rand_unit(rng) * max_magnitude;
    vec_from_angle(angle) * mag
}

/// Uniform `f32` in `[0, 1)` from any `RngCore`. Avoids pulling in `rand`'s
/// distribution machinery so the sim builds with `default-features = false`.
pub fn rand_unit(rng: &mut impl RngCore) -> f32 {
    // Take 24 bits so the result fits exactly into an `f32` mantissa.
    let bits = rng.next_u32() >> 8;
    (bits as f32) * (1.0 / (1u32 << 24) as f32)
}

/// If `velocity` exceeds `max`, returns it scaled to `max`. Otherwise `None`.
pub fn clamp_velocity(velocity: Vec2, max: f32) -> Option<Vec2> {
    let norm_sq = velocity.length_squared();
    if norm_sq > max.powi(2) {
        Some(velocity / norm_sq.sqrt() * max)
    } else {
        None
    }
}

/// Toroidal wrap of a single coordinate within `[0, size)`.
pub fn wrap_coord(c: f32, size: f32) -> f32 {
    if c > size {
        c - size
    } else if c < 0.0 {
        c + size
    } else {
        c
    }
}

/// Clamp a position into `[0, height]` and zero out velocity that points
/// into the wall. Used for entities that should rest against a hard
/// floor/ceiling rather than rebound (i.e. players under gravity).
pub fn clamp_y(pos: &mut Vec2, vel: &mut Vec2, height: f32) {
    if pos.y < 0.0 {
        pos.y = 0.0;
        vel.y = vel.y.max(0.0);
    } else if pos.y > height {
        pos.y = height;
        vel.y = vel.y.min(0.0);
    }
}

/// Reflect a position back inside `[low, high]` and flip vertical velocity.
/// Used for objects that should bounce off the floor/ceiling rather than
/// pile up on it (shots).
pub fn bounce_y(pos: &mut Vec2, vel: &mut Vec2, low: f32, high: f32) {
    if pos.y < low {
        pos.y = 2.0 * low - pos.y;
        vel.y = vel.y.abs();
    } else if pos.y > high {
        pos.y = 2.0 * high - pos.y;
        vel.y = -vel.y.abs();
    }
}

/// Serializable Vec2 wrapper used in protocol messages and tests.
/// Direct `glam::Vec2` is also serde-able with the `serde` feature, but
/// having a tiny stable shape here means protocol changes don't depend on
/// glam's wire format.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WireVec2 {
    pub x: f32,
    pub y: f32,
}

impl From<Vec2> for WireVec2 {
    fn from(v: Vec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

impl From<WireVec2> for Vec2 {
    fn from(v: WireVec2) -> Self {
        Vec2::new(v.x, v.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn approx(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn vec_from_angle_zero_points_up() {
        assert!(approx(vec_from_angle(0.0), Vec2::new(0.0, 1.0)));
    }

    #[test]
    fn vec_from_angle_quarter_points_right() {
        assert!(approx(vec_from_angle(FRAC_PI_2), Vec2::new(1.0, 0.0)));
    }

    #[test]
    fn vec_from_angle_half_points_down() {
        assert!(approx(vec_from_angle(PI), Vec2::new(0.0, -1.0)));
    }

    #[test]
    fn clamp_velocity_under_max_is_none() {
        assert!(clamp_velocity(Vec2::new(3.0, 4.0), 10.0).is_none());
    }

    #[test]
    fn clamp_velocity_over_max_clamps_to_max() {
        let v = clamp_velocity(Vec2::new(30.0, 40.0), 10.0).unwrap();
        assert!((v.length() - 10.0).abs() < 1e-5);
    }
}
