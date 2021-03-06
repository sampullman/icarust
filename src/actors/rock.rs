use ggez::Context;
use crate::util::{Point2, Vector2};
use rand;
use std;
use crate::actors::{Actor, BaseActor, Collidable, Drawable, Updatable};
use crate::physics::{CollisionWorld2, PhysicsId};
use crate::render::camera::Camera;

use crate::assets::{Sprite, Asset, AssetManager};
use crate::util;

const MAX_ROCK_VEL: f32 = 50.0;

const ROCK_BBOX: f32 = 12.0;

#[derive(Debug, Actor, Drawable)]
pub struct Rock {
	pub base: BaseActor<Sprite>,
}

impl Collidable for Rock {}
impl Updatable for Rock {}

pub fn create_rock(ctx: &mut Context, asset_manager: &mut AssetManager) -> Rock {
    Rock {
		base: BaseActor {
            asset: asset_manager.make_sprite(ctx, "/rock.png"),            
        	pos: Point2::origin(),
        	facing: 0.,
        	velocity: Vector2::new(0.0, 0.0),
        	bbox_size: ROCK_BBOX,
            rvel: 0.,
            alive: true,
            physics_id: asset_manager.next_physics_id(),
		},
    }
}

/// Create the `num` rocks.
/// Ensures none of them are within the exclusion zone (nominally the player)
/// This *could* create rocks outside the world bounds, so it should be
/// called before `wrap_actor_position()`
pub fn create_rocks(ctx: &mut Context, asset_manager: &mut AssetManager, num: i32, exclusion: Point2, min_radius: f32, max_radius: f32) -> Vec<Rock> {
    assert!(max_radius > min_radius);
    let new_rock = |_| {
        let mut rock = create_rock(ctx, asset_manager);
        let r_angle = rand::random::<f32>() * 2.0 * std::f32::consts::PI;
        let r_distance = rand::random::<f32>() * (max_radius - min_radius) + min_radius;
        rock.set_position(exclusion + util::vec_from_angle(r_angle) * r_distance);
        rock.set_velocity(util::random_vec(MAX_ROCK_VEL));
        rock
    };
    (0..num).map(new_rock).collect()
}
