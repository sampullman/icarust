use std::collections::BTreeMap;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::entity::{Entity, EntityId, EntityKind, PlayerId, ShotOwner, Tick};
use crate::enemy::{self, ENEMY_SHOT_SPEED, ENEMY_SHOT_TIME};
use crate::event::GameEvent;
use crate::input::PlayerInputs;
use crate::physics;
use crate::player::{self, PLAYER_SHOT_TIME, SHOT_SPEED};
use crate::util::{self, Vec2};

pub const WORLD_WIDTH: f32 = 1280.0;
pub const WORLD_HEIGHT: f32 = 540.0;

pub const ROCK_BBOX: f32 = 12.0;
pub const ROCK_MAX_VEL: f32 = 50.0;
pub const ROCK_SPAWN_MIN_RADIUS: f32 = 100.0;
pub const ROCK_SPAWN_MAX_RADIUS: f32 = 250.0;

pub const SHOT_BBOX: f32 = 6.0;
pub const SHOT_LIFE: f32 = 2.0;

pub const ROCKS_PER_LEVEL_BASE: i32 = 5;
pub const ROCKS_MAX: i32 = 30;

/// Cap on simultaneous enemies. We add one each level-up; this keeps
/// later levels from getting overwhelming.
pub const ENEMIES_MAX: i32 = 3;

/// Rocks within this radius of a (re)spawning player are nudged out so
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
            level: 0,
        };
        world.spawn_rocks(ROCKS_PER_LEVEL_BASE);
        world.spawn_enemy();
        world
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
    /// Pushes any rocks inside `SAFE_SPAWN_RADIUS` out of the way so the
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

    /// Push rocks and enemies out of a disc so a freshly-spawned player
    /// isn't standing on top of one. Hostile entities are moved to the
    /// disc edge along the radial direction; their velocity is preserved
    /// so the world keeps moving.
    fn clear_safe_zone(&mut self, center: Vec2, radius: f32) {
        for entity in self.entities.values_mut() {
            let is_hostile = matches!(entity.kind, EntityKind::Rock | EntityKind::Enemy);
            if !is_hostile || !entity.alive {
                continue;
            }
            let offset = entity.pos - center;
            let dist = offset.length();
            if dist >= radius {
                continue;
            }
            // Pick an outward direction; if the rock is exactly at the
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

        // 2. Move + wrap + per-kind extras.
        // X is toroidal (you can fly off the right edge and come in on
        // the left). Y is a hard wall — players clamp against it and
        // rocks/shots bounce — so the world has a floor and a ceiling
        // instead of teleporting between them.
        let world_size = self.config.world_size;
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
                    util::bounce_y(&mut entity.pos, &mut entity.vel, world_size.y);
                    if let Some(ttl) = entity.ttl.as_mut() {
                        *ttl -= dt;
                        if *ttl <= 0.0 {
                            entity.alive = false;
                        }
                    }
                }
                EntityKind::Rock | EntityKind::Enemy => {
                    util::bounce_y(&mut entity.pos, &mut entity.vel, world_size.y);
                }
            }
        }

        // 3. Collisions. Iteration is over BTreeMap so order is deterministic.
        self.handle_collisions(&mut events);

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

        // 5. Level respawn.
        let any_rocks = self.entities.values().any(|e| matches!(e.kind, EntityKind::Rock));
        if !any_rocks {
            self.level += 1;
            let count = (self.level + ROCKS_PER_LEVEL_BASE).min(ROCKS_MAX);
            self.spawn_rocks(count);
            let enemy_count = self
                .entities
                .values()
                .filter(|e| matches!(e.kind, EntityKind::Enemy))
                .count() as i32;
            if enemy_count < ENEMIES_MAX {
                self.spawn_enemy();
            }
            events.push(GameEvent::LevelUp(self.level));
        }

        self.tick = self.tick.next();
        events
    }

    fn handle_collisions(&mut self, events: &mut Vec<GameEvent>) {
        let rock_ids: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Rock) && e.alive)
            .map(|(id, _)| *id)
            .collect();
        let enemy_ids: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Enemy) && e.alive)
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

        // Player ↔ rock: player dies, rock keeps going.
        for rock_id in &rock_ids {
            for player_id in &player_ids {
                if let (Some(p), Some(r)) = (self.entities.get(player_id), self.entities.get(rock_id)) {
                    if p.alive && r.alive && physics::circles_overlap(p.pos, p.bbox, r.pos, r.bbox) {
                        let pid = match p.kind {
                            EntityKind::Player { player_id } => player_id,
                            _ => continue,
                        };
                        if let Some(p) = self.entities.get_mut(player_id) {
                            p.alive = false;
                        }
                        events.push(GameEvent::PlayerKilled(pid));
                    }
                }
            }
        }

        // Player shot ↔ rock: both die, owner scores.
        for rock_id in &rock_ids {
            for shot_id in &player_shot_ids {
                let hit = match (self.entities.get(shot_id), self.entities.get(rock_id)) {
                    (Some(s), Some(r)) if s.alive && r.alive => {
                        physics::circles_overlap(s.pos, s.bbox, r.pos, r.bbox)
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
                let pos = self.entities.get(rock_id).map(|r| r.pos).unwrap_or(Vec2::ZERO);
                if let Some(s) = self.entities.get_mut(shot_id) {
                    s.alive = false;
                }
                if let Some(r) = self.entities.get_mut(rock_id) {
                    r.alive = false;
                }
                *self.score_by_player.entry(owner_pid).or_insert(0) += 1;
                events.push(GameEvent::RockKilled { pos, killer: owner_pid });
                break;
            }
        }

        // Player ↔ enemy: ramming kills both. The player gets credit for
        // the enemy kill so suicide-runs still count.
        for enemy_id in &enemy_ids {
            for player_id in &player_ids {
                let hit = match (self.entities.get(player_id), self.entities.get(enemy_id)) {
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
                let pos = self.entities.get(enemy_id).map(|e| e.pos).unwrap_or(Vec2::ZERO);
                if let Some(p) = self.entities.get_mut(player_id) {
                    p.alive = false;
                }
                if let Some(e) = self.entities.get_mut(enemy_id) {
                    e.alive = false;
                }
                *self.score_by_player.entry(pid).or_insert(0) += 1;
                events.push(GameEvent::PlayerKilled(pid));
                events.push(GameEvent::EnemyKilled { pos, killer: pid });
            }
        }

        // Player shot ↔ enemy: enemy dies, owner scores.
        for enemy_id in &enemy_ids {
            for shot_id in &player_shot_ids {
                let hit = match (self.entities.get(shot_id), self.entities.get(enemy_id)) {
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
                let pos = self.entities.get(enemy_id).map(|e| e.pos).unwrap_or(Vec2::ZERO);
                if let Some(s) = self.entities.get_mut(shot_id) {
                    s.alive = false;
                }
                if let Some(e) = self.entities.get_mut(enemy_id) {
                    e.alive = false;
                }
                *self.score_by_player.entry(owner_pid).or_insert(0) += 1;
                events.push(GameEvent::EnemyKilled { pos, killer: owner_pid });
                break;
            }
        }

        // Enemy shot ↔ player: shot dies, player dies.
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
                if let Some(p) = self.entities.get_mut(player_id) {
                    p.alive = false;
                }
                events.push(GameEvent::PlayerKilled(pid));
                break;
            }
        }
    }

    /// Spawn one enemy somewhere along the top half of the world, well
    /// away from the player spawn point. Position is drawn from the world
    /// RNG so this is deterministic.
    fn spawn_enemy(&mut self) {
        let world = self.config.world_size;
        let center = world * 0.5;
        // Pick a random angle in the upper half-plane (between 30° and
        // 150° measuring from +X) so the enemy starts above the play area
        // rather than sitting next to the player.
        let span = std::f32::consts::PI * (2.0 / 3.0);
        let base = std::f32::consts::PI / 6.0;
        let angle = base + util::rand_unit(&mut self.rng) * span;
        let radius = world.y * 0.4;
        let mut pos = center + Vec2::new(angle.cos(), angle.sin()) * radius;
        pos.x = util::wrap_coord(pos.x, world.x);
        pos.y = pos.y.clamp(20.0, world.y - 20.0);
        let id = self.alloc_id();
        let enemy = Entity::enemy(id, pos);
        self.entities.insert(id, enemy);
    }

    fn spawn_rocks(&mut self, count: i32) {
        let center = self.config.world_size * 0.5;
        for _ in 0..count {
            let r_angle = util::rand_unit(&mut self.rng) * 2.0 * std::f32::consts::PI;
            let r_distance = util::rand_unit(&mut self.rng)
                * (ROCK_SPAWN_MAX_RADIUS - ROCK_SPAWN_MIN_RADIUS)
                + ROCK_SPAWN_MIN_RADIUS;
            let pos = center + util::vec_from_angle(r_angle) * r_distance;
            let vel = util::random_vec(&mut self.rng, ROCK_MAX_VEL);
            let id = self.alloc_id();
            let rock = Entity::rock(id, pos, vel);
            self.entities.insert(id, rock);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PlayerInput;

    #[test]
    fn world_spawns_initial_rocks() {
        let world = World::new(WorldConfig::default());
        let rocks: Vec<_> = world
            .entities()
            .filter(|e| matches!(e.kind, EntityKind::Rock))
            .collect();
        assert_eq!(rocks.len(), ROCKS_PER_LEVEL_BASE as usize);
    }

    #[test]
    fn world_spawns_one_enemy_at_start() {
        let world = World::new(WorldConfig::default());
        let enemies: Vec<_> = world
            .entities()
            .filter(|e| matches!(e.kind, EntityKind::Enemy))
            .collect();
        assert_eq!(enemies.len(), 1, "expected exactly one starting enemy");
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
    fn respawn_survives_first_tick_with_a_rock_overhead() {
        // End-to-end: rock parked on the spawn point, player added, one
        // tick. The player must still be alive afterwards — that's the
        // condition that makes "press R to restart" actually restart the
        // game from the user's perspective.
        let mut world = World::new(WorldConfig::default());
        let center = Vec2::new(WORLD_WIDTH, WORLD_HEIGHT) * 0.5;
        let rock_id = *world
            .entities_map()
            .keys()
            .next()
            .expect("world should start with rocks");
        let rock = world.entities.get_mut(&rock_id).unwrap();
        rock.pos = center;
        rock.vel = Vec2::ZERO;

        let pid = PlayerId(7);
        world.add_player(pid);
        let _ = world.tick(&PlayerInputs::new(), crate::TICK_DT);

        assert!(
            world.player_entity(pid).is_some(),
            "player should survive the tick after a safe respawn",
        );
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
    fn rocks_persist_when_no_players_present() {
        // World starts with rocks and zero players; ticking should not error.
        let mut world = World::new(WorldConfig::default());
        let inputs = PlayerInputs::new();
        for _ in 0..30 {
            let _ = world.tick(&inputs, crate::TICK_DT);
        }
        let rock_count = world
            .entities()
            .filter(|e| matches!(e.kind, EntityKind::Rock))
            .count();
        assert!(rock_count > 0);
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
