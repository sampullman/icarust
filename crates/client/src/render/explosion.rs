//! Particle-burst explosions used for player crashes. Pure cosmetic — no
//! sim coupling, no shared RNG. Each explosion owns a small bag of
//! particles that integrate locally and fade out.
//!
//! `ExplosionStyle` picks the visuals: which colors, how heavy the
//! debris, gravity strength, lifetime. New `DeathCause` variants (e.g.
//! water splash, hill scrape) add a new style; everything else stays the
//! same.

use ggez::glam::Vec2;
use ggez::graphics::Color;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sim::DeathCause;
use sim::TerrainKind;

use crate::render::camera::Camera;
use crate::render::instance_batch::InstanceQuadBatch;

/// How a given crash looks. Map `DeathCause` → `ExplosionStyle`. Adding a
/// new terrain kind only changes the `for_cause` mapping.
#[derive(Debug, Clone, Copy)]
pub enum ExplosionStyle {
    /// Generic mid-air burst — orange embers + bright core. Used for
    /// enemies and enemy shots.
    FieryBurst,
    /// Ground impact — fewer hot embers, plus brown dust kicked up.
    DustAndEmbers,
}

impl ExplosionStyle {
    pub fn for_cause(cause: &DeathCause) -> Self {
        match cause {
            DeathCause::Terrain(TerrainKind::Ground) => ExplosionStyle::DustAndEmbers,
            DeathCause::Enemy | DeathCause::EnemyShot => ExplosionStyle::FieryBurst,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Particle {
    /// World coords (Y-up).
    pos: Vec2,
    /// World units per second (Y-up — positive y goes upward visually).
    vel: Vec2,
    life: f32,
    max_life: f32,
    color: Color,
    /// Particle radius in world units.
    radius: f32,
    /// Acceleration applied each frame, world units / s². Y-up, so a
    /// negative y accelerates downward.
    accel: Vec2,
}

pub struct Explosion {
    particles: Vec<Particle>,
    /// Total seconds elapsed since spawn, just for `done()`.
    age: f32,
}

const GRAVITY: f32 = 240.0;

impl Explosion {
    pub fn new(pos: Vec2, style: ExplosionStyle, seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let particles = match style {
            ExplosionStyle::FieryBurst => burst_particles(pos, &mut rng, false),
            ExplosionStyle::DustAndEmbers => {
                let mut p = burst_particles(pos, &mut rng, true);
                p.extend(dust_particles(pos, &mut rng));
                p
            }
        };
        Explosion { particles, age: 0.0 }
    }

    pub fn update(&mut self, dt: f32) {
        self.age += dt;
        for p in &mut self.particles {
            p.life -= dt;
            p.vel += p.accel * dt;
            p.pos += p.vel * dt;
        }
        self.particles.retain(|p| p.life > 0.0);
    }

    pub fn done(&self) -> bool {
        self.particles.is_empty()
    }

    pub fn fill(&self, batch: &mut InstanceQuadBatch, camera: &Camera) {
        for p in &self.particles {
            // Fade alpha to zero over the back half of the lifetime.
            let t = (p.life / p.max_life).clamp(0.0, 1.0);
            let alpha = (t * 1.6).min(1.0);
            let mut color = p.color;
            color.a = alpha;
            batch.push_world(camera, p.pos, p.radius, color);
        }
    }
}

fn burst_particles(pos: Vec2, rng: &mut ChaCha8Rng, ground_biased: bool) -> Vec<Particle> {
    let mut out = Vec::with_capacity(28);
    // Bright hot core: a handful of fast, short-lived white/yellow sparks.
    for _ in 0..8 {
        let angle = rng.gen::<f32>() * std::f32::consts::TAU;
        // For ground crashes, bias the spread upward so debris looks
        // like it's bouncing off the floor instead of going down through it.
        let speed = 120.0 + rng.gen::<f32>() * 120.0;
        let mut vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        if ground_biased && vel.y < 0.0 {
            vel.y = -vel.y;
        }
        let life = 0.18 + rng.gen::<f32>() * 0.12;
        out.push(Particle {
            pos,
            vel,
            life,
            max_life: life,
            color: Color::new(1.0, 0.95, 0.65, 1.0),
            radius: 2.0 + rng.gen::<f32>() * 1.5,
            accel: Vec2::new(0.0, -GRAVITY * 0.4),
        });
    }
    // Orange embers: more numerous, slower, longer-lived.
    for _ in 0..18 {
        let angle = rng.gen::<f32>() * std::f32::consts::TAU;
        let speed = 60.0 + rng.gen::<f32>() * 110.0;
        let mut vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        if ground_biased && vel.y < 0.0 {
            vel.y *= -0.4;
        }
        let life = 0.45 + rng.gen::<f32>() * 0.45;
        let r = 0.85 + rng.gen::<f32>() * 0.15;
        let g = 0.30 + rng.gen::<f32>() * 0.25;
        out.push(Particle {
            pos,
            vel,
            life,
            max_life: life,
            color: Color::new(r, g, 0.10, 1.0),
            radius: 1.5 + rng.gen::<f32>() * 1.5,
            accel: Vec2::new(0.0, -GRAVITY),
        });
    }
    out
}

fn dust_particles(pos: Vec2, rng: &mut ChaCha8Rng) -> Vec<Particle> {
    let mut out = Vec::with_capacity(20);
    for _ in 0..20 {
        // Dust kicks out sideways in a low arc — angles in the upper
        // hemisphere only (sin > 0) so it sprays up off the surface.
        let angle = rng.gen::<f32>() * std::f32::consts::PI;
        let speed = 30.0 + rng.gen::<f32>() * 80.0;
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        let life = 0.6 + rng.gen::<f32>() * 0.5;
        let brown = 0.40 + rng.gen::<f32>() * 0.15;
        out.push(Particle {
            pos,
            vel,
            life,
            max_life: life,
            color: Color::new(brown, brown * 0.65, brown * 0.35, 1.0),
            radius: 2.0 + rng.gen::<f32>() * 2.0,
            accel: Vec2::new(0.0, -GRAVITY * 0.6),
        });
    }
    out
}
