use crate::assets::{AssetManager, Sprite};
use crate::input::InputState;
use crate::render::camera::Camera;
use crate::util::{Point2, Vector2};
use ggez::graphics::{Canvas, DrawParam};
use ggez::Context;

pub mod player;
pub mod rock;
pub mod shot;

#[derive(Debug)]
pub struct BaseActor {
    pub asset: Sprite,
    pub pos: Point2,
    pub facing: f32,
    pub velocity: Vector2,
    pub bbox_size: f32,
    pub rvel: f32,
    pub alive: bool,
}

/// Provides field access to a `BaseActor`. The blanket `Actor` impl below
/// derives the rest from this single accessor pair.
pub trait HasBase {
    fn base(&self) -> &BaseActor;
    fn base_mut(&mut self) -> &mut BaseActor;
}

pub trait Actor {
    fn alive(&self) -> bool;
    fn kill(&mut self);

    fn width(&self) -> f32;
    fn height(&self) -> f32;
    fn half_width(&self) -> f32;
    fn half_height(&self) -> f32;

    fn position(&self) -> Point2;
    fn set_position(&mut self, pos: Point2);
    fn add_position(&mut self, pos: Vector2);
    fn x(&self) -> f32;
    fn y(&self) -> f32;
    fn set_x(&mut self, x: f32);
    fn add_x(&mut self, x: f32) {
        let new_x = self.x() + x;
        self.set_x(new_x);
    }
    fn set_y(&mut self, y: f32);
    fn add_y(&mut self, y: f32) {
        let new_y = self.y() + y;
        self.set_y(new_y);
    }

    fn velocity(&self) -> Vector2;
    fn set_velocity(&mut self, vel: Vector2);
    fn add_velocity(&mut self, vel: Vector2) {
        let new_vel = self.velocity() + vel;
        self.set_velocity(new_vel);
    }

    fn facing(&self) -> f32;
    fn set_facing(&mut self, facing: f32);
    fn add_facing(&mut self, facing: f32) {
        let new_facing = self.facing() + facing;
        self.set_facing(new_facing);
    }

    fn bbox_size(&self) -> f32;

    fn rvel(&self) -> f32;

    fn sprite(&self) -> &Sprite;
}

impl<T: HasBase> Actor for T {
    fn alive(&self) -> bool {
        self.base().alive
    }
    fn kill(&mut self) {
        self.base_mut().alive = false;
    }

    fn width(&self) -> f32 {
        self.base().asset.width()
    }
    fn height(&self) -> f32 {
        self.base().asset.height()
    }
    fn half_width(&self) -> f32 {
        self.base().asset.half_width()
    }
    fn half_height(&self) -> f32 {
        self.base().asset.half_height()
    }

    fn position(&self) -> Point2 {
        self.base().pos
    }
    fn set_position(&mut self, pos: Point2) {
        self.base_mut().pos = pos;
    }
    fn add_position(&mut self, pos: Vector2) {
        self.base_mut().pos += pos;
    }
    fn x(&self) -> f32 {
        self.base().pos.x
    }
    fn y(&self) -> f32 {
        self.base().pos.y
    }
    fn set_x(&mut self, x: f32) {
        self.base_mut().pos.x = x;
    }
    fn set_y(&mut self, y: f32) {
        self.base_mut().pos.y = y;
    }

    fn velocity(&self) -> Vector2 {
        self.base().velocity
    }
    fn set_velocity(&mut self, vel: Vector2) {
        self.base_mut().velocity = vel;
    }

    fn facing(&self) -> f32 {
        self.base().facing
    }
    fn set_facing(&mut self, facing: f32) {
        self.base_mut().facing = facing;
    }

    fn bbox_size(&self) -> f32 {
        self.base().bbox_size
    }

    fn rvel(&self) -> f32 {
        self.base().rvel
    }

    fn sprite(&self) -> &Sprite {
        &self.base().asset
    }
}

/// Update position based on current velocity, and rotate by `rvel * dt`.
///
/// Force/clamping is left to specific actor impls (`Player::update`, etc.).
pub fn update_actor_position<T: Actor + ?Sized>(actor: &mut T, dt: f32) {
    let dv = actor.velocity() * dt;
    let new_position = actor.position() + dv;
    actor.set_position(new_position);
    let dr = actor.rvel() * dt;
    actor.add_facing(dr);
}

/// Wraps an actor's position toroidally so leaving one edge re-enters the other.
pub fn wrap_actor_position<T: Actor + ?Sized>(actor: &mut T, wx: f32, wy: f32) {
    let actor_center = actor.position();
    if actor_center.x > wx {
        actor.add_x(-wx);
    } else if actor_center.x < 0. {
        actor.add_x(wx);
    }
    if actor_center.y > wy {
        actor.add_y(-wy);
    } else if actor_center.y < 0. {
        actor.add_y(wy);
    }
}

pub trait Updatable: Actor {
    fn update(
        &mut self,
        _ctx: &mut Context,
        _asset_manager: &mut AssetManager,
        world_coords: (f32, f32),
        dt: f32,
    ) {
        update_actor_position(self, dt);
        wrap_actor_position(self, world_coords.0, world_coords.1);
    }
}

pub trait Inputable: Actor {
    fn handle_input(&mut self, input: &InputState, dt: f32);
}

pub fn draw_actor<T: Actor>(canvas: &mut Canvas, camera: &Camera, actor: &T) {
    let pos = camera.world_to_screen_coords(actor.position());
    let drawparams = DrawParam::new()
        .dest(pos)
        .rotation(actor.facing())
        .offset(Point2::new(0.5, 0.5));
    canvas.draw(&actor.sprite().image, drawparams);
}

/// Draw the actor with horizontal wrapping when near the world edges.
pub fn draw_actor_wrapped<T: Actor>(canvas: &mut Canvas, camera: &Camera, actor: &T) {
    let screen_right = camera.world_width();
    let pos = actor.position();

    if pos.x < actor.half_width() {
        let wrap_pos = Point2::new(pos.x + screen_right, pos.y);
        draw_at(canvas, camera, actor, wrap_pos);
    } else if pos.x > (screen_right - actor.half_width()) {
        let wrap_pos = Point2::new(pos.x - screen_right, pos.y);
        draw_at(canvas, camera, actor, wrap_pos);
    }
    draw_actor(canvas, camera, actor);
}

fn draw_at<T: Actor>(canvas: &mut Canvas, camera: &Camera, actor: &T, world_pos: Point2) {
    let screen_pos = camera.world_to_screen_coords(world_pos);
    let drawparams = DrawParam::new()
        .dest(screen_pos)
        .rotation(actor.facing())
        .offset(Point2::new(0.5, 0.5));
    canvas.draw(&actor.sprite().image, drawparams);
}
