
use rand;
use std;
use ggez::graphics::{Drawable, DrawParam, Point2, Vector2};
use ggez::{Context, GameResult};

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
    println!();
    println!("Welcome to ASTROBLASTO!");
    println!();
    println!("How to play:");
    println!("L/R arrow keys rotate your ship, up thrusts, space bar fires");
    println!();
}

/// Translates the world coordinate system, which
/// has Y pointing up and the origin at the center,
/// to the screen coordinate system, which has Y
/// pointing downward and the origin at the top-left,
fn world_to_screen_coords(_screen_width: u32, screen_height: u32, point: Point2) -> Point2 {
    let height = screen_height as f32;

    Point2::new(point.x, height - point.y)
}

pub fn draw_image(ctx: &mut Context,
              drawable: &Drawable,
              position: Point2,
              facing: f32,
              world_coords: (u32, u32))
              -> GameResult<()> {
    let (screen_w, screen_h) = world_coords;
    let pos = world_to_screen_coords(screen_w, screen_h, position);

    //graphics::draw(ctx, drawable, dest_point, facing)
    drawable.draw_ex(ctx, DrawParam { 
                            dest: pos,
                            rotation: facing,
                            ..Default::default()
    })
}