use crate::entity::{PlayerId, ShotOwner};
use crate::terrain::TerrainKind;
use crate::util::Vec2;
use serde::{Deserialize, Serialize};

/// What killed a player. The client uses this to pick a crash animation —
/// e.g. fiery debris for a rock collision, dust + sparks for a ground
/// crash, eventually a splash for a water crash.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DeathCause {
    Rock,
    Enemy,
    EnemyShot,
    Terrain(TerrainKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum GameEvent {
    PlayerJoined(PlayerId),
    PlayerLeft(PlayerId),
    ShotFired { owner: ShotOwner, pos: Vec2 },
    RockKilled { pos: Vec2, killer: PlayerId },
    EnemyKilled { pos: Vec2, killer: PlayerId },
    PlayerKilled {
        player_id: PlayerId,
        pos: Vec2,
        cause: DeathCause,
    },
    LevelUp(i32),
}
