
use ggez::Context;
use ggez::graphics::{Vector2, Point2};
use ggez::nalgebra as na;
use util::*;
use input::InputState;

pub mod player;
pub mod rock;

use assets::{Sprite, Asset, AssetManager};

const MAX_PHYSICS_VEL: f32 = 250.0;

#[derive(Debug)]
pub struct BaseActor<T: Asset> {
    pub asset: T,
    pub pos: Point2,
    pub facing: f32,
    pub velocity: Vector2,
    pub bbox_size: f32,
    pub rvel: f32,

    // For shots, it is the time left to live,
    // for players and rocks, it is the actual hit points.
    pub life: f32,
}

#[derive(Debug, Actor, Drawable)]
pub struct Shot {
	pub base: BaseActor<Sprite>,
}

pub trait Drawable {
    fn draw(&self, ctx: &mut Context, world_coords: (u32, u32));
}

pub trait Actor: Sized {

    fn width(&self) -> f32;
    fn height(&self) -> f32;
    fn half_width(&self) -> f32;
    fn half_height(&self) -> f32;

    fn position(&self) -> Point2;
	fn set_position(&mut self, pos: Point2);
    fn add_position(&mut self, pos: Point2);
	fn x(&self) -> f32;
	fn y(&self) -> f32;
	fn set_x(&mut self, x: f32);
	fn add_x(&mut self, x: f32) {
        let new_x = self.x() + x;
        self.set_x(new_x)
    }
	fn set_y(&mut self, y: f32);
	fn add_y(&mut self, y: f32) {
        let new_y = self.y() + y;
        self.set_y(new_y)
    }

	fn velocity(&self) -> Vector2;
    fn set_velocity_xy(&mut self, x: f32, y: f32);
    fn set_velocity(&mut self, vel: Vector2);
    fn add_velocity(&mut self, vel: Vector2) {
        let new_vel = self.velocity() + vel;
        self.set_velocity(new_vel)
    }

    fn facing(&self) -> f32;
	fn set_facing(&mut self, facing: f32);
    fn add_facing(&mut self, facing: f32) {
        let new_facing = self.facing() + facing;
        self.set_facing(new_facing)
    }

    fn bbox_size(&self) -> f32;

	fn life(&self) -> f32;
	fn set_life(&mut self, life: f32);
	fn add_life(&mut self, life: f32) {
        let new_life = self.life() + life;
        self.set_life(new_life)
    }

    fn rvel(&self) -> f32;
    fn rotate(&mut self) {
        let rvel = self.rvel();
        self.add_facing(rvel)
    }
}

/// Update position based on current velocity
pub fn update_actor_position<T: Actor>(actor: &mut T, dt: f32) {
    // Clamp the velocity to the max
    let norm_sq = actor.velocity().norm_squared();
    if norm_sq > MAX_PHYSICS_VEL.powi(2) {
        let new_velocity = actor.velocity() / norm_sq.sqrt() * MAX_PHYSICS_VEL;
        actor.set_velocity(new_velocity);
    }
    let dv = actor.velocity() * (dt);
    let new_position = actor.position() + dv;
    actor.set_position(new_position);
    actor.rotate();
}

/// Takes an actor and wraps its position to the bounds of the
/// screen, so if it goes off the left side of the screen it
/// will re-enter on the right side and so on.
pub fn wrap_actor_position<T: Actor>(actor: &mut T, sx: f32, sy: f32) {
    let actor_center = actor.position();
    if actor_center.x > sx {
        actor.add_x(-sx);
    } else if actor_center.x < 0. {
        actor.add_x(sx);
    };
    if actor_center.y > sy {
        actor.set_y(sy);
    } else if actor_center.y < 0. {
        actor.add_y(sy);
    }
}

pub trait Updatable: Actor {
    fn update(&mut self, _ctx: &mut Context, _asset_manager: &mut AssetManager, world_coords: (u32, u32), dt: f32) {
        update_actor_position(self, dt);
        wrap_actor_position(self, world_coords.0 as f32, world_coords.1 as f32)
    }
}

pub trait Inputable: Actor {
    fn handle_input(&mut self, input: &InputState, dt: f32);
}

pub trait Collidable: Actor {
    fn check_collision<T: Actor+Collidable>(&mut self, other: &mut T) -> bool {

        let pdistance = other.position() - self.position();
        if pdistance.norm() < (self.bbox_size() + other.bbox_size()) {

            self.handle_collision(other);
            other.handle_collision(self);
            return true
        }
        return false
    }

    fn handle_collision<T: Actor>(&mut self, _other: &T) {
        self.set_life(0.0);
    }
}
impl Collidable for Shot {}
impl Updatable for Shot {}

const SHOT_LIFE: f32 = 2.0;
const SHOT_BBOX: f32 = 6.0;
const SHOT_RVEL: f32 = 0.1;

pub fn create_shot(ctx: &mut Context, asset_manager: &mut AssetManager) -> Shot {
    Shot {
		base: BaseActor {
            asset: asset_manager.make_sprite(ctx, "/shot.png"),
        	pos: Point2::origin(),
        	facing: 0.,
        	velocity: na::zero(),
        	bbox_size: SHOT_BBOX,
        	life: SHOT_LIFE,
            rvel: SHOT_RVEL,
		},
    }
}