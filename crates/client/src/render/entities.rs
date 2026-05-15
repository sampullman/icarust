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
//! the math consistent with how shots already use rotation = 0.
//!
//! Ships are split into a body mesh and a separate wing mesh so the wings
//! can be scaled independently to fake a banking effect: when the ship
//! flies horizontally on-screen the wings foreshorten, when it climbs
//! straight up they read at full span. See `ship_wing_factor`.

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
/// Tank hulls — olive-drab so they read as armored ground vehicles
/// against the dusty terrain.
pub const TANK_COLOR: Color = Color::new(0.28, 0.30, 0.18, 1.0);
/// Tread "link" overlay color — drawn over the dark tread band each
/// frame to fake the rolling-track motion. Kept independent of
/// `TANK_COLOR` so the contrast against the band stays readable even
/// if the body tint shifts (e.g. flashes white when hit).
pub const TANK_TREAD_LINK_COLOR: Color = Color::new(0.40, 0.42, 0.34, 1.0);
/// Player bullets — dark maroon so they read against the cream sky.
/// (Bright bullets disappear into the background.)
pub const PLAYER_SHOT_COLOR: Color = Color::new(0.28, 0.08, 0.12, 1.0);
/// Enemy bullets — burnt orange, distinct from player shots so the
/// pilot can tell which way to dodge.
pub const ENEMY_SHOT_COLOR: Color = Color::new(0.82, 0.32, 0.14, 1.0);
/// Tank shells — darker red than ship bullets, with a heavier silhouette.
/// Higher contrast against the sky so the player can spot the more
/// dangerous projectile in time to evade.
pub const TANK_SHOT_COLOR: Color = Color::new(0.55, 0.18, 0.10, 1.0);
/// Thrust flame core (bright yellow).
pub const FLAME_CORE_COLOR: Color = Color::new(1.0, 0.92, 0.55, 1.0);
/// Thrust flame edge (orange).
pub const FLAME_EDGE_COLOR: Color = Color::new(0.95, 0.55, 0.18, 1.0);
/// Smoke / damage particles (dusky brown).
pub const SMOKE_COLOR: Color = Color::new(0.42, 0.32, 0.22, 1.0);

/// Wings collapse no further than this fraction of their full span when
/// the ship is broadside to the camera (facing left/right). Going to
/// zero would make wings vanish at PI/2 — keeping a sliver here means
/// the silhouette never disappears completely.
const MIN_WING_FACTOR: f32 = 0.35;

/// How much wing should be visible given the ship's facing angle. Returns
/// 1.0 when pointing straight up or down (full wingspan visible from
/// above) and `MIN_WING_FACTOR` when pointing left or right (banked
/// so wings are nearly edge-on). Used to scale the wing mesh's local-X
/// axis at draw time.
pub fn ship_wing_factor(facing: f32) -> f32 {
    let vertical = facing.cos().abs();
    MIN_WING_FACTOR + (1.0 - MIN_WING_FACTOR) * vertical
}

/// A ship rendered as two layered meshes so the wings can scale
/// independently of the fuselage.
pub struct ShipMesh {
    pub body: Mesh,
    pub wings: Mesh,
}

/// A ground tank — a chassis (drawn horizontal) and a turret that
/// rotates independently to track the player. The turret mesh includes
/// the cannon barrel so a single rotation lines up both.
///
/// The chassis is authored slightly asymmetrically: the front (right
/// side in local space, body_dir=+1) has a more gradual armor slope so
/// flipping with body_dir actually changes the silhouette. Treads are a
/// separate mesh so the live tread link overlay can scroll over them
/// each frame.
pub struct TankMesh {
    pub chassis: Mesh,
    pub turret: Mesh,
    /// Small darker quad used as a single tread "link." Drawn per frame
    /// at scrolling positions to fake the rolling-track motion.
    pub tread_link: Mesh,
}

/// World-Y offset (Y-up) from the tank's `pos` to the turret's pivot
/// (center of the dome on top of the hull). The chassis mesh is
/// authored with its center at local origin; the turret is authored
/// with its dome centered at local origin and the cannon extending up,
/// and `draw_entity` shifts its destination by this much so it sits on
/// top of the hull rather than at the chassis center.
pub const TANK_TURRET_PIVOT_Y: f32 = 6.0;

/// Spacing in world units between successive tread links. Chosen to fit
/// roughly 7–8 links across the 30-unit tread band so motion reads.
pub const TANK_TREAD_LINK_SPACING: f32 = 4.0;

/// World-Y of the tread band's vertical midline relative to `pos.y`
/// (Y-up). Used by the tread-link overlay to land marks on the band.
/// In screen-local terms the tread band sits at y=+3..+7 (i.e. 3..7
/// world units below the chassis center).
pub const TANK_TREAD_BAND_Y: f32 = -5.0;

/// Half-width (world units) of the tread band — i.e. the extent on
/// either side of `pos.x` where tread links should be visible.
pub const TANK_TREAD_HALF_WIDTH: f32 = 14.5;

pub struct EntityMeshes {
    pub player: ShipMesh,
    pub enemy: ShipMesh,
    pub tank: TankMesh,
    pub shot: Mesh,
    /// Tank-fired shell. Bigger and stubbier than `shot` so the heavy
    /// artillery reads as a different threat at a glance.
    pub tank_shell: Mesh,
}

impl EntityMeshes {
    pub fn build(ctx: &mut Context) -> GameResult<Self> {
        Ok(Self {
            player: build_player(ctx)?,
            enemy: build_enemy(ctx)?,
            tank: build_tank(ctx)?,
            shot: build_shot(ctx)?,
            tank_shell: build_tank_shell(ctx)?,
        })
    }
}

/// Plane silhouette pointed up at facing 0. Body = fuselage + tail + nose
/// triangle. Wings = a single horizontal bar that lives in its own mesh
/// so it can be foreshortened by `ship_wing_factor`.
fn build_player(ctx: &mut Context) -> GameResult<ShipMesh> {
    let mut body = MeshBuilder::new();
    // Fuselage — vertical bar, nose at -13, tail at +8.
    body.rectangle(
        DrawMode::fill(),
        Rect::new(-2.0, -13.0, 4.0, 21.0),
        Color::WHITE,
    )?;
    // Tail fin / rear stabilizers.
    body.rectangle(
        DrawMode::fill(),
        Rect::new(-5.0, 5.5, 10.0, 3.0),
        Color::WHITE,
    )?;
    // Nose tip — narrow triangle suggesting a forward gun.
    body.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(-2.0, -13.0),
            Vec2::new(2.0, -13.0),
            Vec2::new(0.0, -17.0),
        ],
        Color::WHITE,
    )?;
    let body_data = body.build();

    let mut wings = MeshBuilder::new();
    wings.rectangle(
        DrawMode::fill(),
        Rect::new(-12.0, -1.4, 24.0, 4.5),
        Color::WHITE,
    )?;
    let wings_data = wings.build();

    Ok(ShipMesh {
        body: Mesh::from_data(ctx, body_data),
        wings: Mesh::from_data(ctx, wings_data),
    })
}

/// Enemy ship — slightly squatter than the player, with a swept-back
/// wing so it reads as "the bad guy" even at a quick glance. Wings live
/// in their own mesh so banking foreshortens them the same way the
/// player's do.
fn build_enemy(ctx: &mut Context) -> GameResult<ShipMesh> {
    let mut body = MeshBuilder::new();
    // Body — short, wide fuselage.
    body.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(0.0, -13.0),
            Vec2::new(4.5, 5.0),
            Vec2::new(-4.5, 5.0),
        ],
        Color::WHITE,
    )?;
    // Rear tail bar — sits in the body so it never foreshortens.
    body.rectangle(
        DrawMode::fill(),
        Rect::new(-5.5, 4.5, 11.0, 2.5),
        Color::WHITE,
    )?;
    let body_data = body.build();

    let mut wings = MeshBuilder::new();
    // Aggressive swept-back triangles, one each side. Mirrored so wing
    // scaling shrinks both halves toward the fuselage symmetrically.
    wings.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(-13.0, 4.0),
            Vec2::new(-2.5, -4.0),
            Vec2::new(-2.5, 4.0),
        ],
        Color::WHITE,
    )?;
    wings.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(13.0, 4.0),
            Vec2::new(2.5, -4.0),
            Vec2::new(2.5, 4.0),
        ],
        Color::WHITE,
    )?;
    let wings_data = wings.build();

    Ok(ShipMesh {
        body: Mesh::from_data(ctx, body_data),
        wings: Mesh::from_data(ctx, wings_data),
    })
}

/// Tank — a sloped chassis with rolling tracks + a rounded turret. The
/// chassis is authored with its center at local origin, treads at the
/// bottom (y=+3..+7 in screen-local, which is "down on screen"). Body
/// rolling direction is conveyed by both the cannon orientation and a
/// subtle hull asymmetry: the front (right side when body_dir=+1) has
/// a more gradual armor slope, the rear is steeper.
///
/// The turret pivots around its dome center. The dome is authored
/// near the local origin so a rotation of `turret_facing` keeps it
/// approximately in place while the cannon swings around. The caller
/// offsets the turret draw destination by `TANK_TURRET_PIVOT_Y` (world
/// units) so the dome lands on top of the hull rather than at the
/// chassis center.
fn build_tank(ctx: &mut Context) -> GameResult<TankMesh> {
    let mut chassis = MeshBuilder::new();
    // Tread band — dark bar running the bottom of the chassis. Tread
    // *links* are not baked in: they're drawn each frame at scrolling
    // positions so the tracks visibly roll while the tank moves.
    chassis.rectangle(
        DrawMode::fill(),
        Rect::new(-15.0, 3.0, 30.0, 4.0),
        Color::new(0.18, 0.18, 0.18, 1.0),
    )?;
    // Lower hull — small platform that sits on the treads.
    chassis.rectangle(
        DrawMode::fill(),
        Rect::new(-14.0, 1.0, 28.0, 2.5),
        Color::new(0.78, 0.78, 0.78, 1.0),
    )?;
    // Upper hull — sloped trapezoid. Rear (left, x<0) is shorter and
    // steeper; front (right, x>0) is longer and more gradual. This
    // gives the silhouette directional asymmetry so `body_dir` flipping
    // visibly mirrors the hull.
    chassis.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(-13.0, 1.0),
            Vec2::new(-10.0, -5.0),
            Vec2::new(8.0, -5.0),
            Vec2::new(13.0, 1.0),
        ],
        Color::WHITE,
    )?;
    // Hatch — small dark square slightly behind center on top of hull.
    chassis.rectangle(
        DrawMode::fill(),
        Rect::new(-4.0, -6.0, 5.0, 1.5),
        Color::new(0.18, 0.18, 0.18, 1.0),
    )?;
    // Road wheels — 5 small circles embedded in the tread band. They
    // stay fixed relative to the chassis; the animated tread links pass
    // over them.
    for x in [-12.0_f32, -6.0, 0.0, 6.0, 12.0] {
        chassis.circle(
            DrawMode::fill(),
            Vec2::new(x, 5.0),
            2.2,
            0.2,
            Color::new(0.55, 0.55, 0.55, 1.0),
        )?;
    }
    let chassis_data = chassis.build();

    let mut turret = MeshBuilder::new();
    // Dome — flat-topped octagon centered near the origin. Rotation
    // around the origin keeps the dome roughly in place while the
    // cannon swings.
    turret.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(-7.0, 1.0),
            Vec2::new(-7.0, -2.0),
            Vec2::new(-4.0, -4.0),
            Vec2::new(4.0, -4.0),
            Vec2::new(7.0, -2.0),
            Vec2::new(7.0, 1.0),
        ],
        Color::WHITE,
    )?;
    // Mantlet — short, slightly thicker block where the barrel mounts.
    turret.rectangle(
        DrawMode::fill(),
        Rect::new(-2.0, -7.0, 4.0, 3.0),
        Color::WHITE,
    )?;
    // Cannon barrel — long thin rectangle along the dome's local +Y
    // (i.e. up the screen at turret_facing=0). Mounted just past the
    // mantlet so a separate seam is visible.
    turret.rectangle(
        DrawMode::fill(),
        Rect::new(-1.4, -17.0, 2.8, 10.5),
        Color::WHITE,
    )?;
    // Muzzle brake — small flare at the end of the cannon.
    turret.rectangle(
        DrawMode::fill(),
        Rect::new(-2.4, -19.0, 4.8, 2.5),
        Color::WHITE,
    )?;
    let turret_data = turret.build();

    let mut tread_link = MeshBuilder::new();
    // One link — a small darker pill. Drawn many times per frame, so
    // keep it cheap. Color is set on the draw call via the tint so a
    // single mesh works for any vehicle that grows tracks later.
    tread_link.rectangle(
        DrawMode::fill(),
        Rect::new(-1.4, -1.2, 2.8, 2.4),
        Color::WHITE,
    )?;
    let tread_link_data = tread_link.build();

    Ok(TankMesh {
        chassis: Mesh::from_data(ctx, chassis_data),
        turret: Mesh::from_data(ctx, turret_data),
        tread_link: Mesh::from_data(ctx, tread_link_data),
    })
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

/// Tank shell — bigger, heavier silhouette than a ship bullet. The head
/// is a pointed pentagon (so the shape reads as "explosive ordnance"
/// rather than a bullet) and the trail is a wider rectangle. Same +Y
/// orientation convention as `build_shot` so it rotates with `facing`.
fn build_tank_shell(ctx: &mut Context) -> GameResult<Mesh> {
    let mut mb = MeshBuilder::new();
    // Pointed shell head — pentagon tip up, base flush with the body.
    mb.polygon(
        DrawMode::fill(),
        &[
            Vec2::new(0.0, -7.5),
            Vec2::new(3.0, -4.0),
            Vec2::new(3.0, 1.0),
            Vec2::new(-3.0, 1.0),
            Vec2::new(-3.0, -4.0),
        ],
        Color::WHITE,
    )?;
    // Body / driving band — a slightly wider rectangle behind the head.
    mb.rectangle(
        DrawMode::fill(),
        Rect::new(-2.4, 1.0, 4.8, 4.5),
        Color::WHITE,
    )?;
    let data = mb.build();
    Ok(Mesh::from_data(ctx, data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    #[test]
    fn wing_factor_full_when_pointing_up_or_down() {
        assert!((ship_wing_factor(0.0) - 1.0).abs() < 1e-5);
        assert!((ship_wing_factor(PI) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn wing_factor_minimum_when_pointing_sideways() {
        assert!((ship_wing_factor(FRAC_PI_2) - MIN_WING_FACTOR).abs() < 1e-5);
        assert!((ship_wing_factor(-FRAC_PI_2) - MIN_WING_FACTOR).abs() < 1e-5);
    }
}
