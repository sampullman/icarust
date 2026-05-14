//! Wire protocol between Icarust client and server.
//!
//! Encodes with `postcard` (compact, no_std-friendly). Messages are binary
//! WebSocket frames; each frame holds exactly one [`ClientMsg`] or [`ServerMsg`].

use serde::{Deserialize, Serialize};
use sim::entity::{EntityId, EntityKind, PlayerId, Tick};
use sim::terrain::TerrainBand;
use sim::util::WireVec2;
use sim::{GameEvent, PlayerInput};

/// Default WebSocket address the server listens on and the client connects
/// to. Override with `ICARUST_SERVER` or a CLI flag in the client.
pub const DEFAULT_ADDR: &str = "127.0.0.1:4015";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        name: String,
    },
    Input {
        tick: Tick,
        input: PlayerInput,
    },
    Bye,
    /// Ask the server to put the player back in the world after dying.
    Respawn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome {
        player_id: PlayerId,
        seed: u64,
        world_size: WireVec2,
        snapshot: Snapshot,
    },
    Snapshot(Snapshot),
    Events {
        tick: Tick,
        events: Vec<GameEvent>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: Tick,
    pub entities: Vec<EntityState>,
    pub score_by_player: Vec<(PlayerId, i32)>,
    pub level: i32,
    /// Active terrain layout. Re-sent every snapshot so the client can
    /// pick up new terrain when levels eventually change it.
    pub terrain: Vec<TerrainBand>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EntityState {
    pub id: EntityId,
    pub kind: EntityKind,
    pub pos: WireVec2,
    pub vel: WireVec2,
    /// Body orientation (radians). For ships and shots this is the nose
    /// angle; for tanks it's the chassis angle (always near `±PI/2`).
    pub facing: f32,
    /// Independent turret aim direction (radians). Only meaningful for
    /// tanks; ships/shots send 0.0.
    pub turret_facing: f32,
    pub alive: bool,
    /// Current HP. Meaningful for any entity with `max_hp > 0` (player,
    /// tanks); zero elsewhere.
    pub hp: i16,
    /// Max HP. Zero on entities that don't carry HP.
    pub max_hp: i16,
    /// True if a player entity is firing thrust this tick. Client uses
    /// this to draw exhaust flames behind the ship.
    pub thrusting: bool,
}

impl EntityState {
    pub fn from_entity(e: &sim::Entity) -> Self {
        EntityState {
            id: e.id,
            kind: e.kind,
            pos: e.pos.into(),
            vel: e.vel.into(),
            facing: e.facing,
            turret_facing: e.turret_facing,
            alive: e.alive,
            hp: e.hp,
            max_hp: e.max_hp,
            thrusting: e.thrusting,
        }
    }
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).expect("postcard encode")
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, postcard::Error> {
    postcard::from_bytes(bytes)
}

/// Build a `Snapshot` from a `sim::World`. Server convenience.
pub fn snapshot_from_world(world: &sim::World) -> Snapshot {
    let entities = world
        .entities()
        .filter(|e| e.alive)
        .map(EntityState::from_entity)
        .collect();
    let score_by_player = world.scores().iter().map(|(p, s)| (*p, *s)).collect();
    let terrain = world.terrain().to_vec();
    Snapshot {
        tick: world.tick_index(),
        entities,
        score_by_player,
        level: world.level(),
        terrain,
    }
}
