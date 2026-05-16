//! Title / intro screen.
//!
//! Owns its own deterministic-seeded animation (ships drifting across the
//! screen, clouds, etc.) so the screen always has motion regardless of
//! what the simulation is doing. Coordinates are in screen pixels — the
//! menu never touches the game camera or world snapshot.
//!
//! Transitions are driven from `MainState`: Space launches into the game,
//! `set_last_score` shows the previous run's score under the title after
//! the player comes back from a death.

use ggez::glam::Vec2;
use ggez::graphics::{
    Canvas, Color, DrawMode, DrawParam, InstanceArray, Mesh, MeshBuilder, MeshData, Vertex,
};
use ggez::{Context, GameResult};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::assets::AssetManager;
use crate::render::camera::Point2;
use crate::render::entities::{ship_wing_factor, EntityMeshes, ENEMY_COLOR, PLAYER_COLOR};
use crate::widget::TextWidget;

/// World-coords scale for menu ships. Ships are larger than in-game so
/// the silhouettes read at a glance even with the title text dominating.
const SHIP_SCALE: f32 = 1.8;
/// How far off-screen a ship/cloud must drift before it wraps to the
/// other side. Generous so we never pop sprites in mid-screen.
const WRAP_MARGIN: f32 = 100.0;
/// Vertical band the bg ships are allowed to use (fractions of screen
/// height). Top stays clear of the title, bottom clear of the controls
/// hint so ships don't drift behind text.
const SHIP_BAND_TOP: f32 = 0.10;
const SHIP_BAND_BOTTOM: f32 = 0.80;

/// Menu palette — pale, cool sky-blue with deep navy ink. Chosen
/// independently from the in-game cream sky so the menu reads as a
/// distinct screen rather than "the world with text over it". The
/// sky is drawn as a two-band gradient (lighter at the horizon) so
/// the screen has a sense of depth without any ground geometry.
const MENU_SKY_TOP: Color = Color::new(0.62, 0.78, 0.93, 1.0);
const MENU_SKY_BOTTOM: Color = Color::new(0.88, 0.94, 0.98, 1.0);
const CLOUD_COLOR: Color = Color::new(0.98, 0.99, 1.00, 1.0);
const TITLE_COLOR: Color = Color::new(0.13, 0.20, 0.36, 1.0);
const TITLE_SHADOW_COLOR: Color = Color::new(0.08, 0.13, 0.26, 0.35);
const PROMPT_COLOR: Color = Color::new(0.16, 0.24, 0.40, 1.0);
const HINT_TEXT_COLOR: Color = Color::new(0.18, 0.26, 0.44, 1.0);
/// Vertical margin from the bottom of the screen to the controls hint
/// baseline. Keeps the text well clear of any "ground" edge feeling.
const HINT_BOTTOM_MARGIN: f32 = 32.0;

struct BgShip {
    /// Screen-pixel position (top-left origin, Y-down).
    pos: Vec2,
    /// Velocity in pixels per second.
    vel: Vec2,
    is_enemy: bool,
}

impl BgShip {
    /// Rotation that puts the ship's nose along its velocity direction.
    /// Ship meshes are authored nose-up (local -Y), so we map a screen-
    /// space velocity to the equivalent rotation around screen origin.
    fn facing(&self) -> f32 {
        self.vel.x.atan2(-self.vel.y)
    }
}

struct BgCloud {
    pos: Vec2,
    drift: f32,
    mesh_idx: usize,
    scale: f32,
}

pub struct Menu {
    title: TextWidget,
    title_shadow: TextWidget,
    prompt: TextWidget,
    controls_hint: TextWidget,
    score_hint: TextWidget,
    ships: Vec<BgShip>,
    clouds: Vec<BgCloud>,
    cloud_meshes: Vec<Mesh>,
    /// Per-mesh `InstanceArray` reused each frame so all clouds sharing
    /// a stamp land in a single draw call.
    cloud_instances: Vec<InstanceArray>,
    /// Single 1×1 quad with vertex colors that interpolates from
    /// `MENU_SKY_TOP` at y=0 to `MENU_SKY_BOTTOM` at y=1. Drawn scaled to
    /// screen size each frame so a resize doesn't need a rebuild.
    sky_gradient: Mesh,
    /// Seconds the menu has been visible; drives the pulsing prompt.
    elapsed: f32,
    /// Screen size cached from the last update so draws stay aligned with
    /// the layout we positioned animation against.
    last_screen: Vec2,
}

impl Menu {
    pub fn new(ctx: &mut Context, am: &mut AssetManager) -> GameResult<Self> {
        let mut title = TextWidget::new(ctx, am, 112.0)?;
        title.set_text("ICARUST", 112.0);
        let mut title_shadow = TextWidget::new(ctx, am, 112.0)?;
        title_shadow.set_text("ICARUST", 112.0);

        let mut prompt = TextWidget::new(ctx, am, 30.0)?;
        prompt.set_text("PRESS SPACE TO LAUNCH", 30.0);

        let mut controls_hint = TextWidget::new(ctx, am, 18.0)?;
        controls_hint.set_text(
            "ARROWS TURN  /  UP THRUSTS  /  SPACE FIRES  /  ESC QUITS",
            18.0,
        );

        let mut score_hint = TextWidget::new(ctx, am, 22.0)?;
        score_hint.set_text("", 22.0);

        // Deterministic seed — same layout every boot keeps the screen
        // recognizable without freezing the motion.
        let mut rng = ChaCha8Rng::seed_from_u64(0xAFEE_C0FF_E55E_1234);

        let cloud_meshes = vec![
            cloud_mesh(
                ctx,
                &[
                    (0.0, 0.0, 28.0),
                    (24.0, -2.0, 22.0),
                    (-22.0, 0.0, 20.0),
                    (8.0, -14.0, 16.0),
                ],
            )?,
            cloud_mesh(
                ctx,
                &[
                    (0.0, 0.0, 38.0),
                    (30.0, 6.0, 26.0),
                    (-30.0, 4.0, 22.0),
                    (6.0, -16.0, 22.0),
                ],
            )?,
            cloud_mesh(
                ctx,
                &[(0.0, 0.0, 22.0), (16.0, -2.0, 18.0), (-16.0, 0.0, 16.0)],
            )?,
        ];

        let initial_screen = Vec2::new(1280.0, 540.0);
        let mut ships = Vec::with_capacity(6);
        for i in 0..6 {
            let is_enemy = i % 2 == 1;
            let dir = if rng.gen::<bool>() { 1.0 } else { -1.0 };
            let speed = 70.0 + rng.gen::<f32>() * 90.0;
            let vy = (rng.gen::<f32>() - 0.5) * 30.0;
            let y_frac = SHIP_BAND_TOP + rng.gen::<f32>() * (SHIP_BAND_BOTTOM - SHIP_BAND_TOP);
            ships.push(BgShip {
                pos: Vec2::new(
                    rng.gen::<f32>() * initial_screen.x,
                    y_frac * initial_screen.y,
                ),
                vel: Vec2::new(dir * speed, vy),
                is_enemy,
            });
        }

        let mut clouds = Vec::with_capacity(9);
        for _ in 0..9 {
            clouds.push(BgCloud {
                pos: Vec2::new(
                    rng.gen::<f32>() * initial_screen.x,
                    rng.gen::<f32>() * initial_screen.y * 0.5,
                ),
                drift: (rng.gen::<f32>() - 0.5) * 14.0,
                mesh_idx: rng.gen_range(0..cloud_meshes.len()),
                scale: 0.85 + rng.gen::<f32>() * 0.55,
            });
        }

        let sky_gradient = gradient_quad(ctx, MENU_SKY_TOP, MENU_SKY_BOTTOM);
        let cloud_instances: Vec<InstanceArray> = (0..cloud_meshes.len())
            .map(|_| InstanceArray::new(ctx, None))
            .collect();

        Ok(Menu {
            title,
            title_shadow,
            prompt,
            controls_hint,
            score_hint,
            ships,
            clouds,
            cloud_meshes,
            cloud_instances,
            sky_gradient,
            elapsed: 0.0,
            last_screen: initial_screen,
        })
    }

    pub fn update(&mut self, dt: f32, screen: Vec2) {
        self.elapsed += dt;
        // If the user resized, re-anchor sprites that fall outside the
        // new band rather than letting them drift forever off-screen.
        let rescaled = (screen - self.last_screen).length() > 1.0;
        self.last_screen = screen;

        let band_top = screen.y * SHIP_BAND_TOP;
        let band_bot = screen.y * SHIP_BAND_BOTTOM;
        for ship in &mut self.ships {
            ship.pos += ship.vel * dt;
            if ship.pos.x < -WRAP_MARGIN {
                ship.pos.x = screen.x + WRAP_MARGIN;
            } else if ship.pos.x > screen.x + WRAP_MARGIN {
                ship.pos.x = -WRAP_MARGIN;
            }
            if rescaled {
                ship.pos.y = ship.pos.y.clamp(band_top, band_bot);
            }
            // Gentle vertical bounce so they don't all clump at the top.
            ship.pos.y = ship.pos.y.clamp(band_top, band_bot);
        }
        let cloud_top = screen.y * 0.45;
        for cloud in &mut self.clouds {
            cloud.pos.x += cloud.drift * dt;
            if cloud.pos.x < -WRAP_MARGIN {
                cloud.pos.x = screen.x + WRAP_MARGIN;
            } else if cloud.pos.x > screen.x + WRAP_MARGIN {
                cloud.pos.x = -WRAP_MARGIN;
            }
            if rescaled {
                cloud.pos.y = cloud.pos.y.min(cloud_top);
            }
        }
    }

    /// Set the line shown under the title — e.g. "LAST SCORE: 5" after
    /// returning from a death. Pass `None` to clear.
    pub fn set_last_score(&mut self, score: Option<i32>) {
        match score {
            Some(s) => self.score_hint.set_text(&format!("LAST SCORE: {s}"), 22.0),
            None => self.score_hint.set_text("", 22.0),
        }
    }

    pub fn draw(
        &mut self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        meshes: &EntityMeshes,
        screen: Vec2,
    ) {
        // Pale-blue sky gradient: deeper at the top, almost-white near
        // the horizon. The mesh is authored on a unit rect; scale fills
        // the screen.
        canvas.draw(
            &self.sky_gradient,
            DrawParam::new()
                .dest(Vec2::ZERO)
                .scale([screen.x, screen.y]),
        );

        // Clouds: one batched draw per stamp instead of one per cloud.
        for inst in self.cloud_instances.iter_mut() {
            inst.clear();
        }
        for cloud in &self.clouds {
            self.cloud_instances[cloud.mesh_idx].push(
                DrawParam::new()
                    .dest(cloud.pos)
                    .scale([cloud.scale, cloud.scale])
                    .color(CLOUD_COLOR),
            );
        }
        for (mesh, inst) in self.cloud_meshes.iter().zip(self.cloud_instances.iter()) {
            if inst.instances().is_empty() {
                continue;
            }
            canvas.draw_instanced_mesh(mesh.clone(), inst, DrawParam::default());
        }

        for ship in &self.ships {
            let tint = if ship.is_enemy { ENEMY_COLOR } else { PLAYER_COLOR };
            let mesh = if ship.is_enemy {
                &meshes.enemy
            } else {
                &meshes.player
            };
            let facing = ship.facing();
            let wing = ship_wing_factor(facing);
            let base = DrawParam::new()
                .dest(ship.pos)
                .rotation(facing)
                .color(tint);
            canvas.draw(&mesh.wings, base.scale([SHIP_SCALE * wing, SHIP_SCALE]));
            canvas.draw(&mesh.body, base.scale([SHIP_SCALE, SHIP_SCALE]));
        }

        // Title — centered, with a faint drop shadow for depth.
        let title_w = self.title.width(ctx);
        let title_h = self.title.height(ctx);
        let title_x = (screen.x - title_w) / 2.0;
        let title_y = (screen.y * 0.18 - title_h * 0.5).max(20.0);
        self.title_shadow
            .set_position(Point2::new(title_x + 4.0, title_y + 4.0));
        self.title.set_position(Point2::new(title_x, title_y));
        self.title_shadow.draw_with(canvas, TITLE_SHADOW_COLOR);
        self.title.draw_with(canvas, TITLE_COLOR);

        // Score line just below the title (only when set).
        let sc_w = self.score_hint.width(ctx);
        let sc_y = title_y + title_h + 8.0;
        self.score_hint
            .set_position(Point2::new((screen.x - sc_w) / 2.0, sc_y));
        self.score_hint.draw_with(canvas, PROMPT_COLOR);

        // Pulsing "press space" prompt.
        let prompt_w = self.prompt.width(ctx);
        let prompt_y = screen.y * 0.62;
        self.prompt
            .set_position(Point2::new((screen.x - prompt_w) / 2.0, prompt_y));
        let pulse = 0.55 + 0.45 * (self.elapsed * 3.0).sin().abs();
        self.prompt.draw_with(
            canvas,
            Color::new(PROMPT_COLOR.r, PROMPT_COLOR.g, PROMPT_COLOR.b, pulse),
        );

        // Controls hint floats over the sky near the bottom — no band.
        // Margin keeps it well clear of the bottom edge so it doesn't read
        // like ground text.
        let ch_w = self.controls_hint.width(ctx);
        let ch_h = self.controls_hint.height(ctx);
        self.controls_hint.set_position(Point2::new(
            (screen.x - ch_w) / 2.0,
            screen.y - HINT_BOTTOM_MARGIN - ch_h,
        ));
        self.controls_hint.draw_with(canvas, HINT_TEXT_COLOR);
    }
}

/// Unit-rect mesh whose top edge is `top` and bottom edge is `bottom`.
/// ggez interpolates the per-vertex colors across the quad, giving a
/// smooth vertical gradient that scales cleanly to any screen size.
fn gradient_quad(ctx: &mut Context, top: Color, bottom: Color) -> Mesh {
    let top_rgba = [top.r, top.g, top.b, top.a];
    let bot_rgba = [bottom.r, bottom.g, bottom.b, bottom.a];
    let vertices = [
        Vertex {
            position: [0.0, 0.0],
            uv: [0.0, 0.0],
            color: top_rgba,
        },
        Vertex {
            position: [1.0, 0.0],
            uv: [1.0, 0.0],
            color: top_rgba,
        },
        Vertex {
            position: [1.0, 1.0],
            uv: [1.0, 1.0],
            color: bot_rgba,
        },
        Vertex {
            position: [0.0, 1.0],
            uv: [0.0, 1.0],
            color: bot_rgba,
        },
    ];
    let indices: [u32; 6] = [0, 1, 2, 0, 2, 3];
    Mesh::from_data(
        ctx,
        MeshData {
            vertices: &vertices,
            indices: &indices,
        },
    )
}

fn cloud_mesh(ctx: &mut Context, circles: &[(f32, f32, f32)]) -> GameResult<Mesh> {
    let mut mb = MeshBuilder::new();
    for (dx, dy, r) in circles {
        mb.circle(DrawMode::fill(), Vec2::new(*dx, *dy), *r, 0.5, Color::WHITE)?;
    }
    Ok(Mesh::from_data(ctx, mb.build()))
}
