# Icarust

A 2D Sopwith/Luftrauser-style shoot-'em-up written in Rust on top of [ggez](https://github.com/ggez/ggez). The player flies a thrust-vector ship with gravity, wraps horizontally, and shoots rocks for points; clearing a level spawns more.

The codebase has been split into a deterministic, server-authoritative client/server architecture. See `server-architecture.md` for the full migration plan.

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

The client always needs a server. `ggez` is pulled from a pinned git rev (currently `62af01d5` from the `web` branch, since that's where wasm/WebGPU support lives). Client resources resolve from `$CARGO_MANIFEST_DIR/../../resources` when run via cargo, otherwise `./resources`.

## Web build

Wasm target lives under `web/`. The same `crates/client` builds for both native (`[[bin]]`) and wasm (`cdylib`); cfg-gates in `crates/client/src/lib.rs` + `src/net/mod.rs` pick the right entry point and net impl. The wasm entry is `wasm_start` (registered via `#[wasm_bindgen(start)]`); the native entry is `native_main`.

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
1. `cargo build -p client --lib --target wasm32-unknown-unknown` (browsers need WebGPU — Chrome on Linux may require `chrome://flags/#enable-unsafe-webgpu`).
2. `wasm-bindgen --target web` over `target/wasm32-unknown-unknown/<profile>/client.wasm`
3. zips `resources/` into `web/public/resources.zip`; `web/src/runner.js` fetches that and stashes it on `window.__GGEZ_RESOURCES_ZIP__` before calling `init()`, so ggez's `Filesystem::new_web` can mount it.

The runner dodges a Vite quirk: dynamic imports of JS files under `publicDir` are blocked even with `@vite-ignore`, so we fetch `client.js` as text, rewrite the `new URL('client_bg.wasm', import.meta.url)` literal to an absolute URL, and `import` a `Blob` URL instead.

**Vite stale-public gotcha:** rebuilding while `vite` is running will cause it to serve the SPA fallback HTML for the new files. Kill and restart vite after every `build.mjs` run.

The wasm client reads `?server=ws://host:4015&name=alice` from `location.search`; defaults are `ws://<page-hostname>:4015` (or `wss://` when the page is https) and `pilot`. The scheme follows `location.protocol` so an https-terminating proxy like nginx works without query params — note that only port 4015 forwards WebSocket upgrades in the local nginx setup.

## Verifying the wasm build in a headless browser

ggez 0.10+ is WebGPU-only on the web. Headless browsers don't expose a WebGPU adapter by default, so the standard Playwright MCP (and `chromium.launch()`) hits `navigator.gpu.requestAdapter() === null` and ggez fails with `GraphicsInitializationError`. You have to launch Chromium with the right flags yourself.

Loop:

1. Start the server: `cargo run -p server` (in the background, or in another shell).
2. Build the wasm bundle: `cd web && npm run build` (or `build:release`).
3. Serve it: `npm run serve`. Vite binds 4010 with `strictPort: true`, so it'll fail loudly instead of silently sliding to another port. Remember the stale-public gotcha above and restart vite after each rebuild.
4. Drive it with `web/test-headless.mjs`, a small Playwright runner that launches Chromium with the flags Linux needs to expose a WebGPU adapter (`--enable-unsafe-webgpu --enable-features=Vulkan --use-angle=vulkan --disable-vulkan-fallback-to-gl-for-testing --ignore-gpu-blocklist`). Read the file if you need to tweak it.

Run it: `node web/test-headless.mjs http://127.0.0.1:<port>/ /tmp/icarust.png`. If `playwright` isn't already in `web/node_modules`, install it once: `cd web && npm i -D playwright && npx playwright install chromium`. Increase `WAIT_MS` (env var) for slow first-frame paths; the runner defaults to 6000 ms.

### Reading the output

- `webgpu: true, hasAdapter: true` — flags took effect and a WebGPU adapter is available. If `hasAdapter` is false, the flags didn't apply (wrong Chromium build, or the args were silently rejected). Check `chrome://gpu` in a real Chrome if confused.
- A canvas at the configured world size (`1280×540`) means ggez reached its first frame. A 300×150 canvas means init bailed before the resize observer fired — look at the console for a Rust panic or a `ggez build_async: ...` line.
- The pilot will be dead by the time the screenshot is taken if `WAIT_MS` ≥ a couple seconds — that's the sim working, not a regression.
- `AudioContext was not allowed to start` warnings are expected in headless (no user gesture); they don't block rendering.

### Don't bother with the Playwright MCP

The Playwright MCP server provided by Claude Code's tools launches Chromium without browser args, so it can't expose WebGPU. You'll see `[WARNING] No available adapters.` and the same `GraphicsInitializationError`. Use a Node-side runner like the one above instead.

## Workspace layout

```
crates/
├── sim/        # pure deterministic simulation. No ggez, no I/O, no global RNG.
├── protocol/   # ClientMsg/ServerMsg/Snapshot, postcard wire format. Shared.
├── server/     # tokio + tokio-tungstenite. Owns World, 60 Hz tick, 20 Hz snapshots.
└── client/     # ggez front-end. Sends inputs, renders latest snapshot, plays audio off events.
                # Builds as both a native bin and a wasm cdylib.
resources/      # runtime assets (player.png, rock.png, shot.png, DejaVuSerif.ttf, pew.ogg, boom.ogg).
web/            # wasm build harness: build.mjs, vite config, runner.js, host page.
```

`sim` and `protocol` are the only crates compiled by both server and client; they must stay free of `ggez`, `tokio`, and platform deps.

### `crates/sim`

```
src/
├── lib.rs       # re-exports + TICK_DT = 1.0/60.0
├── world.rs     # World, WorldConfig, tick(&PlayerInputs, dt) -> Vec<GameEvent>
├── entity.rs    # Entity, EntityId, EntityKind { Player|Rock|Shot }, PlayerId, Tick
├── input.rs     # PlayerInput { xaxis, yaxis, fire }, PlayerInputs (BTreeMap)
├── event.rs     # GameEvent enum
├── physics.rs   # circles_overlap / collides
├── player.rs    # apply_input, apply_forces (pure), thrust/gravity/drag/turn constants
└── util.rs      # Vec2 (= glam::Vec2), vec_from_angle, random_vec, rand_unit,
                 # clamp_velocity, wrap_coord, WireVec2 (serde-stable Vec2 for the wire)
```

### `crates/protocol`

`ClientMsg::{Hello, Input{tick, input}, Bye}`, `ServerMsg::{Welcome{player_id, seed, world_size, snapshot}, Snapshot, Events{tick, events}}`, plus `Snapshot { tick, entities: Vec<EntityState>, score_by_player, level }`. Encoded with `postcard` and shipped as binary WebSocket frames. `DEFAULT_ADDR = "127.0.0.1:4015"`. `snapshot_from_world(&World)` is the server-side helper.

### `crates/server`

```
src/
├── main.rs   # tokio entrypoint, reads ICARUST_LISTEN, calls run_with_listener
└── lib.rs    # accept loop + game_loop task. One global room.
```

`game_loop` owns the `World`, ticks at 60 Hz via `tokio::time::interval`, drains a `Command` mpsc per tick (`Join`/`Leave`/`Input`), broadcasts `Events` whenever a tick produced any, and broadcasts a `Snapshot` every 3rd tick (= 20 Hz). One `broadcast::Sender<Arc<ServerMsg>>` fans out to per-connection writer tasks; a connection that lags is logged but doesn't stall the rest. Player IDs come from a process-wide `AtomicU32` starting at 1.

### `crates/client`

```
src/
├── lib.rs            # MainState + ggez EventHandler + native_main + wasm_start.
│                     # cdylib + rlib. cfg-gated entry points for both targets.
├── main.rs           # 4-line native bin shim that calls native_main().
├── input.rs          # InputState (xaxis/yaxis/fire/quit/restart) → PlayerInput
├── assets.rs         # AssetManager: image cache, Vec<SoundData>, font registration
├── net/
│   ├── mod.rs        # Net trait: try_recv / send / is_connected.
│   │                 # `Net: Send` on native, plain trait on wasm.
│   ├── native.rs     # NativeNet: bg thread runs tokio + tokio-tungstenite
│   └── web.rs        # WebNet: web_sys::WebSocket + Closures push into VecDeque
├── render/camera.rs  # World ↔ screen mapping; Y-up world, Y-down screen
└── widget.rs         # TextWidget: positioned HUD text drawn through camera
```

`MainState::update`:
1. Drain incoming `ServerMsg`s via `Net::try_recv`. `Welcome` sets `local_player_id` and the bootstrap snapshot. `Snapshot` replaces `latest_snapshot`. `Events` plays `pew.ogg` on `ShotFired`, `boom.ogg` on `RockKilled`, flips `game_over` on `PlayerKilled` for the local player.
2. For each fixed step, send `ClientMsg::Input { tick, input }` to the server and move the camera onto the local player's entity from the latest snapshot. The `tick` field is a local monotonic counter; the server doesn't yet use it for resimulation (that's Phase 3).
3. Refresh HUD text if dirty or score/level changed.

`MainState::draw` draws every alive entity from `latest_snapshot`. Players use the wrapped draw path (drawn twice across the world seam); rocks/shots draw once. While `latest_snapshot` is `None` (pre-Welcome), a "Connecting…" overlay shows.

## Key conventions

- **Server is authoritative.** The client runs no physics, no collision, no respawn. All of it is in `sim::World`.
- **Determinism.** `World` owns a seeded `ChaCha8Rng`. `random_vec` and rock spawn take an explicit `&mut impl RngCore`. Entities live in `BTreeMap<EntityId, Entity>` and inputs in `BTreeMap<PlayerId, PlayerInput>` so iteration order is identical across machines. `Tick(u64)` is monotonic; `World::tick(&PlayerInputs, dt) -> Vec<GameEvent>` is pure given prior state.
- **Floats stay `f32`.** Bit-for-bit cross-arch determinism isn't guaranteed; we do snapshot replication, not lockstep. The discipline is for replays / restart-from-snapshot / tests.
- **Fixed timestep:** 60 Hz on both ends. `sim::TICK_DT = 1.0/60.0`. Server uses `tokio::time::interval`; client uses `ctx.time.check_update_time(60)`.
- **World coordinates are Y-up**, screen coordinates are Y-down. Conversion happens in `Camera::world_to_screen_coords`. HUD uses `static_world_to_screen_coords` (Y flip only).
- **Facing convention:** `vec_from_angle(0)` returns `(0, 1)` — facing=0 points up the world Y axis. Note this is `(sin, cos)`, not the usual `(cos, sin)`.
- **World bounds are fixed at `WORLD_WIDTH × WORLD_HEIGHT` (1280 × 540).** Resizing only re-scales the camera (`Camera::set_drawable_size`).
- **Death = `alive = false`.** `World::tick` retains live entities each frame. When the local player dies, the client sets `game_over` and hides the player sprite; an overlay shows "GAME OVER". When all rocks are gone, `World` advances level and spawns `level + 5` rocks (capped at 30).
- **Sound IDs are `Vec<SoundData>` indices.** `play_sound` builds a fresh `Source` and detaches it so plays can overlap. Audio is fired off `GameEvent`s, not by diffing snapshots.
- **WireVec2** is a serde-stable `{x, y}` mirror of `glam::Vec2` so the protocol shape is independent of glam's wire format.

## Behavioral notes (current state)

- **No single-player.** The client cannot run without a server.
- **R-to-restart is a no-op in network mode.** Phase 2 has no respawn flow; a dead player stays dead. A `Respawn` `ClientMsg` would be needed before this comes back.
- **HUD shows the local player's score and the global level.** Multi-player scoreboard is not drawn yet.
- **No client-side interpolation.** The client renders the latest snapshot directly. Phase 3 adds a snapshot ring buffer and renders at `now − render_delay`.
- **No wasm client yet.** Phase 4 swaps `ggez` to its `web` branch and adds a `gloo-net` `Net` impl.

## Controls

Left / Right rotate, Up thrusts, Space fires, Escape quits. (R is currently inert — see above.)

## Tests

`cargo test` runs:

- `sim` unit tests: `physics::circles_overlap`, `util::vec_from_angle` / `clamp_velocity`, the `player::apply_input` / `apply_forces` step (thrust direction, "1 s of held thrust climbs > 50 px," velocity clamp), and `world` tests including `determinism_replay_matches_hash` — the CI gate that asserts `(seed, inputs)` produces a stable entity-state hash and that a different seed diverges.
- `server` integration test (`tests/handshake.rs`): binds `127.0.0.1:0`, drives a fake WebSocket client through `Hello → Welcome → Input(fire) → Events/Snapshot`.

## Style

- Comments should be concise and only describe what/why if existing code, limit historical observations. Extend comments to ~90 line width.
- Don't worry about breaking interfaces, always refactor when there is a clear opportunity for code improvement/cleanliness.