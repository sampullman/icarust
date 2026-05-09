use crate::util::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Tick(pub u64);

impl Tick {
    pub fn next(self) -> Tick {
        Tick(self.0 + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    Player { player_id: PlayerId },
    Rock,
    Shot { owner: PlayerId },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub pos: Vec2,
    pub vel: Vec2,
    pub facing: f32,
    pub bbox: f32,
    pub alive: bool,
    /// Lifetime in seconds (only meaningful for shots).
    pub ttl: Option<f32>,
    /// Seconds until this player can fire next; `<= 0` means ready.
    /// Only meaningful for players.
    pub shot_cooldown: f32,
}

impl Entity {
    pub fn player(id: EntityId, player_id: PlayerId, pos: Vec2) -> Self {
        Entity {
            id,
            kind: EntityKind::Player { player_id },
            pos,
            vel: Vec2::ZERO,
            facing: 0.0,
            bbox: crate::player::PLAYER_BBOX,
            alive: true,
            ttl: None,
            shot_cooldown: 0.0,
        }
    }

    pub fn rock(id: EntityId, pos: Vec2, vel: Vec2) -> Self {
        Entity {
            id,
            kind: EntityKind::Rock,
            pos,
            vel,
            facing: 0.0,
            bbox: crate::world::ROCK_BBOX,
            alive: true,
            ttl: None,
            shot_cooldown: 0.0,
        }
    }

    pub fn shot(id: EntityId, owner: PlayerId, pos: Vec2, vel: Vec2, facing: f32) -> Self {
        Entity {
            id,
            kind: EntityKind::Shot { owner },
            pos,
            vel,
            facing,
            bbox: crate::world::SHOT_BBOX,
            alive: true,
            ttl: Some(crate::world::SHOT_LIFE),
            shot_cooldown: 0.0,
        }
    }

    pub fn player_id(&self) -> Option<PlayerId> {
        match self.kind {
            EntityKind::Player { player_id } => Some(player_id),
            _ => None,
        }
    }
}
