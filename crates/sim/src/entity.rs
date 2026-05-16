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

/// Who fired a shot. Player shots score kills on hostiles; the hostile
/// variants don't credit anyone but still chip away at player HP. `Tank`
/// is split out from `Enemy` so the client can render the heavier shell
/// (bigger sprite, recognizable silhouette) and the server can apply the
/// higher damage value associated with artillery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShotOwner {
    Player(PlayerId),
    Enemy,
    Tank,
}

impl ShotOwner {
    /// Hostile shots damage the player by this many HP per hit. Player
    /// shots don't hit other players, so the value for `Player(_)` is
    /// inert (we still return `1` so calling code stays branch-free).
    pub fn damage(self) -> i16 {
        match self {
            ShotOwner::Tank => 2,
            ShotOwner::Enemy | ShotOwner::Player(_) => 1,
        }
    }

    /// True if this shot was fired by a hostile entity (anything that
    /// can damage a player). Lets shared collision code treat all
    /// non-player ownerships uniformly.
    pub fn is_hostile(self) -> bool {
        matches!(self, ShotOwner::Enemy | ShotOwner::Tank)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    Player { player_id: PlayerId },
    Shot { owner: ShotOwner },
    /// Flying ship enemy.
    Enemy,
    /// Ground vehicle with a tracking turret. Body rolls on the terrain
    /// surface; turret rotates independently (`Entity::turret_facing`).
    Tank,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub pos: Vec2,
    pub vel: Vec2,
    /// Body orientation in radians. For ships and shots this is the nose
    /// direction; for tanks it's the chassis direction (`±PI/2`).
    pub facing: f32,
    /// Independent turret/aim direction. Only meaningful for tanks for
    /// now; ships keep this at 0. Kept on every entity so the wire
    /// shape doesn't have to fork per kind.
    pub turret_facing: f32,
    pub bbox: f32,
    pub alive: bool,
    /// Lifetime in seconds (only meaningful for shots).
    pub ttl: Option<f32>,
    /// Seconds until this entity can fire next; `<= 0` means ready.
    /// Meaningful for players, enemies, and tanks.
    pub shot_cooldown: f32,
    /// Current hit points. Meaningful for players and any hostile that
    /// takes more than one shot to kill (tanks).
    pub hp: i16,
    /// Maximum hit points for this entity.
    pub max_hp: i16,
    /// Seconds since the player last took damage. Players regen HP after a
    /// grace period of `player::PLAYER_REGEN_DELAY` seconds.
    pub damage_timer: f32,
    /// Fractional HP carried over from contact damage between players and
    /// hostiles. Tick accumulates `RAM_DAMAGE_PER_SECOND * dt`; when it
    /// crosses `1.0` we deduct whole HP. Reset to zero whenever the
    /// entity isn't touching an opposing entity that tick, so two short
    /// brushes don't compound into a kill.
    pub contact_damage_accum: f32,
    /// True if the player is firing thrust this tick. Surfaces to the
    /// client so it can draw a flame/exhaust trail.
    pub thrusting: bool,
    /// Per-tick acceleration applied during the move step. Used for
    /// gravity-affected projectiles (tank shells); zero on everything
    /// else. Player gravity lives in `player::apply_forces` rather than
    /// here because it interacts with drag and the speed clamp.
    pub accel: Vec2,
    /// Entity that spawned this one — typically the tank that fired a
    /// shell. Used by friendly-fire collision to skip self-hits without
    /// having to read it back off the wire (this field is sim-only).
    pub source: Option<EntityId>,
    /// True if this shot detonates on contact with terrain instead of
    /// bouncing. Heavy artillery flips this on; ship bullets keep it
    /// off so they ricochet along the hillside. A flat boolean keeps
    /// the move-step branch trivial and leaves room for other behaviors
    /// to opt in (e.g. a future "smart bomb" that also detonates).
    pub detonates_on_terrain: bool,
    /// Player ships start with gravity disabled so a freshly-spawned pilot
    /// doesn't drift into the ground while they're still figuring out the
    /// controls. The first thrust input latches this on; from that point
    /// `player::apply_forces` applies gravity normally. Sim-only — never
    /// surfaces on the wire.
    pub gravity_armed: bool,
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
            turret_facing: 0.0,
            bbox: crate::player::PLAYER_BBOX,
            alive: true,
            ttl: None,
            shot_cooldown: 0.0,
            hp: max_hp,
            max_hp,
            // Start "fully healed" — large value, regen logic is dormant
            // until the first hit lands.
            damage_timer: f32::MAX / 2.0,
            contact_damage_accum: 0.0,
            thrusting: false,
            accel: Vec2::ZERO,
            source: None,
            detonates_on_terrain: false,
            // Newly-spawned pilot: gravity stays off until they thrust.
            gravity_armed: false,
        }
    }

    pub fn shot(id: EntityId, owner: ShotOwner, pos: Vec2, vel: Vec2, facing: f32) -> Self {
        // Player bullets fly farther than enemy bullets because the pilot
        // is usually moving (and dodging) while shooting. The extra
        // life-time lets the shot still cover a meaningful distance even
        // after the pilot peels off.
        let ttl = match owner {
            ShotOwner::Player(_) => crate::player::PLAYER_SHOT_LIFE,
            _ => crate::world::SHOT_LIFE,
        };
        Entity {
            id,
            kind: EntityKind::Shot { owner },
            pos,
            vel,
            facing,
            turret_facing: 0.0,
            bbox: crate::world::SHOT_BBOX,
            alive: true,
            ttl: Some(ttl),
            shot_cooldown: 0.0,
            hp: 0,
            max_hp: 0,
            damage_timer: 0.0,
            contact_damage_accum: 0.0,
            thrusting: false,
            accel: Vec2::ZERO,
            source: None,
            detonates_on_terrain: false,
            gravity_armed: true,
        }
    }

    /// A shot that's affected by per-tick `accel` — used for tank shells
    /// so they arc under gravity. Caller supplies the hit radius, the
    /// lifetime, and the spawning entity so friendly-fire collision can
    /// skip self-hits. Artillery always detonates on terrain rather
    /// than bouncing.
    pub fn artillery_shot(
        id: EntityId,
        owner: ShotOwner,
        pos: Vec2,
        vel: Vec2,
        facing: f32,
        accel: Vec2,
        bbox: f32,
        ttl: f32,
        source: Option<EntityId>,
    ) -> Self {
        let mut e = Self::shot(id, owner, pos, vel, facing);
        e.accel = accel;
        e.bbox = bbox;
        e.ttl = Some(ttl);
        e.source = source;
        e.detonates_on_terrain = true;
        e
    }

    pub fn enemy(id: EntityId, pos: Vec2) -> Self {
        Entity {
            id,
            kind: EntityKind::Enemy,
            pos,
            vel: Vec2::ZERO,
            facing: std::f32::consts::PI, // start pointing down (toward play area)
            turret_facing: 0.0,
            bbox: crate::enemy::ENEMY_BBOX,
            alive: true,
            ttl: None,
            shot_cooldown: 0.0,
            hp: crate::enemy::ENEMY_HP,
            max_hp: crate::enemy::ENEMY_HP,
            damage_timer: 0.0,
            contact_damage_accum: 0.0,
            thrusting: false,
            accel: Vec2::ZERO,
            source: None,
            detonates_on_terrain: false,
            gravity_armed: true,
        }
    }

    /// A ground tank. Spawned with the chassis facing right (`+X`); the
    /// world tick snaps `pos.y` to the terrain surface every step so we
    /// don't need to pin it here.
    pub fn tank(id: EntityId, pos: Vec2) -> Self {
        Entity {
            id,
            kind: EntityKind::Tank,
            pos,
            vel: Vec2::ZERO,
            facing: std::f32::consts::FRAC_PI_2,
            turret_facing: 0.0,
            bbox: crate::tank::TANK_BBOX,
            alive: true,
            ttl: None,
            shot_cooldown: 0.0,
            hp: crate::tank::TANK_HP,
            max_hp: crate::tank::TANK_HP,
            damage_timer: 0.0,
            contact_damage_accum: 0.0,
            thrusting: false,
            accel: Vec2::ZERO,
            source: None,
            detonates_on_terrain: false,
            gravity_armed: true,
        }
    }

    pub fn player_id(&self) -> Option<PlayerId> {
        match self.kind {
            EntityKind::Player { player_id } => Some(player_id),
            _ => None,
        }
    }
}
