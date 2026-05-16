//! Spawn scheduling: drives level progression and decides when to push new
//! hostiles into the world. Decoupled from `World` so each new enemy type
//! plugs in via `SpawnRequest` instead of editing collision/movement code.
//!
//! `WaveDirector::step` is called once per `World::tick` whenever at least
//! one player is in the world. It owns three timers:
//!   - `level_elapsed` — accumulates real (sim) seconds; rolls the level
//!     forward every `level_duration(level)`.
//!   - `enemy_spawn_timer` — countdown to the next enemy pulse, recharged
//!     to `enemy_spawn_interval(level)` after firing.
//!   - `tank_spawn_timer` — same as above but gated on `level >=
//!     TANK_START_LEVEL` and using a slower interval so tanks stay
//!     scarcer than ships.

/// Baseline duration of level 1 in seconds. The actual duration grows
/// linearly with level (see `level_duration`) so the opening levels turn
/// over fast — the player feels progress quickly — and later levels last
/// longer so the climbing difficulty has room to breathe.
pub const LEVEL_DURATION_BASE_SECS: f32 = 18.0;

/// Seconds added to the level duration per level past the first. Pairs
/// with `LEVEL_DURATION_BASE_SECS` to produce L1=18s, L3=25s, L5=32s,
/// roughly doubling between L1 and L10.
pub const LEVEL_DURATION_GROWTH_SECS: f32 = 3.5;

/// Upper bound on a single level's duration. Above this the curve flattens
/// out so very high levels stop dragging on indefinitely.
pub const LEVEL_DURATION_MAX_SECS: f32 = 60.0;

/// Ship enemies present in the world the moment a fresh game starts.
pub const INITIAL_ENEMY_COUNT: i32 = 2;

/// Ship enemies spawned per pulse once the game is running.
pub const ENEMIES_PER_SPAWN: i32 = 2;

/// Tanks spawned per pulse once tanks have unlocked (`level >=
/// TANK_START_LEVEL`).
pub const TANKS_PER_SPAWN: i32 = 1;

/// Level at which tanks begin spawning. The early levels are pure dogfights
/// so the player has a chance to learn the controls before artillery
/// shows up.
pub const TANK_START_LEVEL: i32 = 3;

/// Seconds between ship spawn pulses at level 1.
pub const INITIAL_SPAWN_INTERVAL_SECS: f32 = 10.0;

/// Floor for `enemy_spawn_interval`. Without this, the formula keeps
/// pulling the interval down towards zero at very high levels and turns
/// the game into a meat grinder.
pub const MIN_SPAWN_INTERVAL_SECS: f32 = 2.5;

/// Per-level coefficient on the spawn-interval denominator. Smaller =
/// gentler ramp. With this value the L1→L2 interval shrinks ~17% (10s →
/// 8.5s) instead of the ~40% you'd get from a logarithmic curve. Tuned
/// to feel like a slow squeeze of pressure rather than a cliff.
pub const SPAWN_RAMP_PER_LEVEL: f32 = 0.18;

/// Tanks spawn at `enemy_spawn_interval * TANK_INTERVAL_FACTOR`, so they
/// stay roughly half as common as ship enemies.
pub const TANK_INTERVAL_FACTOR: f32 = 2.0;

/// Hard cap on simultaneous airborne ship enemies. Late levels can still
/// stack pressure via overlapping spawns, but never beyond this number.
pub const ENEMIES_MAX_ALIVE: i32 = 12;

/// Hard cap on simultaneous tanks on the ground.
pub const TANKS_MAX_ALIVE: i32 = 4;

/// Snapshot of current alive hostiles, fed in by `World` so the director
/// can respect the per-kind caps without owning the entity table itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct AliveCounts {
    pub enemies: i32,
    pub tanks: i32,
}

/// One pulse request emitted by the director. `World` consumes the list
/// and turns each entry into a concrete spawn through the kind-specific
/// helpers (`spawn_enemy`, `spawn_tank`). New enemy kinds extend this
/// enum; the dispatch in `World::tick` matches on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnRequest {
    Enemy,
    Tank,
}

/// What `step` decided this tick. `level_up` is `Some(new_level)` exactly
/// once on each transition so `World` can emit `LevelUp` without having
/// to diff levels itself.
#[derive(Debug, Default, Clone)]
pub struct DirectorStep {
    pub level_up: Option<i32>,
    pub spawns: Vec<SpawnRequest>,
}

/// Level/spawn pacing state. One per `World`. Reset on respawn so a
/// player who dies and rejoins gets the level-1 ramp again.
#[derive(Debug, Clone)]
pub struct WaveDirector {
    level_elapsed: f32,
    enemy_spawn_timer: f32,
    tank_spawn_timer: f32,
}

impl WaveDirector {
    pub fn new() -> Self {
        Self {
            level_elapsed: 0.0,
            // First spawn pulse fires one full interval after game start;
            // the level-1 initial wave covers the opening moments.
            enemy_spawn_timer: INITIAL_SPAWN_INTERVAL_SECS,
            // Tanks don't tick until level 3; this is a placeholder so the
            // first tank pulse happens roughly one interval into level 3.
            tank_spawn_timer: tank_spawn_interval(TANK_START_LEVEL),
        }
    }

    /// Hard reset — same shape as `new`. Called from `World::respawn_player`
    /// alongside the level reset so the difficulty curve restarts.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Advance timers by `dt` and emit any level-up / spawn requests.
    /// Caller must skip this when there are no players in the world.
    pub fn step(&mut self, level: i32, dt: f32, alive: AliveCounts) -> DirectorStep {
        let mut out = DirectorStep::default();

        // Level progression. We only emit one bump per call even if a huge
        // dt would have crossed multiple boundaries — in practice dt is
        // 1/60, so this is just defensive.
        self.level_elapsed += dt;
        let mut effective_level = level;
        let duration = level_duration(level);
        if self.level_elapsed >= duration {
            self.level_elapsed -= duration;
            effective_level = level + 1;
            out.level_up = Some(effective_level);
        }

        // Ship enemy pulse.
        self.enemy_spawn_timer -= dt;
        if self.enemy_spawn_timer <= 0.0 {
            let room = (ENEMIES_MAX_ALIVE - alive.enemies).max(0);
            let to_spawn = ENEMIES_PER_SPAWN.min(room);
            for _ in 0..to_spawn {
                out.spawns.push(SpawnRequest::Enemy);
            }
            // Recharge using the post-level-up interval so the new pace
            // takes effect immediately.
            self.enemy_spawn_timer = enemy_spawn_interval(effective_level);
        }

        // Tanks unlock at `TANK_START_LEVEL`. Below that we hold the timer
        // at a sensible default so the first tick of level 3 doesn't
        // immediately dump a tank.
        if effective_level >= TANK_START_LEVEL {
            self.tank_spawn_timer -= dt;
            if self.tank_spawn_timer <= 0.0 {
                let room = (TANKS_MAX_ALIVE - alive.tanks).max(0);
                let to_spawn = TANKS_PER_SPAWN.min(room);
                for _ in 0..to_spawn {
                    out.spawns.push(SpawnRequest::Tank);
                }
                self.tank_spawn_timer = tank_spawn_interval(effective_level);
            }
        } else {
            // Keep the timer "armed" so when tanks unlock the first pulse
            // is roughly one interval into the new regime, not instant.
            self.tank_spawn_timer = tank_spawn_interval(TANK_START_LEVEL);
        }

        out
    }
}

impl Default for WaveDirector {
    fn default() -> Self {
        Self::new()
    }
}

/// Seconds between ship spawn pulses at `level`. Linear-decay curve so
/// the very first jump is small (L1 = 10s → L2 ≈ 8.5s) but the floor is
/// hit only after a long climb (~L20). Each additional level adds
/// `SPAWN_RAMP_PER_LEVEL` to the denominator.
pub fn enemy_spawn_interval(level: i32) -> f32 {
    let extra = (level.max(1) - 1) as f32;
    (INITIAL_SPAWN_INTERVAL_SECS / (1.0 + extra * SPAWN_RAMP_PER_LEVEL))
        .max(MIN_SPAWN_INTERVAL_SECS)
}

/// Seconds between tank spawn pulses at `level`. Same curve as ships,
/// just stretched by `TANK_INTERVAL_FACTOR` so tanks stay rarer.
pub fn tank_spawn_interval(level: i32) -> f32 {
    enemy_spawn_interval(level) * TANK_INTERVAL_FACTOR
}

/// Wall-clock seconds the current `level` lasts before the next one
/// kicks in. Short on the first level so the pilot feels progression
/// quickly, then grows linearly to give later (harder) levels more
/// breathing room. Capped at `LEVEL_DURATION_MAX_SECS`.
pub fn level_duration(level: i32) -> f32 {
    let extra = (level.max(1) - 1) as f32;
    (LEVEL_DURATION_BASE_SECS + extra * LEVEL_DURATION_GROWTH_SECS)
        .min(LEVEL_DURATION_MAX_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive(enemies: i32, tanks: i32) -> AliveCounts {
        AliveCounts { enemies, tanks }
    }

    #[test]
    fn interval_shrinks_with_level_and_clamps_at_floor() {
        let l1 = enemy_spawn_interval(1);
        let l2 = enemy_spawn_interval(2);
        let l10 = enemy_spawn_interval(10);
        assert!((l1 - INITIAL_SPAWN_INTERVAL_SECS).abs() < 1e-3);
        assert!(l2 < l1, "interval should drop from level 1 to 2");
        assert!(l10 < l2, "interval should keep dropping as level grows");
        // Eventually clamps at the floor.
        let l_high = enemy_spawn_interval(10_000);
        assert!((l_high - MIN_SPAWN_INTERVAL_SECS).abs() < 1e-3);
    }

    #[test]
    fn early_levels_have_gentle_difficulty_jumps() {
        // The complaint about the old log curve was that L1→L2 dropped the
        // spawn interval by ~40%. The new linear-decay curve should keep
        // that opening jump under 25% so the pilot doesn't get hit with a
        // wall of new enemies the moment level 1 ends.
        let l1 = enemy_spawn_interval(1);
        let l2 = enemy_spawn_interval(2);
        let drop = (l1 - l2) / l1;
        assert!(
            drop < 0.25,
            "L1→L2 interval drop should be gentle, got {:.0}%",
            drop * 100.0
        );
    }

    #[test]
    fn level_duration_grows_with_level_and_caps() {
        let d1 = level_duration(1);
        let d3 = level_duration(3);
        let d10 = level_duration(10);
        assert!((d1 - LEVEL_DURATION_BASE_SECS).abs() < 1e-3);
        assert!(d3 > d1, "later levels should last longer than level 1");
        assert!(d10 > d3);
        // High levels saturate at the cap.
        let d_high = level_duration(10_000);
        assert!((d_high - LEVEL_DURATION_MAX_SECS).abs() < 1e-3);
    }

    #[test]
    fn first_enemy_pulse_fires_after_initial_interval() {
        let mut d = WaveDirector::new();
        let dt = 1.0 / 60.0;
        // No spawn pulse should fire well before the interval. We give a
        // few-tick buffer either side of the boundary because f32 timer
        // drift can shave a couple of ticks off the nominal interval.
        let early_steps = (INITIAL_SPAWN_INTERVAL_SECS / dt) as i32 - 5;
        for _ in 0..early_steps {
            let s = d.step(1, dt, alive(0, 0));
            assert!(s.spawns.is_empty(), "no spawn expected before interval");
        }
        // Run a generous window past the nominal boundary and confirm we
        // see exactly one ENEMIES_PER_SPAWN pulse worth of requests.
        let mut total = 0;
        for _ in 0..20 {
            let s = d.step(1, dt, alive(0, 0));
            total += s
                .spawns
                .iter()
                .filter(|r| matches!(r, SpawnRequest::Enemy))
                .count();
        }
        assert_eq!(total, ENEMIES_PER_SPAWN as usize);
    }

    #[test]
    fn tanks_skip_below_start_level_and_appear_at_or_above() {
        let mut d = WaveDirector::new();
        // Run long enough to fire many enemy pulses but stay below the
        // tank unlock level by passing `level = 1` every tick.
        let dt = 1.0 / 60.0;
        let mut saw_tank = false;
        for _ in 0..(60 * 60) {
            let s = d.step(1, dt, alive(0, 0));
            if s.spawns.iter().any(|r| matches!(r, SpawnRequest::Tank)) {
                saw_tank = true;
            }
        }
        assert!(!saw_tank, "no tanks before level {TANK_START_LEVEL}");

        // Now run at the unlock level; we should get a tank within a
        // generous window (one tank interval + slack).
        let mut d2 = WaveDirector::new();
        let mut saw_tank = false;
        let budget = ((tank_spawn_interval(TANK_START_LEVEL) + 1.0) / dt).ceil() as i32;
        for _ in 0..budget {
            let s = d2.step(TANK_START_LEVEL, dt, alive(0, 0));
            if s.spawns.iter().any(|r| matches!(r, SpawnRequest::Tank)) {
                saw_tank = true;
                break;
            }
        }
        assert!(saw_tank, "tank pulse should fire once level >= {TANK_START_LEVEL}");
    }

    #[test]
    fn level_advances_after_level_duration() {
        let mut d = WaveDirector::new();
        let dt = 1.0 / 60.0;
        // Generous budget to swallow accumulated f32 timer drift over
        // the per-level tick count.
        let steps = (level_duration(1) / dt).ceil() as i32 + 30;
        let mut bumped_to: Option<i32> = None;
        for _ in 0..steps {
            let s = d.step(1, dt, alive(0, 0));
            if let Some(l) = s.level_up {
                bumped_to = Some(l);
                break;
            }
        }
        assert_eq!(bumped_to, Some(2));
    }

    #[test]
    fn cap_blocks_enemy_pulses_when_world_is_full() {
        let mut d = WaveDirector::new();
        let dt = 1.0 / 60.0;
        // Burn the initial interval so the timer wants to fire.
        let steps = (INITIAL_SPAWN_INTERVAL_SECS / dt).ceil() as i32;
        for _ in 0..steps - 1 {
            d.step(1, dt, alive(ENEMIES_MAX_ALIVE, 0));
        }
        let s = d.step(1, dt, alive(ENEMIES_MAX_ALIVE, 0));
        // Timer fires, but capacity is zero so no requests are issued.
        assert!(s.spawns.iter().all(|r| !matches!(r, SpawnRequest::Enemy)));
    }
}
