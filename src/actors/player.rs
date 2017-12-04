
use actors::*;
use assets::{AssetManager, SoundId, Sprite};

const PLAYER_LIFE: f32 = 1.0;
const PLAYER_BBOX: f32 = 12.0;

const PLAYER_THRUST: f32 = 100.0;
// Rotation in radians per second.
const PLAYER_TURN_RATE: f32 = 3.05;

// Seconds between shots
const PLAYER_SHOT_TIME: f32 = 0.5;
const SHOT_SPEED: f32 = 240.0;

#[derive(Debug, Actor)]
pub struct Player {
	pub actor: BaseActor<Sprite>,
    shot_timeout: f32,
    shot_sound_id: SoundId,
}

impl Collidable for Player {}

pub fn create_player(ctx: &mut Context, asset_manager: &mut AssetManager, screen_width: f32, screen_height: f32) -> Player {
    Player {
		actor: BaseActor {
            asset: asset_manager.make_sprite(ctx, "/player.png"),
        	pos: Point2::new(screen_width / 2.0, screen_height / 2.0),
        	facing: 0.,
        	velocity: na::zero(),
        	bbox_size: PLAYER_BBOX,
        	life: PLAYER_LIFE,
            rvel: 0.,
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

    fn update(&mut self, ctx: &mut Context, asset_manager: &mut AssetManager, dt: f32) {
        self.shot_timeout -= dt;
    }
}

impl Player {

    pub fn can_fire(&self) -> bool {
        self.shot_timeout < 0.0
    }

    pub fn fire_shot(&mut self, ctx: &mut Context, am: &mut AssetManager) -> Shot {

        self.shot_timeout = PLAYER_SHOT_TIME;

        let mut shot = create_shot(ctx, am);
        shot.set_position(self.position());
        shot.set_facing(self.facing());
        let direction = vec_from_angle(shot.facing());
		shot.set_velocity_xy(SHOT_SPEED * direction.x, SHOT_SPEED * direction.y);

        let _ = am.get_sound(self.shot_sound_id).play();
        shot
    }
}

fn player_thrust<T: Actor>(actor: &mut T, dt: f32) {
    let direction_vector = vec_from_angle(actor.facing());
    let thrust_vector = direction_vector * (PLAYER_THRUST);
    actor.add_velocity(thrust_vector * (dt));
}

