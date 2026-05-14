//! Procedural meshes for the in-world entities.
//!
//! Avoiding PNG sprites means we can tint freely (DrawParam.color is
//! multiplied against the mesh's vertex colors) and keep the visual style
//! coherent. Each mesh is built once at startup; drawing rotates and
//! scales the cached `Mesh`.
//!
//! Local coordinates are screen-space (Y-down): a vertex at `(0, -10)`
//! sits above the origin. At `facing == 0` the world's +Y points "up the
//! screen," so meshes are authored nose-up in local coords. That keeps
//! the math consistent with how rocks/shots already use rotation = 0.

use ggez::glam::Vec2;
use ggez::graphics::{Color, DrawMode, Mesh, MeshBuilder, Rect};
use ggez::{Context, GameResult};

/// Maroon ink we use for the player ship body (matches the Luftrauser
/// reference). Tints stay close to a 2-color palette so the world reads
/// as a single illustration rather than a mash-up of asset styles.
pub const PLAYER_COLOR: Color = Color::new(0.36, 0.13, 0.17, 1.0);
/// Enemy hulls are the same dark maroon — tone-matched so the player has
/// to read shape, not color, to spot them.
pub const ENEMY_COLOR: Color = Color::new(0.30, 0.10, 0.13, 1.0);
/// Player bullets — dark maroon so they read against the cream sky.
/// (Bright bullets disappear into the background.)
pub const PLAYER_SHOT_COLOR: Color = Color::new(0.28, 0.08, 0.12, 1.0);
/// Enemy bullets — burnt orange, distinct from player shots so the
/// pilot can tell which way to dodge.
pub const ENEMY_SHOT_COLOR: Color = Color::new(0.82, 0.32, 0.14, 1.0);
/// Rocks reuse the dusty-brown tone seen in the rocks/dust palette.
pub const ROCK_COLOR: Color = Color::new(0.50, 0.36, 0.24, 1.0);
/// Thrust flame core (bright yellow).
pub const FLAME_CORE_COLOR: Color = Color::new(1.0, 0.92, 0.55, 1.0);
/// Thrust flame edge (orange).
pub const FLAME_EDGE_COLOR: Color = Color::new(0.95, 0.55, 0.18, 1.0);
/// Smoke / damage particles (dusky brown).
pub const SMOKE_COLOR: Color = Color::new(0.42, 0.32, 0.22, 1.0);

pub struct EntityMeshes {
    pub player: Mesh,
    pub enemy: Mesh,
    pub rock: Mesh,
    pub shot: Mesh,
}

impl EntityMeshes {
    pub fn build(ctx: &mut Context) -> GameResult<Self> {
        Ok(Self {
            player: build_player(ctx)?,
            enemy: build_enemy(ctx)?,
            rock: build_rock(ctx)?,
            shot: build_shot(ctx)?,
        })
    }
}

/// Plane silhouette pointed up at facing 0. Wings are a wider horizontal
/// bar; the fuselage is a thin vertical bar; a tail fin sits at the back.
/// Authored a touch larger than the sim's collision radius so the ship
/// reads at a glance against the sky.
fn build_player(ctx: &mut Context) -> GameResult<Mesh> {
    let mut mb = MeshBuilder::new();
    // Wings — broadest part of the silhouette so the ship reads at small
    // sizes. Slight rear-sweep by offsetting the bar a touch backwards.
    mb.rectangle(
        DrawMode::fill(),
        Rect::new(-12.0, -1.4, 24.0, 4.5),
        Color::WHITE,
    )?;
    // Fuselage — vertical bar, nose at -13, tail at +8.
    mb.rectangle(
        DrawMode::fill(),
        Rect::new(-2.0, -13.0, 4.0, 21.0),
        Color::WHITE,
    )?;
    // Tail fin / rear stabilizers.
    mb.rectangle(
        DrawMode::fill(),
        Rect::new(-5.0, 5.5, 10.0, 3.0),
        Color::WHITE,
    )?;
    // Nose tip — narrow triangle suggesting a forward gun.
    mb.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(-2.0, -13.0),
            Vec2::new(2.0, -13.0),
            Vec2::new(0.0, -17.0),
        ],
        Color::WHITE,
    )?;
    let data = mb.build();
    Ok(Mesh::from_data(ctx, data))
}

/// Enemy ship — slightly squatter than the player, with a swept-back
/// wing so it reads as "the bad guy" even at a quick glance.
fn build_enemy(ctx: &mut Context) -> GameResult<Mesh> {
    let mut mb = MeshBuilder::new();
    // Body — short, wide fuselage.
    mb.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(0.0, -13.0),
            Vec2::new(4.5, 5.0),
            Vec2::new(-4.5, 5.0),
        ],
        Color::WHITE,
    )?;
    // Wings — a leading-edge sweep using triangles for a more aggressive
    // silhouette than the player's rectangular bar.
    mb.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(-13.0, 4.0),
            Vec2::new(-2.5, -4.0),
            Vec2::new(-2.5, 4.0),
        ],
        Color::WHITE,
    )?;
    mb.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(13.0, 4.0),
            Vec2::new(2.5, -4.0),
            Vec2::new(2.5, 4.0),
        ],
        Color::WHITE,
    )?;
    // Rear tail bar.
    mb.rectangle(
        DrawMode::fill(),
        Rect::new(-5.5, 4.5, 11.0, 2.5),
        Color::WHITE,
    )?;
    let data = mb.build();
    Ok(Mesh::from_data(ctx, data))
}

/// Rough rock blob — irregular polygon so the rocks don't look like
/// perfect circles.
fn build_rock(ctx: &mut Context) -> GameResult<Mesh> {
    // Pre-rolled vertex set. Authored as a single deterministic blob so
    // every rock looks the same (we have one rock mesh, drawn many times).
    let pts: &[Vec2] = &[
        Vec2::new(-9.0, -3.0),
        Vec2::new(-7.0, -8.5),
        Vec2::new(-1.5, -10.5),
        Vec2::new(4.0, -9.0),
        Vec2::new(9.0, -4.5),
        Vec2::new(10.0, 2.0),
        Vec2::new(6.5, 8.0),
        Vec2::new(0.5, 10.5),
        Vec2::new(-5.0, 9.0),
        Vec2::new(-9.5, 4.5),
    ];
    let mut mb = MeshBuilder::new();
    mb.polygon(DrawMode::fill(), pts, Color::WHITE)?;
    let data = mb.build();
    Ok(Mesh::from_data(ctx, data))
}

/// Bullet — small bright pill. We draw two stacked rectangles so the
/// mesh has a visible "head" + "trail" silhouette when rotated.
fn build_shot(ctx: &mut Context) -> GameResult<Mesh> {
    let mut mb = MeshBuilder::new();
    // Bullet head: short bright rectangle, oriented along the +Y axis
    // (so rotating by `facing` aims it the same way as the ship).
    mb.rectangle(
        DrawMode::fill(),
        Rect::new(-1.5, -4.0, 3.0, 6.0),
        Color::WHITE,
    )?;
    // Trail: a slightly slimmer rectangle just behind it.
    mb.rectangle(
        DrawMode::fill(),
        Rect::new(-1.0, 2.0, 2.0, 3.0),
        Color::WHITE,
    )?;
    let data = mb.build();
    Ok(Mesh::from_data(ctx, data))
}
