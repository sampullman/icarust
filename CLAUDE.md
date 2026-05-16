# Icarust

A 2D Sopwith/Luftrauser-style shoot-'em-up written in Rust on top of
[ggez](https://github.com/ggez/ggez). The player flies a thrust-vector ship
with gravity over a toroidal X / hard-Y world, dogfights enemy planes,
dodges tank artillery, and survives wave after wave for score. The codebase
is split into a deterministic, server-authoritative client/server
architecture; `server-architecture.md` is the original migration plan (now
mostly historical).

## Run / build

```
cargo run -p server                                # binds 127.0.0.1:4015
cargo run -p client                                # connects to ws://127.0.0.1:4015
cargo run -p client -- --connect ws://host:4015 --name alice
ICARUST_LISTEN=0.0.0.0:4015 cargo run -p server    # alt listen addr
ICARUST_SERVER=ws://host:4015 cargo run -p client  # alt server URL
cargo check                                         # fast workspace type-check
cargo test                                          # all crates
```

The client always needs a server. `ggez` is pulled from a pinned git rev
(currently `62af01d5` from the `web` branch, since that's where wasm/WebGPU
support lives). Client resources resolve from
`$CARGO_MANIFEST_DIR/../../resources` when run via cargo, otherwise
`./resources`.

## Web build

Wasm target lives under `web/`. The same `crates/client` builds for both
native (`[[bin]]`) and wasm (`cdylib`); cfg-gates in
`crates/client/src/lib.rs` + `src/net/mod.rs` pick the right entry point
and net impl. The wasm entry is `wasm_start` (registered via
`#[wasm_bindgen(start)]`); the native entry is `native_main`.

```
rustup target add wasm32-unknown-unknown
cargo install --locked --version 0.2.120 wasm-bindgen-cli   # match Cargo.lock
cd web && npm install
npm run build                # debug wasm + wasm-bindgen + resources.zip
npm run build:release        # release wasm (recommended for actual play)
npm run serve                # vite at http://localhost:4010 (strictPort)
npm run dev                  # build + serve in one shot
```

The build:
1. `cargo build -p client --lib --target wasm32-unknown-unknown` (browsers
   need WebGPU — Chrome on Linux may require
   `chrome://flags/#enable-unsafe-webgpu`).
2. `wasm-bindgen --target web` over
   `target/wasm32-unknown-unknown/<profile>/client.wasm`.
3. zips `resources/` into `web/public/resources.zip`; `web/src/runner.js`
   fetches that and stashes it on `window.__GGEZ_RESOURCES_ZIP__` before
   calling `init()`, so ggez's `Filesystem::new_web` can mount it.

The runner dodges a Vite quirk: dynamic imports of JS files under
`publicDir` are blocked even with `@vite-ignore`, so we fetch `client.js`
as text, rewrite the `new URL('client_bg.wasm', import.meta.url)` literal
to an absolute URL, and `import` a `Blob` URL instead.

**Vite stale-public gotcha:** rebuilding while `vite` is running will cause
it to serve the SPA fallback HTML for the new files. Kill and restart vite
after every `build.mjs` run.

The wasm client reads `?server=ws://host:4015&name=alice` from
`location.search`; defaults are `ws://<page-hostname>:4015` (or `wss://`
when the page is https) and `pilot`. The scheme follows `location.protocol`
so an https-terminating proxy like nginx works without query params — note
that only port 4015 forwards WebSocket upgrades in the local nginx setup.

## Verifying the wasm build in a headless browser

ggez 0.10+ is WebGPU-only on the web. Headless browsers don't expose a
WebGPU adapter by default, so the standard Playwright MCP (and
`chromium.launch()`) hits `navigator.gpu.requestAdapter() === null` and
ggez fails with `GraphicsInitializationError`. You have to launch Chromium
with the right flags yourself.

Loop:

1. Start the server: `cargo run -p server` (in the background, or in
   another shell).
2. Build the wasm bundle: `cd web && npm run build` (or `build:release`).
3. Serve it: `npm run serve`. Vite binds 4010 with `strictPort: true`, so
   it'll fail loudly instead of silently sliding to another port.
   Remember the stale-public gotcha above and restart vite after each
   rebuild.
4. Drive it with `web/test-headless.mjs`, a small Playwright runner that
   launches Chromium with the flags Linux needs to expose a WebGPU adapter
   (`--enable-unsafe-webgpu --enable-features=Vulkan --use-angle=vulkan
   --disable-vulkan-fallback-to-gl-for-testing --ignore-gpu-blocklist`).
   Read the file if you need to tweak it.

Run it: `node web/test-headless.mjs http://127.0.0.1:<port>/
/tmp/icarust.png`. If `playwright` isn't already in `web/node_modules`,
install it once: `cd web && npm i -D playwright && npx playwright install
chromium`. Increase `WAIT_MS` (env var) for slow first-frame paths; the
runner defaults to 6000 ms.

### Reading the output

- `webgpu: true, hasAdapter: true` — flags took effect and a WebGPU
  adapter is available. If `hasAdapter` is false, the flags didn't apply
  (wrong Chromium build, or the args were silently rejected). Check
  `chrome://gpu` in a real Chrome if confused.
- A canvas at the configured view size (`1280×540`) means ggez reached its
  first frame. A 300×150 canvas means init bailed before the resize
  observer fired — look at the console for a Rust panic or a `ggez
  build_async: ...` line.
- The screenshot lands on the title menu unless the runner navigates past
  it; play frames need the runner to press Space first.
- `AudioContext was not allowed to start` warnings are expected in
  headless (no user gesture); they don't block rendering.

### Don't bother with the Playwright MCP

The Playwright MCP server provided by Claude Code's tools launches
Chromium without browser args, so it can't expose WebGPU. You'll see
`[WARNING] No available adapters.` and the same
`GraphicsInitializationError`. Use a Node-side runner like the one above
instead.

## Workspace layout

```
crates/
├── sim/        # pure deterministic simulation. No ggez, no I/O, no global RNG.
├── protocol/   # ClientMsg/ServerMsg/Snapshot, postcard wire format. Shared.
├── server/     # tokio + tokio-tungstenite. Owns World, 60 Hz tick, 20 Hz snapshots.
└── client/     # ggez front-end. Sends inputs, renders latest snapshot, plays audio off events.
                # Builds as both a native bin and a wasm cdylib.
resources/      # runtime assets (DejaVuSerif.ttf, pew.ogg, boom.ogg, plus a couple
                # unused legacy pngs).
web/            # wasm build harness: build.mjs, vite config, runner.js, host page.
```

`sim` and `protocol` are the only crates compiled by both server and
client; they must stay free of `ggez`, `tokio`, and platform deps.

### `crates/sim`

```
src/
├── lib.rs       # re-exports + TICK_DT = 1.0/60.0
├── world.rs     # World, WorldConfig, tick(&PlayerInputs, dt) -> Vec<GameEvent>.
│                # Owns entity table, RNG, wave director, terrain.
├── entity.rs    # Entity, EntityId, EntityKind { Player|Shot|Enemy|Tank }, PlayerId, Tick,
│                # ShotOwner { Player|Enemy|Tank }.
├── input.rs     # PlayerInput { xaxis, yaxis, fire }, PlayerInputs (BTreeMap).
├── event.rs     # GameEvent enum + DeathCause.
├── physics.rs   # circles_overlap / collides.
├── player.rs    # apply_input, apply_forces (pure), thrust/gravity/drag/turn constants,
│                # contact-damage ramping, HP regen tuning.
├── enemy.rs     # Ship enemy AI (chase + fire). No gravity.
├── tank.rs      # Ground tank AI: roll-to-range, weave-in-place, parabolic-lead turret.
├── terrain.rs   # GroundProfile heightmap, terrain_hit, surface helpers.
├── wave.rs      # WaveDirector — level timer + per-kind spawn pulses with caps.
└── util.rs      # Vec2 (= glam::Vec2), vec_from_angle, random_vec, rand_unit,
                 # clamp_velocity, wrap_coord, clamp_y/bounce_y, toroidal_offset/
                 # toroidal_distance, signed_angular_delta/steer_toward_angle,
                 # WireVec2 (serde-stable Vec2 for the wire).
```

### `crates/protocol`

`ClientMsg::{Hello, Input{tick, input}, Bye, Respawn}`,
`ServerMsg::{Welcome{player_id, seed, world_size, snapshot}, Snapshot,
Events{tick, events}}`, plus `Snapshot { tick, entities, score_by_player,
level, terrain }`. `EntityState` carries `id, kind, pos, vel, facing,
turret_facing, alive, hp, max_hp, thrusting`. Encoded with `postcard` and
shipped as binary WebSocket frames. `DEFAULT_ADDR = "127.0.0.1:4015"`.
`snapshot_from_world(&World)` is the server-side helper.

### `crates/server`

```
src/
├── main.rs   # tokio entrypoint, reads ICARUST_LISTEN, calls run_with_listener
└── lib.rs    # accept loop + game_loop task. One global room.
```

`game_loop` owns the `World`, ticks at 60 Hz via `tokio::time::interval`,
drains a `Command` mpsc per tick
(`Join`/`Leave`/`Input`/`Respawn`), broadcasts `Events` whenever a tick
produced any, and broadcasts a `Snapshot` every 3rd tick (= 20 Hz). One
`broadcast::Sender<Arc<ServerMsg>>` fans out to per-connection writer
tasks; a connection that lags is logged but doesn't stall the rest. Player
IDs come from a process-wide `AtomicU32` starting at 1.

### `crates/client`

```
src/
├── lib.rs            # MainState + ggez EventHandler + native_main + wasm_start.
│                     # cdylib + rlib. cfg-gated entry points for both targets.
├── main.rs           # 4-line native bin shim that calls native_main().
├── input.rs          # InputState (xaxis/yaxis/fire/quit) → PlayerInput.
├── assets.rs         # AssetManager: image cache, Vec<SoundData>, font registration.
├── menu.rs           # Title screen — animation + layout, lives outside the world.
├── net/
│   ├── mod.rs        # Net trait: try_recv / send / is_connected.
│   │                 # `Net: Send` on native, plain trait on wasm.
│   ├── native.rs     # NativeNet: bg thread runs tokio + tokio-tungstenite.
│   └── web.rs        # WebNet: web_sys::WebSocket + Closures push into VecDeque.
├── render/
│   ├── mod.rs            # sub-modules below.
│   ├── camera.rs         # World ↔ screen mapping; Y-up world, Y-down screen.
│   ├── entities.rs       # Procedural meshes (player/enemy ships, tank, shots), tints,
│   │                     # tread-link constants.
│   ├── sky.rs            # Cream sky + parallax cloud field (deterministic).
│   ├── terrain.rs        # TerrainRenderer: batched soil/horizon/grass mesh.
│   ├── particles.rs      # ThrustEmitter + DamageSmoker (smoke / sparks).
│   ├── explosion.rs      # Style-driven particle bursts on death events.
│   └── instance_batch.rs # Shared 1×1 quad batch for particle systems.
└── widget.rs         # TextWidget: positioned HUD text drawn through camera.
```

`MainState::update`:
1. Drain incoming `ServerMsg`s via `Net::try_recv`. `Welcome` sets
   `local_player_id`, the bootstrap snapshot, and the camera's ground
   reference. `Snapshot` replaces `latest_snapshot` and re-syncs the
   terrain renderer. `Events` plays `pew.ogg` on `ShotFired`, `boom.ogg`
   on `EnemyKilled`/`PlayerDamaged`/`PlayerKilled`/`ShellExploded`, spawns
   explosions, and flips into `GameOver` for the local player.
2. For each fixed step, send `ClientMsg::Input { tick, input }` to the
   server and move the camera onto the local player's entity from the
   latest snapshot. The `tick` field is a local monotonic counter; the
   server doesn't yet use it for resimulation.
3. Update particle systems (thrust trail + damage smoke), advance live
   explosions, and refresh HUD text if dirty or score/level changed.

`MainState::draw` first checks `AppState` — `Menu` defers to
`Menu::draw`, `Playing`/`GameOver` draws sky → terrain → thrust trails
(batched) → entities → smoke/spark/explosion particles (batched) →
tank-tread overlay (batched) → HUD + overlays. Entities are
dead-reckoned by `pos + vel * time_since_snapshot` (clamped) so the 20 Hz
snapshot cadence doesn't step visibly between updates.

## Key conventions

- **Server is authoritative.** The client runs no physics, no collision,
  no respawn. All of it is in `sim::World`.
- **Determinism.** `World` owns a seeded `ChaCha8Rng`. All RNG draws take
  an explicit `&mut impl RngCore`. Entities live in
  `BTreeMap<EntityId, Entity>` and inputs in
  `BTreeMap<PlayerId, PlayerInput>` so iteration order is identical
  across machines. `Tick(u64)` is monotonic;
  `World::tick(&PlayerInputs, dt) -> Vec<GameEvent>` is pure given prior
  state.
- **Floats stay `f32`.** Bit-for-bit cross-arch determinism isn't
  guaranteed; we do snapshot replication, not lockstep. The discipline is
  for replays / restart-from-snapshot / tests.
- **Fixed timestep:** 60 Hz on both ends. `sim::TICK_DT = 1.0/60.0`.
  Server uses `tokio::time::interval`; client uses
  `ctx.time.check_update_time(60)`.
- **World coordinates are Y-up**, screen coordinates are Y-down.
  Conversion happens in `Camera::world_to_screen`. HUD is in raw screen
  pixels.
- **Facing convention:** `vec_from_angle(0)` returns `(0, 1)` — facing=0
  points up the world Y axis. Note this is `(sin, cos)`, not the usual
  `(cos, sin)`.
- **World bounds are `WORLD_WIDTH × WORLD_HEIGHT` (3200 × 1080).** The
  visible viewport (`VIEW_WIDTH × VIEW_HEIGHT` = 1280 × 540 in
  `crates/client/src/lib.rs`) is smaller than the world so the camera
  scrolls both horizontally and vertically. The X axis is toroidal in the
  sim — entities that fly off the right reappear on the left — and
  `Camera::world_x_offsets_for` returns up to three world copies near the
  seam so wrap-around stays seamless visually. The Y axis is a hard wall
  for players (clamp at `[0, WORLD_HEIGHT]`); enemy bullets and ship
  enemies bounce off the local terrain surface, artillery shells
  detonate. Resizing the window only changes letterboxing.
- **Player HP and damage.** Players have `PLAYER_MAX_HP` (5) hit points.
  Enemy bullets chip 1 HP; tank shells chip 2 (see `ShotOwner::damage`).
  Player ↔ hostile *contact* is continuous damage — both sides accumulate
  `RAM_DAMAGE_PER_SECOND * dt` HP per overlapping tick; a full-HP pilot
  survives about `RAM_DEATH_SECONDS` (1.5 s) of constant contact. Terrain
  crashes are still instant kills. HP regenerates +1 every
  `PLAYER_REGEN_INTERVAL` after `PLAYER_REGEN_DELAY` damage-free seconds.
  The client renders an HP bar in the HUD, brown smoke from damaged
  ships/tanks (`render::particles::DamageSmoker`), and a one-shot puff +
  spark burst on `GameEvent::PlayerDamaged`/`EnemyDamaged`.
- **Procedural entity art.** `render::entities::EntityMeshes` builds one
  ggez `Mesh` per kind at startup; `visual_for_kind` picks the right
  mesh/tint at draw time. Tanks are a chassis + independent turret + a
  batched tread-link overlay so the tracks visibly roll. Player thrust
  spawns flame particles via `render::particles::ThrustEmitter`.
  Background sky + clouds live in `render::sky::Sky`, seeded so all
  clients see the same skyline. Particles share one `InstanceQuadBatch`
  per layer (behind/overlay), so a busy frame still draws in a handful
  of calls.
- **Death = `alive = false`.** `World::tick` retains live entities each
  frame. When the local player dies, the client moves to `AppState::
  GameOver`; any key press returns to `AppState::Menu`, where Space
  triggers a `ClientMsg::Respawn`. Waves and difficulty advance through
  `wave::WaveDirector` (timed level-up + spawn pulses, capped at
  per-kind limits), independent of how many kills the player has racked
  up.
- **Sound IDs are `Vec<SoundData>` indices.** `play_sound` builds a
  fresh `Source` and detaches it so plays can overlap. Audio is fired
  off `GameEvent`s, not by diffing snapshots. `MainState::play_sound`
  short-circuits when the wasm `AudioContext` is suspended.
- **WireVec2** is a serde-stable `{x, y}` mirror of `glam::Vec2` so the
  protocol shape is independent of glam's wire format.

## App states

The client is in one of three states (`AppState`):

- **Menu** — title screen with animated ships; Space launches into
  `Playing`. If the player is currently dead the launch transition sends
  `ClientMsg::Respawn` first.
- **Playing** — full HUD, world rendering, input forwarded to server.
- **GameOver** — world stays visible behind a "GAME OVER" overlay. Any
  key press returns to `Menu`; Esc always quits.

## Controls

Left / Right rotate, Up thrusts, Space fires (or launches from the
menu, or returns to menu from Game Over), Escape quits.

## Tests

`cargo test` runs:

- `sim` unit tests: physics, util angle/wrap helpers, `player::apply_input`
  / `apply_forces`, enemy/tank AI (fire/turn/dodge), terrain generation +
  hit testing, wave-director timers, and `world` integration tests
  covering damage, contact, terrain crashes, friendly fire, gravity arcs,
  enemy/tank/shell HP, spawn safety, and
  `determinism_replay_matches_hash` — the CI gate that asserts
  `(seed, inputs)` produces a stable entity-state hash and that a
  different seed diverges.
- `server` integration test (`tests/handshake.rs`): binds `127.0.0.1:0`,
  drives a fake WebSocket client through `Hello → Welcome → Input(fire) →
  Events/Snapshot`.

## Style

- Comments should be concise and only describe the why if it isn't
  obvious from the code; limit historical observations. Aim for ~90
  columns of width (and stay under 100).
- Don't worry about breaking interfaces — always refactor when there's a
  clear opportunity for code improvement / cleanliness.
- Shared toroidal-X / angular math goes in `sim::util` so AI modules and
  `World` can pull from one source.
