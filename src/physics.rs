use crate::actors::Actor;
use crate::util::Point2;

pub fn collides<A: Actor, B: Actor>(a: &A, b: &B) -> bool {
    circles_overlap(a.position(), a.bbox_size(), b.position(), b.bbox_size())
}

pub fn circles_overlap(a: Point2, ra: f32, b: Point2, rb: f32) -> bool {
    (a - b).length() < (ra + rb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_when_close() {
        assert!(circles_overlap(
            Point2::new(0.0, 0.0),
            5.0,
            Point2::new(8.0, 0.0),
            5.0,
        ));
    }

    #[test]
    fn no_overlap_when_far() {
        assert!(!circles_overlap(
            Point2::new(0.0, 0.0),
            5.0,
            Point2::new(20.0, 0.0),
            5.0,
        ));
    }

    #[test]
    fn touching_at_combined_radius_does_not_overlap() {
        assert!(!circles_overlap(
            Point2::new(0.0, 0.0),
            5.0,
            Point2::new(10.0, 0.0),
            5.0,
        ));
    }
}
