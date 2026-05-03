use crate::actors;
use crate::actors::{Actor, BaseActor, HasBase, Inputable, Updatable};
use crate::assets::{AssetManager, SoundId};
use crate::actors::shot::{create_shot, Shot};
use crate::input::InputState;
use crate::util;
use crate::util::{Point2, Vector2};
use ggez::Context;

const PLAYER_BBOX: f32 = 12.0;
const PLAYER_THRUST: f32 = 540.0;
const PLAYER_MAX_SPEED: f32 = 220.0;
const PLAYER_GRAVITY: f32 = 110.0;
/// Linear drag coefficient (units/s² per unit/s of speed).
const PLAYER_DRAG: f32 = 0.6;
/// Rotation in radians per second.
const PLAYER_TURN_RATE: f32 = 3.0;

/// Seconds between shots.
const PLAYER_SHOT_TIME: f32 = 0.3;
const SHOT_SPEED: f32 = 340.0;

#[derive(Debug)]
pub struct Player {
    pub base: BaseActor,
    shot_timeout: f32,
    shot_sound_id: SoundId,
}

impl HasBase for Player {
    fn base(&self) -> &BaseActor {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BaseActor {
        &mut self.base
    }
}

pub fn create_player(
    ctx: &mut Context,
    asset_manager: &mut AssetManager,
    world_width: f32,
    world_height: f32,
) -> Player {
    Player {
        base: BaseActor {
            asset: asset_manager.make_sprite(ctx, "/player.png"),
            pos: Point2::new(world_width / 2.0, world_height / 2.0),
            facing: 0.,
            velocity: Vector2::ZERO,
            bbox_size: PLAYER_BBOX,
            rvel: 0.,
            alive: true,
        },
        shot_timeout: -1.0,
        shot_sound_id: asset_manager.add_sound(ctx, "/pew.ogg"),
    }
}

impl Inputable for Player {
    fn handle_input(&mut self, input: &InputState, dt: f32) {
        let (vel, facing) = apply_input(self.velocity(), self.facing(), input, dt);
        self.set_velocity(vel);
        self.set_facing(facing);
    }
}

impl Updatable for Player {
    fn update(
        &mut self,
        _ctx: &mut Context,
        _asset_manager: &mut AssetManager,
        world_coords: (f32, f32),
        dt: f32,
    ) {
        actors::update_actor_position(self, dt);
        actors::wrap_actor_position(self, world_coords.0, world_coords.1);

        self.shot_timeout -= dt;

        self.set_velocity(apply_forces(self.velocity(), dt));
    }
}

/// Pure rotation + thrust step. Returns `(new_velocity, new_facing)`.
pub(crate) fn apply_input(
    velocity: Vector2,
    facing: f32,
    input: &InputState,
    dt: f32,
) -> (Vector2, f32) {
    let new_facing = facing + dt * PLAYER_TURN_RATE * input.xaxis;
    let mut vel = velocity;
    if input.yaxis > 0.0 {
        vel += util::vec_from_angle(new_facing) * PLAYER_THRUST * dt;
    }
    (vel, new_facing)
}

/// Pure drag + gravity + clamp step.
pub(crate) fn apply_forces(velocity: Vector2, dt: f32) -> Vector2 {
    let drag = velocity * -PLAYER_DRAG;
    let gravity = Vector2::new(0.0, -PLAYER_GRAVITY);
    let mut vel = velocity + (gravity + drag) * dt;
    if let Some(clamped) = util::clamp_velocity(vel, PLAYER_MAX_SPEED) {
        vel = clamped;
    }
    vel
}

impl Player {
    pub fn can_fire(&self) -> bool {
        self.shot_timeout < 0.0
    }

    pub fn fire_shot(&mut self, ctx: &mut Context, am: &mut AssetManager) -> Shot {
        self.shot_timeout = PLAYER_SHOT_TIME;

        let mut shot = create_shot(ctx, am);

        shot.set_facing(self.facing());
        let direction = util::vec_from_angle(shot.facing());

        shot.set_velocity(direction * SHOT_SPEED);

        let pos = direction * self.half_height();
        shot.set_position(self.position() + pos);

        am.play_sound(ctx, self.shot_sound_id);
        shot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt() -> f32 {
        1.0 / 60.0
    }

    #[test]
    fn thrust_at_facing_zero_pushes_y_positive() {
        let mut input = InputState::default();
        input.yaxis = 1.0;
        let (vel, _) = apply_input(Vector2::ZERO, 0.0, &input, dt());
        assert!(vel.y > 0.0, "expected +y thrust, got {:?}", vel);
        assert!(vel.x.abs() < 1e-5);
    }

    #[test]
    fn no_thrust_when_yaxis_zero() {
        let input = InputState::default();
        let (vel, _) = apply_input(Vector2::ZERO, 0.0, &input, dt());
        assert_eq!(vel, Vector2::ZERO);
    }

    #[test]
    fn thrust_overcomes_gravity_and_drag_within_one_second() {
        // Hold up for 60 frames at facing=0 and check the player has gained altitude.
        let mut vel = Vector2::ZERO;
        let mut pos = Vector2::ZERO;
        let mut input = InputState::default();
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
        let mut vel = Vector2::new(0.0, 1000.0);
        vel = apply_forces(vel, dt());
        assert!(vel.length() <= PLAYER_MAX_SPEED + 1e-3);
    }
}
