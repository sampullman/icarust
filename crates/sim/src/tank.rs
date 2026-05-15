//! Ground-vehicle tank AI. Rolls along the terrain toward the nearest
//! live player, swivels its turret to track them, and lobs
//! gravity-affected shells. Mirrors the shape of `enemy::step` so a
//! future player-controlled tank can swap the AI target selection for a
//! `PlayerInput` without touching the chassis physics or firing logic.
//!
//! All math is pure: it reads only entity state and returns the new
//! chassis + turret state plus a fire intent. World mutation (spawning
//! shells, advancing cooldown, snapping the chassis to terrain) lives in
//! `world::tick`.
//!
//! Terrain interactions are kept abstract: today the tank assumes flat
//! ground at `surface_y`, but the call site is set up so future water /
//! mountain bands can refuse passage by reporting a different surface
//! (or `None`) without changing the AI.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::util::Vec2;

/// Collision radius. Sized to match the chassis silhouette — wide enough
/// to cover the hull horizontally but tight enough that shots flying well
/// above the cannon tip don't register as hits. The turret/cannon are
/// intentionally outside the hitbox so a clean strike has to land on the
/// armored body.
pub const TANK_BBOX: f32 = 13.0;

/// Vertical offset from the terrain surface to the entity's `pos.y`. The
/// chassis is authored with its center at the local origin, so this is
/// the distance from the bottom of the treads to the chassis center.
/// Kept separate from `TANK_BBOX` so the hitbox can shrink without
/// floating the tank above the ground.
pub const TANK_GROUND_OFFSET: f32 = 7.0;
/// Top rolling speed (world units/s). Deliberately slow — the tank is a
/// patient artillery threat, not a chaser.
pub const TANK_MAX_SPEED: f32 = 90.0;
/// Acceleration while rolling toward target (units/s²).
pub const TANK_THRUST: f32 = 140.0;
/// Drag while idling in firing range (per second).
pub const TANK_DRAG: f32 = 2.5;
/// Turret turn rate (radians/s). Slower than the ship enemy's turn rate
/// so the player can sidestep tank fire by changing altitude quickly.
pub const TANK_TURRET_TURN_RATE: f32 = 1.4;
/// Half-angle of the firing cone (radians) — turret must be within this
/// of the aim solution before pulling the trigger.
pub const TANK_FIRE_CONE: f32 = 0.14;
/// Maximum distance at which the tank attempts to fire.
pub const TANK_FIRE_RANGE: f32 = 950.0;
/// Horizontal distance at which the tank stops rolling and settles
/// into a firing position. Inside this radius the tank just shoots.
pub const TANK_APPROACH_DIST: f32 = 520.0;
/// Seconds between tank shots. Significantly slower than the ship
/// enemy's `ENEMY_SHOT_TIME` (0.9s) so the shells read as a heavy,
/// infrequent threat rather than a stream of bullets.
pub const TANK_SHOT_TIME: f32 = 2.2;
/// Muzzle velocity (units/s) — faster than the ship enemy's 260 so the
/// shells reach across the play area before gravity drops them. Tuned
/// up from the original 460 so a shell's first arc carries it further
/// before the player can simply outrun it.
pub const TANK_SHOT_SPEED: f32 = 540.0;
/// Lifetime override for tank shells. The default `SHOT_LIFE` (2s)
/// expires partway through a steep arc, so artillery is given a longer
/// budget. In practice most shells die earlier by detonating on
/// terrain or impact; this just keeps a high-arc shell airborne long
/// enough to finish its parabola.
pub const TANK_SHELL_LIFE: f32 = 5.0;
/// Magnitude of downward acceleration applied to tank shells (Y-up
/// world; the world stores this as `Vec2::new(0, -mag)` in the shell's
/// `accel`).
pub const TANK_SHOT_GRAVITY: f32 = 240.0;
/// Hit radius for tank shells — wider than the default `SHOT_BBOX` so
/// the heavy shell reads as bigger on screen *and* lands hits that a
/// pinpoint ship bullet would miss. Damage value is selected via the
/// `ShotOwner::Tank` variant.
pub const TANK_SHOT_BBOX: f32 = 9.0;
/// Tank starts at full armor; takes two shots to crack.
pub const TANK_HP: i16 = 2;

/// Lateral acceleration (units/s²) applied while the tank is in firing
/// range. Combined with `TANK_DODGE_FREQ` this produces a small
/// side-to-side sway that simulates the chassis weaving to dodge shots
/// even when it has nothing else to do.
pub const TANK_DODGE_ACCEL: f32 = 220.0;
/// Angular frequency (radians/s) of the dodge sway. Period ≈ 2π / FREQ.
pub const TANK_DODGE_FREQ: f32 = 1.7;

/// One AI step result.
#[derive(Debug, Clone, Copy)]
pub struct TankStep {
    pub vel: Vec2,
    /// Chassis orientation after this step. Tanks bind their body to
    /// horizontal motion so this lives in `{+FRAC_PI_2, -FRAC_PI_2}` once
    /// the tank has moved at least once.
    pub body_facing: f32,
    /// Turret aim direction. Independent of chassis so the cannon can
    /// track an overhead target while the chassis sits still.
    pub turret_facing: f32,
    /// True if the tank wants to fire this tick. Caller is responsible
    /// for checking the cooldown.
    pub fire: bool,
}

/// Compute the next chassis velocity, body angle, turret aim, and fire
/// intent for one tank.
///
/// `target` is `None` when there are no live players — the tank idles
/// (drag-only, no fire). `world_width` is needed to handle toroidal
/// X-wrap when computing the shortest path to the target.
///
/// `dodge_phase` is a per-tank scalar (seconds plus a per-entity offset)
/// that drives the in-range side-to-side sway. The caller derives it so
/// every tank weaves on its own rhythm.
pub fn step(
    pos: Vec2,
    vel: Vec2,
    body_facing: f32,
    turret_facing: f32,
    target: Option<Vec2>,
    world_width: f32,
    dodge_phase: f32,
    dt: f32,
) -> TankStep {
    let Some(target_pos) = target else {
        return TankStep {
            vel: apply_drag(vel, dt),
            body_facing,
            turret_facing,
            fire: false,
        };
    };

    let to_target = shortest_offset(pos, target_pos, world_width);
    let dist = to_target.length();

    // Chassis: roll toward target unless we're already in firing range.
    let mut new_vel = vel;
    let mut new_body = body_facing;
    let dx = to_target.x;
    let abs_dx = dx.abs();
    if abs_dx > TANK_APPROACH_DIST {
        let dir = dx.signum();
        new_vel.x += dir * TANK_THRUST * dt;
        // Snap body angle to one of two orientations so the chassis
        // doesn't try to face a target overhead.
        new_body = if dir >= 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 };
    } else {
        // Within range — instead of just braking, weave side to side so
        // the tank simulates dodging fire from above. Drag is still
        // applied (with half its strength so the sway has room to
        // build) and `TANK_DODGE_ACCEL` injects a sinusoidal lateral
        // force keyed to the per-tank phase.
        let sway = (dodge_phase * TANK_DODGE_FREQ).sin() * TANK_DODGE_ACCEL;
        new_vel.x += sway * dt;
        new_vel.x *= (1.0 - TANK_DRAG * 0.5 * dt).max(0.0);
        // Keep the chassis facing the side it's currently moving
        // toward so the asymmetric hull silhouette matches the motion.
        // Tiny speeds inherit the previous body angle to avoid flicker
        // at the dodge zero-crossing.
        if new_vel.x.abs() > 6.0 {
            new_body = if new_vel.x >= 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 };
        }
    }
    // Cap horizontal speed. Vertical movement is zeroed: the chassis
    // sits on the terrain and the caller pins its Y every tick.
    new_vel.x = new_vel.x.clamp(-TANK_MAX_SPEED, TANK_MAX_SPEED);
    new_vel.y = 0.0;

    // Turret: aim with a parabolic lead to compensate for shell gravity.
    // Flight time approximates as `dist / shot_speed`; the shell drops
    // `0.5 * g * t^2` in that time, so the turret aims that much higher.
    // This is a one-iteration approximation — close enough at typical
    // ranges and cheap to compute every tick.
    let t = (dist / TANK_SHOT_SPEED).max(0.05);
    let lead_y = 0.5 * TANK_SHOT_GRAVITY * t * t;
    // `atan2(dx, dy)` matches our (sin, cos) angle convention: a target
    // straight overhead at +Y gives angle 0.
    let aim_off = Vec2::new(to_target.x, to_target.y + lead_y);
    let target_angle = aim_off.x.atan2(aim_off.y);
    let new_turret = steer_toward(turret_facing, target_angle, TANK_TURRET_TURN_RATE * dt);

    let aim_error = angular_delta(new_turret, target_angle).abs();
    let fire = aim_error < TANK_FIRE_CONE && dist < TANK_FIRE_RANGE;

    TankStep {
        vel: new_vel,
        body_facing: new_body,
        turret_facing: new_turret,
        fire,
    }
}

/// Direction the tank chassis is rolling, expressed as `+1` (right),
/// `-1` (left), or `0` (still). Convenience for renderers and any future
/// kinematics that need to mirror the body without rotating it.
pub fn body_dir_from_facing(body_facing: f32) -> f32 {
    if body_facing > 0.0 {
        1.0
    } else if body_facing < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Shortest vector from `from` to `to`, accounting for X-wrap.
fn shortest_offset(from: Vec2, to: Vec2, world_width: f32) -> Vec2 {
    let mut dx = to.x - from.x;
    let half = world_width * 0.5;
    if dx > half {
        dx -= world_width;
    } else if dx < -half {
        dx += world_width;
    }
    Vec2::new(dx, to.y - from.y)
}

/// Shortest signed angular delta `target - current` in `[-PI, PI]`.
fn angular_delta(current: f32, target: f32) -> f32 {
    let mut d = (target - current) % TAU;
    if d > PI {
        d -= TAU;
    } else if d < -PI {
        d += TAU;
    }
    d
}

fn steer_toward(current: f32, target: f32, max_step: f32) -> f32 {
    let delta = angular_delta(current, target);
    let step = delta.clamp(-max_step, max_step);
    current + step
}

fn apply_drag(vel: Vec2, dt: f32) -> Vec2 {
    Vec2::new(vel.x * (1.0 - TANK_DRAG * dt).max(0.0), 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls_toward_target_when_far_right() {
        let pos = Vec2::new(0.0, 16.0);
        let target = Vec2::new(TANK_APPROACH_DIST + 200.0, 100.0);
        let s = step(pos, Vec2::ZERO, 0.0, 0.0, Some(target), 3200.0, 0.0, 1.0 / 60.0);
        assert!(s.vel.x > 0.0, "tank should accelerate right toward target");
        assert!(s.body_facing > 0.0, "body should face right");
    }

    #[test]
    fn rolls_toward_target_when_far_left() {
        let pos = Vec2::new(2000.0, 16.0);
        let target = Vec2::new(2000.0 - TANK_APPROACH_DIST - 200.0, 100.0);
        let s = step(pos, Vec2::ZERO, 0.0, 0.0, Some(target), 3200.0, 0.0, 1.0 / 60.0);
        assert!(s.vel.x < 0.0, "tank should accelerate left toward target");
        assert!(s.body_facing < 0.0, "body should face left");
    }

    #[test]
    fn dodges_while_in_firing_range() {
        // Within approach distance, the dodge sway should produce a
        // non-zero lateral velocity. Sample at a phase where sin > 0
        // so the direction is unambiguous.
        let pos = Vec2::new(500.0, 16.0);
        let target = Vec2::new(700.0, 100.0); // dx = 200, inside approach
        // Phase chosen so `sin(phase * FREQ)` is comfortably positive.
        let phase = std::f32::consts::FRAC_PI_2 / TANK_DODGE_FREQ;
        let s = step(pos, Vec2::ZERO, FRAC_PI_2, 0.0, Some(target), 3200.0, phase, 1.0 / 60.0);
        assert!(s.vel.x > 0.0, "tank should sway right at this phase, got {}", s.vel.x);
    }

    #[test]
    fn turret_does_not_immediately_fire_at_arbitrary_target() {
        // Turret starts at 0 (pointing +Y). With the target far off-axis,
        // the aim error should exceed the firing cone after one tick.
        let pos = Vec2::new(500.0, 16.0);
        let target = Vec2::new(900.0, 100.0);
        let s = step(pos, Vec2::ZERO, FRAC_PI_2, 0.0, Some(target), 3200.0, 0.0, 1.0 / 60.0);
        assert!(!s.fire, "turret should still be slewing toward target");
    }

    #[test]
    fn turret_fires_when_aimed_and_in_range() {
        // Park the tank, put the player straight overhead, pre-aim the
        // turret. Note we still aim *slightly* high to absorb gravity
        // lead.
        let pos = Vec2::new(500.0, 16.0);
        let target = Vec2::new(500.0, 200.0);
        // Use a turret angle close to the lead-corrected solution.
        let dist = (target - pos).length();
        let t = dist / TANK_SHOT_SPEED;
        let lead_y = 0.5 * TANK_SHOT_GRAVITY * t * t;
        let aim_angle = 0.0_f32.atan2(target.y - pos.y + lead_y); // 0 — straight up
        let s = step(pos, Vec2::ZERO, FRAC_PI_2, aim_angle, Some(target), 3200.0, 0.0, 1.0 / 60.0);
        assert!(s.fire, "turret should fire when aligned and in range");
    }

    #[test]
    fn idles_with_no_target() {
        let s = step(
            Vec2::new(100.0, 16.0),
            Vec2::new(40.0, 0.0),
            FRAC_PI_2,
            0.5,
            None,
            3200.0,
            0.0,
            1.0 / 60.0,
        );
        assert!(!s.fire);
        assert_eq!(s.turret_facing, 0.5, "turret holds last aim while idling");
        assert!(s.vel.x.abs() < 40.0, "drag should reduce velocity");
    }
}
