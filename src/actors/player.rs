
use actors::*;
use assets::{AssetManager, SoundId, Sprite};

const PLAYER_BBOX: f32 = 12.0;

const PLAYER_THRUST: f32 = 420.0;
const PLAYER_GRAVITY: f32 = 120.0;
// Rotation in radians per second.
const PLAYER_TURN_RATE: f32 = 3.1;

// Seconds between shots
const PLAYER_SHOT_TIME: f32 = 0.5;
const SHOT_SPEED: f32 = 240.0;

#[derive(Debug, Actor, WrappedDrawable)]
pub struct Player {
	pub base: BaseActor<Sprite>,
    shot_timeout: f32,
    shot_sound_id: SoundId,
}

impl Collidable for Player {}

pub fn create_player(ctx: &mut Context, asset_manager: &mut AssetManager, screen_width: f32, screen_height: f32) -> Player {
    Player {
		base: BaseActor {
            asset: asset_manager.make_sprite(ctx, "/player.png"),
        	pos: Point2::new(screen_width / 2.0, screen_height / 2.0),
        	facing: 0.,
        	velocity: na::zero(),
        	bbox_size: PLAYER_BBOX,
            rvel: 0.,
            alive: true,
		},
        shot_timeout: 0.0,
        shot_sound_id: asset_manager.add_sound(ctx, "/pew.ogg"),
    }
}

impl Inputable for Player {

    fn handle_input(&mut self, input: &InputState, dt: f32) {
        self.add_facing(dt * PLAYER_TURN_RATE * input.xaxis);

        if input.yaxis > 0.0 {
            player_thrust(self, dt);
        }
    }
}

impl Updatable for Player {

    fn update(&mut self, _ctx: &mut Context, _asset_manager: &mut AssetManager, world_coords: (u32, u32), dt: f32) {
        update_actor_position(self, dt);
        wrap_actor_position(self, world_coords.0 as f32, world_coords.1 as f32);
        
        self.shot_timeout -= dt;

        let direction_vector = vec_from_angle(self.facing());
        let drag_vector = direction_vector * -1.25;
        let gravity_vector = Vector2::new(0.0, -PLAYER_GRAVITY);
        self.add_velocity((gravity_vector + drag_vector) * dt);
    }
}

impl Player {

    pub fn can_fire(&self) -> bool {
        self.shot_timeout < 0.0
    }

    pub fn fire_shot(&mut self, ctx: &mut Context, am: &mut AssetManager) -> Shot {
        use na::Vector2;

        self.shot_timeout = PLAYER_SHOT_TIME;

        let mut shot = create_shot(ctx, am);

        shot.set_facing(self.facing());
        let direction = vec_from_angle(shot.facing());
		shot.set_velocity_xy(SHOT_SPEED * direction.x, SHOT_SPEED * direction.y);

        let player_center = Point2::new(self.x()+self.half_width(), self.y()-self.half_height());
        let shot_pos = self.center(); //player_center + (direction.normalize() * self.half_height());
        shot.set_position(shot_pos);
        println!("Shot {}", self.facing());

        let _ = am.get_sound(self.shot_sound_id).play();
        shot
    }
}

fn player_thrust<T: Actor>(actor: &mut T, dt: f32) {
    let direction_vector = vec_from_angle(actor.facing());
    let thrust_vector = direction_vector * (PLAYER_THRUST);
    actor.add_velocity(thrust_vector * (dt));
}

