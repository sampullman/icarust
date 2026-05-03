use ggez::glam::Vec2;

pub type Point2 = Vec2;
pub type Vector2 = Vec2;

/// Create a unit vector representing the given angle (in radians)
pub fn vec_from_angle(angle: f32) -> Vector2 {
    let vx = angle.sin();
    let vy = angle.cos();
    Vector2::new(vx, vy)
}

pub fn random_vec(max_magnitude: f32) -> Vector2 {
    let angle = rand::random::<f32>() * 2.0 * std::f32::consts::PI;
    let mag = rand::random::<f32>() * max_magnitude;
    vec_from_angle(angle) * mag
}

pub fn print_instructions() {
    println!("\nWelcome to Icarust!\n");
    println!("How to play:");
    println!("L/R arrow keys rotate your ship, up thrusts, space bar fires\n");
}

pub fn clamp_velocity(velocity: Vector2, max: f32) -> Option<Vector2> {
    let norm_sq = velocity.length_squared();
    if norm_sq > max.powi(2) {
        return Some(velocity / norm_sq.sqrt() * max);
    }
    None
}
