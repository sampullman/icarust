
use ggez::graphics::{Vector2, Point2};
use ggez::nalgebra as na;

#[derive(Debug, Copy, Clone)]
pub enum ActorType {
    Player,
    Rock,
    Shot,
}

#[derive(Debug)]
pub struct BaseActor {
    pub tag: ActorType,
    pub pos: Point2,
    pub facing: f32,
    pub velocity: Vector2,
    pub bbox_size: f32,
    pub rvel: f32,

    // For shots, it is the time left to live,
    // for players and rocks, it is the actual hit points.
    pub life: f32,
}

#[derive(Debug, Actor)]
pub struct Player {
	pub actor: BaseActor,
}

#[derive(Debug, Actor)]
pub struct Shot {
	pub actor: BaseActor,
}

#[derive(Debug, Actor)]
pub struct Rock {
	pub actor: BaseActor,
}

pub trait Actor: Sized {

    fn tag(&self) -> ActorType;

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

impl Collidable for Player {
}

impl Collidable for Rock {
}

impl Collidable for Shot {
}

const PLAYER_LIFE: f32 = 1.0;
const SHOT_LIFE: f32 = 2.0;
const ROCK_LIFE: f32 = 1.0;

const PLAYER_BBOX: f32 = 12.0;
const ROCK_BBOX: f32 = 12.0;
const SHOT_BBOX: f32 = 6.0;

const SHOT_RVEL: f32 = 0.1;

pub fn create_player() -> Player {
    Player {
		actor: BaseActor {
        	tag: ActorType::Player,
        	pos: Point2::origin(),
        	facing: 0.,
        	velocity: na::zero(),
        	bbox_size: PLAYER_BBOX,
        	life: PLAYER_LIFE,
            rvel: 0.,
		},
    }
}

pub fn create_rock() -> Rock {
    Rock {
		actor: BaseActor {
        	tag: ActorType::Rock,
        	pos: Point2::origin(),
        	facing: 0.,
        	velocity: na::zero(),
        	bbox_size: ROCK_BBOX,
        	life: ROCK_LIFE,
            rvel: 0.,
		},
    }
}

pub fn create_shot() -> Shot {
    Shot {
		actor: BaseActor {
        	tag: ActorType::Shot,
        	pos: Point2::origin(),
        	facing: 0.,
        	velocity: na::zero(),
        	bbox_size: SHOT_BBOX,
        	life: SHOT_LIFE,
            rvel: SHOT_RVEL,
		},
    }
}