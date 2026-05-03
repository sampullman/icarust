use ggez::glam::Vec2;

pub type Point2 = Vec2;
pub type Vector2 = Vec2;

/// Unit vector for `angle` radians.
///
/// World space is Y-up, and the convention here is `(sin, cos)` rather than the
/// usual `(cos, sin)` so that `facing == 0` points along +Y (up). Increasing
/// `angle` rotates clockwise on screen, matching the sprite rotation applied by
/// `ggez::graphics::DrawParam::rotation`.
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
    println!("  Left/Right arrows: rotate");
    println!("  Up arrow:          thrust");
    println!("  Space:             fire");
    println!("  R:                 restart after death");
    println!("  Escape:            quit\n");
}

pub fn clamp_velocity(velocity: Vector2, max: f32) -> Option<Vector2> {
    let norm_sq = velocity.length_squared();
    if norm_sq > max.powi(2) {
        return Some(velocity / norm_sq.sqrt() * max);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn approx(a: Vector2, b: Vector2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn vec_from_angle_zero_points_up() {
        assert!(approx(vec_from_angle(0.0), Vector2::new(0.0, 1.0)));
    }

    #[test]
    fn vec_from_angle_quarter_points_right() {
        assert!(approx(vec_from_angle(FRAC_PI_2), Vector2::new(1.0, 0.0)));
    }

    #[test]
    fn vec_from_angle_half_points_down() {
        assert!(approx(vec_from_angle(PI), Vector2::new(0.0, -1.0)));
    }

    #[test]
    fn clamp_velocity_under_max_is_none() {
        assert!(clamp_velocity(Vector2::new(3.0, 4.0), 10.0).is_none());
    }

    #[test]
    fn clamp_velocity_over_max_clamps_to_max() {
        let v = clamp_velocity(Vector2::new(30.0, 40.0), 10.0).unwrap();
        assert!((v.length() - 10.0).abs() < 1e-5);
    }
}
