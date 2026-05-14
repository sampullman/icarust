use crate::entity::{PlayerId, ShotOwner};
use crate::terrain::TerrainKind;
use crate::util::Vec2;
use serde::{Deserialize, Serialize};

/// What killed a player. The client uses this to pick a crash animation —
/// e.g. fiery debris for an enemy collision, dust + sparks for a ground
/// crash, eventually a splash for a water crash.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DeathCause {
    Enemy,
    EnemyShot,
    Terrain(TerrainKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum GameEvent {
    PlayerJoined(PlayerId),
    PlayerLeft(PlayerId),
    ShotFired { owner: ShotOwner, pos: Vec2 },
    /// A hostile (ship or tank) took a non-fatal hit. The client uses this
    /// to play a metallic clink and spark, distinct from the bigger
    /// `EnemyKilled` explosion.
    EnemyDamaged { pos: Vec2, hp: i16 },
    EnemyKilled { pos: Vec2, killer: PlayerId },
    /// Player took a non-fatal hit. The client can use this to play an
    /// "ouch" sound and spawn a little burst of smoke without ending the
    /// game.
    PlayerDamaged {
        player_id: PlayerId,
        pos: Vec2,
        /// HP remaining after the hit.
        hp: i16,
    },
    PlayerKilled {
        player_id: PlayerId,
        pos: Vec2,
        cause: DeathCause,
    },
    LevelUp(i32),
}
