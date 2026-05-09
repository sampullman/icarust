use crate::entity::Entity;
use crate::util::Vec2;

pub fn collides(a: &Entity, b: &Entity) -> bool {
    circles_overlap(a.pos, a.bbox, b.pos, b.bbox)
}

pub fn circles_overlap(a: Vec2, ra: f32, b: Vec2, rb: f32) -> bool {
    (a - b).length() < (ra + rb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_when_close() {
        assert!(circles_overlap(
            Vec2::new(0.0, 0.0),
            5.0,
            Vec2::new(8.0, 0.0),
            5.0,
        ));
    }

    #[test]
    fn no_overlap_when_far() {
        assert!(!circles_overlap(
            Vec2::new(0.0, 0.0),
            5.0,
            Vec2::new(20.0, 0.0),
            5.0,
        ));
    }

    #[test]
    fn touching_at_combined_radius_does_not_overlap() {
        assert!(!circles_overlap(
            Vec2::new(0.0, 0.0),
            5.0,
            Vec2::new(10.0, 0.0),
            5.0,
        ));
    }
}
