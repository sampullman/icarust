
use actors::*;

const PLAYER_LIFE: f32 = 1.0;
const PLAYER_BBOX: f32 = 12.0;

const PLAYER_THRUST: f32 = 100.0;
// Rotation in radians per second.
const PLAYER_TURN_RATE: f32 = 3.05;

#[derive(Debug, Actor)]
pub struct Player {
	pub actor: BaseActor,
}

impl Collidable for Player {}

pub fn create_player(screen_width: f32, screen_height: f32) -> Player {
    Player {
		actor: BaseActor {
        	tag: ActorType::Player,
        	pos: Point2::new(screen_width / 2.0, screen_height / 2.0),
        	facing: 0.,
        	velocity: na::zero(),
        	bbox_size: PLAYER_BBOX,
        	life: PLAYER_LIFE,
            rvel: 0.,
		},
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

fn player_thrust<T: Actor>(actor: &mut T, dt: f32) {
    let direction_vector = vec_from_angle(actor.facing());
    let thrust_vector = direction_vector * (PLAYER_THRUST);
    actor.add_velocity(thrust_vector * (dt));
}

