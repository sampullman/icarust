use std::collections::{BTreeMap, BTreeSet};

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::entity::{Entity, EntityId, EntityKind, PlayerId, ShotOwner, Tick};
use crate::enemy::{self, ENEMY_SHOT_SPEED, ENEMY_SHOT_TIME};
use crate::event::{DeathCause, GameEvent};
use crate::input::PlayerInputs;
use crate::physics;
use crate::player::{
    self, PLAYER_REGEN_DELAY, PLAYER_REGEN_INTERVAL, PLAYER_SHOT_TIME, RAM_DAMAGE_PER_SECOND,
    SHOT_SPEED,
};
use crate::tank::{
    self, TANK_GROUND_OFFSET, TANK_SHELL_LIFE, TANK_SHOT_BBOX, TANK_SHOT_GRAVITY,
    TANK_SHOT_SPEED, TANK_SHOT_TIME,
};
use crate::terrain::{self, TerrainBand};
use crate::util::{self, Vec2};
use crate::wave::{AliveCounts, SpawnRequest, WaveDirector, INITIAL_ENEMY_COUNT};

/// World is wider than the visible viewport so the camera can scroll instead
/// of wrapping at the screen edge. The X axis is toroidal (see
/// `util::wrap_coord`); the client camera follows the local player and draws
/// each entity at up to three positions (`x`, `x ± WORLD_WIDTH`) so the wrap
/// is invisible from the player's perspective.
pub const WORLD_WIDTH: f32 = 3200.0;
/// Visible viewport height in world units. Fixed regardless of display
/// resolution — the screen letterboxes / stretches to this aspect ratio.
pub const VIEW_HEIGHT: f32 = 540.0;
/// Total play-area height in world units (Y-up). `0` is the floor (ground
/// surface lives in `[GROUND_MIN_HEIGHT, GROUND_MAX_HEIGHT]`) and
/// `WORLD_HEIGHT` is the ceiling. We extend the world one viewport above
/// the base play area so pilots have headroom to climb into.
pub const WORLD_HEIGHT: f32 = VIEW_HEIGHT * 2.0;

/// Default spawn altitude. Sits well above the visible viewport floor so a
/// fresh pilot has plenty of headroom before gravity (or a tank shell)
/// becomes a problem. Paired with `Entity::gravity_armed`, which keeps the
/// ship motionless until the player thrusts for the first time — so the
/// spawn position is also where the pilot waits.
pub const SPAWN_Y: f32 = VIEW_HEIGHT * 1.2;

pub const SHOT_BBOX: f32 = 6.0;
pub const SHOT_LIFE: f32 = 2.0;

/// Hostiles refuse to spawn within this radius of any live player so the
/// pilot never has to deal with one materialising in their lap.
pub const ENEMY_SAFE_SPAWN_RADIUS: f32 = 380.0;

/// Hostiles within this radius of a (re)spawning player are nudged out so
/// the player isn't killed on the same tick they appear.
pub const SAFE_SPAWN_RADIUS: f32 = 80.0;

#[derive(Debug, Clone, Copy)]
pub struct WorldConfig {
    pub seed: u64,
    pub world_size: Vec2,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 0x1CA_2057,
            world_size: Vec2::new(WORLD_WIDTH, WORLD_HEIGHT),
        }
    }
}

pub struct World {
    config: WorldConfig,
    rng: ChaCha8Rng,
    tick: Tick,
    next_entity_id: u64,
    entities: BTreeMap<EntityId, Entity>,
    players: BTreeMap<PlayerId, EntityId>,
    score_by_player: BTreeMap<PlayerId, i32>,
    level: i32,
    terrain: Vec<TerrainBand>,
    director: WaveDirector,
}

impl World {
    pub fn new(config: WorldConfig) -> Self {
        let rng = ChaCha8Rng::seed_from_u64(config.seed);
        let terrain = terrain::default_terrain(config.world_size.x, config.seed);
        let mut world = World {
            config,
            rng,
            tick: Tick(0),
            next_entity_id: 0,
            entities: BTreeMap::new(),
            players: BTreeMap::new(),
            score_by_player: BTreeMap::new(),
            level: 1,
            terrain,
            director: WaveDirector::new(),
        };
        world.spawn_initial_wave();
        world
    }

    pub fn terrain(&self) -> &[TerrainBand] {
        &self.terrain
    }

    pub fn config(&self) -> WorldConfig {
        self.config
    }

    pub fn world_size(&self) -> Vec2 {
        self.config.world_size
    }

    pub fn tick_index(&self) -> Tick {
        self.tick
    }

    pub fn level(&self) -> i32 {
        self.level
    }

    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }

    pub fn entities_map(&self) -> &BTreeMap<EntityId, Entity> {
        &self.entities
    }

    pub fn score(&self, player_id: PlayerId) -> i32 {
        self.score_by_player.get(&player_id).copied().unwrap_or(0)
    }

    pub fn scores(&self) -> &BTreeMap<PlayerId, i32> {
        &self.score_by_player
    }

    pub fn player_entity(&self, player_id: PlayerId) -> Option<&Entity> {
        self.players.get(&player_id).and_then(|id| self.entities.get(id))
    }

    pub fn has_player(&self, player_id: PlayerId) -> bool {
        self.players.contains_key(&player_id)
    }

    /// Spawn a player horizontally centered at `SPAWN_Y`. No-op if already
    /// present. Pushes any hostiles inside `SAFE_SPAWN_RADIUS` out of the way
    /// so the new ship isn't killed on the same tick it appears.
    pub fn add_player(&mut self, player_id: PlayerId) -> Option<EntityId> {
        if self.players.contains_key(&player_id) {
            return None;
        }
        let id = self.alloc_id();
        let spawn = Vec2::new(self.config.world_size.x * 0.5, SPAWN_Y);
        self.clear_safe_zone(spawn, SAFE_SPAWN_RADIUS);
        let entity = Entity::player(id, player_id, spawn);
        self.entities.insert(id, entity);
        self.players.insert(player_id, id);
        self.score_by_player.entry(player_id).or_insert(0);
        Some(id)
    }

    /// Put a dead player back in the world and reset the hostile state so they
    /// aren't dropped into mid-battle. Wipes existing enemies, tanks, and
    /// shots, drops the level back to 1, resets the spawn director, and spawns
    /// a fresh starting wave at safe distance. Other live players stay put —
    /// their entities and scores are preserved.
    pub fn respawn_player(&mut self, player_id: PlayerId) -> Option<EntityId> {
        if self.players.contains_key(&player_id) {
            return None;
        }
        self.entities
            .retain(|_, e| matches!(e.kind, EntityKind::Player { .. }));
        self.level = 1;
        self.director.reset();
        let id = self.add_player(player_id);
        self.spawn_initial_wave();
        id
    }

    /// Push hostiles out of a disc so a freshly-spawned player isn't standing
    /// on top of one. They're moved to the disc edge along the radial
    /// direction; velocity is preserved so the world keeps moving.
    fn clear_safe_zone(&mut self, center: Vec2, radius: f32) {
        for entity in self.entities.values_mut() {
            let hostile = matches!(entity.kind, EntityKind::Enemy | EntityKind::Tank);
            if !hostile || !entity.alive {
                continue;
            }
            let offset = entity.pos - center;
            let dist = offset.length();
            if dist >= radius {
                continue;
            }
            // Outward direction; if the hostile is exactly at the center, kick
            // it along +X so the math is well-defined.
            let dir = if dist > 1e-3 {
                offset / dist
            } else {
                Vec2::new(1.0, 0.0)
            };
            entity.pos = center + dir * radius;
        }
    }

    pub fn remove_player(&mut self, player_id: PlayerId) {
        if let Some(eid) = self.players.remove(&player_id) {
            self.entities.remove(&eid);
        }
        self.score_by_player.remove(&player_id);
    }

    /// Advance one fixed step.
    pub fn tick(&mut self, inputs: &PlayerInputs, dt: f32) -> Vec<GameEvent> {
        let mut events = Vec::new();

        // 1. Apply input + fire shots.
        let mut new_shots: Vec<Entity> = Vec::new();
        let player_eids: Vec<EntityId> = self.players.values().copied().collect();
        for eid in player_eids {
            let Some(entity) = self.entities.get_mut(&eid) else {
                continue;
            };
            if !entity.alive {
                continue;
            }
            let player_id = match entity.kind {
                EntityKind::Player { player_id } => player_id,
                _ => continue,
            };
            let input = inputs.get(&player_id).copied().unwrap_or_default();

            let (vel, facing) = player::apply_input(entity.vel, entity.facing, &input, dt);
            entity.vel = vel;
            entity.facing = facing;
            entity.thrusting = input.yaxis > 0.0;
            // First thrust input arms gravity for the rest of this life. Stays
            // on through respawn; `Entity::player` resets it to false.
            if input.yaxis > 0.0 {
                entity.gravity_armed = true;
            }

            entity.shot_cooldown -= dt;

            if input.fire && entity.shot_cooldown <= 0.0 {
                entity.shot_cooldown = PLAYER_SHOT_TIME;
                let pos = entity.pos;
                new_shots.push(fire_bullet(
                    &mut self.next_entity_id,
                    pos,
                    facing,
                    ShotOwner::Player(player_id),
                    player::PLAYER_BBOX,
                    SHOT_SPEED,
                    &mut events,
                ));
            }
        }
        for shot in new_shots {
            self.entities.insert(shot.id, shot);
        }

        // 1b. Enemy AI. Each enemy targets the nearest live player, steers
        // toward it, and may fire. Snapshotting target positions before
        // mutating means the order of enemies in the BTreeMap doesn't
        // affect the AI decisions on this tick.
        let player_targets = self.live_player_positions();
        let world_width = self.config.world_size.x;
        let enemy_eids = self.live_ids_matching(|e| matches!(e.kind, EntityKind::Enemy));
        let mut enemy_shots: Vec<Entity> = Vec::new();
        for eid in enemy_eids {
            let Some(entity) = self.entities.get_mut(&eid) else {
                continue;
            };
            let target = nearest_target(entity.pos, &player_targets, world_width);
            let step = enemy::step(entity.pos, entity.vel, entity.facing, target, world_width, dt);
            entity.vel = step.vel;
            entity.facing = step.facing;
            entity.shot_cooldown -= dt;

            if step.fire && entity.shot_cooldown <= 0.0 {
                entity.shot_cooldown = ENEMY_SHOT_TIME;
                let pos = entity.pos;
                let facing = entity.facing;
                enemy_shots.push(fire_bullet(
                    &mut self.next_entity_id,
                    pos,
                    facing,
                    ShotOwner::Enemy,
                    enemy::ENEMY_BBOX,
                    ENEMY_SHOT_SPEED,
                    &mut events,
                ));
            }
        }
        for shot in enemy_shots {
            self.entities.insert(shot.id, shot);
        }

        // 1c. Tank AI. Each tank rolls toward the nearest player and
        // tracks them with its turret. Shells fire less often than ship
        // bullets but carry gravity, so the AI aims with a parabolic
        // lead built into `tank::step`. Identical pattern to enemy AI
        // so a future player-controlled tank only has to replace the
        // `target` selection.
        let tank_eids = self.live_ids_matching(|e| matches!(e.kind, EntityKind::Tank));
        let mut tank_shots: Vec<Entity> = Vec::new();
        // Current sim time used to drive each tank's dodge oscillator. The
        // per-entity offset (id * 0.83) keeps neighbouring tanks out of
        // phase so they don't sway in lockstep.
        let now = self.tick.0 as f32 * dt;
        for eid in tank_eids {
            let Some(entity) = self.entities.get_mut(&eid) else {
                continue;
            };
            let target = nearest_target(entity.pos, &player_targets, world_width);
            let dodge_phase = now + eid.0 as f32 * 0.83;
            let step = tank::step(
                entity.pos,
                entity.vel,
                entity.facing,
                entity.turret_facing,
                target,
                world_width,
                dodge_phase,
                dt,
            );
            entity.vel = step.vel;
            entity.facing = step.body_facing;
            entity.turret_facing = step.turret_facing;
            entity.shot_cooldown -= dt;

            if step.fire && entity.shot_cooldown <= 0.0 {
                entity.shot_cooldown = TANK_SHOT_TIME;
                // Spawn the shell at the end of the barrel so the tank
                // doesn't immediately collide with its own shot.
                let direction = util::vec_from_angle(entity.turret_facing);
                let spawn_pos = entity.pos + direction * (entity.bbox + 6.0);
                let turret = entity.turret_facing;
                let shot_id = EntityId(self.next_entity_id);
                self.next_entity_id += 1;
                let owner = ShotOwner::Tank;
                tank_shots.push(Entity::artillery_shot(
                    shot_id,
                    owner,
                    spawn_pos,
                    direction * TANK_SHOT_SPEED,
                    turret,
                    Vec2::new(0.0, -TANK_SHOT_GRAVITY),
                    TANK_SHOT_BBOX,
                    TANK_SHELL_LIFE,
                    Some(eid),
                ));
                events.push(GameEvent::ShotFired { owner, pos: spawn_pos });
            }
        }
        for shot in tank_shots {
            self.entities.insert(shot.id, shot);
        }

        // 2. Move + wrap + per-kind extras. X is toroidal (fly off the right
        // edge and come back on the left); Y is a hard wall — players clamp,
        // enemies and shots bounce. Bouncers use the local terrain surface
        // (per-x height) so ricochets follow the hills instead of tracking the
        // tallest peak. Shots flagged `detonates_on_terrain` (artillery)
        // detonate instead of bouncing; their impact positions are emitted as
        // `ShellExploded` events after the loop so the borrow stays simple
        // inside.
        let world_size = self.config.world_size;
        let mut detonations: Vec<Vec2> = Vec::new();
        for entity in self.entities.values_mut() {
            if !entity.alive {
                continue;
            }
            entity.pos += entity.vel * dt;
            entity.pos.x = util::wrap_coord(entity.pos.x, world_size.x);

            match entity.kind {
                EntityKind::Player { .. } => {
                    util::clamp_y(&mut entity.pos, &mut entity.vel, world_size.y);
                    entity.vel = player::apply_forces(entity.vel, dt, entity.gravity_armed);
                }
                EntityKind::Shot { owner } => {
                    let surface = terrain::surface_y_at(entity.pos.x, &self.terrain);
                    let floor = surface + entity.bbox;
                    if entity.detonates_on_terrain && entity.pos.y <= floor {
                        // Pin the boom to the impact point on the surface
                        // (entity.pos.x, ground top) so the client renders
                        // the explosion sitting on the hill rather than
                        // half-buried, and kill the shell. The collision
                        // pass after the loop won't try to also damage
                        // anything because `alive = false`.
                        let impact = Vec2::new(entity.pos.x, surface);
                        entity.alive = false;
                        detonations.push(impact);
                        continue;
                    }
                    // Player bullets are absorbed by terrain instead of
                    // ricocheting; otherwise downward shots arc back at the
                    // pilot. Enemy bullets keep bouncing so they still skim
                    // along the hills.
                    if matches!(owner, ShotOwner::Player(_)) && entity.pos.y <= floor {
                        entity.alive = false;
                        continue;
                    }
                    // Standard bullet: bounce off the local top-of-terrain
                    // (plus the shot's own radius) so it doesn't sink in
                    // before reflecting.
                    util::bounce_y(&mut entity.pos, &mut entity.vel, floor, world_size.y);
                    // Per-shot acceleration (tank shells use this for
                    // gravity). Applied after the bounce so the bounce
                    // reverses pre-gravity velocity.
                    if entity.accel != Vec2::ZERO {
                        entity.vel += entity.accel * dt;
                    }
                    if let Some(ttl) = entity.ttl.as_mut() {
                        *ttl -= dt;
                        if *ttl <= 0.0 {
                            entity.alive = false;
                        }
                    }
                }
                EntityKind::Enemy => {
                    // Enemies bounce off the same local surface as shots so
                    // they skim along hills instead of tracking the tallest
                    // peak in the world.
                    let floor = terrain::surface_y_at(entity.pos.x, &self.terrain) + entity.bbox;
                    util::bounce_y(&mut entity.pos, &mut entity.vel, floor, world_size.y);
                }
                EntityKind::Tank => {
                    // Tank chassis is locked to the terrain surface. Vertical
                    // velocity is zeroed in `tank::step`; we still pin Y here
                    // in case anything else perturbs it (collisions, external
                    // nudges). `pos.y` is raised by `TANK_GROUND_OFFSET`
                    // (decoupled from the collision radius).
                    let ground = terrain::ground_surface_at(entity.pos.x, &self.terrain);
                    entity.pos.y = ground + TANK_GROUND_OFFSET;
                    entity.vel.y = 0.0;
                }
            }
        }
        for pos in detonations.drain(..) {
            events.push(GameEvent::ShellExploded { pos });
        }

        // 2b. Terrain. A player whose hitbox dips into a terrain band crashes
        // there. Done before entity-vs-entity collision so a player who rams
        // both an enemy and the ground in the same tick is recorded as a
        // terrain crash (deterministic tiebreak).
        self.handle_terrain(&mut events);

        // 3. Collisions. BTreeMap iteration is deterministic.
        self.handle_collisions(&mut events, dt);

        // 3b. HP regen for live players.
        self.handle_regen(dt);

        // 4. Clear dead. Track player removals for housekeeping.
        let mut removed_players: Vec<PlayerId> = Vec::new();
        self.entities.retain(|_, e| {
            if !e.alive {
                if let EntityKind::Player { player_id } = e.kind {
                    removed_players.push(player_id);
                }
                false
            } else {
                true
            }
        });
        for pid in removed_players {
            self.players.remove(&pid);
        }

        // 5. Wave director. Levels run on a wall-clock timer (see
        // `wave::level_duration`) and the director also decides when to push
        // fresh hostiles into the world. Only ticked while someone is alive to
        // fight — an empty world freezes the level counter and the spawn
        // timers so nothing is wasted.
        if !self.players.is_empty() {
            let alive = self.alive_hostile_counts();
            let step = self.director.step(self.level, dt, alive);
            if let Some(new_level) = step.level_up {
                self.level = new_level;
                events.push(GameEvent::LevelUp(self.level));
            }
            for req in step.spawns {
                match req {
                    SpawnRequest::Enemy => self.spawn_enemy(),
                    SpawnRequest::Tank => self.spawn_tank(),
                }
            }
        }

        self.tick = self.tick.next();
        events
    }

    fn handle_terrain(&mut self, events: &mut Vec<GameEvent>) {
        let player_eids =
            self.live_ids_matching(|e| matches!(e.kind, EntityKind::Player { .. }));
        for eid in player_eids {
            let Some(entity) = self.entities.get_mut(&eid) else {
                continue;
            };
            if !entity.alive {
                continue;
            }
            let Some(kind) = terrain::terrain_hit(entity.pos, entity.bbox, &self.terrain) else {
                continue;
            };
            let player_id = match entity.kind {
                EntityKind::Player { player_id } => player_id,
                _ => continue,
            };
            // Pin the impact to the player's X but the band's local surface Y
            // so the client draws the boom sitting on the hillside rather than
            // half-buried at the world's tallest peak.
            let surface_y = self
                .terrain
                .iter()
                .filter(|b| b.kind == kind)
                .map(|b| b.profile.height_at(entity.pos.x))
                .fold(0.0_f32, f32::max);
            let pos = Vec2::new(entity.pos.x, surface_y);
            entity.alive = false;
            events.push(GameEvent::PlayerKilled {
                player_id,
                pos,
                cause: DeathCause::Terrain(kind),
            });
        }
    }

    fn handle_collisions(&mut self, events: &mut Vec<GameEvent>, dt: f32) {
        // Treat ships and tanks as one bucket: both lose HP from player shots
        // and both deal contact damage. New hostile kinds plug into the same
        // logic by joining the `hostile_ids` filter below.
        let hostile_ids =
            self.live_ids_matching(|e| matches!(e.kind, EntityKind::Enemy | EntityKind::Tank));
        let player_ids =
            self.live_ids_matching(|e| matches!(e.kind, EntityKind::Player { .. }));
        let player_shot_ids = self.live_ids_matching(
            |e| matches!(e.kind, EntityKind::Shot { owner: ShotOwner::Player(_) }),
        );
        let enemy_shot_ids = self.live_ids_matching(|e| match e.kind {
            EntityKind::Shot { owner } => owner.is_hostile(),
            _ => false,
        });

        // Player ↔ hostile contact: continuous damage instead of an instant
        // kill. Each overlapping tick drains `RAM_DAMAGE_PER_SECOND * dt` off
        // both sides, so a full-HP pilot survives `RAM_DEATH_SECONDS` of
        // constant contact before exploding. The same rate flows back into the
        // hostile — a brief brush wipes weak ships (1 HP) almost instantly,
        // heavier chassis take proportionally longer.
        let mut contacted_players: BTreeSet<EntityId> = BTreeSet::new();
        let mut contacted_hostiles: BTreeSet<EntityId> = BTreeSet::new();
        let dose = RAM_DAMAGE_PER_SECOND * dt;
        for hostile_id in &hostile_ids {
            for player_id in &player_ids {
                let hit = match (self.entities.get(player_id), self.entities.get(hostile_id)) {
                    (Some(p), Some(e)) if p.alive && e.alive => {
                        physics::circles_overlap(p.pos, p.bbox, e.pos, e.bbox)
                    }
                    _ => false,
                };
                if !hit {
                    continue;
                }
                let pid = match self.entities.get(player_id).map(|p| p.kind) {
                    Some(EntityKind::Player { player_id }) => player_id,
                    _ => continue,
                };
                contacted_players.insert(*player_id);
                contacted_hostiles.insert(*hostile_id);

                // Damage the hostile first so a frame that kills both still
                // credits the player.
                if let Some(h) = self.entities.get_mut(hostile_id) {
                    if h.alive {
                        h.contact_damage_accum += dose;
                        let drop = h.contact_damage_accum.floor() as i16;
                        if drop > 0 {
                            h.contact_damage_accum -= drop as f32;
                            h.hp = h.hp.saturating_sub(drop);
                            let pos = h.pos;
                            if h.hp <= 0 {
                                h.alive = false;
                                *self.score_by_player.entry(pid).or_insert(0) += 1;
                                events.push(GameEvent::EnemyKilled {
                                    pos,
                                    killer: Some(pid),
                                });
                            } else {
                                let hp_remaining = h.hp;
                                events.push(GameEvent::EnemyDamaged {
                                    pos,
                                    hp: hp_remaining,
                                });
                            }
                        }
                    }
                }

                if let Some(p) = self.entities.get_mut(player_id) {
                    if !p.alive {
                        continue;
                    }
                    // Suspend regen while in contact, even on ticks that
                    // don't yet pop a whole HP.
                    p.damage_timer = 0.0;
                    p.contact_damage_accum += dose;
                    let drop = p.contact_damage_accum.floor() as i16;
                    if drop > 0 {
                        p.contact_damage_accum -= drop as f32;
                        p.hp = p.hp.saturating_sub(drop);
                        let pos = p.pos;
                        if p.hp <= 0 {
                            p.alive = false;
                            events.push(GameEvent::PlayerKilled {
                                player_id: pid,
                                pos,
                                cause: DeathCause::Enemy,
                            });
                        } else {
                            let hp_remaining = p.hp;
                            events.push(GameEvent::PlayerDamaged {
                                player_id: pid,
                                pos,
                                hp: hp_remaining,
                            });
                        }
                    }
                }
            }
        }

        // Anyone who ended the tick without an overlapping partner resets
        // their accumulator so a sequence of brief touches doesn't silently
        // compound into a kill.
        for id in &player_ids {
            if !contacted_players.contains(id) {
                if let Some(e) = self.entities.get_mut(id) {
                    e.contact_damage_accum = 0.0;
                }
            }
        }
        for id in &hostile_ids {
            if !contacted_hostiles.contains(id) {
                if let Some(e) = self.entities.get_mut(id) {
                    e.contact_damage_accum = 0.0;
                }
            }
        }

        // Player shot ↔ hostile: deduct 1 HP. If HP falls to zero the
        // hostile dies and the owner scores; otherwise we emit a damage
        // event so the client can play a hit spark. The shot is consumed
        // either way (no shoot-through).
        for hostile_id in &hostile_ids {
            for shot_id in &player_shot_ids {
                let hit = match (self.entities.get(shot_id), self.entities.get(hostile_id)) {
                    (Some(s), Some(e)) if s.alive && e.alive => {
                        physics::circles_overlap(s.pos, s.bbox, e.pos, e.bbox)
                    }
                    _ => false,
                };
                if !hit {
                    continue;
                }
                let owner_pid = match self.entities.get(shot_id).map(|s| s.kind) {
                    Some(EntityKind::Shot { owner: ShotOwner::Player(pid) }) => pid,
                    _ => continue,
                };
                if let Some(s) = self.entities.get_mut(shot_id) {
                    s.alive = false;
                }
                let Some(h) = self.entities.get_mut(hostile_id) else {
                    continue;
                };
                h.hp -= 1;
                let pos = h.pos;
                if h.hp <= 0 {
                    h.alive = false;
                    *self.score_by_player.entry(owner_pid).or_insert(0) += 1;
                    events.push(GameEvent::EnemyKilled { pos, killer: Some(owner_pid) });
                } else {
                    let hp_remaining = h.hp;
                    events.push(GameEvent::EnemyDamaged { pos, hp: hp_remaining });
                }
                break;
            }
        }

        // Hostile shot ↔ player: shot dies; player loses HP based on the
        // shot's owner (`ShotOwner::damage`). Hits within `PLAYER_REGEN_DELAY`
        // stack so a focused volley still drops the pilot. Tank shells chip
        // more HP per hit and also emit `ShellExploded` so the client can
        // render the boom alongside the damage feedback.
        for shot_id in &enemy_shot_ids {
            for player_id in &player_ids {
                let hit = match (self.entities.get(shot_id), self.entities.get(player_id)) {
                    (Some(s), Some(p)) if s.alive && p.alive => {
                        physics::circles_overlap(s.pos, s.bbox, p.pos, p.bbox)
                    }
                    _ => false,
                };
                if !hit {
                    continue;
                }
                let pid = match self.entities.get(player_id).map(|p| p.kind) {
                    Some(EntityKind::Player { player_id }) => player_id,
                    _ => continue,
                };
                let (damage, is_shell, shot_pos) = match self.entities.get(shot_id) {
                    Some(s) => match s.kind {
                        EntityKind::Shot { owner } => {
                            (owner.damage(), matches!(owner, ShotOwner::Tank), s.pos)
                        }
                        _ => (1, false, s.pos),
                    },
                    _ => continue,
                };
                if let Some(s) = self.entities.get_mut(shot_id) {
                    s.alive = false;
                }
                let Some(p) = self.entities.get_mut(player_id) else {
                    continue;
                };
                p.hp = p.hp.saturating_sub(damage);
                p.damage_timer = 0.0;
                let pos = p.pos;
                if p.hp <= 0 {
                    p.alive = false;
                    events.push(GameEvent::PlayerKilled {
                        player_id: pid,
                        pos,
                        cause: DeathCause::EnemyShot,
                    });
                } else {
                    events.push(GameEvent::PlayerDamaged {
                        player_id: pid,
                        pos,
                        hp: p.hp,
                    });
                }
                if is_shell {
                    events.push(GameEvent::ShellExploded { pos: shot_pos });
                }
                break;
            }
        }

        // Tank shell ↔ other hostile: friendly fire. A shell that lands on
        // another tank or ship enemy detonates and deducts HP using the same
        // damage curve as a hit on the player. The shell skips the entity that
        // fired it (via `source`) so a freshly-spawned shell can't detonate on
        // its own chassis. Friendly-fire kills emit `EnemyKilled` with
        // `killer: None` so the score logic ignores them — only player shots
        // and rams credit a score.
        let shell_ids = self.live_ids_matching(
            |e| matches!(e.kind, EntityKind::Shot { owner: ShotOwner::Tank }),
        );
        for shell_id in &shell_ids {
            let (shell_source, shell_pos, shell_bbox) = match self.entities.get(shell_id) {
                Some(s) if s.alive => (s.source, s.pos, s.bbox),
                _ => continue,
            };
            for hostile_id in &hostile_ids {
                if Some(*hostile_id) == shell_source {
                    continue;
                }
                let hit = match self.entities.get(hostile_id) {
                    Some(h) if h.alive => {
                        physics::circles_overlap(shell_pos, shell_bbox, h.pos, h.bbox)
                    }
                    _ => false,
                };
                if !hit {
                    continue;
                }
                if let Some(s) = self.entities.get_mut(shell_id) {
                    s.alive = false;
                }
                let Some(h) = self.entities.get_mut(hostile_id) else {
                    continue;
                };
                h.hp = h.hp.saturating_sub(ShotOwner::Tank.damage());
                let pos = h.pos;
                if h.hp <= 0 {
                    h.alive = false;
                    events.push(GameEvent::EnemyKilled { pos, killer: None });
                } else {
                    let hp_remaining = h.hp;
                    events.push(GameEvent::EnemyDamaged { pos, hp: hp_remaining });
                }
                events.push(GameEvent::ShellExploded { pos: shell_pos });
                break;
            }
        }
    }

    /// Tick the regen clock on every live player. After `PLAYER_REGEN_DELAY`
    /// of damage-free flight, HP climbs back one tick every
    /// `PLAYER_REGEN_INTERVAL` seconds. Called once per world tick.
    fn handle_regen(&mut self, dt: f32) {
        for entity in self.entities.values_mut() {
            if !entity.alive {
                continue;
            }
            if !matches!(entity.kind, EntityKind::Player { .. }) {
                continue;
            }
            entity.damage_timer += dt;
            if entity.hp >= entity.max_hp {
                continue;
            }
            // Pay out one HP every REGEN_INTERVAL seconds past the delay.
            if entity.damage_timer >= PLAYER_REGEN_DELAY + PLAYER_REGEN_INTERVAL {
                entity.hp = (entity.hp + 1).min(entity.max_hp);
                entity.damage_timer -= PLAYER_REGEN_INTERVAL;
            }
        }
    }

    /// Tally alive ship/tank counts in one pass. Allocation-free so it's
    /// cheap to call every tick.
    fn alive_hostile_counts(&self) -> AliveCounts {
        let (enemies, tanks) = self.entities.values().fold((0, 0), |(e, t), entity| {
            if !entity.alive {
                return (e, t);
            }
            match entity.kind {
                EntityKind::Enemy => (e + 1, t),
                EntityKind::Tank => (e, t + 1),
                _ => (e, t),
            }
        });
        AliveCounts { enemies, tanks }
    }

    /// Seed the world with the starting wave. Ships only — tanks unlock
    /// later through the wave director. Called from `new` and again from
    /// `respawn_player` after the world is wiped clean.
    fn spawn_initial_wave(&mut self) {
        for _ in 0..INITIAL_ENEMY_COUNT {
            self.spawn_enemy();
        }
    }

    /// Spawn one enemy well outside any live player's view so it flies in
    /// from off-screen rather than appearing on top of them. Retries up to
    /// `MAX_ATTEMPTS` if the rolled position falls inside the safe radius,
    /// then accepts the last attempt. RNG draws per call are bounded so
    /// determinism is preserved.
    fn spawn_enemy(&mut self) {
        let world = self.config.world_size;
        let player_positions = self.live_player_positions();

        // Each attempt consumes the same number of RNG draws so the
        // determinism contract holds — same world state in, same attempts out.
        const MAX_ATTEMPTS: usize = 8;
        let mut chosen = Vec2::ZERO;
        for attempt in 0..MAX_ATTEMPTS {
            let x = util::rand_unit(&mut self.rng) * world.x;
            // Spread across most of the playable altitude band so the
            // camera's vertical headroom actually carries enemies, not just
            // empty sky. Keep clear of the top/bottom margins so they're not
            // pinned against a wall.
            let y = 60.0 + util::rand_unit(&mut self.rng) * (world.y - 120.0);
            let pos = Vec2::new(x, y);
            let safe = player_positions
                .iter()
                .all(|p| util::toroidal_distance(pos, *p, world.x) >= ENEMY_SAFE_SPAWN_RADIUS);
            if safe || attempt == MAX_ATTEMPTS - 1 {
                chosen = pos;
                if safe {
                    break;
                }
            }
        }
        // Final clamp so we never spawn at the very edge of the play area.
        chosen.y = chosen.y.clamp(40.0, world.y - 40.0);
        let id = self.alloc_id();
        let enemy = Entity::enemy(id, chosen);
        self.entities.insert(id, enemy);
    }

    /// Spawn one tank rolling on the ground at a player-safe X. Same retry
    /// shape as `spawn_enemy` so determinism is preserved; we just roll an X
    /// and pin the Y to the terrain surface. Skips X-ranges that aren't
    /// passable for ground vehicles (no-op today; future water bands will
    /// start filtering here).
    fn spawn_tank(&mut self) {
        let world = self.config.world_size;
        let player_positions = self.live_player_positions();

        const MAX_ATTEMPTS: usize = 8;
        let mut chosen_x = 0.0_f32;
        for attempt in 0..MAX_ATTEMPTS {
            let x = util::rand_unit(&mut self.rng) * world.x;
            let ground = terrain::ground_surface_at(x, &self.terrain);
            let probe = Vec2::new(x, ground + TANK_GROUND_OFFSET);
            let passable = terrain::passable_for_ground_vehicle(x, &self.terrain);
            let safe = player_positions
                .iter()
                .all(|p| util::toroidal_distance(probe, *p, world.x) >= ENEMY_SAFE_SPAWN_RADIUS);
            if passable && (safe || attempt == MAX_ATTEMPTS - 1) {
                chosen_x = x;
                if safe {
                    break;
                }
            }
        }
        let ground = terrain::ground_surface_at(chosen_x, &self.terrain);
        let pos = Vec2::new(chosen_x, ground + TANK_GROUND_OFFSET);
        let id = self.alloc_id();
        let tank = Entity::tank(id, pos);
        self.entities.insert(id, tank);
    }

    fn alloc_id(&mut self) -> EntityId {
        let id = EntityId(self.next_entity_id);
        self.next_entity_id += 1;
        id
    }

    /// World positions of every live player. Used by AI and by spawn logic
    /// that wants to avoid materialising on top of someone.
    fn live_player_positions(&self) -> Vec<Vec2> {
        self.entities
            .values()
            .filter(|e| matches!(e.kind, EntityKind::Player { .. }) && e.alive)
            .map(|e| e.pos)
            .collect()
    }

    /// Collect the IDs of every alive entity matching `pred`. Used by the
    /// collision and AI passes to grab a snapshot of which IDs to walk
    /// before they start mutating the entity table.
    fn live_ids_matching<F: Fn(&Entity) -> bool>(&self, pred: F) -> Vec<EntityId> {
        self.entities
            .iter()
            .filter(|(_, e)| e.alive && pred(e))
            .map(|(id, _)| *id)
            .collect()
    }
}

/// Build a regular (non-artillery) bullet and queue the matching
/// `ShotFired` event. Takes a `&mut u64` for the id counter rather than
/// `&mut World` so it can be called inside a loop that already borrows
/// `World::entities`.
fn fire_bullet(
    next_entity_id: &mut u64,
    pos: Vec2,
    facing: f32,
    owner: ShotOwner,
    offset: f32,
    speed: f32,
    events: &mut Vec<GameEvent>,
) -> Entity {
    let direction = util::vec_from_angle(facing);
    let spawn_pos = pos + direction * offset;
    let id = EntityId(*next_entity_id);
    *next_entity_id += 1;
    events.push(GameEvent::ShotFired { owner, pos: spawn_pos });
    Entity::shot(id, owner, spawn_pos, direction * speed, facing)
}

/// Pick the closest target position from `candidates`, accounting for
/// X-wrap. Returns `None` when the slice is empty.
fn nearest_target(from: Vec2, candidates: &[Vec2], world_width: f32) -> Option<Vec2> {
    candidates
        .iter()
        .map(|&c| (util::toroidal_offset(from, c, world_width).length_squared(), c))
        .min_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PlayerInput;

    #[test]
    fn world_spawns_initial_enemy_wave() {
        let world = World::new(WorldConfig::default());
        let enemies: Vec<_> = world
            .entities()
            .filter(|e| matches!(e.kind, EntityKind::Enemy))
            .collect();
        assert_eq!(enemies.len(), crate::wave::INITIAL_ENEMY_COUNT as usize);
    }

    #[test]
    fn world_has_no_tanks_at_level_one_start() {
        // Tanks unlock at `TANK_START_LEVEL`; a fresh world is at level 1
        // so the initial wave must contain ship enemies only.
        let world = World::new(WorldConfig::default());
        let tanks: Vec<_> = world
            .entities()
            .filter(|e| matches!(e.kind, EntityKind::Tank))
            .collect();
        assert!(tanks.is_empty(), "no tanks should be present at level 1 start");
    }

    /// Pin the player at high altitude with zero velocity, and clear any
    /// hostiles so they can't intervene. Used by long-running tests below
    /// that need the world to stay quiet while the wave director runs.
    fn pin_player_aloft(world: &mut World, pid: PlayerId) {
        let eid = *world.players.get(&pid).unwrap();
        if let Some(p) = world.entities.get_mut(&eid) {
            p.pos = Vec2::new(WORLD_WIDTH * 0.5, WORLD_HEIGHT - 80.0);
            p.vel = Vec2::ZERO;
        }
    }

    #[test]
    fn director_spawns_tanks_once_level_reaches_unlock() {
        // Run with one player and force the level up to the tank unlock.
        // Within a generous window after the level transition we should
        // see a tank materialise.
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);
        world.level = crate::wave::TANK_START_LEVEL;
        let dt = crate::TICK_DT;
        let budget = ((crate::wave::tank_spawn_interval(crate::wave::TANK_START_LEVEL)
            + 2.0)
            / dt)
            .ceil() as i32;
        let mut saw_tank = false;
        for _ in 0..budget {
            // Re-pin the player every tick so gravity doesn't crash them
            // and end the director's tick window. Also wipe any hostiles
            // that the director itself spawns above the player so we
            // don't eat damage from them.
            pin_player_aloft(&mut world, pid);
            world.entities.retain(|_, e| {
                !matches!(e.kind, EntityKind::Enemy)
                    && !matches!(e.kind, EntityKind::Shot { .. })
            });
            world.tick(&PlayerInputs::new(), dt);
            if world
                .entities()
                .any(|e| matches!(e.kind, EntityKind::Tank) && e.alive)
            {
                saw_tank = true;
                break;
            }
        }
        assert!(
            saw_tank,
            "a tank should spawn within one tank-interval at level {}",
            crate::wave::TANK_START_LEVEL
        );
    }

    #[test]
    fn level_advances_on_timer_with_player_present() {
        // Run for one level's worth of ticks (see `wave::level_duration`)
        // with a player in the world; the level counter must advance
        // even though no wave has been "cleared".
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);
        let dt = crate::TICK_DT;
        let steps = (crate::wave::level_duration(1) / dt).ceil() as i32 + 30;
        let start_level = world.level();
        for _ in 0..steps {
            // Keep the pilot alive and in clear air; the director only
            // runs while at least one player is alive.
            if !world.has_player(pid) {
                world.respawn_player(pid);
            }
            pin_player_aloft(&mut world, pid);
            // Clear hostiles and shots so they can't damage the pinned
            // player. The director keeps trying to spawn more — that's
            // fine, we just wipe them again on the next iteration.
            world.entities.retain(|_, e| {
                matches!(e.kind, EntityKind::Player { .. })
            });
            world.tick(&PlayerInputs::new(), dt);
        }
        assert!(
            world.level() > start_level,
            "level should advance on the wave-director timer (got {} from {})",
            world.level(),
            start_level
        );
    }

    #[test]
    fn tank_shell_explodes_on_terrain() {
        // Drop a shell moving straight down with detonates_on_terrain set;
        // it should die and emit a ShellExploded event instead of
        // bouncing.
        let mut world = World::new(WorldConfig::default());
        // Strip out hostiles + players so only the injected shell is around.
        world.entities.clear();
        let shell_id = world.alloc_id();
        let shell_x = WORLD_WIDTH * 0.5;
        let ground_y = terrain::ground_surface_at(shell_x, world.terrain());
        let start = Vec2::new(shell_x, ground_y + 40.0);
        let shell = Entity::artillery_shot(
            shell_id,
            ShotOwner::Tank,
            start,
            Vec2::new(0.0, -400.0),
            std::f32::consts::PI,
            Vec2::new(0.0, -crate::tank::TANK_SHOT_GRAVITY),
            crate::tank::TANK_SHOT_BBOX,
            crate::tank::TANK_SHELL_LIFE,
            None,
        );
        world.entities.insert(shell_id, shell);
        let mut detonated = false;
        for _ in 0..30 {
            let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
            if evs.iter().any(|e| matches!(e, GameEvent::ShellExploded { .. })) {
                detonated = true;
                break;
            }
        }
        assert!(detonated, "shell should explode on terrain instead of bouncing");
        assert!(
            world.entities.get(&shell_id).is_none(),
            "shell should be cleared from the world after detonation"
        );
    }

    #[test]
    fn tank_shell_friendly_fires_other_tank() {
        // Park two tanks side by side, drop a shell on top of tank B with
        // `source = A`. The shell should damage B and not credit anyone
        // for the kill.
        let mut world = World::new(WorldConfig::default());
        world.entities.retain(|_, e| !matches!(e.kind, EntityKind::Tank | EntityKind::Enemy));
        let pid = PlayerId(0);
        world.add_player(pid);

        let a_x = WORLD_WIDTH * 0.5 - 200.0;
        let b_x = WORLD_WIDTH * 0.5 + 200.0;
        let a_pos = Vec2::new(a_x, terrain::ground_surface_at(a_x, world.terrain()) + crate::tank::TANK_GROUND_OFFSET);
        let b_pos = Vec2::new(b_x, terrain::ground_surface_at(b_x, world.terrain()) + crate::tank::TANK_GROUND_OFFSET);
        let tank_a = world.alloc_id();
        let tank_b = world.alloc_id();
        let mut a = Entity::tank(tank_a, a_pos);
        let mut b = Entity::tank(tank_b, b_pos);
        // Keep them from auto-firing during the test.
        a.shot_cooldown = 100.0;
        b.shot_cooldown = 100.0;
        world.entities.insert(tank_a, a);
        world.entities.insert(tank_b, b);

        // Inject a stationary shell overlapping tank B but well above
        // the terrain floor (TANK_SHOT_BBOX = 9), so the terrain-detonate
        // check doesn't claim it before the friendly-fire pass. The
        // shell is sourced from tank A, so it should hit B and skip A.
        let shell_id = world.alloc_id();
        let shell = Entity::artillery_shot(
            shell_id,
            ShotOwner::Tank,
            b_pos + Vec2::new(0.0, 12.0),
            Vec2::ZERO,
            0.0,
            Vec2::ZERO,
            crate::tank::TANK_SHOT_BBOX,
            crate::tank::TANK_SHELL_LIFE,
            Some(tank_a),
        );
        world.entities.insert(shell_id, shell);

        let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
        let dmg_or_kill = evs.iter().find(|e| {
            matches!(e, GameEvent::EnemyDamaged { .. } | GameEvent::EnemyKilled { .. })
        });
        assert!(dmg_or_kill.is_some(), "shell should damage tank B");
        // Score must not change — friendly fire doesn't credit a player.
        assert_eq!(world.score(pid), 0);
        // Tank A should be untouched (no self-damage).
        assert_eq!(
            world.entities.get(&tank_a).map(|t| t.hp),
            Some(crate::tank::TANK_HP),
            "shell source must not self-damage"
        );
    }

    #[test]
    fn tank_shell_deals_more_damage_than_enemy_bullet() {
        // Inject one tank shell and one enemy bullet, both overlapping
        // the player, and confirm the shell deducts two HP while the
        // bullet deducts one. Locks the `ShotOwner::damage()` mapping
        // through the live collision path.
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);
        // Wipe hostiles so they don't interfere with the injected shots.
        world.entities.retain(|_, e| matches!(e.kind, EntityKind::Player { .. }));
        let player_eid = *world.players.get(&pid).unwrap();
        let starting_hp = world.entities.get(&player_eid).unwrap().hp;
        let player_pos = world.entities.get(&player_eid).unwrap().pos;

        // Tank shell — wider hitbox + 2 damage. `source: None` so the
        // shell doesn't accidentally match an entity ID.
        let shell_id = world.alloc_id();
        let shell = Entity::artillery_shot(
            shell_id,
            ShotOwner::Tank,
            player_pos,
            Vec2::ZERO,
            0.0,
            Vec2::ZERO,
            crate::tank::TANK_SHOT_BBOX,
            crate::tank::TANK_SHELL_LIFE,
            None,
        );
        world.entities.insert(shell_id, shell);

        let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
        let dmg = evs.iter().find_map(|e| match e {
            GameEvent::PlayerDamaged { hp, .. } => Some(*hp),
            _ => None,
        });
        assert_eq!(
            dmg,
            Some(starting_hp - 2),
            "tank shell should deal 2 HP of damage"
        );

        // Now an ordinary enemy bullet — should chip exactly 1 HP.
        let player_pos = world.entities.get(&player_eid).unwrap().pos;
        let bullet_id = world.alloc_id();
        let bullet = Entity::shot(bullet_id, ShotOwner::Enemy, player_pos, Vec2::ZERO, 0.0);
        world.entities.insert(bullet_id, bullet);
        let before = world.entities.get(&player_eid).unwrap().hp;
        let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
        let dmg = evs.iter().find_map(|e| match e {
            GameEvent::PlayerDamaged { hp, .. } => Some(*hp),
            _ => None,
        });
        assert_eq!(dmg, Some(before - 1), "ship bullet should deal 1 HP of damage");
    }

    #[test]
    fn player_takes_two_shots_to_kill_tank() {
        // Park a tank on the ground and overlap a player-owned shot with
        // it twice. First hit emits `EnemyDamaged`, second emits
        // `EnemyKilled`. We inject the shot directly so we don't have to
        // fight the player ship's gravity to land a hit at ground level.
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);
        world.entities.retain(|_, e| !matches!(e.kind, EntityKind::Tank));

        // Sample ground at the tank's X (terrain is now hilly), so the
        // injected shot lands at the same Y the tank gets pinned to.
        let tank_x = WORLD_WIDTH * 0.5 + 300.0;
        let ground = terrain::ground_surface_at(tank_x, world.terrain());
        let tank_pos = Vec2::new(tank_x, ground + crate::tank::TANK_GROUND_OFFSET);
        let tank_id = world.alloc_id();
        let mut tank = Entity::tank(tank_id, tank_pos);
        tank.shot_cooldown = 10.0; // don't return fire during the test
        world.entities.insert(tank_id, tank);

        let inject_shot = |world: &mut World| {
            let shot_id = world.alloc_id();
            let shot = Entity::shot(
                shot_id,
                ShotOwner::Player(pid),
                tank_pos,
                Vec2::ZERO,
                0.0,
            );
            world.entities.insert(shot_id, shot);
        };

        inject_shot(&mut world);
        let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
        let damaged = evs.iter().any(|e| matches!(e, GameEvent::EnemyDamaged { .. }));
        let killed_first = evs.iter().any(|e| matches!(e, GameEvent::EnemyKilled { .. }));
        assert!(damaged, "first hit should emit EnemyDamaged");
        assert!(!killed_first, "first hit should not kill the tank");
        assert_eq!(
            world.entities.get(&tank_id).map(|t| t.hp),
            Some(crate::tank::TANK_HP - 1),
            "tank HP should drop by one after the first hit"
        );

        inject_shot(&mut world);
        let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
        let killed = evs
            .iter()
            .any(|e| matches!(e, GameEvent::EnemyKilled { killer: Some(k), .. } if *k == pid));
        assert!(killed, "second hit should kill the tank");
        assert!(world.entities.get(&tank_id).is_none(), "tank should be cleared");
    }

    #[test]
    fn tank_shells_arc_under_gravity() {
        // Build a stationary tank, force-fire one shell, then tick a
        // few times and confirm the shell's vertical velocity has
        // become more negative than its initial value (it's falling).
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);
        world.entities.retain(|_, e| !matches!(e.kind, EntityKind::Tank));
        let tank_id = world.alloc_id();
        let ground_y = terrain::ground_surface_at(WORLD_WIDTH * 0.5, world.terrain());
        let tank_pos = Vec2::new(WORLD_WIDTH * 0.5, ground_y + crate::tank::TANK_GROUND_OFFSET);
        let mut tank = Entity::tank(tank_id, tank_pos);
        tank.turret_facing = 0.0; // straight up
        tank.shot_cooldown = 0.0;
        world.entities.insert(tank_id, tank);

        // Drop the player far from the tank so the AI continues to want
        // to fire (target above keeps the cone close enough).
        let player_eid = *world.players.get(&pid).unwrap();
        if let Some(p) = world.entities.get_mut(&player_eid) {
            p.pos = tank_pos + Vec2::new(0.0, 400.0);
        }

        // Tick until we see the first ShotFired by the tank.
        let mut shell_id: Option<EntityId> = None;
        for _ in 0..300 {
            let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
            if evs.iter().any(|e| matches!(e, GameEvent::ShotFired { owner: ShotOwner::Tank, .. })) {
                shell_id = world
                    .entities_map()
                    .iter()
                    .filter(|(_, e)| matches!(e.kind, EntityKind::Shot { owner: ShotOwner::Tank }))
                    .map(|(id, _)| *id)
                    .max();
                break;
            }
        }
        let shell_id = shell_id.expect("tank should have fired a shell");
        let initial_vy = world.entities.get(&shell_id).unwrap().vel.y;
        assert!(initial_vy > 0.0, "shell should leave the barrel moving up");

        // Tick a handful of times; vel.y should drop monotonically until
        // either the shell expires or hits the ground.
        for _ in 0..30 {
            world.tick(&PlayerInputs::new(), crate::TICK_DT);
        }
        let later_vy = match world.entities.get(&shell_id) {
            Some(s) => s.vel.y,
            None => return, // shell expired/landed, that's fine — gravity got it
        };
        assert!(
            later_vy < initial_vy,
            "gravity should bend the shell down, initial={initial_vy} later={later_vy}",
        );
    }

    #[test]
    fn player_shot_can_kill_enemy_and_credit_score() {
        // Park an enemy directly in front of the player and fire. The
        // shot should land within a handful of ticks.
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);

        // Place the enemy 60 px directly above the player. facing=0 means
        // the player is pointing +Y, so the shot heads straight at it.
        let player_pos = world
            .player_entity(pid)
            .expect("player should exist")
            .pos;
        let enemy_eid = *world
            .entities_map()
            .iter()
            .find_map(|(id, e)| matches!(e.kind, EntityKind::Enemy).then_some(id))
            .expect("world should have an enemy");
        // Park it stationary just out of bbox-overlap range so the player
        // doesn't ram it before firing.
        let enemy = world.entities.get_mut(&enemy_eid).unwrap();
        enemy.pos = player_pos + Vec2::new(0.0, 60.0);
        enemy.vel = Vec2::ZERO;
        enemy.facing = 0.0; // pointed away — won't return fire on tick 0
        // Reset its cooldown so we don't lucky-eat an enemy shot.
        enemy.shot_cooldown = 5.0;

        let mut inputs = PlayerInputs::new();
        inputs.insert(
            pid,
            PlayerInput {
                xaxis: 0.0,
                yaxis: 0.0,
                fire: true,
            },
        );

        let mut killed = false;
        for _ in 0..30 {
            let evs = world.tick(&inputs, crate::TICK_DT);
            if evs
                .iter()
                .any(|e| matches!(e, GameEvent::EnemyKilled { killer: Some(k), .. } if *k == pid))
            {
                killed = true;
                break;
            }
        }
        assert!(killed, "player shot should have killed the enemy");
        assert_eq!(world.score(pid), 1);
    }

    #[test]
    fn player_takes_two_shots_to_kill_enemy_ship() {
        // Inject two player-owned shots, tick once each. The first must
        // emit `EnemyDamaged` and leave the enemy alive; the second must
        // emit `EnemyKilled`. Locks in `ENEMY_HP = 2` and asserts the
        // shared damage path treats ships and tanks symmetrically.
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);
        world
            .entities
            .retain(|_, e| !matches!(e.kind, EntityKind::Enemy | EntityKind::Tank));
        let enemy_pos = Vec2::new(WORLD_WIDTH * 0.5 + 100.0, SPAWN_Y);
        let enemy_id = world.alloc_id();
        let mut enemy = Entity::enemy(enemy_id, enemy_pos);
        enemy.vel = Vec2::ZERO;
        enemy.shot_cooldown = 100.0;
        world.entities.insert(enemy_id, enemy);

        let inject_shot = |world: &mut World| {
            let shot_id = world.alloc_id();
            let shot = Entity::shot(
                shot_id,
                ShotOwner::Player(pid),
                enemy_pos,
                Vec2::ZERO,
                0.0,
            );
            world.entities.insert(shot_id, shot);
        };

        inject_shot(&mut world);
        let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
        assert!(
            evs.iter().any(|e| matches!(e, GameEvent::EnemyDamaged { .. })),
            "first hit should emit EnemyDamaged"
        );
        assert!(
            !evs.iter().any(|e| matches!(e, GameEvent::EnemyKilled { .. })),
            "first hit should not kill the enemy"
        );
        assert_eq!(
            world.entities.get(&enemy_id).map(|e| e.hp),
            Some(crate::enemy::ENEMY_HP - 1),
        );

        inject_shot(&mut world);
        let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
        assert!(
            evs.iter()
                .any(|e| matches!(e, GameEvent::EnemyKilled { killer: Some(k), .. } if *k == pid)),
            "second hit should kill the enemy and credit the player"
        );
        assert!(world.entities.get(&enemy_id).is_none());
    }

    #[test]
    fn player_contact_does_not_instakill_on_first_overlap() {
        // Park an enemy directly on top of the player and tick a single
        // frame. The pre-refactor behavior was an instant double kill;
        // continuous contact damage must leave the pilot alive after one
        // tick (dose ≈ 0.055 HP).
        let mut world = World::new(WorldConfig::default());
        world.entities.retain(|_, e| !matches!(e.kind, EntityKind::Enemy | EntityKind::Tank));
        let pid = PlayerId(0);
        world.add_player(pid);
        let player_pos = world.player_entity(pid).unwrap().pos;
        let enemy_id = world.alloc_id();
        let mut enemy = Entity::enemy(enemy_id, player_pos);
        // Stop the enemy from drifting off (or shooting) during the tick.
        enemy.vel = Vec2::ZERO;
        enemy.shot_cooldown = 100.0;
        world.entities.insert(enemy_id, enemy);

        let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
        assert!(
            world.player_entity(pid).is_some(),
            "single tick of overlap should not kill the player"
        );
        assert!(
            !evs.iter().any(|e| matches!(e, GameEvent::PlayerKilled { .. })),
            "no PlayerKilled event should fire on first overlap tick"
        );
        assert_eq!(
            world.player_entity(pid).unwrap().hp,
            crate::player::PLAYER_MAX_HP,
            "full HP should be preserved across a single sub-1.0 damage tick"
        );
    }

    #[test]
    fn player_contact_kills_after_ram_death_seconds() {
        // A high-HP hostile that doesn't die from contact damage is the
        // worst-case scenario: the player has to fly away or die. Tick
        // for `RAM_DEATH_SECONDS` worth of frames and confirm the kill
        // event fires with DeathCause::Enemy.
        let mut world = World::new(WorldConfig::default());
        world.entities.retain(|_, e| !matches!(e.kind, EntityKind::Enemy | EntityKind::Tank));
        let pid = PlayerId(0);
        world.add_player(pid);
        let player_eid = *world.players.get(&pid).unwrap();
        let player_pos = world.entities.get(&player_eid).unwrap().pos;

        // Indestructible-for-this-test bumper: stash a huge HP pool on the
        // enemy so its accumulator never crosses a whole HP first.
        let bumper_id = world.alloc_id();
        let mut bumper = Entity::enemy(bumper_id, player_pos);
        bumper.hp = i16::MAX;
        bumper.max_hp = i16::MAX;
        bumper.vel = Vec2::ZERO;
        bumper.shot_cooldown = 100.0;
        world.entities.insert(bumper_id, bumper);

        // Each tick we re-pin the bumper on top of the player so they
        // stay overlapping even though the player drifts under gravity.
        let max_ticks = ((crate::player::RAM_DEATH_SECONDS / crate::TICK_DT).ceil() as i32) + 2;
        let mut death_cause: Option<crate::DeathCause> = None;
        for _ in 0..max_ticks {
            let p_pos = match world.entities.get(&player_eid) {
                Some(p) if p.alive => p.pos,
                _ => break,
            };
            if let Some(b) = world.entities.get_mut(&bumper_id) {
                b.pos = p_pos;
            }
            let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
            if let Some(cause) = evs.iter().find_map(|e| match e {
                GameEvent::PlayerKilled { player_id, cause, .. } if *player_id == pid => {
                    Some(*cause)
                }
                _ => None,
            }) {
                death_cause = Some(cause);
                break;
            }
        }
        assert_eq!(
            death_cause,
            Some(crate::DeathCause::Enemy),
            "sustained overlap should kill the player with DeathCause::Enemy"
        );
        assert!(world.player_entity(pid).is_none(), "player entity should be gone");
    }

    #[test]
    fn brief_contact_does_not_compound_after_separation() {
        // Touch for ~0.8s, fly away for half a second, touch again. Total
        // overlap is under RAM_DEATH_SECONDS, so the accumulator must
        // reset between touches and the player must still be alive.
        let mut world = World::new(WorldConfig::default());
        world.entities.retain(|_, e| !matches!(e.kind, EntityKind::Enemy | EntityKind::Tank));
        let pid = PlayerId(0);
        world.add_player(pid);
        let player_eid = *world.players.get(&pid).unwrap();
        // Pin the pilot up in clear air so 2 s of unpowered drift can't
        // dunk them into the ground — we only want contact damage to be
        // a kill vector for this test.
        let pin_pos = Vec2::new(WORLD_WIDTH * 0.5, WORLD_HEIGHT - 80.0);
        if let Some(p) = world.entities.get_mut(&player_eid) {
            p.pos = pin_pos;
            p.vel = Vec2::ZERO;
        }
        let bumper_id = world.alloc_id();
        let mut bumper = Entity::enemy(bumper_id, pin_pos);
        bumper.hp = i16::MAX;
        bumper.max_hp = i16::MAX;
        bumper.vel = Vec2::ZERO;
        bumper.shot_cooldown = 100.0;
        world.entities.insert(bumper_id, bumper);

        let press = |world: &mut World, ticks: i32, overlap: bool| {
            for _ in 0..ticks {
                if let Some(p) = world.entities.get_mut(&player_eid) {
                    if !p.alive {
                        return;
                    }
                    p.pos = pin_pos;
                    p.vel = Vec2::ZERO;
                }
                let target = if overlap { pin_pos } else { pin_pos + Vec2::new(400.0, 0.0) };
                if let Some(b) = world.entities.get_mut(&bumper_id) {
                    b.pos = target;
                }
                world.tick(&PlayerInputs::new(), crate::TICK_DT);
            }
        };

        // ~0.8s of overlap (under the 1.5s lethal threshold).
        press(&mut world, 48, true);
        assert!(world.player_entity(pid).is_some(), "should still be alive after 0.8s");
        // Half a second clear so the accumulator resets and the regen
        // clock isn't being held at zero.
        press(&mut world, 30, false);
        // Another 0.8s of contact. With the accumulator reset, total
        // continuous overlap is below the death threshold.
        press(&mut world, 48, true);
        assert!(
            world.player_entity(pid).is_some(),
            "two short overlaps separated by a gap must not compound to a kill"
        );
    }

    #[test]
    fn contact_kills_enemy_and_credits_player_score() {
        // A bog-standard 1 HP enemy parked on the player should die from
        // contact damage within a few ticks, and the player should bank
        // the score even though no shot was fired.
        let mut world = World::new(WorldConfig::default());
        world.entities.retain(|_, e| !matches!(e.kind, EntityKind::Enemy | EntityKind::Tank));
        let pid = PlayerId(0);
        world.add_player(pid);
        let player_pos = world.player_entity(pid).unwrap().pos;
        let enemy_id = world.alloc_id();
        let mut enemy = Entity::enemy(enemy_id, player_pos);
        enemy.vel = Vec2::ZERO;
        enemy.shot_cooldown = 100.0;
        world.entities.insert(enemy_id, enemy);

        let mut killed = false;
        for _ in 0..60 {
            let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
            if evs
                .iter()
                .any(|e| matches!(e, GameEvent::EnemyKilled { killer: Some(k), .. } if *k == pid))
            {
                killed = true;
                break;
            }
        }
        assert!(killed, "contact should kill a 1-HP enemy within a second");
        assert_eq!(world.score(pid), 1);
    }

    #[test]
    fn player_can_join_and_be_removed() {
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(7);
        assert!(world.add_player(pid).is_some());
        assert!(world.add_player(pid).is_none(), "duplicate join should noop");
        assert!(world.player_entity(pid).is_some());
        world.remove_player(pid);
        assert!(world.player_entity(pid).is_none());
    }

    #[test]
    fn respawn_resets_enemy_wave_and_level() {
        // Kill the player, run a few ticks, then respawn. The world
        // should drop back to level 1 with a fresh wave that doesn't
        // overlap any old enemy positions.
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);

        // Force level + extra enemies into the world by clearing
        // everything, bumping the counter, and triggering a wave.
        world.entities.retain(|_, e| matches!(e.kind, EntityKind::Player { .. }));
        world.level = 5;
        for _ in 0..6 {
            world.spawn_enemy();
        }
        let before_ids: Vec<EntityId> = world
            .entities_map()
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Enemy))
            .map(|(id, _)| *id)
            .collect();
        assert!(before_ids.len() >= 2);

        world.remove_player(pid);
        let new_eid = world.respawn_player(pid).expect("respawn should put pid back");
        assert_eq!(world.level(), 1);
        let after_ids: Vec<EntityId> = world
            .entities_map()
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Enemy))
            .map(|(id, _)| *id)
            .collect();
        // None of the pre-respawn enemy IDs should still exist.
        for id in before_ids {
            assert!(!after_ids.contains(&id), "enemy {id:?} survived respawn");
        }
        assert!(world.player_entity(pid).is_some(), "player should be back");
        // The freshly-allocated player entity should not collide with
        // the player_id mapping under the old (cleared) entity ID.
        assert_eq!(world.players.get(&pid).copied(), Some(new_eid));
    }

    #[test]
    fn fire_input_emits_shotfired_event() {
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);
        let mut inputs = PlayerInputs::new();
        inputs.insert(
            pid,
            PlayerInput {
                xaxis: 0.0,
                yaxis: 0.0,
                fire: true,
            },
        );
        let events = world.tick(&inputs, crate::TICK_DT);
        assert!(events.iter().any(|e| matches!(e, GameEvent::ShotFired { .. })));
    }

    #[test]
    fn enemies_persist_when_no_players_present() {
        // World starts with enemies and zero players; ticking should not error,
        // and the level shouldn't auto-advance because we gate on `any_players`.
        let mut world = World::new(WorldConfig::default());
        let starting_level = world.level();
        let inputs = PlayerInputs::new();
        for _ in 0..30 {
            let _ = world.tick(&inputs, crate::TICK_DT);
        }
        assert_eq!(world.level(), starting_level);
    }

    #[test]
    fn player_touching_ground_crashes_with_terrain_cause() {
        // Park the player just above the ground band and tick once. The
        // terrain check must fire and produce a Terrain(Ground) death.
        let mut world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        world.add_player(pid);
        let player_eid = *world.players.get(&pid).unwrap();
        // bbox (12) + a sliver: with no movement this tick the bottom
        // of the hitbox sits just inside the ground. Sample the ground
        // before grabbing the mutable player borrow so they don't
        // overlap on `world`.
        let ground_y = terrain::ground_surface_at(WORLD_WIDTH * 0.5, world.terrain());
        let player = world.entities.get_mut(&player_eid).unwrap();
        player.pos = Vec2::new(WORLD_WIDTH * 0.5, ground_y + 11.0);
        player.vel = Vec2::ZERO;

        let evs = world.tick(&PlayerInputs::new(), crate::TICK_DT);
        let crash = evs.iter().find_map(|e| match e {
            GameEvent::PlayerKilled { player_id, cause, .. } if *player_id == pid => Some(*cause),
            _ => None,
        });
        assert_eq!(
            crash,
            Some(crate::DeathCause::Terrain(crate::TerrainKind::Ground)),
            "player on the ground should crash with a Terrain(Ground) cause"
        );
        assert!(world.player_entity(pid).is_none());
    }

    #[test]
    fn enemy_shot_bounces_off_terrain_surface() {
        // Drop an enemy shot moving straight down near the ground. After
        // enough ticks for it to reach the surface, its velocity should
        // be positive (bouncing upward) and its position should sit above
        // the terrain surface plus its bbox. Player shots take a
        // different path — see `player_shot_is_absorbed_by_terrain`.
        let mut world = World::new(WorldConfig::default());
        // Manually inject a downward shot at a known height; tick.
        let id = world.alloc_id();
        let owner = ShotOwner::Enemy;
        let shot_x = WORLD_WIDTH * 0.5;
        let ground_y = terrain::ground_surface_at(shot_x, world.terrain());
        let start = Vec2::new(shot_x, ground_y + 40.0);
        let vel = Vec2::new(0.0, -400.0);
        world.entities.insert(
            id,
            Entity::shot(id, owner, start, vel, std::f32::consts::PI),
        );

        for _ in 0..30 {
            world.tick(&PlayerInputs::new(), crate::TICK_DT);
            // Stop as soon as it's reflected upward.
            if let Some(s) = world.entities.get(&id) {
                if s.vel.y > 0.0 {
                    let local = terrain::surface_y_at(s.pos.x, world.terrain());
                    let floor = local + s.bbox;
                    assert!(
                        s.pos.y >= floor - 1.0,
                        "shot should bounce above the terrain surface, got pos.y={}",
                        s.pos.y
                    );
                    return;
                }
            }
        }
        panic!("enemy shot never bounced off terrain");
    }

    #[test]
    fn player_shot_is_absorbed_by_terrain() {
        // A player-owned shot fired into the ground should disappear
        // instead of ricocheting back. We aim a downward shot at the
        // surface and confirm the entity is gone within a handful of
        // ticks, with no upward reflection in between.
        let mut world = World::new(WorldConfig::default());
        let id = world.alloc_id();
        let owner = ShotOwner::Player(PlayerId(99));
        let shot_x = WORLD_WIDTH * 0.5;
        let ground_y = terrain::ground_surface_at(shot_x, world.terrain());
        let start = Vec2::new(shot_x, ground_y + 40.0);
        let vel = Vec2::new(0.0, -400.0);
        world.entities.insert(
            id,
            Entity::shot(id, owner, start, vel, std::f32::consts::PI),
        );
        let mut max_vy = f32::MIN;
        for _ in 0..30 {
            world.tick(&PlayerInputs::new(), crate::TICK_DT);
            if let Some(s) = world.entities.get(&id) {
                max_vy = max_vy.max(s.vel.y);
            } else {
                // Absorbed — done. Sanity-check it never bounced first.
                assert!(
                    max_vy <= 0.0 + 1e-3,
                    "player shot should never reflect upward, peak vy={max_vy}"
                );
                return;
            }
        }
        panic!("player shot should have been absorbed by terrain");
    }

    #[test]
    fn player_shot_outlives_enemy_shot_in_air() {
        // Same arena, two shots fired horizontally well above the ground
        // so neither one runs into terrain. Tick long enough for the
        // shorter TTL (enemy) to expire; the player's shot should still
        // be alive because `PLAYER_SHOT_LIFE > world::SHOT_LIFE`.
        let mut world = World::new(WorldConfig::default());
        world.entities.clear();
        let high_y = WORLD_HEIGHT - 60.0;
        let pos = Vec2::new(WORLD_WIDTH * 0.5, high_y);
        // Slow horizontal velocity so neither shot exits the world before
        // TTL expiry.
        let vel = Vec2::new(50.0, 0.0);
        let player_id = world.alloc_id();
        world.entities.insert(
            player_id,
            Entity::shot(player_id, ShotOwner::Player(PlayerId(0)), pos, vel, 0.0),
        );
        let enemy_id = world.alloc_id();
        world.entities.insert(
            enemy_id,
            Entity::shot(enemy_id, ShotOwner::Enemy, pos, vel, 0.0),
        );

        // Tick just past the enemy bullet's TTL but well before the
        // player's so the difference is observable.
        let between =
            (crate::world::SHOT_LIFE + crate::player::PLAYER_SHOT_LIFE) * 0.5;
        let steps = (between / crate::TICK_DT).ceil() as i32;
        for _ in 0..steps {
            world.tick(&PlayerInputs::new(), crate::TICK_DT);
        }
        assert!(
            world.entities.get(&enemy_id).is_none(),
            "enemy bullet should have expired by mid-window"
        );
        assert!(
            world.entities.get(&player_id).is_some(),
            "player bullet should still be alive past the enemy bullet's TTL"
        );
    }

    #[test]
    fn fresh_pilot_does_not_drift_until_thrust() {
        // Spawn a pilot, run for half a second with no input — they must
        // sit perfectly still. Then thrust for one tick; gravity arms
        // and the ship starts falling (assuming the thrust pulse is
        // released).
        let mut world = World::new(WorldConfig::default());
        world.entities.retain(|_, e| !matches!(e.kind, EntityKind::Enemy | EntityKind::Tank));
        let pid = PlayerId(0);
        world.add_player(pid);
        let eid = *world.players.get(&pid).unwrap();
        let spawn_pos = world.entities.get(&eid).unwrap().pos;

        // 30 ticks (~0.5s) with no input.
        for _ in 0..30 {
            world.tick(&PlayerInputs::new(), crate::TICK_DT);
        }
        let p = world.entities.get(&eid).unwrap();
        assert_eq!(p.pos, spawn_pos, "unarmed pilot should not drift");
        assert_eq!(p.vel, Vec2::ZERO);
        assert!(!p.gravity_armed);

        // One thrust pulse arms gravity, then release.
        let mut inputs = PlayerInputs::new();
        inputs.insert(
            pid,
            PlayerInput { xaxis: 0.0, yaxis: 1.0, fire: false },
        );
        world.tick(&inputs, crate::TICK_DT);
        let p_after = world.entities.get(&eid).unwrap();
        assert!(p_after.gravity_armed, "first thrust should arm gravity");

        // Stop thrusting; over the next 30 ticks the pilot must fall.
        for _ in 0..30 {
            world.tick(&PlayerInputs::new(), crate::TICK_DT);
        }
        let p_falling = world.entities.get(&eid).unwrap();
        assert!(
            p_falling.vel.y < 0.0,
            "armed pilot should accumulate downward velocity, got {:?}",
            p_falling.vel,
        );
    }

    #[test]
    fn spawn_sits_well_above_ground() {
        // The player spawn should clear the terrain by a healthy margin
        // so a startled pilot doesn't crash before they thrust. Concretely:
        // distance from spawn Y to the local ground surface should exceed
        // the entire viewport height, so even a free-fall would take many
        // seconds to terminate.
        let world = World::new(WorldConfig::default());
        let pid = PlayerId(0);
        // World::new doesn't pre-populate players, so we just inspect the
        // SPAWN_Y constant in isolation.
        let _ = pid;
        let ground = terrain::ground_surface_at(WORLD_WIDTH * 0.5, world.terrain());
        let headroom = SPAWN_Y - ground;
        assert!(
            headroom > VIEW_HEIGHT,
            "spawn should clear ground by more than one viewport, got {headroom}",
        );
    }

    #[test]
    fn enemies_spawn_outside_player_safe_radius() {
        // Repeatedly clear and respawn the enemy wave; each newly spawned
        // enemy should sit at least `ENEMY_SAFE_SPAWN_RADIUS` from the
        // player. We try several seeds to keep the test from passing by
        // luck.
        for seed in 0..16u64 {
            let mut world = World::new(WorldConfig {
                seed: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                world_size: Vec2::new(WORLD_WIDTH, WORLD_HEIGHT),
            });
            let pid = PlayerId(0);
            world.add_player(pid);
            // Clear and trigger fresh spawns several times. We force a
            // mixed ship+tank wave so both kinds are exercised.
            for _ in 0..4 {
                world.entities.retain(|_, e| matches!(e.kind, EntityKind::Player { .. }));
                for _ in 0..4 {
                    world.spawn_enemy();
                }
                for _ in 0..2 {
                    world.spawn_tank();
                }
                let player_pos = world.player_entity(pid).unwrap().pos;
                for (_, e) in world.entities_map() {
                    if matches!(e.kind, EntityKind::Enemy | EntityKind::Tank) {
                        let d = util::toroidal_distance(e.pos, player_pos, WORLD_WIDTH);
                        assert!(
                            d >= ENEMY_SAFE_SPAWN_RADIUS - 1.0,
                            "enemy spawned too close to player: d={d} radius={ENEMY_SAFE_SPAWN_RADIUS}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn determinism_replay_matches_hash() {
        // Two worlds with identical seeds and identical input streams must
        // produce identical entity state. Hash sorted (id, pos, vel, facing, alive)
        // tuples after N ticks and compare.
        fn run_and_hash(seed: u64) -> u64 {
            let mut w = World::new(WorldConfig {
                seed,
                world_size: Vec2::new(WORLD_WIDTH, WORLD_HEIGHT),
            });
            w.add_player(PlayerId(0));
            let mut inputs = PlayerInputs::new();
            inputs.insert(
                PlayerId(0),
                PlayerInput {
                    xaxis: 0.5,
                    yaxis: 1.0,
                    fire: true,
                },
            );
            for _ in 0..600 {
                let _ = w.tick(&inputs, crate::TICK_DT);
            }
            // Order-sensitive FNV-1a over (id, pos, vel, facing, alive).
            // BTreeMap iteration is ordered by id, so the hash is stable
            // across runs as long as the simulation is deterministic.
            let mut h: u64 = 1469598103934665603;
            let mix = |h: &mut u64, x: u64| {
                *h ^= x;
                *h = h.wrapping_mul(1099511628211);
            };
            for (id, e) in w.entities_map() {
                mix(&mut h, id.0);
                mix(&mut h, e.pos.x.to_bits() as u64);
                mix(&mut h, e.pos.y.to_bits() as u64);
                mix(&mut h, e.vel.x.to_bits() as u64);
                mix(&mut h, e.vel.y.to_bits() as u64);
                mix(&mut h, e.facing.to_bits() as u64);
                mix(&mut h, e.alive as u64);
            }
            h
        }
        let a = run_and_hash(42);
        let b = run_and_hash(42);
        assert_eq!(a, b, "same seed + same inputs must produce same world");
        let c = run_and_hash(43);
        assert_ne!(a, c, "different seed should diverge");
    }
}
