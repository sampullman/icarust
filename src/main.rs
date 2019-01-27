//! A Sopwith/Luftrauser style shoot 'em up

#[macro_use] extern crate icarust_derive;
use ggez::event::*;
use ggez::{conf, Context, GameResult, graphics, timer};

use std::env;
use std::path;

use ggez::graphics::{Point2};

mod actors;
use crate::actors::*;
use crate::actors::shot::Shot;
use crate::actors::player::{create_player, Player};
use crate::actors::rock::{create_rocks, Rock};

mod util;
use crate::util::*;

mod input;
use crate::input::*;

mod render;
use crate::render::camera::Camera;

pub mod assets;
use crate::assets::{AssetManager, SoundId};

pub mod widget;
use crate::widget::{Widget, TextWidget};

pub mod physics;
use crate::physics::CollisionWorld2;

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 540;

const WORLD_WIDTH: f32 = WINDOW_WIDTH as f32 * 4.0;
const WORLD_HEIGHT: f32 = WINDOW_HEIGHT as f32;

/// **********************************************************************
/// `MainState` is the game's global state, it keeps track of
/// everything needed for running the game.
/// **********************************************************************

struct MainState {
    asset_manager: AssetManager,
    world: CollisionWorld2,
    camera: Camera,
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
        let rock_count = 5;
        let rocks = create_rocks(ctx, &mut am, rock_count, player.position(), 100.0, 250.0);

        let debug_text = TextWidget::new(ctx, &mut am, 16)?;
        let score_text = TextWidget::new(ctx, &mut am, 16)?;
        let level_text = TextWidget::new(ctx, &mut am, 16)?;

        let hit_sound_id = am.add_sound(ctx, "/boom.ogg");

        let world = physics::new_world(rock_count);
        let player_group_id = world.make_group();
        let rock_group_id = world.make_group();
        let shot_group_id = world.make_group();
        world.set_group_whitelist(rock_group_id, &[player_group_id]);
        world.set_group_whitelist(shot_group_id, &[rock_group_id]);

        let s = MainState {
            asset_manager: am,
            world:  world,
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
        self.shots.retain(|s| s.alive());
        self.rocks.retain(|r| r.alive());
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
        let level_pos = Point2::new(self.level_text.half_width() + 10.0, 10.0);
        self.level_text.set_position(level_pos);

        let score_pos = Point2::new(self.level_text.width() as f32 + self.score_text.half_width() + 25.0, 10.0);
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

                // Update player state based on the input.
                self.player.handle_input(&self.input, seconds);
                if self.input.fire && self.player.can_fire() {
                    self.shots.push(self.player.fire_shot(ctx, am));
                }

                self.player.update(ctx, am, coords, seconds);

                // Then the shots...
                self.shots.iter_mut().for_each(|s| s.update(ctx, am, coords, seconds));

                // And finally the rocks.
                for act in &mut self.rocks {
                    act.update(ctx, am, coords, seconds);
                }
                physics::update_world(&mut self.world, &self.player, &self.rocks);
            }

            self.camera.move_to(self.player.position());

            // Handle the result of movements: collision detection,
            // object death, and if we have killed all the rocks
            // in the level, spawn more of them.
            self.handle_collisions();

            self.clear_dead_stuff();

            self.check_for_level_respawn(ctx);

            // This is a little messy
            if self.gui_dirty {
                self.update_ui(ctx);
                self.gui_dirty = false;
            }

            // Check for our end state.
            if self.input.quit {
                ctx.quit().unwrap();
                break
            } else if !self.player.alive() {
            
                //println!("Game over!");
                //let _ = ctx.quit();
            }
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult<()> {

        // Clear the screen
        graphics::clear(ctx);
        {
            self.player.draw(ctx, &self.camera);
            self.shots.iter().for_each(|s| s.draw(ctx, &self.camera));
            self.rocks.iter().for_each(|r| r.draw(ctx, &self.camera));
        }
        let p1 = self.camera.world_to_screen_coords(Point2::new(-WORLD_WIDTH, 32.0));
        let p2 = self.camera.world_to_screen_coords(Point2::new(WORLD_WIDTH, 32.0));
        let _ = graphics::line(ctx, &[p1, p2], 2.0);

        self.debug_text.draw(ctx, &self.camera);
        self.level_text.draw(ctx, &self.camera);
        self.score_text.draw(ctx, &self.camera);

        // Flip the screen
        graphics::present(ctx);

        // Yield the timeslice
        // This tells the OS that we're done using the CPU but it should
        // get back to this program as soon as it can.
        // This ideally prevents the game from using 100% CPU all the time
        // even if vsync is off.
        // The actual behavior can be platform-specific.
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
    use ggez::ContextBuilder;

    let mut cb = ContextBuilder::new("icarust", "ggez")
        .window_setup(conf::WindowSetup::default().title("Icarust"))
        .window_mode(conf::WindowMode::default().dimensions(WINDOW_WIDTH, WINDOW_HEIGHT));

    // Add CARGO_MANIFEST_DIR/resources to the filesystems paths so
    // we look in the cargo project for files.
    if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        let mut path = path::PathBuf::from(manifest_dir);
        path.push("resources");
        println!("Adding path {:?}", path);
        // We need this re-assignment alas, see
        // https://aturon.github.io/ownership/builders.html
        // under "Consuming builders"
        cb = cb.add_resource_path(path);
    } else {
        println!("Not building from cargo?  Ok.");
    }

    let ctx = &mut cb.build().unwrap();

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
