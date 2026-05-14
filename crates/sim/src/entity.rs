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

/// Who fired a shot. Player shots score on rock/enemy kills; enemy shots
/// don't credit anyone but still chip away at player HP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShotOwner {
    Player(PlayerId),
    Enemy,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    Player { player_id: PlayerId },
    Rock,
    Shot { owner: ShotOwner },
    Enemy,
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
    /// Only meaningful for players and enemies.
    pub shot_cooldown: f32,
    /// Current hit points. Only meaningful for players; non-player kinds
    /// keep this at 0.
    pub hp: i16,
    /// Maximum hit points for this entity (only meaningful for players).
    pub max_hp: i16,
    /// Seconds since the player last took damage. Players regen HP after a
    /// grace period of `player::PLAYER_REGEN_DELAY` seconds.
    pub damage_timer: f32,
    /// True if the player is firing thrust this tick. Surfaces to the
    /// client so it can draw a flame/exhaust trail.
    pub thrusting: bool,
}

impl Entity {
    pub fn player(id: EntityId, player_id: PlayerId, pos: Vec2) -> Self {
        let max_hp = crate::player::PLAYER_MAX_HP;
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
            hp: max_hp,
            max_hp,
            // Start "fully healed" — large value, regen logic is dormant
            // until the first hit lands.
            damage_timer: f32::MAX / 2.0,
            thrusting: false,
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
            hp: 0,
            max_hp: 0,
            damage_timer: 0.0,
            thrusting: false,
        }
    }

    pub fn shot(id: EntityId, owner: ShotOwner, pos: Vec2, vel: Vec2, facing: f32) -> Self {
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
            hp: 0,
            max_hp: 0,
            damage_timer: 0.0,
            thrusting: false,
        }
    }

    pub fn enemy(id: EntityId, pos: Vec2) -> Self {
        Entity {
            id,
            kind: EntityKind::Enemy,
            pos,
            vel: Vec2::ZERO,
            facing: std::f32::consts::PI, // start pointing down (toward play area)
            bbox: crate::enemy::ENEMY_BBOX,
            alive: true,
            ttl: None,
            shot_cooldown: 0.0,
            hp: 0,
            max_hp: 0,
            damage_timer: 0.0,
            thrusting: false,
        }
    }

    pub fn player_id(&self) -> Option<PlayerId> {
        match self.kind {
            EntityKind::Player { player_id } => Some(player_id),
            _ => None,
        }
    }
}
