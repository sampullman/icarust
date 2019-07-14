
use ggez::Context;
use crate::assets::{Asset, AssetManager};
use crate::util;
use crate::util::{Vector2, Point2};
use crate::input::InputState;
use crate::physics::{CollisionData, CollisionWorld2, PhysicsId};
use crate::render::camera::Camera;

pub mod player;
pub mod rock;
pub mod shot;

const MAX_PHYSICS_VEL: f32 = 320.0;

#[derive(Debug)]
pub struct BaseActor<T: Asset> {
    pub asset: T,
    pub pos: Point2,
    pub facing: f32,
    pub velocity: Vector2,
    pub bbox_size: f32,
    pub rvel: f32,
    pub alive: bool,
    pub physics_id: PhysicsId,
}

pub trait Drawable {
    fn draw(&self, ctx: &mut Context, camera: &Camera);
}

pub trait Actor: Sized {

    fn alive(&self) -> bool;
    fn kill(&mut self);

    fn width(&self, ctx: &mut Context) -> f32;
    fn height(&self, ctx: &mut Context) -> f32;
    fn half_width(&self, ctx: &mut Context) -> f32;
    fn half_height(&self, ctx: &mut Context) -> f32;
    fn center(&self) -> Point2;

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

    fn rvel(&self) -> f32;
    fn rotate(&mut self) {
        let rvel = self.rvel();
        self.add_facing(rvel)
    }

    fn physics_id(&self) -> PhysicsId;

    fn add_to_world(&mut self, world: &mut CollisionWorld2, id: PhysicsId);
}

/// Update position based on current velocity
pub fn update_actor_position<T: Actor>(actor: &mut T, dt: f32) {
    // Clamp the velocity to the max
    if let Some(clamped) = util::clamp_velocity(actor.velocity(), MAX_PHYSICS_VEL) {
        actor.set_velocity(clamped);
    }
    let dv = actor.velocity() * dt;
    let new_position = actor.position() + dv;
    actor.set_position(new_position);
    actor.rotate();
}

/// Wraps an actor's position to the bounds of the world
pub fn wrap_actor_position<T: Actor>(actor: &mut T, wx: f32, wy: f32) {
    let actor_center = actor.position();
    if actor_center.x > wx {
        actor.add_x(-wx);
    } else if actor_center.x < 0. {
        actor.add_x(wx);
    };
    if actor_center.y > wy {
        actor.set_y(wy);
    } else if actor_center.y < 0. {
        actor.add_y(wy);
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

    // Provide current velocity for physics simulation
    fn collision_data(&self) -> CollisionData {
        /*
        let pdistance = other.position() - self.position();
        if pdistance.norm() < (self.bbox_size() + other.bbox_size()) {

            self.handle_collision(other);
            other.handle_collision(self);
            return true
        }
        return false
        */
        CollisionData::new(self.physics_id(), Some(self.velocity()))
    }

    fn handle_collision<T: Actor>(&mut self, _other: &T) {
        self.kill();
    }
}
