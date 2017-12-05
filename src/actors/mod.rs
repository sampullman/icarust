
use ggez::Context;
use ggez::graphics::{Vector2, Point2};
use ggez::nalgebra as na;
use rand;
use std;
use util::*;
use input::InputState;

pub mod player;

use assets::{Sprite, Asset, AssetManager};

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

#[derive(Debug, Actor, Drawable)]
pub struct Rock {
	pub base: BaseActor<Sprite>,
}

pub trait Drawable {
    fn draw(&self, ctx: &mut Context, world_coords: (u32, u32));
}

pub trait Actor: Sized {

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

pub trait Updatable {
    fn update(&mut self, ctx: &mut Context, asset_manager: &mut AssetManager, dt: f32);
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

impl Collidable for Rock {}
impl Collidable for Shot {}

const SHOT_LIFE: f32 = 2.0;
const ROCK_LIFE: f32 = 1.0;
const MAX_ROCK_VEL: f32 = 50.0;

const ROCK_BBOX: f32 = 12.0;
const SHOT_BBOX: f32 = 6.0;

const SHOT_RVEL: f32 = 0.1;

pub fn create_rock(ctx: &mut Context, asset_manager: &mut AssetManager) -> Rock {
    Rock {
		base: BaseActor {
            asset: asset_manager.make_sprite(ctx, "/rock.png"),            
        	pos: Point2::origin(),
        	facing: 0.,
        	velocity: na::zero(),
        	bbox_size: ROCK_BBOX,
        	life: ROCK_LIFE,
            rvel: 0.,
		},
    }
}

/// Create the given number of rocks.
/// Makes sure that none of them are within the
/// given exclusion zone (nominally the player)
/// Note that this *could* create rocks outside the
/// bounds of the playing field, so it should be
/// called before `wrap_actor_position()` happens.
pub fn create_rocks(ctx: &mut Context, asset_manager: &mut AssetManager, num: i32, exclusion: Point2, min_radius: f32, max_radius: f32) -> Vec<Rock> {
    assert!(max_radius > min_radius);
    let new_rock = |_| {
        let mut rock = create_rock(ctx, asset_manager);
        let r_angle = rand::random::<f32>() * 2.0 * std::f32::consts::PI;
        let r_distance = rand::random::<f32>() * (max_radius - min_radius) + min_radius;
        rock.set_position(exclusion + vec_from_angle(r_angle) * r_distance);
        rock.set_velocity(random_vec(MAX_ROCK_VEL));
        rock
    };
    (0..num).map(new_rock).collect()
}

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