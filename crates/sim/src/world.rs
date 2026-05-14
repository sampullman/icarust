use std::collections::BTreeMap;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::entity::{Entity, EntityId, EntityKind, PlayerId, ShotOwner, Tick};
use crate::enemy::{self, ENEMY_SHOT_SPEED, ENEMY_SHOT_TIME};
use crate::event::{DeathCause, GameEvent};
use crate::input::PlayerInputs;
use crate::physics;
use crate::player::{
    self, PLAYER_REGEN_DELAY, PLAYER_REGEN_INTERVAL, PLAYER_SHOT_TIME, SHOT_SPEED,
};
use crate::tank::{self, TANK_GROUND_OFFSET, TANK_SHOT_GRAVITY, TANK_SHOT_SPEED, TANK_SHOT_TIME};
use crate::terrain::{self, TerrainBand};
use crate::util::{self, Vec2};

/// World is wider than the visible viewport so the camera can scroll
/// instead of wrapping at the screen edge. The X axis is still toroidal
/// (sim::util::wrap_coord) — entities that fly off the right reappear on
/// the left — but the client camera follows the local player and draws
/// each entity in up to three positions (`x`, `x ± WORLD_WIDTH`) so the
/// wrap is invisible from the player's perspective.
pub const WORLD_WIDTH: f32 = 3200.0;
pub const WORLD_HEIGHT: f32 = 540.0;

pub const SHOT_BBOX: f32 = 6.0;
pub const SHOT_LIFE: f32 = 2.0;

/// Ship enemies present at the start of level 1. Each subsequent level
/// adds one more, capped at `ENEMIES_MAX`.
pub const ENEMIES_PER_LEVEL_BASE: i32 = 2;
/// Cap on simultaneous ship enemies. Without this, late levels turn into
/// bullet hell that's no longer fun.
pub const ENEMIES_MAX: i32 = 8;

/// Tanks present at the start of level 1. Tanks scale up at half the
/// rate of ships so the player isn't drowned in artillery.
pub const TANKS_PER_LEVEL_BASE: i32 = 1;
/// Cap on simultaneous tanks.
pub const TANKS_MAX: i32 = 4;

/// Hostiles refuse to spawn within this radius of any live player so the
/// pilot never has to deal with one materialising in their lap.
pub const ENEMY_SAFE_SPAWN_RADIUS: f32 = 380.0;

/// Hostiles within this radius of a (re)spawning player are nudged out
/// so the player isn't killed on the same tick they appear.
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
}

impl World {
    pub fn new(config: WorldConfig) -> Self {
        let rng = ChaCha8Rng::seed_from_u64(config.seed);
        let mut world = World {
            config,
            rng,
            tick: Tick(0),
            next_entity_id: 0,
            entities: BTreeMap::new(),
            players: BTreeMap::new(),
            score_by_player: BTreeMap::new(),
            level: 1,
            terrain: terrain::default_terrain(),
        };
        world.spawn_level_hostiles();
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

    /// Spawn a player at world center. No-op if already present.
    /// Pushes any enemies inside `SAFE_SPAWN_RADIUS` out of the way so the
    /// new ship isn't killed on the same tick it appears.
    pub fn add_player(&mut self, player_id: PlayerId) -> Option<EntityId> {
        if self.players.contains_key(&player_id) {
            return None;
        }
        let id = self.alloc_id();
        let center = self.config.world_size * 0.5;
        self.clear_safe_zone(center, SAFE_SPAWN_RADIUS);
        let entity = Entity::player(id, player_id, center);
        self.entities.insert(id, entity);
        self.players.insert(player_id, id);
        self.score_by_player.entry(player_id).or_insert(0);
        Some(id)
    }

    /// Put a dead player back in the world and reset the hostile state so
    /// they aren't dropped into mid-battle. Wipes existing enemies, tanks,
    /// and shots, drops the level back to 1, and spawns a fresh wave at
    /// safe distance. Other live players stay put — their entities and
    /// scores are preserved.
    pub fn respawn_player(&mut self, player_id: PlayerId) -> Option<EntityId> {
        if self.players.contains_key(&player_id) {
            return None;
        }
        self.entities
            .retain(|_, e| matches!(e.kind, EntityKind::Player { .. }));
        self.level = 1;
        let id = self.add_player(player_id);
        self.spawn_level_hostiles();
        id
    }

    /// Push hostiles out of a disc so a freshly-spawned player isn't
    /// standing on top of one. They're moved to the disc edge along the
    /// radial direction; velocity is preserved so the world keeps moving.
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
            // Pick an outward direction; if the enemy is exactly at the
            // center, kick it along +x so the math is well-defined.
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

            entity.shot_cooldown -= dt;

            if input.fire && entity.shot_cooldown <= 0.0 {
                entity.shot_cooldown = PLAYER_SHOT_TIME;
                let direction = util::vec_from_angle(facing);
                let spawn_pos = entity.pos + direction * player::PLAYER_BBOX;
                let shot_id = EntityId(self.next_entity_id);
                self.next_entity_id += 1;
                let owner = ShotOwner::Player(player_id);
                let shot = Entity::shot(shot_id, owner, spawn_pos, direction * SHOT_SPEED, facing);
                events.push(GameEvent::ShotFired { owner, pos: spawn_pos });
                new_shots.push(shot);
            }
        }
        for shot in new_shots {
            self.entities.insert(shot.id, shot);
        }

        // 1b. Enemy AI. Each enemy targets the nearest live player, steers
        // toward it, and may fire. Snapshotting target positions before
        // mutating means the order of enemies in the BTreeMap doesn't
        // affect the AI decisions on this tick.
        let player_targets: Vec<Vec2> = self
            .entities
            .values()
            .filter(|e| matches!(e.kind, EntityKind::Player { .. }) && e.alive)
            .map(|e| e.pos)
            .collect();
        let world_width = self.config.world_size.x;
        let enemy_eids: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Enemy) && e.alive)
            .map(|(id, _)| *id)
            .collect();
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
                let direction = util::vec_from_angle(entity.facing);
                let spawn_pos = entity.pos + direction * enemy::ENEMY_BBOX;
                let shot_id = EntityId(self.next_entity_id);
                self.next_entity_id += 1;
                let owner = ShotOwner::Enemy;
                let shot = Entity::shot(
                    shot_id,
                    owner,
                    spawn_pos,
                    direction * ENEMY_SHOT_SPEED,
                    entity.facing,
                );
                events.push(GameEvent::ShotFired { owner, pos: spawn_pos });
                enemy_shots.push(shot);
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
        let tank_eids: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Tank) && e.alive)
            .map(|(id, _)| *id)
            .collect();
        let mut tank_shots: Vec<Entity> = Vec::new();
        for eid in tank_eids {
            let Some(entity) = self.entities.get_mut(&eid) else {
                continue;
            };
            let target = nearest_target(entity.pos, &player_targets, world_width);
            let step = tank::step(
                entity.pos,
                entity.vel,
                entity.facing,
                entity.turret_facing,
                target,
                world_width,
                dt,
            );
            entity.vel = step.vel;
            entity.facing = step.body_facing;
            entity.turret_facing = step.turret_facing;
            entity.shot_cooldown -= dt;

            if step.fire && entity.shot_cooldown <= 0.0 {
                entity.shot_cooldown = TANK_SHOT_TIME;
                let direction = util::vec_from_angle(entity.turret_facing);
                // Spawn the shell at the end of the barrel, well clear of
                // the chassis so the tank doesn't immediately collide
                // with its own shot.
                let spawn_pos = entity.pos + direction * (entity.bbox + 6.0);
                let shot_id = EntityId(self.next_entity_id);
                self.next_entity_id += 1;
                let owner = ShotOwner::Enemy;
                let shell = Entity::artillery_shot(
                    shot_id,
                    owner,
                    spawn_pos,
                    direction * TANK_SHOT_SPEED,
                    entity.turret_facing,
                    Vec2::new(0.0, -TANK_SHOT_GRAVITY),
                );
                events.push(GameEvent::ShotFired { owner, pos: spawn_pos });
                tank_shots.push(shell);
            }
        }
        for shot in tank_shots {
            self.entities.insert(shot.id, shot);
        }

        // 2. Move + wrap + per-kind extras.
        // X is toroidal (you can fly off the right edge and come in on
        // the left). Y is a hard wall — players clamp against it and
        // enemies/shots bounce. Shots bounce off the terrain surface
        // rather than the bare bottom of the world so they ricochet
        // visibly off the ground.
        let world_size = self.config.world_size;
        let surface = terrain::surface_y(&self.terrain);
        for entity in self.entities.values_mut() {
            if !entity.alive {
                continue;
            }
            entity.pos += entity.vel * dt;
            entity.pos.x = util::wrap_coord(entity.pos.x, world_size.x);

            match entity.kind {
                EntityKind::Player { .. } => {
                    util::clamp_y(&mut entity.pos, &mut entity.vel, world_size.y);
                    entity.vel = player::apply_forces(entity.vel, dt);
                }
                EntityKind::Shot { .. } => {
                    // Bounce off the top of the terrain (plus the shot's
                    // own radius) so it doesn't sink in before reflecting.
                    let floor = surface + entity.bbox;
                    util::bounce_y(&mut entity.pos, &mut entity.vel, floor, world_size.y);
                    // Apply any per-shot acceleration (tank shells use
                    // this for gravity). Done after bounce so the bounce
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
                    // Enemies bounce off the same surface as shots so they
                    // can skim along the ground without sinking through it.
                    let floor = surface + entity.bbox;
                    util::bounce_y(&mut entity.pos, &mut entity.vel, floor, world_size.y);
                }
                EntityKind::Tank => {
                    // Tank chassis is locked to the terrain surface.
                    // Vertical velocity is killed in `tank::step`; we
                    // still pin Y here in case anything else perturbs
                    // it (collisions, external nudges). The chassis
                    // mesh is authored with its center at `pos`, so we
                    // raise `pos.y` by the per-kind ground offset (not
                    // by the collision radius — those are decoupled).
                    let ground = terrain::ground_surface_at(entity.pos.x, &self.terrain);
                    entity.pos.y = ground + TANK_GROUND_OFFSET;
                    entity.vel.y = 0.0;
                }
            }
        }

        // 2b. Terrain. A player whose hitbox dips into a terrain band
        // crashes there. Done before entity-vs-entity collision so a
        // player who somehow rams an enemy and the ground in the same
        // tick is recorded as a terrain crash (the ground has the last
        // word when both fire — minor, but we want to be deterministic).
        self.handle_terrain(&mut events);

        // 3. Collisions. Iteration is over BTreeMap so order is deterministic.
        self.handle_collisions(&mut events);

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

        // 5. Level progression. Once a player has cleared every hostile
        // (ships and tanks) the level goes up and a new (larger) wave
        // spawns. Skip while no players are around — without someone to
        // chase the wave would just hang in air.
        let any_hostiles = self
            .entities
            .values()
            .any(|e| matches!(e.kind, EntityKind::Enemy | EntityKind::Tank));
        let any_players = !self.players.is_empty();
        if !any_hostiles && any_players {
            self.level += 1;
            self.spawn_level_hostiles();
            events.push(GameEvent::LevelUp(self.level));
        }

        self.tick = self.tick.next();
        events
    }

    fn handle_terrain(&mut self, events: &mut Vec<GameEvent>) {
        let player_eids: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Player { .. }) && e.alive)
            .map(|(id, _)| *id)
            .collect();
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
            // Pin the impact point to the player's X but the band's
            // surface Y so the client can draw the boom sitting on the
            // ground rather than half-buried.
            let surface_y = self
                .terrain
                .iter()
                .filter(|b| b.kind == kind)
                .map(|b| b.top_y)
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

    fn handle_collisions(&mut self, events: &mut Vec<GameEvent>) {
        // Treat ships and tanks as one bucket: both die from ramming the
        // player, and both lose HP from player shots (tanks just have
        // more HP to spend). New hostile kinds plug into the same logic
        // by joining this filter.
        let hostile_ids: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| {
                matches!(e.kind, EntityKind::Enemy | EntityKind::Tank) && e.alive
            })
            .map(|(id, _)| *id)
            .collect();
        let player_ids: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Player { .. }) && e.alive)
            .map(|(id, _)| *id)
            .collect();
        let player_shot_ids: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| {
                matches!(e.kind, EntityKind::Shot { owner: ShotOwner::Player(_) }) && e.alive
            })
            .map(|(id, _)| *id)
            .collect();
        let enemy_shot_ids: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Shot { owner: ShotOwner::Enemy }) && e.alive)
            .map(|(id, _)| *id)
            .collect();

        // Player ↔ hostile: ramming kills both. The player gets credit
        // for the kill so suicide-runs still count. Hostile HP is ignored
        // — a chassis impact at flight speed totals both ships regardless.
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
                let player_pos = self.entities.get(player_id).map(|p| p.pos).unwrap_or(Vec2::ZERO);
                let pos = self.entities.get(hostile_id).map(|e| e.pos).unwrap_or(Vec2::ZERO);
                if let Some(p) = self.entities.get_mut(player_id) {
                    p.alive = false;
                }
                if let Some(e) = self.entities.get_mut(hostile_id) {
                    e.alive = false;
                }
                *self.score_by_player.entry(pid).or_insert(0) += 1;
                events.push(GameEvent::PlayerKilled {
                    player_id: pid,
                    pos: player_pos,
                    cause: DeathCause::Enemy,
                });
                events.push(GameEvent::EnemyKilled { pos, killer: pid });
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
                    events.push(GameEvent::EnemyKilled { pos, killer: owner_pid });
                } else {
                    let hp_remaining = h.hp;
                    events.push(GameEvent::EnemyDamaged { pos, hp: hp_remaining });
                }
                break;
            }
        }

        // Enemy shot ↔ player: shot dies; player loses one HP and only
        // dies when HP hits zero. Repeat hits within `PLAYER_REGEN_DELAY`
        // stack, so a focused volley will still drop the pilot.
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
                if let Some(s) = self.entities.get_mut(shot_id) {
                    s.alive = false;
                }
                let Some(p) = self.entities.get_mut(player_id) else {
                    continue;
                };
                p.hp -= 1;
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
                break;
            }
        }
    }

    /// Tick the regen clock on every live player. After `PLAYER_REGEN_DELAY`
    /// of damage-free flight, HP starts climbing back at one tick every
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

    /// Number of ship enemies that should be airborne at the current level.
    /// We scale with the level so each clear ramps up the pressure, capped
    /// at `ENEMIES_MAX` so things stay readable.
    fn target_enemy_count(&self) -> i32 {
        (ENEMIES_PER_LEVEL_BASE + self.level - 1).clamp(1, ENEMIES_MAX)
    }

    /// Number of tanks that should be rolling at the current level. Half
    /// the pace of ship enemies — tanks are heavier threats so we add
    /// them more slowly.
    fn target_tank_count(&self) -> i32 {
        (TANKS_PER_LEVEL_BASE + (self.level - 1) / 2).clamp(0, TANKS_MAX)
    }

    /// Spawn hostiles to bring ship + tank counts up to the current
    /// level's targets. New hostile kinds should plug in here alongside
    /// the existing two; the level-up trigger checks "no hostiles" so
    /// each kind contributes to clearing.
    fn spawn_level_hostiles(&mut self) {
        let enemy_target = self.target_enemy_count();
        let tank_target = self.target_tank_count();
        let (enemy_current, tank_current) = self.entities.values().fold((0, 0), |(e, t), entity| {
            if !entity.alive {
                return (e, t);
            }
            match entity.kind {
                EntityKind::Enemy => (e + 1, t),
                EntityKind::Tank => (e, t + 1),
                _ => (e, t),
            }
        });
        for _ in enemy_current..enemy_target {
            self.spawn_enemy();
        }
        for _ in tank_current..tank_target {
            self.spawn_tank();
        }
    }

    /// Spawn one enemy well outside any live player's view so it flies in
    /// from off-screen rather than appearing on top of them. We retry a
    /// handful of times if the rolled position falls inside the safe
    /// radius around any player, then accept the last attempt. RNG draws
    /// per call are bounded so determinism is preserved.
    fn spawn_enemy(&mut self) {
        let world = self.config.world_size;
        let player_positions: Vec<Vec2> = self
            .entities
            .values()
            .filter(|e| matches!(e.kind, EntityKind::Player { .. }) && e.alive)
            .map(|e| e.pos)
            .collect();

        // Up to 8 attempts to find a player-clear spot. Each attempt
        // consumes the same number of RNG draws so the determinism
        // contract holds — same world state in, same attempts out.
        const MAX_ATTEMPTS: usize = 8;
        let mut chosen = Vec2::ZERO;
        for attempt in 0..MAX_ATTEMPTS {
            let x = util::rand_unit(&mut self.rng) * world.x;
            // Vertical: bias toward the upper half so enemies approach
            // from above rather than along the floor. Keep clear of the
            // top/bottom margins so they're not pinned against a wall.
            let y = 60.0 + util::rand_unit(&mut self.rng) * (world.y * 0.55);
            let pos = Vec2::new(x, y);
            let safe = player_positions
                .iter()
                .all(|p| toroidal_dist(pos, *p, world.x) >= ENEMY_SAFE_SPAWN_RADIUS);
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

    /// Spawn one tank rolling on the ground at a player-safe X. Same
    /// retry shape as `spawn_enemy` so determinism is preserved; we just
    /// roll an X and pin the Y to the terrain surface. Skips X-ranges
    /// that are not passable for ground vehicles (no-op today; future
    /// water bands will start filtering here).
    fn spawn_tank(&mut self) {
        let world = self.config.world_size;
        let player_positions: Vec<Vec2> = self
            .entities
            .values()
            .filter(|e| matches!(e.kind, EntityKind::Player { .. }) && e.alive)
            .map(|e| e.pos)
            .collect();

        const MAX_ATTEMPTS: usize = 8;
        let mut chosen_x = 0.0_f32;
        for attempt in 0..MAX_ATTEMPTS {
            let x = util::rand_unit(&mut self.rng) * world.x;
            let ground = terrain::ground_surface_at(x, &self.terrain);
            let probe = Vec2::new(x, ground + TANK_GROUND_OFFSET);
            let passable = terrain::passable_for_ground_vehicle(x, &self.terrain);
            let safe = player_positions
                .iter()
                .all(|p| toroidal_dist(probe, *p, world.x) >= ENEMY_SAFE_SPAWN_RADIUS);
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
}

/// Pick the closest target position from `candidates`, accounting for X-wrap.
/// Returns `None` when the slice is empty.
fn nearest_target(from: Vec2, candidates: &[Vec2], world_width: f32) -> Option<Vec2> {
    let mut best: Option<(f32, Vec2)> = None;
    let half = world_width * 0.5;
    for &c in candidates {
        let mut dx = c.x - from.x;
        if dx > half {
            dx -= world_width;
        } else if dx < -half {
            dx += world_width;
        }
        let dy = c.y - from.y;
        let d2 = dx * dx + dy * dy;
        match best {
            Some((bd, _)) if bd <= d2 => {}
            _ => best = Some((d2, c)),
        }
    }
    best.map(|(_, c)| c)
}

/// Shortest distance between two points accounting for X-wrap.
fn toroidal_dist(a: Vec2, b: Vec2, world_width: f32) -> f32 {
    let half = world_width * 0.5;
    let mut dx = (a.x - b.x).abs();
    if dx > half {
        dx = world_width - dx;
    }
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
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
        assert_eq!(enemies.len(), ENEMIES_PER_LEVEL_BASE as usize);
    }

    #[test]
    fn world_spawns_initial_tank_wave() {
        let world = World::new(WorldConfig::default());
        let tanks: Vec<_> = world
            .entities()
            .filter(|e| matches!(e.kind, EntityKind::Tank))
            .collect();
        assert_eq!(tanks.len(), TANKS_PER_LEVEL_BASE as usize);
        let ground = terrain::ground_surface_at(0.0, world.terrain());
        for t in &tanks {
            assert!(
                (t.pos.y - (ground + crate::tank::TANK_GROUND_OFFSET)).abs() < 1e-3,
                "tank should sit on the ground surface, got y={}",
                t.pos.y
            );
            assert_eq!(t.hp, crate::tank::TANK_HP);
            assert_eq!(t.max_hp, crate::tank::TANK_HP);
        }
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

        let ground = terrain::ground_surface_at(0.0, world.terrain());
        let tank_pos = Vec2::new(WORLD_WIDTH * 0.5 + 300.0, ground + crate::tank::TANK_GROUND_OFFSET);
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
            .any(|e| matches!(e, GameEvent::EnemyKilled { killer, .. } if *killer == pid));
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
        let tank_pos = Vec2::new(WORLD_WIDTH * 0.5, terrain::GROUND_HEIGHT + crate::tank::TANK_GROUND_OFFSET);
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
            if evs.iter().any(|e| matches!(e, GameEvent::ShotFired { owner: ShotOwner::Enemy, .. })) {
                // Find the newest enemy shot — should be the tank shell.
                shell_id = world
                    .entities_map()
                    .iter()
                    .filter(|(_, e)| matches!(e.kind, EntityKind::Shot { owner: ShotOwner::Enemy }))
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
                .any(|e| matches!(e, GameEvent::EnemyKilled { killer, .. } if *killer == pid))
            {
                killed = true;
                break;
            }
        }
        assert!(killed, "player shot should have killed the enemy");
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
        // everything and bumping the counter.
        world.entities.retain(|_, e| matches!(e.kind, EntityKind::Player { .. }));
        world.level = 5;
        world.spawn_level_hostiles();
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
        let player = world.entities.get_mut(&player_eid).unwrap();
        // bbox (12) + a sliver: with no movement this tick the bottom
        // of the hitbox sits just inside the ground.
        player.pos = Vec2::new(WORLD_WIDTH * 0.5, terrain::GROUND_HEIGHT + 11.0);
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
    fn shot_bounces_off_terrain_surface() {
        // Drop a shot moving straight down near the ground. After enough
        // ticks for it to reach the surface, its velocity should be
        // positive (bouncing upward) and its position should sit above
        // the terrain surface plus its bbox.
        let mut world = World::new(WorldConfig::default());
        // Manually inject a downward shot at a known height; tick.
        let id = world.alloc_id();
        let owner = ShotOwner::Player(PlayerId(99));
        let start = Vec2::new(WORLD_WIDTH * 0.5, terrain::GROUND_HEIGHT + 40.0);
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
                    let floor = terrain::GROUND_HEIGHT + s.bbox;
                    assert!(
                        s.pos.y >= floor - 1.0,
                        "shot should bounce above the terrain surface, got pos.y={}",
                        s.pos.y
                    );
                    return;
                }
            }
        }
        panic!("shot never bounced off terrain");
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
            // Clear and trigger fresh spawns several times.
            for _ in 0..4 {
                world.entities.retain(|_, e| matches!(e.kind, EntityKind::Player { .. }));
                world.spawn_level_hostiles();
                let player_pos = world.player_entity(pid).unwrap().pos;
                for (_, e) in world.entities_map() {
                    if matches!(e.kind, EntityKind::Enemy | EntityKind::Tank) {
                        let d = toroidal_dist(e.pos, player_pos, WORLD_WIDTH);
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
