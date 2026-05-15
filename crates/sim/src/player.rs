use crate::input::PlayerInput;
use crate::util::{self, Vec2};

pub const PLAYER_BBOX: f32 = 12.0;
pub const PLAYER_THRUST: f32 = 594.0;
pub const PLAYER_MAX_SPEED: f32 = 264.0;
pub const PLAYER_GRAVITY: f32 = 110.0;
/// Linear drag coefficient (units/s² per unit/s of speed).
pub const PLAYER_DRAG: f32 = 0.6;
/// Rotation in radians per second.
pub const PLAYER_TURN_RATE: f32 = 3.0;
/// Seconds between shots.
pub const PLAYER_SHOT_TIME: f32 = 0.3;
pub const SHOT_SPEED: f32 = 340.0;

/// Player hit-point ceiling. Enemy bullets chip one HP at a time; rocks,
/// ramming, and terrain crashes are still instant kills.
pub const PLAYER_MAX_HP: i16 = 5;
/// Seconds of "no damage taken" before HP starts ticking back up.
pub const PLAYER_REGEN_DELAY: f32 = 3.0;
/// Seconds between each +1 HP tick once regen kicks in.
pub const PLAYER_REGEN_INTERVAL: f32 = 1.5;

/// Pure rotation + thrust step. Returns `(new_velocity, new_facing)`.
pub fn apply_input(velocity: Vec2, facing: f32, input: &PlayerInput, dt: f32) -> (Vec2, f32) {
    let new_facing = facing + dt * PLAYER_TURN_RATE * input.xaxis;
    let mut vel = velocity;
    if input.yaxis > 0.0 {
        vel += util::vec_from_angle(new_facing) * PLAYER_THRUST * dt;
    }
    (vel, new_facing)
}

/// Pure drag + gravity + clamp step.
pub fn apply_forces(velocity: Vec2, dt: f32) -> Vec2 {
    let drag = velocity * -PLAYER_DRAG;
    let gravity = Vec2::new(0.0, -PLAYER_GRAVITY);
    let mut vel = velocity + (gravity + drag) * dt;
    if let Some(clamped) = util::clamp_velocity(vel, PLAYER_MAX_SPEED) {
        vel = clamped;
    }
    vel
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt() -> f32 {
        1.0 / 60.0
    }

    #[test]
    fn thrust_at_facing_zero_pushes_y_positive() {
        let mut input = PlayerInput::default();
        input.yaxis = 1.0;
        let (vel, _) = apply_input(Vec2::ZERO, 0.0, &input, dt());
        assert!(vel.y > 0.0, "expected +y thrust, got {:?}", vel);
        assert!(vel.x.abs() < 1e-5);
    }

    #[test]
    fn no_thrust_when_yaxis_zero() {
        let input = PlayerInput::default();
        let (vel, _) = apply_input(Vec2::ZERO, 0.0, &input, dt());
        assert_eq!(vel, Vec2::ZERO);
    }

    #[test]
    fn thrust_overcomes_gravity_and_drag_within_one_second() {
        let mut vel = Vec2::ZERO;
        let mut pos = Vec2::ZERO;
        let mut input = PlayerInput::default();
        input.yaxis = 1.0;
        for _ in 0..60 {
            let (v, _) = apply_input(vel, 0.0, &input, dt());
            vel = v;
            pos += vel * dt();
            vel = apply_forces(vel, dt());
        }
        assert!(
            pos.y > 50.0,
            "expected meaningful upward travel after 1s of thrust, got pos={:?}, vel={:?}",
            pos,
            vel
        );
        assert!(vel.y > 0.0);
    }

    #[test]
    fn velocity_clamps_to_max_speed() {
        let mut vel = Vec2::new(0.0, 1000.0);
        vel = apply_forces(vel, dt());
        assert!(vel.length() <= PLAYER_MAX_SPEED + 1e-3);
    }
}
