//! An Asteroids-ish example game to show off ggez.
//! The idea is that this game is simple but still
//! non-trivial enough to be interesting.

#[macro_use] extern crate icarust_derive;
extern crate ggez;
extern crate rand;
use ggez::audio;
use ggez::conf;
use ggez::event::*;
use ggez::{Context, GameResult};
use ggez::graphics;
use ggez::timer;

use std::env;
use std::path;

use ggez::graphics::{Vector2, Point2};

mod actors;
use actors::*;

/// Create a unit vector representing the
/// given angle (in radians)
/// BUGGO: TODO: We should be able to create these from a Rotation?
fn vec_from_angle(angle: f32) -> Vector2 {
    let vx = angle.sin();
    let vy = angle.cos();
    Vector2::new(vx, vy)
}

/// BUGGO: TODO: Vector2 implements Rand so this is unnecessary
fn random_vec(max_magnitude: f32) -> Vector2 {
    let angle = rand::random::<f32>() * 2.0 * std::f32::consts::PI;
    let mag = rand::random::<f32>() * max_magnitude;
    vec_from_angle(angle) * (mag)
}

/// *********************************************************************
/// Now we have some initializer functions for different game objects.
/// **********************************************************************

const MAX_ROCK_VEL: f32 = 50.0;

/// Create the given number of rocks.
/// Makes sure that none of them are within the
/// given exclusion zone (nominally the player)
/// Note that this *could* create rocks outside the
/// bounds of the playing field, so it should be
/// called before `wrap_actor_position()` happens.
fn create_rocks(num: i32, exclusion: Point2, min_radius: f32, max_radius: f32) -> Vec<Rock> {
    assert!(max_radius > min_radius);
    let new_rock = |_| {
        let mut rock = create_rock();
        let r_angle = rand::random::<f32>() * 2.0 * std::f32::consts::PI;
        let r_distance = rand::random::<f32>() * (max_radius - min_radius) + min_radius;
        rock.set_position(exclusion + vec_from_angle(r_angle) * r_distance);
        rock.set_velocity(random_vec(MAX_ROCK_VEL));
        rock
    };
    (0..num).map(new_rock).collect()
}

/// *********************************************************************
/// Now we make functions to handle physics.  We do simple Newtonian
/// physics (so we do have inertia), and cap the max speed so that we
/// don't have to worry too much about small objects clipping through
/// each other.
///
/// Our unit of world space is simply pixels, though we do transform
/// the coordinate system so that +y is up and -y is down.
/// **********************************************************************

const SHOT_SPEED: f32 = 200.0;
const SPRITE_SIZE: u32 = 32;

// Acceleration in pixels per second, more or less.
const PLAYER_THRUST: f32 = 100.0;
// Rotation in radians per second.
const PLAYER_TURN_RATE: f32 = 3.05;
// Seconds between shots
const PLAYER_SHOT_TIME: f32 = 0.5;


fn player_handle_input<T: Actor>(actor: &mut T, input: &InputState, dt: f32) {
    actor.add_facing(dt * PLAYER_TURN_RATE * input.xaxis);

    if input.yaxis > 0.0 {
        player_thrust(actor, dt);
    }
}

fn player_thrust<T: Actor>(actor: &mut T, dt: f32) {
    let direction_vector = vec_from_angle(actor.facing());
    let thrust_vector = direction_vector * (PLAYER_THRUST);
    actor.add_velocity(thrust_vector * (dt));
}

const MAX_PHYSICS_VEL: f32 = 250.0;

fn update_actor_position<T: Actor>(actor: &mut T, dt: f32) {
    // Clamp the velocity to the max efficiently
    let norm_sq = actor.velocity().norm_squared();
    if norm_sq > MAX_PHYSICS_VEL.powi(2) {
        let new_velocity = actor.velocity() / norm_sq.sqrt() * MAX_PHYSICS_VEL;
        actor.set_velocity(new_velocity);
    }
    let dv = actor.velocity() * (dt);
    let new_position = actor.position() + dv;
    actor.set_position(new_position);
    actor.rotate();
}

/// Takes an actor and wraps its position to the bounds of the
/// screen, so if it goes off the left side of the screen it
/// will re-enter on the right side and so on.
fn wrap_actor_position<T: Actor>(actor: &mut T, sx: f32, sy: f32) {
    // Wrap screen
    let screen_x_bounds = sx / 2.0;
    let screen_y_bounds = sy / 2.0;
    let sprite_half_size = (SPRITE_SIZE / 2) as f32;
    let actor_center = actor.position() - Vector2::new(-sprite_half_size, sprite_half_size);
    if actor_center.x > screen_x_bounds {
        actor.add_x(-sx);
    } else if actor_center.x < -screen_x_bounds {
        actor.add_x(sx);
    };
    if actor_center.y > screen_y_bounds {
        actor.add_y(-sy);
    } else if actor_center.y < -screen_y_bounds {
        actor.add_y(sy);
    }
}

fn handle_timed_life<T: Actor>(actor: &mut T, dt: f32) {
	actor.add_life(-dt)
}


/// Translates the world coordinate system, which
/// has Y pointing up and the origin at the center,
/// to the screen coordinate system, which has Y
/// pointing downward and the origin at the top-left,
fn world_to_screen_coords(screen_width: u32, screen_height: u32, point: Point2) -> Point2 {
    let width = screen_width as f32;
    let height = screen_height as f32;
    let x = point.x + width / 2.0;
    let y = height - (point.y + height / 2.0);
    Point2::new(x, y)
}

/// **********************************************************************
/// So that was the real meat of our game.  Now we just need a structure
/// to contain the images, sounds, etc. that we need to hang on to; this
/// is our "asset management system".  All the file names and such are
/// just hard-coded.
/// **********************************************************************

struct Assets {
    player_image: graphics::Image,
    shot_image: graphics::Image,
    rock_image: graphics::Image,
    font: graphics::Font,
    shot_sound: audio::Source,
    hit_sound: audio::Source,
}

impl Assets {
    fn new(ctx: &mut Context) -> GameResult<Assets> {
        let player_image = graphics::Image::new(ctx, "/player.png")?;
        let shot_image = graphics::Image::new(ctx, "/shot.png")?;
        let rock_image = graphics::Image::new(ctx, "/rock.png")?;
        // let font_path = path::Path::new("/consolefont.png");
        // let font_chars =
        //"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!,.?;'\"";
        // let font = graphics::Font::new_bitmap(ctx, font_path, font_chars)?;
        let font = graphics::Font::new(ctx, "/DejaVuSerif.ttf", 18)?;

        let shot_sound = audio::Source::new(ctx, "/pew.ogg")?;
        let hit_sound = audio::Source::new(ctx, "/boom.ogg")?;
        Ok(Assets {
               player_image: player_image,
               shot_image: shot_image,
               rock_image: rock_image,
               font: font,
               shot_sound: shot_sound,
               hit_sound: hit_sound,
           })
    }

    fn actor_image<T: Actor>(&mut self, actor: &T) -> &mut graphics::Image {
        match actor.tag() {
            ActorType::Player => &mut self.player_image,
            ActorType::Rock => &mut self.rock_image,
            ActorType::Shot => &mut self.shot_image,
        }
    }
}

/// **********************************************************************
/// The `InputState` is exactly what it sounds like, it just keeps track of
/// the user's input state so that we turn keyboard events into something
/// state-based and device-independent.
/// **********************************************************************
#[derive(Debug)]
struct InputState {
    xaxis: f32,
    yaxis: f32,
    fire: bool,
}

impl Default for InputState {
    fn default() -> Self {
        InputState {
            xaxis: 0.0,
            yaxis: 0.0,
            fire: false,
        }
    }
}

/// **********************************************************************
/// Now we're getting into the actual game loop.  The `MainState` is our
/// game's "global" state, it keeps track of everything we need for
/// actually running the game.
///
/// Our game objects are simply a vector for each actor type, and we
/// probably mingle gameplay-state (like score) and hardware-state
/// (like gui_dirty) a little more than we should, but for something
/// this small it hardly matters.
/// **********************************************************************

struct MainState {
    player: Player,
    shots: Vec<Shot>,
    rocks: Vec<Rock>,
    level: i32,
    score: i32,
    assets: Assets,
    screen_width: u32,
    screen_height: u32,
    input: InputState,
    player_shot_timeout: f32,
    gui_dirty: bool,
    score_display: graphics::Text,
    level_display: graphics::Text,
}


impl MainState {
    fn new(ctx: &mut Context) -> GameResult<MainState> {
        ctx.print_resource_stats();
        graphics::set_background_color(ctx, (0, 0, 0, 255).into());

        println!("Game resource path: {:?}", ctx.filesystem);

        print_instructions();

        let assets = Assets::new(ctx)?;
        let score_disp = graphics::Text::new(ctx, "score", &assets.font)?;
        let level_disp = graphics::Text::new(ctx, "level", &assets.font)?;

        let player = create_player();
        let rocks = create_rocks(5, player.position(), 100.0, 250.0);

        let s = MainState {
            player: player,
            shots: Vec::new(),
            rocks: rocks,
            level: 0,
            score: 0,
            assets: assets,
            screen_width: ctx.conf.window_mode.width,
            screen_height: ctx.conf.window_mode.height,
            input: InputState::default(),
            player_shot_timeout: 0.0,
            gui_dirty: true,
            score_display: score_disp,
            level_display: level_disp,
        };

        Ok(s)
    }

    fn fire_player_shot(&mut self) {
        self.player_shot_timeout = PLAYER_SHOT_TIME;

        let player = &self.player;
        let mut shot = create_shot();
        shot.set_position(player.position());
        shot.set_facing(player.facing());
        let direction = vec_from_angle(shot.facing());
		shot.set_velocity_xy(SHOT_SPEED * direction.x, SHOT_SPEED * direction.y);

        self.shots.push(shot);
        let _ = self.assets.shot_sound.play();
    }



    fn clear_dead_stuff(&mut self) {
        self.shots.retain(|s| s.life() > 0.0);
        self.rocks.retain(|r| r.life() > 0.0);
    }

    fn handle_collisions(&mut self) {
        for rock in &mut self.rocks {
            self.player.check_collision(rock);
            
            for shot in &mut self.shots {
               if shot.check_collision(rock) {
                    self.score += 1;
                    self.gui_dirty = true;
                    let _ = self.assets.hit_sound.play();
               }
            }
        }
    }

    fn check_for_level_respawn(&mut self) {
        if self.rocks.is_empty() {
            self.level += 1;
            self.gui_dirty = true;
            let r = create_rocks(self.level + 5, self.player.position(), 100.0, 250.0);
            self.rocks.extend(r);
        }
    }

    fn update_ui(&mut self, ctx: &mut Context) {
        let score_str = format!("Score: {}", self.score);
        let level_str = format!("Level: {}", self.level);
        let score_text = graphics::Text::new(ctx, &score_str, &self.assets.font).unwrap();
        let level_text = graphics::Text::new(ctx, &level_str, &self.assets.font).unwrap();

        self.score_display = score_text;
        self.level_display = level_text;
    }
}


/// **********************************************************************
/// A couple of utility functions.
/// **********************************************************************

fn print_instructions() {
    println!();
    println!("Welcome to ASTROBLASTO!");
    println!();
    println!("How to play:");
    println!("L/R arrow keys rotate your ship, up thrusts, space bar fires");
    println!();
}


fn draw_actor<T: Actor>(assets: &mut Assets,
              ctx: &mut Context,
              actor: &T,
              world_coords: (u32, u32))
              -> GameResult<()> {
    let (screen_w, screen_h) = world_coords;
    let pos = world_to_screen_coords(screen_w, screen_h, actor.position());
    // let pos = Vector2::new(1.0, 1.0);
    let px = pos.x as f32;
    let py = pos.y as f32;
    let dest_point = graphics::Point2::new(px, py);
    let image = assets.actor_image(actor);
    graphics::draw(ctx, image, dest_point, actor.facing() as f32)

}

/// **********************************************************************
/// Now we implement the `EventHandler` trait from `ggez::event`, which provides
/// ggez with callbacks for updating and drawing our game, as well as
/// handling input events.
/// **********************************************************************
impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult<()> {
        const DESIRED_FPS: u32 = 60;

        while timer::check_update_time(ctx, DESIRED_FPS) {
            let seconds = 1.0 / (DESIRED_FPS as f32);

            // Update the player state based on the user input.
            player_handle_input(&mut self.player, &self.input, seconds);
            self.player_shot_timeout -= seconds;
            if self.input.fire && self.player_shot_timeout < 0.0 {
                self.fire_player_shot();
            }

            // Update the physics for all actors.
            // First the player...
            update_actor_position(&mut self.player, seconds);
            wrap_actor_position(&mut self.player,
                                self.screen_width as f32,
                                self.screen_height as f32);

            // Then the shots...
            for act in &mut self.shots {
                update_actor_position(act, seconds);
                wrap_actor_position(act, self.screen_width as f32, self.screen_height as f32);
                handle_timed_life(act, seconds);
            }

            // And finally the rocks.
            for act in &mut self.rocks {
                update_actor_position(act, seconds);
                wrap_actor_position(act, self.screen_width as f32, self.screen_height as f32);
            }

            // Handle the results of things moving:
            // collision detection, object death, and if
            // we have killed all the rocks in the level,
            // spawn more of them.
            self.handle_collisions();

            self.clear_dead_stuff();

            self.check_for_level_respawn();

            // Using a gui_dirty flag here is a little
            // messy but fine here.
            if self.gui_dirty {
                self.update_ui(ctx);
                self.gui_dirty = false;
            }

            // Finally we check for our end state.
            // I want to have a nice death screen eventually,
            // but for now we just quit.
            if self.player.life() <= 0.0 {
                println!("Game over!");
                let _ = ctx.quit();
            }
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult<()> {
        // Our drawing is quite simple.
        // Just clear the screen...
        graphics::clear(ctx);

        // Loop over all objects drawing them...
        {
            let assets = &mut self.assets;
            let coords = (self.screen_width, self.screen_height);

            let p = &self.player;
            draw_actor(assets, ctx, p, coords)?;

            for s in &self.shots {
                draw_actor(assets, ctx, s, coords)?;
            }

            for r in &self.rocks {
                draw_actor(assets, ctx, r, coords)?;
            }
        }


        // And draw the GUI elements in the right places.
        let level_dest = graphics::Point2::new((self.level_display.width() / 2) as f32 + 10.0,
                                               (self.level_display.height() / 2) as f32 + 10.0);
        let score_dest = graphics::Point2::new((self.score_display.width() / 2) as f32 + 200.0,
                                               (self.score_display.height() / 2) as f32 + 10.0);
        graphics::draw(ctx, &self.level_display, level_dest, 0.0)?;
        graphics::draw(ctx, &self.score_display, score_dest, 0.0)?;

        // Then we flip the screen...
        graphics::present(ctx);

        // And yield the timeslice
        // This tells the OS that we're done using the CPU but it should
        // get back to this program as soon as it can.
        // This ideally prevents the game from using 100% CPU all the time
        // even if vsync is off.
        // The actual behavior can be a little platform-specific.
        timer::yield_now();
        Ok(())
    }

    // Handle key events.  These just map keyboard events
    // and alter our input state appropriately.
    fn key_down_event(&mut self,
                      ctx: &mut Context,
                      keycode: Keycode,
                      _keymod: Mod,
                      _repeat: bool) {
        match keycode {
            Keycode::Up => {
                self.input.yaxis = 1.0;
            }
            Keycode::Left => {
                self.input.xaxis = -1.0;
            }
            Keycode::Right => {
                self.input.xaxis = 1.0;
            }
            Keycode::Space => {
                self.input.fire = true;
            }
            Keycode::Escape => ctx.quit().unwrap(),
            _ => (), // Do nothing
        }
    }


    fn key_up_event(&mut self, _ctx: &mut Context, keycode: Keycode, _keymod: Mod, _repeat: bool) {
        match keycode {
            Keycode::Up => {
                self.input.yaxis = 0.0;
            }
            Keycode::Left | Keycode::Right => {
                self.input.xaxis = 0.0;
            }
            Keycode::Space => {
                self.input.fire = false;
            }
            _ => (), // Do nothing
        }
    }
}

pub fn main() {
    let mut c = conf::Conf::new();
    c.window_title = "Astroblasto!".to_string();
    c.window_mode.width = 640;
    c.window_mode.height = 480;

    let ctx = &mut Context::load_from_conf("icarust", "ggez", c).unwrap();

    // We add the CARGO_MANIFEST_DIR/resources do the filesystems paths so
    // we we look in the cargo project for files.
    if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        let mut path = path::PathBuf::from(manifest_dir);
        path.push("resources");
        ctx.filesystem.mount(&path, true);
        println!("Adding path {:?}", path);
    } else {
        println!("No manifest directory; cannot load resources.");
    }

    match MainState::new(ctx) {
        Err(e) => {
            println!("Could not load game!");
            println!("Error: {}", e);
        }
        Ok(ref mut game) => {
            let result = run(ctx, game);
            if let Err(e) = result {
                println!("Error encountered running game: {}", e);
            } else {
                println!("Game exited cleanly.");
            }
        }
    }
}
