//! Icarust client library.
//!
//! Houses `MainState`, the ggez `EventHandler`, asset/input/render plumbing,
//! and both native and wasm entry points. The `[[bin]]` target in this crate
//! is a thin shim that calls `native_main()`; the wasm entry is exported via
//! `#[wasm_bindgen(start)]`.

use ggez::conf;
use ggez::event::{self, EventHandler};
use ggez::glam::Vec2;
use ggez::graphics::{self, Canvas, Color, DrawParam};
use ggez::input::keyboard::KeyInput;
use ggez::{Context, ContextBuilder, GameResult};

use protocol::{ClientMsg, EntityState, ServerMsg, Snapshot};
use sim::entity::EntityKind;
use sim::{GameEvent, PlayerId, Tick};

use crate::render::explosion::{Explosion, ExplosionStyle};

const ENEMY_TINT: Color = Color::new(0.95, 0.30, 0.30, 1.0);
const ENEMY_SHOT_TINT: Color = Color::new(1.0, 0.45, 0.20, 1.0);

pub mod assets;
pub mod input;
pub mod net;
pub mod render;
pub mod widget;

use crate::assets::{AssetManager, SoundId, Sprite};
use crate::input::InputState;
use crate::net::Net;
use crate::render::camera::{Camera, Point2};
use crate::widget::TextWidget;

const VIEW_WIDTH: f32 = sim::world::WORLD_WIDTH;
const VIEW_HEIGHT: f32 = sim::world::WORLD_HEIGHT;

fn print_instructions() {
    tracing::info!("Welcome to Icarust!");
    tracing::info!("Controls: Left/Right rotate, Up thrust, Space fire, R restart, Esc quit");
}

struct Sprites {
    player: Sprite,
    rock: Sprite,
    shot: Sprite,
}

impl Sprites {
    /// Pick the sprite + tint to draw for a given entity kind. Enemies
    /// reuse the player sprite tinted red so we don't need a separate
    /// asset for now.
    fn for_kind(&self, kind: &EntityKind) -> (&Sprite, Color) {
        use sim::entity::ShotOwner;
        match kind {
            EntityKind::Player { .. } => (&self.player, Color::WHITE),
            EntityKind::Rock => (&self.rock, Color::WHITE),
            EntityKind::Shot { owner: ShotOwner::Player(_) } => (&self.shot, Color::WHITE),
            EntityKind::Shot { owner: ShotOwner::Enemy } => (&self.shot, ENEMY_SHOT_TINT),
            EntityKind::Enemy => (&self.player, ENEMY_TINT),
        }
    }
}

pub struct MainState {
    asset_manager: AssetManager,
    camera: Camera,
    net: Box<dyn Net>,
    sprites: Sprites,
    shot_sound_id: SoundId,
    hit_sound_id: SoundId,
    input: InputState,
    local_player_id: Option<PlayerId>,
    latest_snapshot: Option<Snapshot>,
    /// Monotonic counter we tag outgoing inputs with. The server doesn't yet
    /// use this for resimulation but the field shape matches what Phase 3
    /// will need.
    next_input_tick: Tick,
    gui_dirty: bool,
    score_text: TextWidget,
    level_text: TextWidget,
    game_over_text: TextWidget,
    restart_hint_text: TextWidget,
    disconnected_text: TextWidget,
    game_over: bool,
    cached_score: i32,
    cached_level: i32,
    /// Active particle bursts. Owned client-side; not part of the sim.
    /// Each `PlayerKilled` event spawns one.
    explosions: Vec<Explosion>,
    /// Monotonic counter used as a per-explosion RNG seed so simultaneous
    /// bursts don't render identically.
    next_explosion_seed: u64,
}

impl MainState {
    pub fn new(ctx: &mut Context, net: Box<dyn Net>) -> GameResult<MainState> {
        print_instructions();

        let mut am = AssetManager::new();

        let (drawable_w, drawable_h) = ctx.gfx.drawable_size();

        let sprites = Sprites {
            player: am.make_sprite(ctx, "/player.png"),
            rock: am.make_sprite(ctx, "/rock.png"),
            shot: am.make_sprite(ctx, "/shot.png"),
        };

        let shot_sound_id = am.add_sound(ctx, "/pew.ogg");
        let hit_sound_id = am.add_sound(ctx, "/boom.ogg");

        let score_text = TextWidget::new(ctx, &mut am, 18.0)?;
        let level_text = TextWidget::new(ctx, &mut am, 18.0)?;
        let mut game_over_text = TextWidget::new(ctx, &mut am, 48.0)?;
        game_over_text.set_text("GAME OVER", 48.0);
        let mut restart_hint_text = TextWidget::new(ctx, &mut am, 22.0)?;
        restart_hint_text.set_text("press R to restart", 22.0);
        let mut disconnected_text = TextWidget::new(ctx, &mut am, 24.0)?;
        disconnected_text.set_text("Connecting…", 24.0);

        let camera = Camera::new(drawable_w, drawable_h, VIEW_WIDTH, VIEW_HEIGHT);

        Ok(MainState {
            asset_manager: am,
            camera,
            net,
            sprites,
            shot_sound_id,
            hit_sound_id,
            input: InputState::default(),
            local_player_id: None,
            latest_snapshot: None,
            next_input_tick: Tick(0),
            gui_dirty: true,
            score_text,
            level_text,
            game_over_text,
            restart_hint_text,
            disconnected_text,
            game_over: false,
            cached_score: 0,
            cached_level: 0,
            explosions: Vec::new(),
            next_explosion_seed: 1,
        })
    }

    fn handle_server_msg(&mut self, ctx: &Context, msg: ServerMsg) {
        match msg {
            ServerMsg::Welcome {
                player_id,
                snapshot,
                ..
            } => {
                self.local_player_id = Some(player_id);
                self.latest_snapshot = Some(snapshot);
                self.gui_dirty = true;
            }
            ServerMsg::Snapshot(snap) => {
                self.latest_snapshot = Some(snap);
                // If our entity is back in the world, we're alive again.
                if self.game_over && self.local_player().is_some() {
                    self.game_over = false;
                    self.gui_dirty = true;
                }
            }
            ServerMsg::Events { events, .. } => {
                self.handle_game_events(ctx, &events);
            }
        }
    }

    fn handle_game_events(&mut self, ctx: &Context, events: &[GameEvent]) {
        for ev in events {
            match ev {
                GameEvent::ShotFired { .. } => {
                    self.asset_manager.play_sound(ctx, self.shot_sound_id);
                }
                GameEvent::RockKilled { .. } | GameEvent::EnemyKilled { .. } => {
                    self.asset_manager.play_sound(ctx, self.hit_sound_id);
                    self.gui_dirty = true;
                }
                GameEvent::PlayerKilled { player_id, pos, cause } => {
                    self.spawn_explosion(Vec2::new(pos.x, pos.y), ExplosionStyle::for_cause(cause));
                    self.asset_manager.play_sound(ctx, self.hit_sound_id);
                    if Some(*player_id) == self.local_player_id {
                        self.game_over = true;
                        self.gui_dirty = true;
                    }
                }
                GameEvent::LevelUp(_) | GameEvent::PlayerJoined(_) | GameEvent::PlayerLeft(_) => {
                    self.gui_dirty = true;
                }
            }
        }
    }

    fn spawn_explosion(&mut self, pos: Vec2, style: ExplosionStyle) {
        let seed = self.next_explosion_seed;
        self.next_explosion_seed = self.next_explosion_seed.wrapping_add(1);
        self.explosions.push(Explosion::new(pos, style, seed));
    }

    fn local_player(&self) -> Option<&EntityState> {
        let snap = self.latest_snapshot.as_ref()?;
        let pid = self.local_player_id?;
        snap.entities.iter().find(|e| match e.kind {
            EntityKind::Player { player_id } => player_id == pid,
            _ => false,
        })
    }

    fn refresh_hud(&mut self, ctx: &mut Context) {
        let snap = match &self.latest_snapshot {
            Some(s) => s,
            None => return,
        };
        let score = self
            .local_player_id
            .and_then(|pid| {
                snap.score_by_player
                    .iter()
                    .find(|(p, _)| *p == pid)
                    .map(|(_, s)| *s)
            })
            .unwrap_or(0);
        let level = snap.level;
        self.cached_score = score;
        self.cached_level = level;

        self.score_text.set_text(&format!("Score: {score}"), 18.0);
        self.level_text.set_text(&format!("Level: {level}"), 18.0);

        // HUD is positioned in screen pixels so it stays in the same
        // place regardless of fullscreen letterboxing.
        let screen = self.camera.screen_size();
        self.level_text.set_position(Point2::new(10.0, 10.0));
        let level_w = self.level_text.width(ctx);
        self.score_text
            .set_position(Point2::new(level_w + 25.0, 10.0));

        let go_w = self.game_over_text.width(ctx);
        let go_h = self.game_over_text.height(ctx);
        self.game_over_text.set_position(Point2::new(
            (screen.x - go_w) / 2.0,
            (screen.y - go_h) / 2.0,
        ));

        let hint_w = self.restart_hint_text.width(ctx);
        self.restart_hint_text.set_position(Point2::new(
            (screen.x - hint_w) / 2.0,
            (screen.y - go_h) / 2.0 + go_h + 8.0,
        ));

        let dc_w = self.disconnected_text.width(ctx);
        let dc_h = self.disconnected_text.height(ctx);
        self.disconnected_text.set_position(Point2::new(
            (screen.x - dc_w) / 2.0,
            (screen.y - dc_h) / 2.0,
        ));
    }

    /// Draw an entity at every visible toroidal copy. Only X wraps —
    /// Y is a hard wall in the sim — so we just need to draw extras on
    /// the opposite horizontal edge when an entity is within a sprite-
    /// half of one.
    fn draw_entity(&self, canvas: &mut Canvas, entity: &EntityState) {
        let view = self.camera.view_size();
        let (sprite, tint) = self.sprites.for_kind(&entity.kind);
        // Sprite half-extent in world units (sprites are scaled by
        // camera.scale on draw, so the source pixel size already covers
        // a `sprite_pixels / scale` world-unit disc).
        let half_world = sprite.half_width().max(sprite.half_height()) / self.camera.scale();
        let pos = Vec2::new(entity.pos.x, entity.pos.y);

        for dx in [-1.0, 0.0, 1.0] {
            let p = Vec2::new(pos.x + dx * view.x, pos.y);
            if p.x < -half_world || p.x > view.x + half_world {
                continue;
            }
            self.draw_entity_at(canvas, entity, p, sprite, tint);
        }
    }

    fn draw_entity_at(
        &self,
        canvas: &mut Canvas,
        entity: &EntityState,
        world_pos: Vec2,
        sprite: &Sprite,
        tint: Color,
    ) {
        let screen = self.camera.world_to_screen(world_pos);
        let scale = self.camera.scale();
        let drawparams = DrawParam::new()
            .dest(screen)
            .rotation(entity.facing)
            .scale([scale, scale])
            .offset(Point2::new(0.5, 0.5))
            .color(tint);
        canvas.draw(&sprite.image, drawparams);
    }
}

impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        const DESIRED_FPS: u32 = 60;

        // Drain whatever the network has for us before stepping the loop.
        while let Some(msg) = self.net.try_recv() {
            self.handle_server_msg(ctx, msg);
        }

        while ctx.time.check_update_time(DESIRED_FPS) {
            if self.input.quit {
                ctx.request_quit();
                self.net.send(&ClientMsg::Bye);
                break;
            }

            // R restarts only when dead. Edge-trigger so one keypress sends
            // exactly one Respawn.
            if self.game_over && self.input.restart {
                self.net.send(&ClientMsg::Respawn);
                self.input.restart = false;
            }

            self.next_input_tick = self.next_input_tick.next();
            let input_msg = ClientMsg::Input {
                tick: self.next_input_tick,
                input: self.input.to_player_input(),
            };
            self.net.send(&input_msg);
        }

        // Step active explosions on real elapsed time so they look the
        // same regardless of the fixed-step input cadence.
        let dt = ctx.time.delta().as_secs_f32();
        for ex in &mut self.explosions {
            ex.update(dt);
        }
        self.explosions.retain(|e| !e.done());

        let snap_score = self
            .latest_snapshot
            .as_ref()
            .and_then(|s| {
                self.local_player_id.and_then(|pid| {
                    s.score_by_player
                        .iter()
                        .find(|(p, _)| *p == pid)
                        .map(|(_, sc)| *sc)
                })
            })
            .unwrap_or(0);
        let snap_level = self.latest_snapshot.as_ref().map(|s| s.level).unwrap_or(0);
        if self.gui_dirty || snap_score != self.cached_score || snap_level != self.cached_level {
            self.refresh_hud(ctx);
            self.gui_dirty = false;
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let mut canvas = graphics::Canvas::from_frame(ctx, Color::BLACK);

        if let Some(snap) = &self.latest_snapshot {
            // Terrain underneath everything.
            render::terrain::draw(&mut canvas, &self.camera, &snap.terrain);

            for entity in &snap.entities {
                if !entity.alive {
                    continue;
                }
                self.draw_entity(&mut canvas, entity);
            }

            // Explosions render on top of entities so a fresh boom is
            // visible even while the doomed sprite is still being drawn
            // by the same snapshot.
            for ex in &self.explosions {
                ex.draw(&mut canvas, &self.camera);
            }

            self.level_text.draw(&mut canvas);
            self.score_text.draw(&mut canvas);
            if self.game_over {
                self.game_over_text.draw(&mut canvas);
                self.restart_hint_text.draw(&mut canvas);
            }
        } else {
            self.disconnected_text.draw(&mut canvas);
        }

        canvas.finish(ctx)?;
        ggez::timer::yield_now();
        Ok(())
    }

    fn key_down_event(
        &mut self,
        _ctx: &mut Context,
        input: KeyInput,
        _repeat: bool,
    ) -> GameResult {
        self.input.handle_key_down(input);
        Ok(())
    }

    fn key_up_event(&mut self, _ctx: &mut Context, input: KeyInput) -> GameResult {
        self.input.handle_key_up(input);
        Ok(())
    }

    fn resize_event(&mut self, _ctx: &mut Context, width: f32, height: f32) -> GameResult {
        self.camera.set_screen_size(width, height);
        self.gui_dirty = true;
        Ok(())
    }
}

/// Build a ggez `ContextBuilder` configured the way Icarust wants it. The
/// caller drives the actual context construction: native uses `build()`
/// (sync via `pollster::block_on`), wasm uses `build_async().await` from
/// inside `wasm_bindgen_futures::spawn_local` because WebGPU init is async.
pub fn build_ggez(resource_dir: Option<std::path::PathBuf>) -> ContextBuilder {
    let window_setup = conf::WindowSetup::default()
        .title("Icarust")
        // Tells ggez to append its <canvas> to this element on web. Harmless
        // on native — winit ignores the field.
        .web_canvas_parent_id("ggez-canvas-host");

    let mut cb = ContextBuilder::new("icarust", "ggez")
        .window_setup(window_setup)
        .window_mode(
            conf::WindowMode::default()
                .dimensions(VIEW_WIDTH, VIEW_HEIGHT)
                .resizable(true),
        );

    if let Some(dir) = resource_dir {
        cb = cb.add_resource_path(dir);
    }

    cb
}

/// Native entry point. Connects the websocket, builds the ggez context, runs
/// the event loop. Returns once the window is closed.
#[cfg(not(target_arch = "wasm32"))]
pub fn native_main() -> GameResult {
    use crate::net::NativeNet;
    use std::env;
    use std::path;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let (url, name) = parse_args();
    let net: Box<dyn Net> = match NativeNet::connect(url, name) {
        Ok(n) => Box::new(n),
        Err(e) => {
            eprintln!("could not start net layer: {e:#}");
            std::process::exit(1);
        }
    };

    let resource_dir = if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        let mut path = path::PathBuf::from(manifest_dir);
        path.pop();
        path.pop();
        path.push("resources");
        path
    } else {
        path::PathBuf::from("./resources")
    };
    tracing::info!(resource_dir = %resource_dir.display(), "mounting resources");

    let (mut ctx, events_loop) = build_ggez(Some(resource_dir)).build()?;
    let game = MainState::new(&mut ctx, net)?;
    event::run(ctx, events_loop, game)
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_args() -> (String, String) {
    // crude flag parser: --connect <url> --name <name>
    let mut url = std::env::var("ICARUST_SERVER")
        .unwrap_or_else(|_| format!("ws://{}", protocol::DEFAULT_ADDR));
    let mut name = std::env::var("ICARUST_NAME").unwrap_or_else(|_| "pilot".to_string());
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--connect" => {
                if let Some(v) = args.next() {
                    url = v;
                }
            }
            "--name" => {
                if let Some(v) = args.next() {
                    name = v;
                }
            }
            _ => {}
        }
    }
    (url, name)
}

/// Wasm entry point. wasm-bindgen calls this as the module's `start` hook
/// when `init()` is awaited in JS. Reads connection params from
/// `window.location.search` (`?server=ws://…&name=…`), wires up a
/// `WebSocket`-backed `Net`, and hands control to the ggez event loop.
///
/// WebGPU adapter/device init resolves only after the JS event loop runs,
/// so context construction is awaited inside `spawn_local`. `wasm_start`
/// itself returns immediately so wasm-bindgen's `init()` promise resolves.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() -> Result<(), wasm_bindgen::JsValue> {
    use crate::net::WebNet;

    console_error_panic_hook::set_once();
    let _ = tracing_wasm::try_set_as_global_default();

    let (url, name) = wasm_parse_args();
    tracing::info!(%url, %name, "connecting");
    let net = WebNet::connect(&url, name)
        .map_err(|e| wasm_bindgen::JsValue::from_str(&format!("WebNet::connect: {e:#}")))?;

    let cb = build_ggez(None);
    wasm_bindgen_futures::spawn_local(async move {
        let (mut ctx, events_loop) = match cb.build_async().await {
            Ok(x) => x,
            Err(e) => {
                web_sys::console::error_1(&format!("ggez build_async: {e}").into());
                return;
            }
        };
        let game = match MainState::new(&mut ctx, Box::new(net)) {
            Ok(g) => g,
            Err(e) => {
                web_sys::console::error_1(&format!("MainState::new: {e}").into());
                return;
            }
        };
        // `event::run` on wasm uses `EventLoopExtWebSys::spawn_app` and
        // returns immediately; JS keeps driving the loop.
        if let Err(e) = event::run(ctx, events_loop, game) {
            web_sys::console::error_1(&format!("event::run: {e}").into());
        }
    });
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn wasm_parse_args() -> (String, String) {
    let default_url = "ws://127.0.0.1:4015".to_string();
    let default_name = "pilot".to_string();

    let Some(window) = web_sys::window() else {
        return (default_url, default_name);
    };
    let Ok(search) = window.location().search() else {
        return (default_url, default_name);
    };
    let params = match web_sys::UrlSearchParams::new_with_str(&search) {
        Ok(p) => p,
        Err(_) => return (default_url, default_name),
    };

    let url = params.get("server").unwrap_or_else(|| {
        // Default to same host as the page, port 4015. Use wss when the
        // page is https so a TLS-terminating proxy (e.g. nginx) works
        // without requiring a query string.
        let loc = window.location();
        let host = loc.hostname().unwrap_or_else(|_| "127.0.0.1".into());
        let scheme = match loc.protocol().as_deref() {
            Ok("https:") => "wss",
            _ => "ws",
        };
        format!("{scheme}://{host}:4015")
    });
    let url = if url.is_empty() { default_url } else { url };
    let name = params.get("name").unwrap_or(default_name);
    (url, name)
}
