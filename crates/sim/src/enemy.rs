//! Simple chase-and-fire enemy AI. Picks the nearest live player, faces them,
//! thrusts when roughly aligned, and fires when
//! aligned and within range. No gravity — enemies cruise around like
//! drones, not like the player ship.
//!
//! All math is pure and deterministic: it reads only entity state and
//! returns the new `(velocity, facing, fire?)`. World mutation (spawning
//! shots, advancing cooldown) lives in `world::tick`.

use std::f32::consts::{PI, TAU};

use crate::util::{self, Vec2};

pub const ENEMY_BBOX: f32 = 14.0;
/// Acceleration applied when thrusting toward the target (units/s²).
pub const ENEMY_THRUST: f32 = 180.0;
pub const ENEMY_MAX_SPEED: f32 = 130.0;
/// Per-second turn rate (radians) — slower than the player so the player
/// can out-maneuver them.
pub const ENEMY_TURN_RATE: f32 = 1.6;
/// Velocity decays toward zero at this fraction per second when not
/// thrusting. Keeps enemies from drifting forever after a turn.
pub const ENEMY_DRAG: f32 = 0.8;
/// Half-angle of the firing cone (radians). The enemy must be facing
/// within this of the target before it pulls the trigger.
pub const ENEMY_FIRE_CONE: f32 = 0.25;
/// Max distance at which the enemy will attempt to fire.
pub const ENEMY_FIRE_RANGE: f32 = 360.0;
/// Seconds between enemy shots.
pub const ENEMY_SHOT_TIME: f32 = 0.9;
pub const ENEMY_SHOT_SPEED: f32 = 260.0;

/// One AI step result.
#[derive(Debug, Clone, Copy)]
pub struct EnemyStep {
    pub vel: Vec2,
    pub facing: f32,
    /// True if the enemy wants to fire this tick (cooldown is checked by
    /// the caller).
    pub fire: bool,
}

/// Compute the next velocity, facing, and fire intent for an enemy.
///
/// `target` is `None` when there are no live players; in that case the
/// enemy idles (drag-only, no thrust, no fire).
///
/// `world_width` is needed to handle toroidal X-wrap when computing the
/// shortest path to the target.
pub fn step(
    pos: Vec2,
    vel: Vec2,
    facing: f32,
    target: Option<Vec2>,
    world_width: f32,
    dt: f32,
) -> EnemyStep {
    let Some(target_pos) = target else {
        return EnemyStep {
            vel: apply_drag(vel, dt),
            facing,
            fire: false,
        };
    };

    let to_target = shortest_offset(pos, target_pos, world_width);
    let dist = to_target.length();

    let new_facing = if dist > 1e-3 {
        // atan2(dx, dy) matches our (sin, cos) facing convention: a target
        // straight up (+Y) gives angle 0.
        let target_angle = to_target.x.atan2(to_target.y);
        steer_toward(facing, target_angle, ENEMY_TURN_RATE * dt)
    } else {
        facing
    };

    // Thrust if we're roughly pointed at the target. The cone is wider
    // than the firing cone so the enemy keeps closing distance even when
    // it isn't dead-on aligned.
    let aim_error = angular_delta(new_facing, to_target.x.atan2(to_target.y)).abs();
    let mut new_vel = vel;
    if aim_error < ENEMY_FIRE_CONE * 3.0 {
        new_vel += util::vec_from_angle(new_facing) * ENEMY_THRUST * dt;
    }
    new_vel = apply_drag(new_vel, dt);
    if let Some(clamped) = util::clamp_velocity(new_vel, ENEMY_MAX_SPEED) {
        new_vel = clamped;
    }

    let fire = aim_error < ENEMY_FIRE_CONE && dist < ENEMY_FIRE_RANGE;

    EnemyStep {
        vel: new_vel,
        facing: new_facing,
        fire,
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

/// Step `current` toward `target` by at most `max_step` radians.
fn steer_toward(current: f32, target: f32, max_step: f32) -> f32 {
    let delta = angular_delta(current, target);
    let step = delta.clamp(-max_step, max_step);
    current + step
}

fn apply_drag(vel: Vec2, dt: f32) -> Vec2 {
    vel * (1.0 - ENEMY_DRAG * dt).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idles_when_no_target() {
        let s = step(
            Vec2::new(100.0, 100.0),
            Vec2::new(20.0, 0.0),
            0.0,
            None,
            1280.0,
            1.0 / 60.0,
        );
        assert!(!s.fire);
        // Drag should reduce speed, not increase it.
        assert!(s.vel.length() < 20.0);
    }

    #[test]
    fn fires_when_aligned_and_in_range() {
        // Target straight ahead (+Y), enemy facing up.
        let pos = Vec2::new(100.0, 100.0);
        let target = Vec2::new(100.0, 200.0);
        let s = step(pos, Vec2::ZERO, 0.0, Some(target), 1280.0, 1.0 / 60.0);
        assert!(s.fire);
    }

    #[test]
    fn does_not_fire_when_target_far_away() {
        let pos = Vec2::new(100.0, 100.0);
        // Aligned (target straight up), but well outside fire range.
        let target = Vec2::new(100.0, 100.0 + ENEMY_FIRE_RANGE + 50.0);
        let s = step(pos, Vec2::ZERO, 0.0, Some(target), 1280.0, 1.0 / 60.0);
        assert!(!s.fire);
    }

    #[test]
    fn does_not_fire_when_facing_wrong_way() {
        let pos = Vec2::new(100.0, 100.0);
        // Target straight ahead, enemy pointing the opposite way (PI).
        let target = Vec2::new(100.0, 200.0);
        let s = step(pos, Vec2::ZERO, PI, Some(target), 1280.0, 1.0 / 60.0);
        assert!(!s.fire);
    }

    #[test]
    fn turns_toward_target_over_time() {
        let pos = Vec2::new(100.0, 100.0);
        let target = Vec2::new(100.0, 200.0); // straight up — target angle 0
        let mut facing = PI; // pointing down
        for _ in 0..120 {
            let s = step(pos, Vec2::ZERO, facing, Some(target), 1280.0, 1.0 / 60.0);
            facing = s.facing;
        }
        assert!(facing.abs() < 0.1, "expected facing ~0, got {}", facing);
    }

    #[test]
    fn shortest_offset_wraps_x() {
        // From x=10 to x=1270 on a 1280-wide world: short path is -20, not +1260.
        let off = shortest_offset(Vec2::new(10.0, 0.0), Vec2::new(1270.0, 0.0), 1280.0);
        assert!((off.x - -20.0).abs() < 1e-3, "got {}", off.x);
    }
}
