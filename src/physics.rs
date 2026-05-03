use crate::actors::Actor;

pub fn collides<A: Actor, B: Actor>(a: &A, b: &B) -> bool {
    let distance = a.position() - b.position();
    distance.length() < (a.bbox_size() + b.bbox_size())
}
