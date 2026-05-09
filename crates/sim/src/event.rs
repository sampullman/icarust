use crate::entity::{PlayerId, ShotOwner};
use crate::util::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum GameEvent {
    PlayerJoined(PlayerId),
    PlayerLeft(PlayerId),
    ShotFired { owner: ShotOwner, pos: Vec2 },
    RockKilled { pos: Vec2, killer: PlayerId },
    EnemyKilled { pos: Vec2, killer: PlayerId },
    PlayerKilled(PlayerId),
    LevelUp(i32),
}
