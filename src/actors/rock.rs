use ggez::Context;
use ggez::graphics::{Point2, Vector2};
use rand;
use std;
use actors::{Actor, BaseActor, Collidable, Drawable, Updatable};
use na;

use assets::{Sprite, Asset, AssetManager};
use util::*;

const ROCK_LIFE: f32 = 1.0;
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
