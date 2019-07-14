
use std::cell::Cell;
use na;
use na::{Isometry2};
use ncollide2d::world::{CollisionWorld, CollisionGroups, GeometricQueryType, CollisionObject, CollisionObjectHandle};
use ncollide2d::narrow_phase::{ContactAlgorithm};
use ncollide2d::shape::{Plane, Ball, Cuboid, ShapeHandle};
use ggez::{Context};
use crate::actors::{Actor, Collidable};
use crate::util::{Vector2};

type NAPoint2 = na::Point2<f32>;
type NAVector2 = na::Vector2<f32>;

pub type PhysicsId = CollisionObjectHandle;
pub type GroupId = usize;

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
    fn new(id: GroupId) -> CollisionGroup {
        CollisionGroup {
            groups: CollisionGroups::new(),
            id: id,
        }
    }
}

pub struct CollisionWorld2 {
    world: CollisionWorld<f32, CollisionData>,
    object_id_pool: Vec<PhysicsId>,
    groups: Vec<CollisionGroup>,
}

impl CollisionWorld2 {

    fn new() -> CollisionWorld2 {
        CollisionWorld2 {
            world: CollisionWorld::new(0.02),
            object_id_pool: Vec::new(),
            groups: Vec::new(),
        }
    }

    pub fn add<T: Actor+Collidable>(&mut self, ctx: &mut Context, actor: T, query: Query, group_id: GroupId, shape: Shape) -> PhysicsId {
        let query_type = match query {
            Proximity => GeometricQueryType::Proximity(0.0),
            Contact => GeometricQueryType::Contacts(0.0, 0.0),
        };
        let shape_handle = match shape {
            Rect => {
                let cube = Cuboid::new(NAVector2::new(actor.width(ctx), actor.height(ctx)));
                ShapeHandle::new(cube)
            },
            Circle => {
                let ball = Ball::new((actor.width(ctx) + actor.height(ctx)) / 2.0);
                ShapeHandle::new(ball)
            }
        };
        let position = Isometry2::new(NAVector2::new(actor.x(), actor.y()), na::zero());
        if let Some(groups) = self.get_group(group_id) {
            return self.world.add(position, shape_handle, groups, query_type, actor.collision_data());
        }
        CollisionObjectHandle(0)
    }

    pub fn make_group(&mut self) -> GroupId {
        let id: GroupId = self.groups.len() + 1;
        self.groups.push(CollisionGroup::new(id));
        id
    }

    pub fn get_group(&self, id: GroupId) -> Option<CollisionGroups> {
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
    pub velocity: Option<Cell<Vector2>>,
    pub hit: Cell<Option<PhysicsId>>,
}

impl CollisionData {
    pub fn new(id: PhysicsId, velocity: Option<Vector2>) -> CollisionData {
        let init_velocity;
        if let Some(velocity) = velocity {
            init_velocity = Some(Cell::new(velocity))
            
        } else {
            init_velocity = None
        }

        CollisionData {
            id,
            velocity: init_velocity,
            hit: Cell::new(None)
        }
    }
}

/*
impl ContactHandler<NAPoint2, Isometry2<f32>, CollisionData> for VelocityBouncer {
    fn handle_contact_started(&mut self,
                              co1: &CollisionObject2<f32, CollisionData>,
                              co2: &CollisionObject2<f32, CollisCollisionDataionObjectData>,
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
                              _: &CollisionObject2<f32, CollisionData>,
                              _: &CollisionObject2<f32, CollisionData>) {
        // We don't care.
    }
}
*/

pub fn new_world(ctx: &mut Context, rock_count: i32) -> CollisionWorld2 {
    let plane_bottom = ShapeHandle::new(Plane::new(NAVector2::y_axis()));
    let plane_bottom_pos = Isometry2::new(NAVector2::new(0.0, 50.0), na::zero());
    let plane_data = CollisionData::new(CollisionObjectHandle(0), None); // TODO -- generate unique id

    let mut others_groups = CollisionGroups::new();
    others_groups.set_membership(&[3]);
    others_groups.set_whitelist(&[1, 2]);

    let mut world = CollisionWorld2::new();

    let contacts_query = GeometricQueryType::Contacts(0.0, 0.0);

    // actor: T, query: Query, group_id: GroupId, shape: Shape
    world.world.add(plane_bottom_pos, plane_bottom, others_groups, contacts_query, plane_data);

    //world.register_contact_handler("VelocityBouncer", VelocityBouncer);

    world
}

pub fn update_world<T, U>(world: &mut CollisionWorld2, player: &T, rocks: &Vec<U>)
        where T: Actor, U: Actor {

    let player_pos = Isometry2::new(NAVector2::new(player.x(), player.y()), na::zero());
    world.world.set_position(player.physics_id(), player_pos);

    let mut index = 1;
    for rock in rocks.iter() {
        world.world.set_position(rock.physics_id(), Isometry2::new(NAVector2::new(rock.x(), rock.y()), na::zero()));
        index += 1;
    }
    world.world.update();
}
