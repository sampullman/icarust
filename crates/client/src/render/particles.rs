//! Local-only particle systems for thrust flames and damage smoke. Pure
//! cosmetic effects driven by the latest snapshot — not authoritative and not
//! deterministic with the server. Integrated on wall-clock dt so they look
//! the same regardless of the fixed-step input cadence.
//!
//! Two emitters:
//!   * `ThrustEmitter` spits a flame trail behind any player whose
//!     `thrusting` flag is set.
//!   * `DamageSmoker` puffs brown smoke from any entity whose HP is below max
//!     (players, ship enemies, tanks). Intensity scales with how hurt they
//!     are; callers pass a per-class multiplier so heavy chassis can read
//!     differently from a wounded ship.

use ggez::glam::Vec2;
use ggez::graphics::Color;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sim::EntityId;
use std::collections::HashMap;

use crate::render::camera::Camera;
use crate::render::entities::{FLAME_CORE_COLOR, FLAME_EDGE_COLOR, SMOKE_COLOR};
use crate::render::instance_batch::InstanceQuadBatch;

/// One particle. Positions/velocities are world-space (Y-up).
#[derive(Debug, Clone, Copy)]
struct Particle {
    pos: Vec2,
    vel: Vec2,
    life: f32,
    max_life: f32,
    /// Particle base color. The alpha channel is overridden each draw
    /// from the remaining-life fraction.
    color: Color,
    radius: f32,
    /// Acceleration applied each frame, world units / s². Y-up — negative
    /// y values pull the particle downward.
    accel: Vec2,
}

impl Particle {
    fn step(&mut self, dt: f32) {
        self.life -= dt;
        self.vel += self.accel * dt;
        self.pos += self.vel * dt;
    }

    fn dead(&self) -> bool {
        self.life <= 0.0
    }

    /// Append this particle's current state to the shared batch. Handles
    /// the wrap-seam fan-out so a single particle near the X edge still
    /// contributes the right number of instances.
    fn fill(&self, batch: &mut InstanceQuadBatch, camera: &Camera) {
        let t = (self.life / self.max_life).clamp(0.0, 1.0);
        let mut color = self.color;
        color.a = (t * 1.4).min(1.0);
        batch.push_world(camera, self.pos, self.radius, color);
    }
}

/// Spawns flame particles behind any player whose `thrusting` flag is set.
/// Tracks emission cadence so a short tap of thrust still produces a few
/// puffs (rather than depending on whether we sampled the right frame).
pub struct ThrustEmitter {
    particles: Vec<Particle>,
    /// Per-entity emission accumulator. We emit one particle per
    /// `1/EMIT_RATE` seconds while thrusting; this carries the remainder
    /// between frames so cadence is independent of fps.
    accumulator: HashMap<EntityId, f32>,
    rng: ChaCha8Rng,
}

const THRUST_EMIT_RATE: f32 = 80.0;

impl ThrustEmitter {
    pub fn new(seed: u64) -> Self {
        Self {
            particles: Vec::new(),
            accumulator: HashMap::new(),
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    /// Tell the emitter that an entity is firing thrust this frame. `pos`
    /// is the ship's world position, `facing` its world-space heading
    /// (radians). Should be called every frame so the accumulator stays
    /// honest.
    pub fn note_thrust(&mut self, id: EntityId, pos: Vec2, facing: f32, dt: f32, thrusting: bool) {
        let emits;
        {
            let acc = self.accumulator.entry(id).or_insert(0.0);
            if !thrusting {
                *acc = 0.0;
                return;
            }
            *acc += dt * THRUST_EMIT_RATE;
            emits = acc.floor() as i32;
            *acc -= emits as f32;
        }
        for _ in 0..emits {
            self.emit_one(pos, facing);
        }
    }

    /// Drop accumulators for entities that no longer exist in the snapshot
    /// so the map doesn't grow without bound.
    pub fn retain_ids<F: Fn(EntityId) -> bool>(&mut self, keep: F) {
        self.accumulator.retain(|id, _| keep(*id));
    }

    fn emit_one(&mut self, pos: Vec2, facing: f32) {
        // Local backward direction: opposite of facing. Use vec_from_angle
        // so we get the same sin/cos convention the sim uses (facing=0
        // means +Y world).
        let forward = sim::vec_from_angle(facing);
        let back = -forward;
        // Spawn a few world-units behind the ship, with a tiny perpendicular
        // jitter so the flame fans out instead of being a single straight line.
        let perp = Vec2::new(forward.y, -forward.x);
        let jitter = (self.rng.gen::<f32>() - 0.5) * 5.0;
        let spawn = pos + back * 12.0 + perp * jitter;
        // Velocity is mostly backwards relative to the ship, with a small
        // random spread to give the trail some life.
        let speed = 110.0 + self.rng.gen::<f32>() * 50.0;
        let spread = (self.rng.gen::<f32>() - 0.5) * 0.45;
        let dir = rotate(back, spread);
        let vel = dir * speed;
        // Two-tier particle: the first ~80% draw as a yellow core, then we
        // taper to an orange edge so the trail looks layered without
        // needing two passes. Pick the color per spawn.
        let hot = self.rng.gen::<f32>() < 0.45;
        let color = if hot { FLAME_CORE_COLOR } else { FLAME_EDGE_COLOR };
        let life = 0.18 + self.rng.gen::<f32>() * 0.18;
        self.particles.push(Particle {
            pos: spawn,
            vel,
            life,
            max_life: life,
            color,
            radius: if hot { 2.2 } else { 3.2 },
            // Slight rising bias so the flame doesn't pull straight down
            // under gravity — looks more like exhaust, less like falling embers.
            accel: Vec2::new(0.0, 30.0),
        });
    }

    pub fn update(&mut self, dt: f32) {
        for p in &mut self.particles {
            p.step(dt);
        }
        self.particles.retain(|p| !p.dead());
    }

    /// Append every live particle to the shared batch. The caller flushes
    /// the batch once, so the cost is one draw call regardless of how many
    /// ships are thrusting.
    pub fn fill(&self, batch: &mut InstanceQuadBatch, camera: &Camera) {
        for p in &self.particles {
            p.fill(batch, camera);
        }
    }
}

/// Soft brown smoke that puffs out of damaged ships. Intensity scales
/// with the fraction of HP missing, so heavier damage produces a heavier
/// trail without needing a separate effect for each tier.
pub struct DamageSmoker {
    particles: Vec<Particle>,
    accumulator: HashMap<EntityId, f32>,
    rng: ChaCha8Rng,
}

impl DamageSmoker {
    pub fn new(seed: u64) -> Self {
        Self {
            particles: Vec::new(),
            accumulator: HashMap::new(),
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    /// Emit smoke whose rate scales with the fraction of HP missing.
    /// `intensity` is a per-entity multiplier on the rate so different
    /// hostile classes can read at different smoke densities without
    /// duplicating the bookkeeping — e.g. tanks pass < 1.0 to keep the
    /// "a little bit of smoke" feel while a player ship trails heavier.
    pub fn note_health(
        &mut self,
        id: EntityId,
        pos: Vec2,
        hp: i16,
        max_hp: i16,
        intensity: f32,
        dt: f32,
    ) {
        if max_hp <= 0 || hp >= max_hp || intensity <= 0.0 {
            self.accumulator.remove(&id);
            return;
        }
        let missing = (max_hp - hp).max(0) as f32 / max_hp as f32;
        // 0–20 puffs/s depending on how hurt we are, scaled by intensity.
        // Healthy → 0; last sliver of HP → constant heavy smoke.
        let rate = (6.0 + missing * 16.0) * intensity;
        let emits;
        {
            let acc = self.accumulator.entry(id).or_insert(0.0);
            *acc += dt * rate;
            emits = acc.floor() as i32;
            *acc -= emits as f32;
        }
        for _ in 0..emits {
            self.emit_one(pos);
        }
    }

    pub fn retain_ids<F: Fn(EntityId) -> bool>(&mut self, keep: F) {
        self.accumulator.retain(|id, _| keep(*id));
    }

    /// One-shot burst when a hit lands. Lets the client react to a
    /// `PlayerDamaged` event without waiting for the steady stream to
    /// catch up.
    pub fn puff_burst(&mut self, pos: Vec2, count: usize) {
        for _ in 0..count {
            self.emit_one(pos);
        }
    }

    /// Bright spark shower for the "moment of impact" feedback. Reads
    /// on top of the brown smoke as a hot orange/yellow shatter so the
    /// player can tell a shot landed even before any HP bar moves.
    /// Sparks are fast, short-lived, and fall under gravity so they
    /// don't linger like the smoke does.
    pub fn spark_burst(&mut self, pos: Vec2, count: usize) {
        for _ in 0..count {
            let angle = self.rng.gen::<f32>() * std::f32::consts::TAU;
            let speed = 110.0 + self.rng.gen::<f32>() * 130.0;
            let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed);
            let life = 0.12 + self.rng.gen::<f32>() * 0.20;
            // Roughly half-and-half mix of hot core sparks and cooler
            // orange edge sparks. Core sparks are tinier and brighter
            // so the center of the burst pops.
            let hot = self.rng.gen::<f32>() < 0.45;
            let color = if hot { FLAME_CORE_COLOR } else { FLAME_EDGE_COLOR };
            self.particles.push(Particle {
                pos,
                vel,
                life,
                max_life: life,
                color,
                radius: if hot { 1.3 } else { 1.8 },
                // Steep gravity so sparks plummet — reinforces the
                // "shrapnel" feel rather than slow drifting smoke.
                accel: Vec2::new(0.0, -360.0),
            });
        }
    }

    fn emit_one(&mut self, pos: Vec2) {
        let angle = self.rng.gen::<f32>() * std::f32::consts::TAU;
        let speed = 18.0 + self.rng.gen::<f32>() * 28.0;
        let life = 0.6 + self.rng.gen::<f32>() * 0.5;
        let vel = Vec2::new(angle.cos() * speed, angle.sin() * speed + 16.0);
        self.particles.push(Particle {
            pos,
            vel,
            life,
            max_life: life,
            color: SMOKE_COLOR,
            radius: 2.6 + self.rng.gen::<f32>() * 1.8,
            // Drift slowly up — smoke rises against gravity.
            accel: Vec2::new(0.0, 24.0),
        });
    }

    pub fn update(&mut self, dt: f32) {
        for p in &mut self.particles {
            p.step(dt);
        }
        self.particles.retain(|p| !p.dead());
    }

    pub fn fill(&self, batch: &mut InstanceQuadBatch, camera: &Camera) {
        for p in &self.particles {
            p.fill(batch, camera);
        }
    }
}

/// Rotate `v` by `angle` radians (screen-space sense, but we treat it as
/// pure 2D so it works in world space too).
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let c = angle.cos();
    let s = angle.sin();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

