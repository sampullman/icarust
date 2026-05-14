# Icarust — Multiplayer / Web Migration Plan

Plan for moving Icarust from a single-binary native ggez game to a deterministic, server-authoritative multiplayer game that runs in the browser via the ggez `web` branch.

## Goals

- Server owns the truth. Clients ship inputs and render snapshots — nothing more.
- Same Rust simulation runs on the server and (for replay/debug) on the client. Deterministic.
- One client codebase builds for native and `wasm32-unknown-unknown` against ggez's `web` branch.
- Transport: WebSockets only. Same protocol on native and web.

## Constraints worth naming up front

The current code mixes simulation with `ggez::Context` and `AssetManager`. Examples:

- `Player::fire_shot(ctx, am)` constructs a `Shot` and triggers audio mid-physics (`src/actors/player.rs:116`).
- `create_rocks(ctx, am, …)` allocates `Sprite`s while spawning rocks (`src/actors/rock.rs:42`).
- `MainState::handle_collisions` calls `play_sound(ctx, …)` from inside the physics step (`src/main.rs:142`).
- `BaseActor` itself carries a `Sprite` field (`src/actors/mod.rs:14`).

None of that can run on a headless server, and none of it survives the client/server split. The simulation has to be extracted as a pure module before networking is meaningful. That extraction is the real work; the network code on top of it is small.

`rand::random()` is also called directly from `create_rocks` and `random_vec`. Both must take an explicit RNG so the world state is reproducible from `(seed, input_history)`.

## Target layout

Cargo workspace with four crates:

```
crates/
├── sim/        # pure simulation. no ggez, no I/O, no global RNG.
│               # World, Entity, EntityKind, EntityId, Tick, PlayerId,
│               # PlayerInput, GameEvent. Deterministic.
├── protocol/   # ClientMsg / ServerMsg, serde + postcard. Shared, no_std-friendly.
├── server/     # tokio + tokio-tungstenite. Owns World, runs 60 Hz tick,
│               # broadcasts snapshots ~20 Hz and events as they fire.
└── client/     # ggez (web branch), native + wasm32. Captures input,
                # ships to server, renders interpolated snapshots, plays audio
                # off event stream.
```

`sim` and `protocol` are the only crates compiled by both server and client; they must stay free of `ggez`, `tokio`, and platform-specific deps.

## `sim` crate — pure deterministic simulation

Replace the asset-bearing actors with pure entities:

```rust
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,        // Player { player_id }, Rock, Shot { owner }
    pub pos: Vec2,
    pub vel: Vec2,
    pub facing: f32,
    pub bbox: f32,
    pub alive: bool,
    pub ttl: Option<f32>,        // shots
    pub shot_cooldown: f32,      // players
}
```

Concrete moves:

- `actors::{player,rock,shot}` → `sim::entities`. Drop the `Sprite` field; drop sound IDs. Keep the `BaseActor` / `HasBase` trait pattern minus the asset.
- `apply_input` and `apply_forces` (`src/actors/player.rs:86`, `:101`) are already pure — move verbatim. Existing tests come along.
- `physics::collides` / `circles_overlap` move unchanged.
- `MainState::{handle_collisions, clear_dead_stuff, check_for_level_respawn}` become methods on `sim::World`.
- `MainState::update`'s 60 Hz loop body becomes `World::tick(&PlayerInputs, dt) -> Vec<GameEvent>` where `dt` is fixed at `1.0 / 60.0`.

### Determinism

Non-negotiable from day one:

- `World` owns a `rand_chacha::ChaCha8Rng` seeded from `World::new(seed)`. All randomness — rock spawn angles, distances, velocities — goes through it. No more `rand::random`.
- `random_vec` and `create_rocks` take `&mut impl RngCore`.
- Entities are stored in a `BTreeMap<EntityId, Entity>` (or a `Vec` indexed by a monotonic `EntityId`) so iteration order is identical across machines. No `HashMap` over entities.
- `PlayerInputs` is a `BTreeMap<PlayerId, PlayerInput>` for the same reason.
- `Tick(u64)` is monotonic. Every tick is `(prev_world, inputs_for_tick) -> next_world + events`. Pure.
- Floats: stick with `f32`, accept that strict bit-for-bit determinism across architectures isn't guaranteed. We're doing snapshot replication, not lockstep, so this is acceptable. The discipline still pays off for replays, server restart-from-snapshot, and tests.

A unit test should run `World::tick` N times against a recorded input log and assert the entity hash matches a golden value.

## `protocol` crate

`serde` + `postcard` (compact, `no_std`, builds clean for wasm).

```rust
// Client → Server
enum ClientMsg {
    Hello { name: String },
    Input { tick: Tick, input: PlayerInput },   // sent every client tick
    Bye,
}

// Server → Client
enum ServerMsg {
    Welcome { player_id: PlayerId, seed: u64, snapshot: Snapshot },
    Snapshot(Snapshot),                          // ~20 Hz
    Events { tick: Tick, events: Vec<GameEvent> }, // as they fire
}

struct Snapshot {
    tick: Tick,
    entities: Vec<EntityState>,                  // pos, vel, facing, kind, id, alive
    score_by_player: Vec<(PlayerId, i32)>,
    level: i32,
}

enum GameEvent {
    PlayerJoined(PlayerId),
    PlayerLeft(PlayerId),
    ShotFired { owner: PlayerId, pos: Vec2 },
    RockKilled { pos: Vec2 },
    PlayerKilled(PlayerId),
    LevelUp(i32),
}
```

Snapshot at 20 Hz (every 3rd server tick). Events at the moment they happen so the client can fire `pew.ogg` / `boom.ogg` without diffing snapshots. Keep the wire format trivial — no delta encoding, no quantization. Tens of entities, easily under a few KB per snapshot.

## `server` crate

Pure Rust, no ggez. Runtime: `tokio`. Transport: `tokio-tungstenite`.

```
main
├── accept loop: per-connection task
│   reads ClientMsg, writes per-player input slot via mpsc
│   forwards ServerMsg from a broadcast::Sender
└── game loop task
    tokio::time::interval(1/60s)
    each tick:
      1. drain input mpsc → BTreeMap<PlayerId, PlayerInput>
      2. world.tick(&inputs, 1.0/60.0) → Vec<GameEvent>
      3. broadcast Events
      4. if tick % 3 == 0: broadcast Snapshot
```

One global room for v1. Player join → spawn ship at world center, allocate `PlayerId`, broadcast `PlayerJoined`. Disconnect → mark dead, broadcast `PlayerLeft`, remove next tick.

## `client` crate

`MainState` shrinks substantially. What stays:

- `AssetManager`, `Camera`, `TextWidget`s.
- `InputState` (`src/input.rs`) — already produces `xaxis/yaxis/fire` cleanly.
- `draw_actor` / `draw_actor_wrapped`.

What's added:

- `Net` trait with two impls: `tokio-tungstenite` on native, `gloo-net::websocket` on wasm. The trait is the single boundary — nothing else in the client knows about the platform.
- Snapshot ring buffer (last 3 snapshots).
- `local_player_id`, set from `Welcome`.
- A render delay constant (~100 ms) used to interpolate between snapshots.

What's removed: every `Player`/`Rock`/`Shot` field on `MainState`, all physics calls in `update`, all of `handle_collisions`, all of `check_for_level_respawn`. Done in the simulation now.

```
update():
  drain incoming ServerMsg
    Welcome      → store player_id, seed (kept for replay only), bootstrap from snapshot
    Snapshot(s)  → push into ring buffer
    Events(es)   → for each event, play sound / fire animation hook
  send ClientMsg::Input { tick, input } from current InputState
  (no physics)

draw():
  pick the two snapshots straddling (now - render_delay)
  for each entity id present in both, lerp pos/facing
  draw via the existing draw_actor codepath, sprite chosen by entity.kind
```

Ship audio fires off the `Events` stream, not by diffing snapshots — events carry the position so spatialization is possible later.

### Native vs wasm wiring

- Native: tokio runtime on a background thread, `crossbeam::channel` (or `tokio::sync::mpsc` polled non-blockingly) into the ggez main thread.
- Wasm: `wasm_bindgen_futures::spawn_local` for the WS task, `futures::channel::mpsc` to deliver `ServerMsg`s into the ggez `update` loop.

Both are hidden behind `Net`. The client's `update`/`draw` see the same `&mut dyn Net`.

## ggez `web` branch

Local checkout: `/home/sama/git/personal/ggez`, currently on `web`. We're free to patch it. If a patch needs to be visible to CI or other machines, ping for a push to the GitHub remote.

`Cargo.toml` for the `client` crate:

```toml
ggez = { path = "../../../ggez" }   # development; switch to git branch="web" later
```

Audio works on the web branch — no fallback needed. Keep `pew.ogg` / `boom.ogg` wired through `AssetManager::play_sound`, fire from the event handler in `update`.

Wasm build via `trunk`:

```
client/
├── Cargo.toml
├── index.html           # trunk shim, canvas + WS URL
└── src/...
```

Resources: ggez on web reads through a virtual filesystem. The simplest workable approach is `include_bytes!`-ing the runtime assets behind `#[cfg(target_arch = "wasm32")]` and registering them with the asset manager at startup; the native build keeps the existing `add_resource_path` flow.

## Phased rollout

Each phase ends with a working game. No phase leaves the tree broken.

### Phase 1 — Workspace split + sim extraction

No networking. No ggez bump. Goal: client still runs `World::tick` locally and the game looks identical.

1. Convert to a Cargo workspace; create `crates/sim` and `crates/client`. Move `main.rs` and friends into `client`.
2. Move `physics.rs`, `util.rs` (minus print/instructions), the `apply_input`/`apply_forces` functions, and the per-actor data into `sim`. Strip `Sprite` and sound IDs from entities.
3. Introduce `EntityId`, `PlayerId`, `Tick`, `World`. Implement `World::tick(&PlayerInputs, dt) -> Vec<GameEvent>`.
4. Replace `rand::random` with a `ChaCha8Rng` carried by `World`; thread it through `create_rocks` and `random_vec`.
5. Client wraps a single-player `World` with `PlayerId(0)`, drives it from `InputState` each frame, plays sound off the returned events, renders by mapping `EntityKind` to sprites in `AssetManager`.
6. Existing tests move with the code into `sim`. Add a determinism test: replay an input log and hash entity state.

Done when: `cargo run` plays exactly like today, `cargo test -p sim` is green, `MainState` no longer contains physics.

### Phase 2 — Protocol + native server, two clients in the same world

1. Create `crates/protocol` with the message enums and `Snapshot` / `GameEvent` / `EntityState`.
2. Create `crates/server` with a tokio main, a per-connection task, a broadcast game-loop task, and a tick interval. One global room.
3. Add the `Net` trait in `client` with a native `tokio-tungstenite` impl. Client stops driving its own `World`; it sends `Input`, receives `Snapshot`, renders the latest snapshot directly (no interpolation yet — single snapshot, immediate).
4. Two `cargo run -p client` instances against `cargo run -p server` see each other's ships.

Done when: two native clients can join the same room and shoot the same rocks; server is the only place rocks die.

### Phase 3 — Snapshot interpolation + event-driven audio

1. Snapshot ring buffer in client; render at `now - render_delay`. Tune the delay until motion looks smooth at simulated 80 ms RTT.
2. Route audio through `Events` exclusively. Remove any remaining client-side hit detection.
3. HUD shows score per player and level from the snapshot.

Done when: motion is smooth, sounds fire on shots and rock kills, no client-side physics is left anywhere.

### Phase 4 — Wasm client against the same server

1. Switch `client`'s `ggez` dep to the local `web` branch path.
2. Add a wasm `Net` impl using `gloo-net::websocket` and `wasm_bindgen_futures`.
3. Add `index.html` and a `Trunk.toml`. Embed runtime assets via `include_bytes!` under `#[cfg(target_arch = "wasm32")]`.
4. Fix any API drift surfaced by the web branch upgrade. If the fix needs to live in ggez itself, patch the local checkout and request a remote push.

Done when: the wasm client served by `trunk serve` connects to the native `server` and plays alongside a native client.

## Risks worth tracking

- **Determinism slippage.** The discipline (BTreeMap iteration, seeded RNG, no wall-clock reads in `sim`) has to hold up under future contributions. A determinism test in `sim` catches drift early — make it a CI gate from Phase 1.
- **Web-branch API drift.** Native and wasm both build against the same ggez branch, so a breaking change in the web branch breaks native too. Acceptable, but means Phase 4 might force a small native-side fix.
- **Asset loading on wasm.** ggez's web FS is the first real surprise area. Embedding via `include_bytes!` sidesteps it; if we want hot-reloadable assets later, that's a separate piece of work.
- **Tokio on the native client.** A background runtime is fine but the channel between it and the ggez main thread has to be non-blocking — a blocking recv inside `update` will stall the renderer.
