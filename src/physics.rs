
use std::cell::Cell;
use na;
use na::{Vector2, Point2, Isometry2};
use ncollide::world::{CollisionWorld, CollisionGroups, GeometricQueryType, CollisionObject2};
use ncollide::narrow_phase::{ContactHandler, ContactAlgorithm2};
use ncollide::shape::{Plane, Ball, Cuboid, ShapeHandle2};

#[derive(Clone)]
struct CollisionObjectData {
    pub name:     &'static str,
    pub velocity: Option<Cell<Vector2<f32>>>
}

impl CollisionObjectData {
    pub fn new(name: &'static str, velocity: Option<Vector2<f32>>) -> CollisionObjectData {
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

struct VelocityBouncer;

impl ContactHandler<Point2<f32>, Isometry2<f32>, CollisionObjectData> for VelocityBouncer {
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
        println!("CONTACT! {} {}", co1.data.name, co2.data.name);
    }

    fn handle_contact_stopped(&mut self,
                              _: &CollisionObject2<f32, CollisionObjectData>,
                              _: &CollisionObject2<f32, CollisionObjectData>) {
        // We don't care.
    }
}

pub fn test_collide(player_point: Point2<f32>, rocks: &Vec<Point2<f32>>) {
    let plane_bottom = ShapeHandle2::new(Plane::new(Vector2::y()));
    let plane_bottom_pos = Isometry2::new(Vector2::new(0.0, 50.0), na::zero());
    let plane_data = CollisionObjectData::new("ground", None);

    // Shared cuboid for the rectangular areas.
    let player = ShapeHandle2::new(Cuboid::new(Vector2::new(32f32, 32.0)));
    let player_data = CollisionObjectData::new("player", None);
    let player_pos = Isometry2::new(Vector2::new(player_point.x, player_point.y), na::zero());
    let mut player_groups = CollisionGroups::new();
    player_groups.set_membership(&[1]);

    // Rock shape.
    let rock = ShapeHandle2::new(Ball::new(16f32));
    let rock_data = CollisionObjectData::new("rock", None);
    let mut rock_groups = CollisionGroups::new();
    rock_groups.set_membership(&[2]);
    rock_groups.set_whitelist(&[1]);

    let mut others_groups = CollisionGroups::new();
    others_groups.set_membership(&[3]);
    others_groups.set_whitelist(&[1, 2]);

    let mut world = CollisionWorld::new(0.02, true);

    let contacts_query = GeometricQueryType::Contacts(0.0);

    world.deferred_add(0, plane_bottom_pos, plane_bottom, others_groups, contacts_query, plane_data);
    let mut index = 1;
    for rock_point in rocks.into_iter() {
        let rock_pos = Isometry2::new(Vector2::new(rock_point.x, rock_point.y), na::zero());
        world.deferred_add(index, rock_pos, rock.clone(), rock_groups, contacts_query, rock_data.clone());
        index += 1;
    }
    world.deferred_add(index, player_pos, player, player_groups, GeometricQueryType::Contacts(0.0), player_data);

    world.register_contact_handler("VelocityBouncer", VelocityBouncer);

    world.update()
}