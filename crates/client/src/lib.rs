//! Icarust client library.
//!
//! Houses `MainState`, the ggez `EventHandler`, asset/input/render plumbing,
//! and both native and wasm entry points. The `[[bin]]` target in this crate
//! is a thin shim that calls `native_main()`; the wasm entry is exported via
//! `#[wasm_bindgen(start)]`.

use ggez::conf;
use ggez::event::EventHandler;
use ggez::glam::Vec2;
use ggez::graphics::{self, Canvas, Color, DrawParam, Mesh};
use ggez::input::keyboard::KeyInput;
use ggez::{Context, ContextBuilder, GameResult};

use protocol::{ClientMsg, EntityState, ServerMsg, Snapshot};
use sim::entity::EntityKind;
use sim::{GameEvent, PlayerId, Tick};

use crate::render::explosion::{Explosion, ExplosionStyle};

pub mod assets;
pub mod input;
pub mod menu;
pub mod net;
pub mod render;
pub mod widget;

use crate::assets::{AssetManager, SoundId};
use crate::input::InputState;
use crate::menu::Menu;
use crate::net::Net;
use crate::render::camera::{Camera, Point2};
use crate::render::entities::{
    ship_wing_factor, EntityMeshes, ShipMesh, TankMesh, ENEMY_COLOR, ENEMY_SHOT_COLOR,
    PLAYER_COLOR, PLAYER_SHOT_COLOR, TANK_COLOR, TANK_SHOT_COLOR, TANK_TREAD_BAND_Y,
    TANK_TREAD_HALF_WIDTH, TANK_TREAD_LINK_COLOR, TANK_TREAD_LINK_SPACING, TANK_TURRET_PIVOT_Y,
};
use crate::render::particles::{DamageSmoker, ThrustEmitter};
use crate::render::sky::{Sky, SKY_COLOR};
use crate::widget::TextWidget;

/// Top-level UI state. The simulation keeps running on the server in all
/// states; this just gates what we render and how we route input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppState {
    /// Title screen. Server is connected and the player may exist in the
    /// world, but we draw the menu instead. Space launches the player into
    /// `Playing` (sending `Respawn` if they're currently dead).
    Menu,
    /// Live gameplay — full HUD, world rendering, input forwarded to server.
    Playing,
    /// Player just died. World stays visible behind a "GAME OVER" overlay.
    /// Any key press returns to `Menu`; Esc still quits.
    GameOver,
}

/// Visible window onto the world, in world units. The world itself is
/// larger in both axes (`sim::world::WORLD_WIDTH` and `WORLD_HEIGHT`) so
/// the camera can scroll instead of jumping at the seam or pinning to a
/// single altitude band.
const VIEW_WIDTH: f32 = 1280.0;
const VIEW_HEIGHT: f32 = sim::world::VIEW_HEIGHT;
/// How quickly the camera homes in on the player each second. 8.0 is a
/// good middle ground — responsive but doesn't snap.
const CAMERA_FOLLOW_RATE: f32 = 8.0;

fn print_instructions() {
    tracing::info!("Welcome to Icarust!");
    tracing::info!("Controls: Left/Right rotate, Up thrust, Space fire, Esc quit");
}

/// What to draw for an entity. Ships need their wings scaled separately
/// from the body for a banking effect; tanks need an independent turret
/// rotation; everything else is a single mesh. New entity-render shapes
/// (helicopters, gunboats, ...) join this enum.
enum EntityVisual<'a> {
    Ship { ship: &'a ShipMesh, tint: Color },
    Tank { tank: &'a TankMesh, tint: Color },
    Single { mesh: &'a Mesh, tint: Color },
}

/// Pick the right `EntityVisual` for a given entity kind. Each kind has a
/// dedicated mesh built procedurally at startup (see `render::entities`);
/// the color here multiplies against the mesh's white vertices so we can
/// retint at draw time without re-uploading geometry.
fn visual_for_kind<'a>(meshes: &'a EntityMeshes, kind: &EntityKind) -> EntityVisual<'a> {
    use sim::entity::ShotOwner;
    match kind {
        EntityKind::Player { .. } => EntityVisual::Ship {
            ship: &meshes.player,
            tint: PLAYER_COLOR,
        },
        EntityKind::Enemy => EntityVisual::Ship {
            ship: &meshes.enemy,
            tint: ENEMY_COLOR,
        },
        EntityKind::Tank => EntityVisual::Tank {
            tank: &meshes.tank,
            tint: TANK_COLOR,
        },
        EntityKind::Shot {
            owner: ShotOwner::Player(_),
        } => EntityVisual::Single {
            mesh: &meshes.shot,
            tint: PLAYER_SHOT_COLOR,
        },
        EntityKind::Shot {
            owner: ShotOwner::Enemy,
        } => EntityVisual::Single {
            mesh: &meshes.shot,
            tint: ENEMY_SHOT_COLOR,
        },
        EntityKind::Shot {
            owner: ShotOwner::Tank,
        } => EntityVisual::Single {
            mesh: &meshes.tank_shell,
            tint: TANK_SHOT_COLOR,
        },
    }
}

/// Dead-reckoned position for `e` given how long ago the snapshot
/// arrived. The server's authoritative position lags by ~50 ms (one
/// snapshot interval); rendering at `pos + vel * elapsed` masks the
/// stair-step that the raw snapshot produces. We clamp the lookahead
/// so packet loss or a hidden-tab pause doesn't fling sprites across
/// the world before the next snapshot lands. X wraps with the world.
fn extrapolated_pos(e: &EntityState, time_since_snapshot: f32) -> Vec2 {
    // 120 ms covers two snapshot intervals — enough to ride out one
    // missed packet without letting drift accumulate visibly.
    const MAX_LOOKAHEAD_SECS: f32 = 0.12;
    let t = time_since_snapshot.min(MAX_LOOKAHEAD_SECS);
    let world_w = sim::world::WORLD_WIDTH;
    let mut x = e.pos.x + e.vel.x * t;
    let y = e.pos.y + e.vel.y * t;
    x = x.rem_euclid(world_w);
    Vec2::new(x, y)
}

/// Rough half-extent (world units) of each entity kind's mesh. Used to
/// decide whether to draw an entity at a wrap-mirrored copy on the
/// opposite side of the world seam. Kept generous so we never pop a
/// sprite in late.
fn sprite_half_extent(kind: &EntityKind) -> f32 {
    use sim::entity::ShotOwner;
    match kind {
        EntityKind::Player { .. } | EntityKind::Enemy => 18.0,
        // Tank silhouette is widest at the cannon when the turret is
        // horizontal — about 22 world units from chassis center.
        EntityKind::Tank => 24.0,
        EntityKind::Shot {
            owner: ShotOwner::Tank,
        } => 9.0,
        EntityKind::Shot { .. } => 6.0,
    }
}

/// Draw the animated tread links for one tank on top of its chassis.
/// Links are anchored to ground X (a function of `pos.x` alone) so they
/// appear to slide backwards as the chassis rolls forward. `cand` is
/// the chassis's wrap-adjusted world X for the current draw copy.
fn draw_tank_treads(
    canvas: &mut Canvas,
    camera: &Camera,
    tank: &TankMesh,
    pos: Vec2,
    cand: f32,
    scale: f32,
) {
    let spacing = TANK_TREAD_LINK_SPACING;
    // Range of integer link indices whose world X falls within the
    // tread band centred on `pos.x`.
    let k_min = ((pos.x - TANK_TREAD_HALF_WIDTH) / spacing).ceil() as i32;
    let k_max = ((pos.x + TANK_TREAD_HALF_WIDTH) / spacing).floor() as i32;
    let band_world_y = pos.y + TANK_TREAD_BAND_Y;
    for k in k_min..=k_max {
        let local_offset = (k as f32) * spacing - pos.x;
        let world = Vec2::new(cand + local_offset, band_world_y);
        let screen = camera.world_to_screen(world);
        canvas.draw(
            &tank.tread_link,
            DrawParam::new()
                .dest(screen)
                .rotation(0.0)
                .scale([scale, scale])
                .color(TANK_TREAD_LINK_COLOR),
        );
    }
}

pub struct MainState {
    asset_manager: AssetManager,
    camera: Camera,
    net: Box<dyn Net>,
    /// Procedurally-built mesh per entity kind. Cheaper to retint at
    /// draw time than to reload art.
    meshes: EntityMeshes,
    sky: Sky,
    /// Lazily-built terrain mesh + grass. Rebuilt when the server provides a new layout
    /// today: never after the first snapshot, but the renderer compares signatures every
    /// sync so it's safe.
    terrain_renderer: render::terrain::TerrainRenderer,
    shot_sound_id: SoundId,
    hit_sound_id: SoundId,
    input: InputState,
    local_player_id: Option<PlayerId>,
    latest_snapshot: Option<Snapshot>,
    /// True until we receive the first snapshot — we snap the camera
    /// straight onto the player instead of easing in.
    camera_initialized: bool,
    /// Monotonic counter we tag outgoing inputs with. The server doesn't yet
    /// use this for resimulation but the field shape matches what Phase 3
    /// will need.
    next_input_tick: Tick,
    gui_dirty: bool,
    score_text: TextWidget,
    level_text: TextWidget,
    game_over_text: TextWidget,
    game_over_hint: TextWidget,
    disconnected_text: TextWidget,
    /// Top-level UI state. See `AppState` for transitions.
    app_state: AppState,
    /// Title screen owns its own animation; ticked while `app_state == Menu`.
    menu: Menu,
    /// Edge-triggered: Space pressed in `Menu`. Consumed at the next update.
    request_start: bool,
    /// Edge-triggered: any key pressed in `GameOver`. Consumed at the next update.
    request_back_to_menu: bool,
    cached_score: i32,
    cached_level: i32,
    /// Seconds since `latest_snapshot` arrived. The server snapshots at
    /// 20 Hz but we render at ≥60 Hz, so without extrapolation entities
    /// visibly step every 50 ms. We dead-reckon `pos + vel * elapsed`
    /// between snapshots and reset to 0 on each new one.
    time_since_snapshot: f32,
    /// Active particle bursts. Owned client-side; not part of the sim.
    /// Each `PlayerKilled` event spawns one.
    explosions: Vec<Explosion>,
    /// Monotonic counter used as a per-explosion RNG seed so simultaneous
    /// bursts don't render identically.
    next_explosion_seed: u64,
    /// Flame trail behind any thrusting player.
    thrust: ThrustEmitter,
    /// Brown smoke streaming from damaged players.
    smoke: DamageSmoker,
    /// Set once `net.is_connected()` first returns false, to swap the overlay
    /// text and skip the input-send loop.
    disconnected: bool,
}

impl MainState {
    pub fn new(ctx: &mut Context, net: Box<dyn Net>) -> GameResult<MainState> {
        print_instructions();

        let mut am = AssetManager::new();

        let (drawable_w, drawable_h) = ctx.gfx.drawable_size();

        let meshes = EntityMeshes::build(ctx)?;
        let world_w = sim::world::WORLD_WIDTH;
        let world_h = sim::world::WORLD_HEIGHT;
        let sky = Sky::build(ctx, Vec2::new(world_w, world_h), 0xC10D_C10D)?;

        let shot_sound_id = am.add_sound(ctx, "/pew.ogg");
        let hit_sound_id = am.add_sound(ctx, "/boom.ogg");

        let score_text = TextWidget::new(ctx, &mut am, 18.0)?;
        let level_text = TextWidget::new(ctx, &mut am, 18.0)?;
        let mut game_over_text = TextWidget::new(ctx, &mut am, 48.0)?;
        game_over_text.set_text("GAME OVER", 48.0);
        let mut game_over_hint = TextWidget::new(ctx, &mut am, 22.0)?;
        game_over_hint.set_text("press any key for menu", 22.0);
        let mut disconnected_text = TextWidget::new(ctx, &mut am, 24.0)?;
        disconnected_text.set_text("Connecting…", 24.0);
        let menu = Menu::new(ctx, &mut am)?;

        // Use the deepest valley as the camera's floor reference so the
        // pilot can dive into low spots without the camera bottoming
        // out on the tallest peak.
        let ground_y = sim::terrain::GROUND_MIN_HEIGHT;
        let camera = Camera::new(
            drawable_w,
            drawable_h,
            world_w,
            world_h,
            VIEW_WIDTH,
            VIEW_HEIGHT,
            ground_y,
        );

        Ok(MainState {
            asset_manager: am,
            camera,
            net,
            meshes,
            sky,
            terrain_renderer: render::terrain::TerrainRenderer::empty(),
            shot_sound_id,
            hit_sound_id,
            input: InputState::default(),
            local_player_id: None,
            latest_snapshot: None,
            camera_initialized: false,
            next_input_tick: Tick(0),
            gui_dirty: true,
            score_text,
            level_text,
            game_over_text,
            game_over_hint,
            disconnected_text,
            app_state: AppState::Menu,
            menu,
            request_start: false,
            request_back_to_menu: false,
            cached_score: 0,
            cached_level: 0,
            explosions: Vec::new(),
            next_explosion_seed: 1,
            thrust: ThrustEmitter::new(0xF1A4E_AB1u64),
            smoke: DamageSmoker::new(0x5_E0FFEEu64),
            disconnected: false,
            time_since_snapshot: 0.0,
        })
    }

    fn handle_server_msg(&mut self, ctx: &mut Context, msg: ServerMsg) {
        match msg {
            ServerMsg::Welcome {
                player_id,
                snapshot,
                ..
            } => {
                self.local_player_id = Some(player_id);
                self.camera
                    .set_ground_y(sim::terrain::min_surface_y(&snapshot.terrain));
                self.terrain_renderer.sync(ctx, &snapshot.terrain);
                self.latest_snapshot = Some(snapshot);
                self.time_since_snapshot = 0.0;
                self.gui_dirty = true;
            }
            ServerMsg::Snapshot(snap) => {
                // Terrain doesn't change today, but both the camera clamp
                // and the cached terrain mesh need to follow if a future
                // server hands us a new layout. `sync` is a no-op when
                // the terrain matches the cached signature.
                self.camera
                    .set_ground_y(sim::terrain::min_surface_y(&snap.terrain));
                self.terrain_renderer.sync(ctx, &snap.terrain);
                self.latest_snapshot = Some(snap);
                self.time_since_snapshot = 0.0;
                // If we requested a respawn while in GameOver / Menu and
                // our entity is back in the world, drop the overlay so the
                // next snapshot draws live gameplay.
                if self.app_state == AppState::GameOver && self.local_player().is_some() {
                    self.app_state = AppState::Playing;
                    self.gui_dirty = true;
                }
            }
            ServerMsg::Events { events, .. } => {
                // While the title screen is up we don't want incidental
                // explosions / shot sounds from the live world leaking
                // through (the player isn't watching it). Still process
                // them in Playing/GameOver so deaths and audio land.
                if self.app_state != AppState::Menu {
                    self.handle_game_events(ctx, &events);
                }
            }
        }
    }

    fn play_sound(&self, ctx: &Context, id: SoundId) {
        // ggez's wasm `play_detached` is now a no-op while the browser
        // `AudioContext` is suspended (so we don't leak rodio sinks), but
        // building a `Source` still touches the filesystem and allocates,
        // so skip the work entirely until audio is actually running.
        if !ctx.audio.is_running() {
            return;
        }
        self.asset_manager.play_sound(ctx, id);
    }

    fn handle_game_events(&mut self, ctx: &Context, events: &[GameEvent]) {
        for ev in events {
            match ev {
                GameEvent::ShotFired { .. } => {
                    self.play_sound(ctx, self.shot_sound_id);
                }
                GameEvent::EnemyKilled { pos, .. } => {
                    self.spawn_explosion(Vec2::new(pos.x, pos.y), ExplosionStyle::FieryBurst);
                    self.play_sound(ctx, self.hit_sound_id);
                    self.gui_dirty = true;
                }
                GameEvent::EnemyDamaged { pos, .. } => {
                    // Smaller feedback than a kill: a brief spark shower
                    // plus a few smoke puffs so the player can tell the
                    // bullet landed even though the hostile is still
                    // alive. Sparks read more clearly than smoke against
                    // armored hulls (tanks especially).
                    let p = Vec2::new(pos.x, pos.y);
                    self.smoke.spark_burst(p, 10);
                    self.smoke.puff_burst(p, 4);
                    self.play_sound(ctx, self.hit_sound_id);
                }
                GameEvent::PlayerDamaged { pos, .. } => {
                    // Player hit feedback: sparks plus smoke. The
                    // steady-state smoke trail is driven from snapshot
                    // HP, but a one-shot burst sells the impact at the
                    // right moment.
                    let p = Vec2::new(pos.x, pos.y);
                    self.smoke.spark_burst(p, 8);
                    self.smoke.puff_burst(p, 4);
                    self.play_sound(ctx, self.hit_sound_id);
                }
                GameEvent::PlayerKilled {
                    player_id,
                    pos,
                    cause,
                } => {
                    self.spawn_explosion(Vec2::new(pos.x, pos.y), ExplosionStyle::for_cause(cause));
                    self.play_sound(ctx, self.hit_sound_id);
                    // Only flip into GameOver if we're currently playing —
                    // dying while still on the title screen leaves the menu
                    // intact and we'll just Respawn whenever the player
                    // hits Space.
                    if Some(*player_id) == self.local_player_id
                        && self.app_state == AppState::Playing
                    {
                        self.app_state = AppState::GameOver;
                        self.gui_dirty = true;
                    }
                }
                GameEvent::ShellExploded { pos } => {
                    // Tank shells terminate in a chunky boom regardless
                    // of whether they hit dirt, a tank, or the player.
                    // Pair the dust-style explosion with the standard
                    // hit sound so the impact lands audibly.
                    self.spawn_explosion(Vec2::new(pos.x, pos.y), ExplosionStyle::DustAndEmbers);
                    self.play_sound(ctx, self.hit_sound_id);
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

    fn extrapolated_pos(&self, e: &EntityState) -> Vec2 {
        extrapolated_pos(e, self.time_since_snapshot)
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

        let hint_w = self.game_over_hint.width(ctx);
        self.game_over_hint.set_position(Point2::new(
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

    /// Draw an entity at every visible toroidal copy. Only X wraps — Y
    /// is a hard wall in the sim — so we ask the camera for the up-to-
    /// three candidate X positions that land inside the viewport.
    ///
    /// Ships draw their body and wings separately so the wings can be
    /// scaled along the local-X axis (`ship_wing_factor`) to fake banking.
    /// Pre-rotation scaling means the foreshortening rotates correctly
    /// with the ship's facing.
    fn draw_entity(&self, canvas: &mut Canvas, entity: &EntityState) {
        let visual = visual_for_kind(&self.meshes, &entity.kind);
        let half = sprite_half_extent(&entity.kind);
        let scale = self.camera.scale();
        let pos = self.extrapolated_pos(entity);
        for cand in self
            .camera
            .world_x_offsets_for(pos.x, half)
            .into_iter()
            .flatten()
        {
            let screen = self.camera.world_to_screen(Vec2::new(cand, pos.y));
            match visual {
                EntityVisual::Single { mesh, tint } => {
                    let params = DrawParam::new()
                        .dest(screen)
                        .rotation(entity.facing)
                        .scale([scale, scale])
                        .color(tint);
                    canvas.draw(mesh, params);
                }
                EntityVisual::Ship { ship, tint } => {
                    let wing = ship_wing_factor(entity.facing);
                    let base = DrawParam::new()
                        .dest(screen)
                        .rotation(entity.facing)
                        .color(tint);
                    canvas.draw(&ship.wings, base.scale([scale * wing, scale]));
                    canvas.draw(&ship.body, base.scale([scale, scale]));
                }
                EntityVisual::Tank { tank, tint } => {
                    // Chassis: body angle determines whether we flip
                    // horizontally. Body facing of +PI/2 (right) yields
                    // scale_x = +1; -PI/2 (left) yields -1; falling back
                    // to +1 when facing is exactly 0 (just-spawned tank
                    // before it picked a side).
                    let body_dir = if entity.facing < 0.0 { -1.0 } else { 1.0 };
                    canvas.draw(
                        &tank.chassis,
                        DrawParam::new()
                            .dest(screen)
                            .rotation(0.0)
                            .scale([scale * body_dir, scale])
                            .color(tint),
                    );
                    // Tread link overlay — drawn at world positions
                    // anchored to the ground (a function of pos.x), so
                    // they appear to scroll opposite to motion as the
                    // chassis rolls. Independent of body_dir flip.
                    draw_tank_treads(canvas, &self.camera, tank, pos, cand, scale);
                    // Turret: pivots on top of the hull rather than at
                    // the chassis center, so we offset the destination
                    // by `TANK_TURRET_PIVOT_Y` world units in Y-up.
                    let turret_world = Vec2::new(cand, pos.y + TANK_TURRET_PIVOT_Y);
                    let turret_screen = self.camera.world_to_screen(turret_world);
                    canvas.draw(
                        &tank.turret,
                        DrawParam::new()
                            .dest(turret_screen)
                            .rotation(entity.turret_facing)
                            .scale([scale, scale])
                            .color(tint),
                    );
                }
            }
        }
    }

    /// Render the live world + HUD (Playing / GameOver). The Menu draw
    /// path is handled separately in `draw`.
    fn draw_world(&mut self, _ctx: &mut Context, canvas: &mut Canvas) {
        if let Some(snap) = &self.latest_snapshot {
            // Background: cream sky + parallax clouds, behind everything.
            self.sky.draw(canvas, &self.camera);

            // Terrain (soil polygon + horizon stripe + grass tufts)
            // underneath the play layer. The renderer was synced from
            // the latest snapshot in `handle_server_msg`, so we don't
            // re-touch `snap.terrain` here.
            self.terrain_renderer.draw(canvas, &self.camera);

            // Thrust trails behind ships (drawn before the ships so the
            // flame appears to come out of the tail rather than over it).
            self.thrust.draw(canvas, &self.camera);

            for entity in &snap.entities {
                if !entity.alive {
                    continue;
                }
                self.draw_entity(canvas, entity);
            }

            // Damage smoke on top of ships so torn-up hulls smolder
            // visibly, then explosions on top of everything.
            self.smoke.draw(canvas, &self.camera);
            for ex in &self.explosions {
                ex.draw(canvas, &self.camera);
            }

            self.level_text.draw(canvas);
            self.score_text.draw(canvas);
            if let Some(p) = self.local_player() {
                if p.alive && p.max_hp > 0 {
                    self.draw_hp_bar(canvas, p.hp, p.max_hp);
                }
            }
            if self.app_state == AppState::GameOver {
                self.game_over_text.draw(canvas);
                self.game_over_hint.draw(canvas);
            }
            if self.disconnected {
                self.disconnected_text.draw(canvas);
            }
        } else {
            self.disconnected_text.draw(canvas);
        }
    }

    /// Apply pending UI transitions queued from key handlers. Runs once at
    /// the top of `update` so the rest of the frame sees the new state.
    fn apply_state_transitions(&mut self) {
        if self.request_start && self.app_state == AppState::Menu {
            self.request_start = false;
            // If we're currently dead, ask the server to respawn us before
            // entering Playing. Otherwise just take the camera off the menu.
            if self.local_player().is_none() && !self.disconnected {
                self.net.send(&ClientMsg::Respawn);
            }
            // Drop any keys that were already held while on the menu so we
            // don't immediately fire / thrust from a stale state.
            self.input = InputState::default();
            self.app_state = AppState::Playing;
            self.gui_dirty = true;
        }
        if self.request_back_to_menu && self.app_state == AppState::GameOver {
            self.request_back_to_menu = false;
            self.menu.set_last_score(Some(self.cached_score));
            self.input = InputState::default();
            self.app_state = AppState::Menu;
            self.gui_dirty = true;
        }
    }

    /// HP bar pinned to the top-left, just under the score/level text. Shows
    /// only when the local player has a meaningful HP — i.e. they're alive
    /// and the snapshot carries a max_hp > 0.
    fn draw_hp_bar(&self, canvas: &mut Canvas, hp: i16, max_hp: i16) {
        let bar_w: f32 = 200.0;
        let bar_h: f32 = 10.0;
        let x: f32 = 10.0;
        let y: f32 = 36.0;
        // Background — semi-translucent dark plate so the bar reads
        // against any sky color.
        canvas.draw(
            &graphics::Quad,
            DrawParam::new()
                .dest(Vec2::new(x - 1.0, y - 1.0))
                .scale([bar_w + 2.0, bar_h + 2.0])
                .color(Color::new(0.20, 0.10, 0.12, 0.75)),
        );
        canvas.draw(
            &graphics::Quad,
            DrawParam::new()
                .dest(Vec2::new(x, y))
                .scale([bar_w, bar_h])
                .color(Color::new(0.32, 0.22, 0.20, 1.0)),
        );
        let frac = (hp as f32 / max_hp as f32).clamp(0.0, 1.0);
        let fill_w = bar_w * frac;
        // Color shifts from green-ish at full to red as you take hits.
        let fill = if frac > 0.5 {
            Color::new(0.55, 0.78, 0.40, 1.0)
        } else if frac > 0.25 {
            Color::new(0.95, 0.78, 0.32, 1.0)
        } else {
            Color::new(0.92, 0.30, 0.28, 1.0)
        };
        canvas.draw(
            &graphics::Quad,
            DrawParam::new()
                .dest(Vec2::new(x, y))
                .scale([fill_w, bar_h])
                .color(fill),
        );
    }
}

impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        const DESIRED_FPS: u32 = 60;
        // Bound catch-up work so re-foregrounding doesn't stall on a full queue.
        const MAX_DRAIN_PER_UPDATE: usize = 64;

        // Skip everything while hidden — playing audio for queued events
        // leaks rodio sinks on wasm. Drain residual dt so unhiding doesn't
        // fire thousands of catch-up input ticks.
        if !ctx.gfx.is_page_visible() {
            while ctx.time.check_update_time(DESIRED_FPS) {}
            return Ok(());
        }

        let mut drained = 0;
        while drained < MAX_DRAIN_PER_UPDATE {
            let Some(msg) = self.net.try_recv() else {
                break;
            };
            self.handle_server_msg(ctx, msg);
            drained += 1;
        }

        let connected = self.net.is_connected();
        if !connected && !self.disconnected {
            self.disconnected = true;
            self.disconnected_text
                .set_text("Disconnected — server unreachable", 24.0);
            self.gui_dirty = true;
        }

        // Resolve pending UI transitions before the fixed-step loop runs,
        // so the right state's input/animation runs this frame.
        self.apply_state_transitions();

        while ctx.time.check_update_time(DESIRED_FPS) {
            if self.input.quit {
                ctx.request_quit();
                self.net.send(&ClientMsg::Bye);
                break;
            }

            // Sends to a dead socket allocate and drop; skip the churn.
            if !connected {
                continue;
            }

            // Only forward live inputs while playing. In Menu / GameOver the
            // ship sits idle on the server (or stays dead) until the player
            // launches into Playing.
            let outgoing_input = if self.app_state == AppState::Playing {
                self.input.to_player_input()
            } else {
                sim::PlayerInput::default()
            };
            self.next_input_tick = self.next_input_tick.next();
            let input_msg = ClientMsg::Input {
                tick: self.next_input_tick,
                input: outgoing_input,
            };
            self.net.send(&input_msg);
        }

        // Step active explosions on real elapsed time so they look the
        // same regardless of the fixed-step input cadence.
        let dt = ctx.time.delta().as_secs_f32();
        self.time_since_snapshot += dt;
        for ex in &mut self.explosions {
            ex.update(dt);
        }
        self.explosions.retain(|e| !e.done());

        // Drive camera + particle systems from the latest snapshot. The
        // camera tracks the local player horizontally; particle emitters
        // read `thrusting` / `hp` directly from the snapshot so a freshly-
        // taken hit shows smoke before the next event arrives. Take()ing
        // the snapshot lets us pass &self to extrapolated_pos while also
        // mutating self.{camera,thrust,smoke}; we put it back before
        // returning so subsequent reads still see the latest state. Cheap
        // — Option::take/replace just moves the snapshot, no clone.
        let time_since_snapshot = self.time_since_snapshot;
        if let Some(snap) = self.latest_snapshot.take() {
            // Local player drives the camera. Snap on first frame, ease
            // afterwards.
            let local = self.local_player_id.and_then(|pid| {
                snap.entities.iter().find(|e| match e.kind {
                    EntityKind::Player { player_id } => player_id == pid && e.alive,
                    _ => false,
                })
            });
            if let Some(p) = local {
                let target = extrapolated_pos(p, time_since_snapshot);
                if !self.camera_initialized {
                    self.camera.snap_to(target);
                    self.camera_initialized = true;
                } else {
                    self.camera.follow(target, dt * CAMERA_FOLLOW_RATE);
                }
            }
            // Pump every player's thrust/smoke emitters every frame so
            // they stop cleanly when the ship dies or the flag flips.
            // Tanks also smoke when damaged (lower intensity so the
            // "a little bit of smoke" reads as battlefield damage rather
            // than a death-spiral). Smoke is anchored to the turret
            // dome so it puffs out of the hull top instead of the
            // treads.
            for e in &snap.entities {
                if !e.alive {
                    continue;
                }
                let pos = extrapolated_pos(e, time_since_snapshot);
                match e.kind {
                    EntityKind::Player { .. } => {
                        self.thrust
                            .note_thrust(e.id, pos, e.facing, dt, e.thrusting);
                        self.smoke.note_health(e.id, pos, e.hp, e.max_hp, 1.0, dt);
                    }
                    EntityKind::Tank => {
                        let smoke_pos = Vec2::new(pos.x, pos.y + TANK_TURRET_PIVOT_Y);
                        self.smoke
                            .note_health(e.id, smoke_pos, e.hp, e.max_hp, 0.55, dt);
                    }
                    _ => {}
                }
            }
            // Forget particles for entities not in the snapshot. Linear
            // scan over the (small) entity list beats allocating a HashSet
            // every frame for typical entity counts.
            let entities = &snap.entities;
            self.thrust
                .retain_ids(|id| entities.iter().any(|e| e.id == id));
            self.smoke
                .retain_ids(|id| entities.iter().any(|e| e.id == id));
            self.latest_snapshot = Some(snap);
        }
        self.thrust.update(dt);
        self.smoke.update(dt);
        let world_size = Vec2::new(sim::world::WORLD_WIDTH, sim::world::WORLD_HEIGHT);
        self.sky.update(dt, world_size);
        if self.app_state == AppState::Menu {
            self.menu.update(dt, self.camera.screen_size());
        }

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
        if !ctx.gfx.is_page_visible() {
            return Ok(());
        }
        let mut canvas = graphics::Canvas::from_frame(ctx, SKY_COLOR);

        match self.app_state {
            AppState::Menu => {
                let screen = self.camera.screen_size();
                self.menu.draw(ctx, &mut canvas, &self.meshes, screen);
                if self.disconnected {
                    self.disconnected_text.draw(&mut canvas);
                }
            }
            AppState::Playing | AppState::GameOver => {
                self.draw_world(ctx, &mut canvas);
            }
        }

        canvas.finish(ctx)?;
        ggez::timer::yield_now();
        Ok(())
    }

    fn key_down_event(&mut self, _ctx: &mut Context, input: KeyInput, repeat: bool) -> GameResult {
        // Esc always quits, regardless of which screen we're on.
        let code = match input.event.physical_key {
            ggez::winit::keyboard::PhysicalKey::Code(c) => Some(c),
            _ => None,
        };
        if code == Some(ggez::input::keyboard::KeyCode::Escape) {
            self.input.quit = true;
            return Ok(());
        }

        match self.app_state {
            AppState::Menu => {
                if !repeat && code == Some(ggez::input::keyboard::KeyCode::Space) {
                    self.request_start = true;
                }
            }
            AppState::Playing => {
                self.input.handle_key_down(input);
            }
            AppState::GameOver => {
                // Any key press returns to the menu. Ignore key repeat so
                // a held key doesn't immediately bounce us in and out.
                if !repeat {
                    self.request_back_to_menu = true;
                }
            }
        }
        Ok(())
    }

    fn key_up_event(&mut self, _ctx: &mut Context, input: KeyInput) -> GameResult {
        // Only Playing tracks held-key state; other states ignore key-ups.
        if self.app_state == AppState::Playing {
            self.input.handle_key_up(input);
        }
        Ok(())
    }

    fn resize_event(&mut self, _ctx: &mut Context, width: f32, height: f32) -> GameResult {
        self.camera.set_screen_size(width, height);
        self.gui_dirty = true;
        Ok(())
    }
}

/// Build a ggez `ContextBuilder` configured the way Icarust wants it. The
/// caller drives the actual context construction via
/// [`ContextBuilder::custom_run`], which abstracts over the native/web split.
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

    // `run_with` lets us capture `net` in the state-builder closure — the
    // plain `run::<G>()` path only passes `&mut Context` to `Game::new`.
    build_ggez(Some(resource_dir)).run_with(move |ctx| MainState::new(ctx, net))
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
/// `WebSocket`-backed `Net`, and delegates to `ContextBuilder::run_with`,
/// which on wasm spawns the async build/state setup onto the JS event loop
/// and returns immediately.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() -> Result<(), wasm_bindgen::JsValue> {
    use crate::net::WebNet;

    console_error_panic_hook::set_once();
    // tracing-wasm defaults to `report_logs_in_timings: true`, which calls
    // `performance.mark()` + `performance.measure()` for every span — the
    // browser then retains those PerformanceMark/Measure entries forever
    // (no rotation), leaking ~70KB/sec while the game is running. Disable
    // the timings path; we still get console.log output.
    let mut tracing_cfg = tracing_wasm::WASMLayerConfigBuilder::new();
    tracing_cfg.set_report_logs_in_timings(false);
    tracing_cfg.set_max_level(tracing::Level::INFO);
    tracing_wasm::set_as_global_default_with_config(tracing_cfg.build());

    // ggez's WebGpuUnavailable error gets swallowed inside `run_with` (it
    // only goes to console.error), so catch the most common cause —
    // `navigator.gpu` missing entirely — up front and put a readable note
    // in the page's status banner.
    if !webgpu_available() {
        show_init_error(
            "Your browser doesn't expose WebGPU. \
             Try Chrome with `chrome://flags/#enable-unsafe-webgpu`, \
             or Firefox Nightly with `dom.webgpu.enabled`.",
        );
        return Ok(());
    }

    let (url, name) = wasm_parse_args();
    tracing::info!(%url, %name, "connecting");
    let net = WebNet::connect(&url, name)
        .map_err(|e| wasm_bindgen::JsValue::from_str(&format!("WebNet::connect: {e:#}")))?;

    let _ =
        build_ggez(None).run_with(move |ctx| MainState::new(ctx, Box::new(net) as Box<dyn Net>));
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn webgpu_available() -> bool {
    web_sys::window()
        .map(|w| {
            let nav = w.navigator();
            !js_sys::Reflect::get(&nav, &wasm_bindgen::JsValue::from_str("gpu"))
                .map(|v| v.is_undefined() || v.is_null())
                .unwrap_or(true)
        })
        .unwrap_or(false)
}

/// Write a fallback message into `#status` (the boot banner in index.html) so
/// init failures are visible without opening devtools.
#[cfg(target_arch = "wasm32")]
fn show_init_error(msg: &str) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Some(el) = doc.get_element_by_id("status") {
            el.set_text_content(Some(msg));
        }
    }
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
