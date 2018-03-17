
use rand;
use std;
use ggez::graphics::{Drawable, Point2, Vector2};
use ggez::{graphics, Context, GameResult};

/// Create a unit vector representing the given angle (in radians)
pub fn vec_from_angle(angle: f32) -> Vector2 {
    let vx = angle.sin();
    let vy = angle.cos();
    Vector2::new(vx, vy)
}

/// BUGGO: TODO: Vector2 implements Rand so this is unnecessary
pub fn random_vec(max_magnitude: f32) -> Vector2 {
    let angle = rand::random::<f32>() * 2.0 * std::f32::consts::PI;
    let mag = rand::random::<f32>() * max_magnitude;
    vec_from_angle(angle) * (mag)
}

pub fn print_instructions() {
    println!("\nWelcome to Icarust!\n");
    println!("How to play:");
    println!("L/R arrow keys rotate your ship, up thrusts, space bar fires\n");
}

pub fn clamp_velocity(velocity: Vector2, max: f32) -> Option<Vector2> {
    let norm_sq = velocity.norm_squared();
    if norm_sq > max.powi(2) {
        return Some(velocity / norm_sq.sqrt() * max)
    }
    None
}

/// Translate the world coordinates (Y pointing up, origin at bottom left)
/// to screen coordinates (Y pointing down, origin at top left)
fn world_to_screen_coords(_screen_width: u32, screen_height: u32, point: Point2) -> Point2 {
    let height = screen_height as f32;

    Point2::new(point.x, height - point.y)
}

pub fn draw_image(ctx: &mut Context,
              drawable: &Drawable,
              position: Point2,
              facing: f32,
              world_coords: (u32, u32)) -> GameResult<()> {

    let (screen_w, screen_h) = world_coords;
    let pos = world_to_screen_coords(screen_w, screen_h, position);

    let drawparams = graphics::DrawParam {
        dest: pos,
        rotation: facing,
        offset: graphics::Point2::new(0.5, 0.5),
        ..Default::default()
    };
    graphics::draw_ex(ctx, drawable, drawparams)
}