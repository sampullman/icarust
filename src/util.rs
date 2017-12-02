
use rand;
use std;
use ggez::graphics::Vector2;

/// Create a unit vector representing the
/// given angle (in radians)
/// BUGGO: TODO: We should be able to create these from a Rotation?
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