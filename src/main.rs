//! An Asteroids-ish example game to show off ggez.
//! The idea is that this game is simple but still
//! non-trivial enough to be interesting.

#[macro_use] extern crate icarust_derive;
extern crate ggez;
extern crate rand;
extern crate nalgebra as na;
extern crate ncollide;
use ggez::conf;
use ggez::event::*;
use ggez::{Context, GameResult};
use ggez::graphics;
use ggez::timer;

use std::env;
use std::path;

use ggez::graphics::{Point2};

mod actors;
use actors::*;
use actors::player::{create_player, Player};
use actors::rock::{create_rocks, Rock};

mod util;
use util::*;

mod input;
use input::*;

pub mod assets;
use assets::{AssetManager, SoundId};

pub mod widget;
use widget::{Widget, TextWidget};

pub mod physics;

fn handle_timed_life<T: Actor>(actor: &mut T, dt: f32) {
	actor.add_life(-dt)
}

/// **********************************************************************
/// The `MainState` is our game's "global" state, it keeps track of
/// everything we need for actually running the game.
///
/// Our game objects are simply a vector for each actor type, and we
/// probably mingle gameplay-state (like score) and hardware-state
/// (like gui_dirty) a little more than we should, but for something
/// this small it hardly matters.
/// **********************************************************************

struct MainState {
    asset_manager: AssetManager,
    player: Player,
    shots: Vec<Shot>,
    rocks: Vec<Rock>,
    level: i32,
    score: i32,
    hit_sound_id: SoundId,
    screen_width: u32,
    screen_height: u32,
    input: InputState,
    gui_dirty: bool,
    debug_text: TextWidget,
    score_text: TextWidget,
    level_text: TextWidget,
}

impl MainState {
    fn new(ctx: &mut Context) -> GameResult<MainState> {
        ctx.print_resource_stats();
        graphics::set_background_color(ctx, (0, 0, 0, 255).into());

        println!("Game resource path: {:?}", ctx.filesystem);

        print_instructions();

        let mut am = AssetManager::new();

        let screen_width = ctx.conf.window_mode.width;
        let screen_height = ctx.conf.window_mode.height;

        let player = create_player(ctx, &mut am, screen_width as f32, screen_height as f32);
        let rocks = create_rocks(ctx, &mut am, 5, player.position(), 100.0, 250.0);

        let debug_text = TextWidget::new(ctx, &mut am, 16)?;
        let score_text = TextWidget::new(ctx, &mut am, 16)?;
        let level_text = TextWidget::new(ctx, &mut am, 16)?;

        let hit_sound_id = am.add_sound(ctx, "/boom.ogg");

        let s = MainState {
            asset_manager: am,
            player: player,
            shots: Vec::new(),
            rocks: rocks,
            level: 0,
            score: 0,
            hit_sound_id: hit_sound_id,
            screen_width: screen_width,
            screen_height: screen_height,
            input: InputState::default(),
            gui_dirty: true,
            debug_text: debug_text,
            score_text: score_text,
            level_text: level_text,
        };

        Ok(s)
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
                    let _ = self.asset_manager.get_sound(self.hit_sound_id).play();
               }
            }
        }
    }

    fn check_for_level_respawn(&mut self, ctx: &mut Context) {
        if self.rocks.is_empty() {
            self.level += 1;
            self.gui_dirty = true;
            let r = create_rocks(ctx, &mut self.asset_manager, self.level + 5, self.player.position(), 100.0, 250.0);
            self.rocks.extend(r);
        }
    }

    fn update_ui(&mut self, ctx: &mut Context) {
        let am = &mut self.asset_manager;

        //self.debug_text.set_text(format!("{:.1}, {:.1}", self.player.x(), self.player.y()));
        self.score_text.set_text(ctx, am, &format!("Score: {}", self.score));
        self.level_text.set_text(ctx, am, &format!("Level: {}", self.level));

        // Set TextWidget positions
        //let debug_disp = Point2::new((self.screen_width - ((self.debug_text.width() + 20) / 2)) as f32,
        //                             (self.screen_height - (self.debug_text.height() + 5)) as f32);
        let level_pos = Point2::new((self.level_text.width() / 2) as f32 + 10.0, 10.0);
        self.level_text.set_position(level_pos);

        let score_pos = Point2::new((self.level_text.width() + self.score_text.width() / 2) as f32 + 20.0, 10.0);
        self.score_text.set_position(score_pos);
    }
}

impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult<()> {
        const DESIRED_FPS: u32 = 60;

        while timer::check_update_time(ctx, DESIRED_FPS) {
            let seconds = 1.0 / (DESIRED_FPS as f32);
            let coords = (self.screen_width, self.screen_height);

            {
                let am = &mut self.asset_manager;

                // Update the player state based on the user input.
                self.player.handle_input(&self.input, seconds);
                if self.input.fire && self.player.can_fire() {
                    self.shots.push(self.player.fire_shot(ctx, am));
                }

                self.player.update(ctx, am, coords, seconds);

                // Then the shots...
                for act in &mut self.shots {
                    act.update(ctx, am, coords, seconds);
                    handle_timed_life(act, seconds);
                }

                // And finally the rocks.
                let mut rocks_pos: Vec<Point2> = Vec::new();
                for act in &mut self.rocks {
                    act.update(ctx, am, coords, seconds);
                    rocks_pos.push(act.position());
                }
                physics::test_collide(self.player.position(), &rocks_pos);
            }

            // Handle the results of things moving:
            // collision detection, object death, and if
            // we have killed all the rocks in the level,
            // spawn more of them.
            self.handle_collisions();

            self.clear_dead_stuff();

            self.check_for_level_respawn(ctx);

            // Using a gui_dirty flag here is a little
            // messy but fine here.
            if self.gui_dirty {
                self.update_ui(ctx);
                self.gui_dirty = false;
            }

            // Finally we check for our end state.
            if self.input.quit {
                ctx.quit().unwrap();
                break
            } else if self.player.life() <= 0.0 {
            
                //println!("Game over!");
                //let _ = ctx.quit();
            }
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult<()> {

        // Clear the screen
        graphics::clear(ctx);

        let coords = (self.screen_width, self.screen_height);
        {

            self.player.draw(ctx, coords);

            for s in &self.shots {
                s.draw(ctx, coords);
            }

            for r in &self.rocks {
                r.draw(ctx, coords);
            }
        }

        self.debug_text.draw(ctx, coords);
        self.level_text.draw(ctx, coords);
        self.score_text.draw(ctx, coords);

        // Then we flip the screen
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

    fn key_down_event(&mut self, _ctx: &mut Context, keycode: Keycode, _keymod: Mod, _repeat: bool) {

        self.input.handle_key_down(keycode, _keymod)
    }


    fn key_up_event(&mut self, _ctx: &mut Context, keycode: Keycode, _keymod: Mod, _repeat: bool) {

        self.input.handle_key_up(keycode, _keymod)
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
