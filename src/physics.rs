
use std::cell::Cell;
use ggez::graphics::{Point2};
use na;
use na::{Vector2, Point2, Isometry2};
use ncollide::world::{CollisionWorld, CollisionGroups, GeometricQueryType, CollisionObject2};
use ncollide::narrow_phase::{ContactHandler, ContactAlgorithm2};
use ncollide::shape::{Plane, Ball, Cuboid, ShapeHandle2};
use actors::Actor;

pub type CollisionWorld2 = CollisionWorld<f32, CollisionObjectData>;
type NAPoint2 = na::Point2<f32>;
type NAVector2 = na::Vector2<f32>;

pub type PhysicsId = u32;
pub type GroupId = u32;

// Helper/Wrapper for GeometricQueryType
pub enum Query {
    Contact,
    Proximity,
}

// Indicator for collision geometry type
pub enum Shape {
    Rect,
    Circle,
}

/// Wrap ncollide's CollisionGroups for easy access to it's ID
/// This changes the semantics to assume a group only has a single member
struct CollisionGroup {
    groups: CollisionGroups,
    id: GroupId
}

impl CollisionGroup {
    fn new::(id: GroupId) {
        CollisionGroup {
            groups: CollisionGroups::new(),
            id: id,
        }
    }
}

pub struct CollisionWorld2 {
    world: CollisionWorld<Point2<f32>, Isometry2<f32>, CollisionObjectData>,
    object_id_pool: Vec<PhysicsId>,
    groups: Vec<CollisionGroup>,
}

impl CollisionWorld2 {

    fn new() -> CollisionWorld2 {
        CollitionWorld2 {
            world: CollisionWorld::new(0.02, true),
            object_id_pool: Vec::new(),
            groups: Vec::new(),
        }
    }

    pub fn add<T: Actor+Collidable>(&mut self, actor: T, query: Query, group_id: GroupId, shape: Shape) -> PhysicsId {
        let query_type = match query {
            Proximity -> GeometricQueryType::Proximity(0.0),
            Contact -> GeometricQueryType::Contact(0.0),
        };
        let shape_handle = ShapeHandle2::new(match shape {
            Rect -> Cuboid::new(Vector2::new(actor.width(), actor.height())),
            Circle -> Ball::new((actor.width() + actor.height()) / 2.0),
        });
        let index = self.get_index();
        let position = Isometry2::new(Vector2::new(actor.x(), actor.y()), na::zero());
        let group = self.get_group(group_id);
        self.world.deferred_add(index, position, shape_handle, group, query_type, actor.collision_data());
    }

    pub fn make_group(&mut self) -> GroupId {
        let id: GroupId = self.groups.len() + 1;
        self.groups.push(CollisionGroup::new(id));
        id
    }

    pub fn get_group(&self, id: GroupId) -> Option<&CollisionGroups> {
        for group in self.groups.iter() {
            if group.id == id {
                return Some(group.groups)
            }
        }
        return None
    }

}

#[derive(Clone)]
pub struct CollisionData {
    pub id: PhysicsId,
    pub velocity: Option<Cell<Vector2<f32>>>,
    pub hit: Cell<Option<PhysicsId>>,
}

impl CollisionObjectData {
    pub fn new(name: &'static str, velocity: Option<NAVector2>) -> CollisionObjectData {
        let init_velocity;
        if let Some(velocity) = velocity {
            init_velocity = Some(Cell::new(velocity))
            
        } else {
            init_velocity = None
        }

        CollisionObjectData {
            name:     name,
            velocity: init_velocity
        }
    }
}

/*
impl ContactHandler<NAPoint2, Isometry2<f32>, CollisionObjectData> for VelocityBouncer {
    fn handle_contact_started(&mut self,
                              co1: &CollisionObject2<f32, CollisionObjectData>,
                              co2: &CollisionObject2<f32, CollisionObjectData>,
                              alg: &ContactAlgorithm2<f32>) {
        // NOTE: real-life applications would avoid this systematic allocation.
        let mut collector = Vec::new();
        alg.contacts(&mut collector);

        // The ball is the one with a non-None velocity.
        if let Some(ref vel) = co1.data.velocity {
            let normal = collector[0].normal;
            vel.set(vel.get() - 2.0 * na::dot(&vel.get(), &normal) * normal);
        }
        if let Some(ref vel) = co2.data.velocity {
            let normal = -collector[0].normal;
            vel.set(vel.get() - 2.0 * na::dot(&vel.get(), &normal) * normal);
        }
        co1.data.hit = Some(co2.data.id);
        co2.data.hit = Some(co1.data.id);
        println!("CONTACT! {} {}", co1.data.id, co2.data.id);
    }

    fn handle_contact_stopped(&mut self,
                              _: &CollisionObject2<f32, CollisionObjectData>,
                              _: &CollisionObject2<f32, CollisionObjectData>) {
        // We don't care.
    }
}
*/

pub fn new_world(rock_count: i32) -> CollisionWorld2 {
    let plane_bottom = ShapeHandle::new(Plane::new(NAVector2::y_axis()));
    let plane_bottom_pos = Isometry2::new(NAVector2::new(0.0, 50.0), na::zero());
    let plane_data = CollisionObjectData::new("ground", None);

    let mut others_groups = CollisionGroups::new();
    others_groups.set_membership(&[3]);
    others_groups.set_whitelist(&[1, 2]);

    let mut world = CollisionWorld2::new();

    let contacts_query = GeometricQueryType::Contacts(0.0, 0.0);

    actor: T, query: Query, group_id: GroupId, shape: Shape
    world.add(0, plane_bottom_pos, plane_bottom, others_groups, contacts_query, plane_data);

    //world.register_contact_handler("VelocityBouncer", VelocityBouncer);

    world
}

pub fn update_world<T, U>(world: &mut CollisionWorld2, player: &T, rocks: &Vec<U>)
        where T: Actor, U: Actor {

    let player_pos = Isometry2::new(Vector2::new(player.x(), player.y()), na::zero());
    world.deferred_set_position(6, player_pos);

    let mut index = 1;
    for rock in rocks.iter() {
        world.deferred_set_position(index, Isometry2::new(Vector2::new(rock.x(), rock.y()), na::zero()));
        index += 1;
    }
    world.update();
}
