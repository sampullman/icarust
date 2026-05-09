//! Pure deterministic simulation for Icarust.
//!
//! No I/O, no rendering, no global RNG, no wall-clock reads. Everything
//! random goes through the [`World`]'s seeded `ChaCha8Rng`. Entities live
//! in a [`std::collections::BTreeMap`] keyed by [`EntityId`] so iteration
//! order matches across machines.
//!
//! `World::tick(&PlayerInputs, dt) -> Vec<GameEvent>` is the single
//! authoritative step. `dt` is fixed at `1.0 / 60.0` in production.

pub mod enemy;
pub mod entity;
pub mod event;
pub mod input;
pub mod physics;
pub mod player;
pub mod util;
pub mod world;

pub use entity::{Entity, EntityId, EntityKind, PlayerId, ShotOwner, Tick};
pub use event::GameEvent;
pub use input::{PlayerInput, PlayerInputs};
pub use util::{Vec2, vec_from_angle};
pub use world::{World, WorldConfig};

/// Fixed simulation step — 60 Hz.
pub const TICK_DT: f32 = 1.0 / 60.0;
