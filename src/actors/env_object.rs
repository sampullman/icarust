
use ggez::Context;
use ggez::graphics::{Point2};
use assets::{Sprite, Asset, AssetManager};

impl Collidable for Shot {}

impl Updatable for Shot {

    fn update(&mut self, _ctx: &mut Context, _asset_manager: &mut AssetManager, world_coords: (u32, u32), dt: f32) {
        update_actor_position(self, dt);
        wrap_actor_position(self, world_coords.0 as f32, world_coords.1 as f32);
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
        	velocity: na::zero(),
        	bbox_size: SHOT_BBOX,
            rvel: SHOT_RVEL,
            alive: true,
		},
        time_to_live: SHOT_LIFE,
    }
}