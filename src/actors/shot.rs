
use ggez::Context;
use ggez::graphics::{Point2, Vector2};
use crate::actors;
use crate::actors::{BaseActor, Actor, Collidable, Drawable, Updatable};
use crate::assets::{Sprite, Asset, AssetManager};
use crate::physics::{CollisionWorld2, PhysicsId};
use crate::render::camera::Camera;

#[derive(Debug, Actor, Drawable)]
pub struct Shot {
	pub base: BaseActor<Sprite>,
    pub time_to_live: f32,
}

impl Collidable for Shot {}

impl Updatable for Shot {

    fn update(&mut self, _ctx: &mut Context, _asset_manager: &mut AssetManager, world_coords: (u32, u32), dt: f32) {
        actors::update_actor_position(self, dt);
        actors::wrap_actor_position(self, world_coords.0 as f32, world_coords.1 as f32);
	    self.time_to_live -= dt;
        if self.time_to_live < 0.0 {
            self.kill();
        }
    }
}

const SHOT_LIFE: f32 = 2.0;
const SHOT_BBOX: f32 = 6.0;
const SHOT_RVEL: f32 = 0.1;

pub fn create_shot(ctx: &mut Context, asset_manager: &mut AssetManager) -> Shot {
    Shot {
		base: BaseActor {
            asset: asset_manager.make_sprite(ctx, "/shot.png"),
        	pos: Point2::origin(),
        	facing: 0.,
        	velocity: Vector2::new(0.0, 0.0),
        	bbox_size: SHOT_BBOX,
            rvel: SHOT_RVEL,
            alive: true,
            physics_id: asset_manager.next_physics_id(),
		},
        time_to_live: SHOT_LIFE,
    }
}
