//! A Sopwith/Luftrauser style shoot 'em up.

use ggez::conf;
use ggez::event::{self, EventHandler};
use ggez::graphics::{self, Color};
use ggez::input::keyboard::KeyInput;
use ggez::{Context, ContextBuilder, GameResult};

use std::env;
use std::path;

mod actors;
use crate::actors::player::{create_player, Player};
use crate::actors::rock::{create_rocks, Rock};
use crate::actors::shot::Shot;
use crate::actors::{draw_actor, draw_actor_wrapped, Actor, Inputable, Updatable};

mod util;
use crate::util::{print_instructions, Point2};

mod input;
use crate::input::InputState;

mod render;
use crate::render::camera::Camera;

pub mod assets;
use crate::assets::{AssetManager, SoundId};

pub mod widget;
use crate::widget::TextWidget;

pub mod physics;

const WINDOW_WIDTH: f32 = 1280.0;
const WINDOW_HEIGHT: f32 = 540.0;

const ROCKS_PER_LEVEL_BASE: i32 = 5;
const ROCKS_MAX: i32 = 30;

struct MainState {
    asset_manager: AssetManager,
    camera: Camera,
    player: Player,
    shots: Vec<Shot>,
    rocks: Vec<Rock>,
    level: i32,
    score: i32,
    hit_sound_id: SoundId,
    input: InputState,
    gui_dirty: bool,
    score_text: TextWidget,
    level_text: TextWidget,
    game_over_text: TextWidget,
    game_over: bool,
}

impl MainState {
    fn new(ctx: &mut Context) -> GameResult<MainState> {
        print_instructions();

        let mut am = AssetManager::new();

        let (drawable_w, drawable_h) = ctx.gfx.drawable_size();

        let player = create_player(ctx, &mut am, WINDOW_WIDTH, WINDOW_HEIGHT);
        let rocks = create_rocks(
            ctx,
            &mut am,
            ROCKS_PER_LEVEL_BASE,
            player.position(),
            100.0,
            250.0,
        );

        let score_text = TextWidget::new(ctx, &mut am, 18.0)?;
        let level_text = TextWidget::new(ctx, &mut am, 18.0)?;
        let mut game_over_text = TextWidget::new(ctx, &mut am, 48.0)?;
        game_over_text.set_text(ctx, &mut am, "GAME OVER — press R", 48.0);

        let hit_sound_id = am.add_sound(ctx, "/boom.ogg");

        let camera = Camera::new(drawable_w as u32, drawable_h as u32, WINDOW_WIDTH, WINDOW_HEIGHT);

        Ok(MainState {
            asset_manager: am,
            player,
            camera,
            shots: Vec::new(),
            rocks,
            level: 0,
            score: 0,
            hit_sound_id,
            input: InputState::default(),
            gui_dirty: true,
            score_text,
            level_text,
            game_over_text,
            game_over: false,
        })
    }

    fn restart(&mut self, ctx: &mut Context) {
        self.player = create_player(ctx, &mut self.asset_manager, WINDOW_WIDTH, WINDOW_HEIGHT);
        self.shots.clear();
        self.rocks = create_rocks(
            ctx,
            &mut self.asset_manager,
            ROCKS_PER_LEVEL_BASE,
            self.player.position(),
            100.0,
            250.0,
        );
        self.level = 0;
        self.score = 0;
        self.gui_dirty = true;
        self.game_over = false;
    }

    fn clear_dead_stuff(&mut self) {
        self.shots.retain(|s| s.alive());
        self.rocks.retain(|r| r.alive());
    }

    fn handle_collisions(&mut self, ctx: &Context) {
        for rock in &mut self.rocks {
            if !rock.alive() {
                continue;
            }
            if physics::collides(&self.player, rock) {
                self.player.kill();
            }
            for shot in &mut self.shots {
                if !shot.alive() {
                    continue;
                }
                if physics::collides(shot, rock) {
                    shot.kill();
                    rock.kill();
                    self.score += 1;
                    self.gui_dirty = true;
                    self.asset_manager.play_sound(ctx, self.hit_sound_id);
                    break;
                }
            }
        }
    }

    fn check_for_level_respawn(&mut self, ctx: &mut Context) {
        if self.rocks.is_empty() {
            self.level += 1;
            self.gui_dirty = true;
            let count = (self.level + ROCKS_PER_LEVEL_BASE).min(ROCKS_MAX);
            let r = create_rocks(
                ctx,
                &mut self.asset_manager,
                count,
                self.player.position(),
                100.0,
                250.0,
            );
            self.rocks.extend(r);
        }
    }

    fn update_ui(&mut self, ctx: &mut Context) {
        let am = &mut self.asset_manager;

        self.score_text
            .set_text(ctx, am, &format!("Score: {}", self.score), 18.0);
        self.level_text
            .set_text(ctx, am, &format!("Level: {}", self.level), 18.0);

        let level_pos = Point2::new(self.level_text.half_width(ctx) + 10.0, 10.0);
        self.level_text.set_position(level_pos);

        let score_pos = Point2::new(
            self.level_text.width(ctx) + self.score_text.half_width(ctx) + 25.0,
            10.0,
        );
        self.score_text.set_position(score_pos);

        let go_pos = Point2::new(
            (WINDOW_WIDTH - self.game_over_text.width(ctx)) / 2.0
                + self.game_over_text.half_width(ctx),
            WINDOW_HEIGHT / 2.0 - self.game_over_text.half_height(ctx),
        );
        self.game_over_text.set_position(go_pos);
    }
}

impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        const DESIRED_FPS: u32 = 60;

        while ctx.time.check_update_time(DESIRED_FPS) {
            let seconds = 1.0 / (DESIRED_FPS as f32);
            let coords = (WINDOW_WIDTH, WINDOW_HEIGHT);

            if self.game_over {
                if self.input.restart {
                    self.input.restart = false;
                    self.restart(ctx);
                }
                if self.input.quit {
                    ctx.request_quit();
                    break;
                }
                continue;
            }

            {
                let am = &mut self.asset_manager;

                self.player.handle_input(&self.input, seconds);
                if self.input.fire && self.player.can_fire() {
                    self.shots.push(self.player.fire_shot(ctx, am));
                }

                self.player.update(ctx, am, coords, seconds);

                for shot in &mut self.shots {
                    shot.update(ctx, am, coords, seconds);
                }

                for rock in &mut self.rocks {
                    rock.update(ctx, am, coords, seconds);
                }
            }

            self.camera.move_to(self.player.position());

            self.handle_collisions(ctx);
            self.clear_dead_stuff();

            if !self.player.alive() {
                self.game_over = true;
                self.gui_dirty = true;
            } else {
                self.check_for_level_respawn(ctx);
            }

            if self.gui_dirty {
                self.update_ui(ctx);
                self.gui_dirty = false;
            }

            if self.input.quit {
                ctx.request_quit();
                break;
            }
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let mut canvas = graphics::Canvas::from_frame(ctx, Color::BLACK);

        if self.player.alive() {
            draw_actor_wrapped(&mut canvas, &self.camera, &self.player);
        }
        for shot in &self.shots {
            draw_actor(&mut canvas, &self.camera, shot);
        }
        for rock in &self.rocks {
            draw_actor(&mut canvas, &self.camera, rock);
        }

        self.level_text.draw(&mut canvas, &self.camera);
        self.score_text.draw(&mut canvas, &self.camera);
        if self.game_over {
            self.game_over_text.draw(&mut canvas, &self.camera);
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
        self.camera.set_drawable_size(width, height);
        Ok(())
    }
}

pub fn main() -> GameResult {
    let resource_dir = if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        let mut path = path::PathBuf::from(manifest_dir);
        path.push("resources");
        path
    } else {
        path::PathBuf::from("./resources")
    };
    println!("Adding path {:?}", resource_dir);

    let cb = ContextBuilder::new("icarust", "ggez")
        .window_setup(conf::WindowSetup::default().title("Icarust"))
        .window_mode(conf::WindowMode::default().dimensions(WINDOW_WIDTH, WINDOW_HEIGHT))
        .add_resource_path(resource_dir);

    let (mut ctx, events_loop) = cb.build()?;
    let game = MainState::new(&mut ctx)?;
    event::run(ctx, events_loop, game)
}
