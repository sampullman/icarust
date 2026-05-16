//! Pure deterministic simulation for Icarust.
//!
//! No I/O, no rendering, no global RNG, no wall-clock reads. Everything
//! random flows through the [`World`]'s seeded `ChaCha8Rng`. Entities live
//! in a [`std::collections::BTreeMap`] keyed by [`EntityId`] so iteration
//! order matches across machines, which is what makes
//! `(seed, input_history)` reproducible.
//!
//! `World::tick(&PlayerInputs, dt) -> Vec<GameEvent>` is the single
//! authoritative step. `dt` is fixed at `1.0 / 60.0` in production. AI
//! (`enemy::step`, `tank::step`) and wave scheduling (`wave::WaveDirector`)
//! live in their own modules but are driven from `World::tick`.

pub mod enemy;
pub mod entity;
pub mod event;
pub mod input;
pub mod physics;
pub mod player;
pub mod tank;
pub mod terrain;
pub mod util;
pub mod wave;
pub mod world;

pub use entity::{Entity, EntityId, EntityKind, PlayerId, ShotOwner, Tick};
pub use event::{DeathCause, GameEvent};
pub use input::{PlayerInput, PlayerInputs};
pub use terrain::{TerrainBand, TerrainKind};
pub use util::{Vec2, vec_from_angle};
pub use world::{World, WorldConfig};

/// Fixed simulation step — 60 Hz.
pub const TICK_DT: f32 = 1.0 / 60.0;
