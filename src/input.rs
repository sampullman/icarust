use ggez::input::keyboard::KeyInput;
use ggez::winit::keyboard::{Key, NamedKey};

#[derive(Debug, Default)]
pub struct InputState {
    pub xaxis: f32,
    pub yaxis: f32,
    pub fire: bool,
    pub quit: bool,
    pub restart: bool,
    left_held: bool,
    right_held: bool,
}

impl InputState {
    pub fn handle_key_down(&mut self, input: KeyInput) {
        match input.event.logical_key {
            Key::Named(NamedKey::ArrowUp) => self.yaxis = 1.0,
            Key::Named(NamedKey::ArrowLeft) => {
                self.left_held = true;
                self.recompute_xaxis();
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.right_held = true;
                self.recompute_xaxis();
            }
            Key::Named(NamedKey::Space) => self.fire = true,
            Key::Named(NamedKey::Escape) => self.quit = true,
            Key::Character(ref c) if c.as_str().eq_ignore_ascii_case("r") => self.restart = true,
            _ => {}
        }
    }

    pub fn handle_key_up(&mut self, input: KeyInput) {
        match input.event.logical_key {
            Key::Named(NamedKey::ArrowUp) => self.yaxis = 0.0,
            Key::Named(NamedKey::ArrowLeft) => {
                self.left_held = false;
                self.recompute_xaxis();
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.right_held = false;
                self.recompute_xaxis();
            }
            Key::Named(NamedKey::Space) => self.fire = false,
            Key::Character(ref c) if c.as_str().eq_ignore_ascii_case("r") => self.restart = false,
            _ => {}
        }
    }

    fn recompute_xaxis(&mut self) {
        self.xaxis = match (self.left_held, self.right_held) {
            (true, false) => -1.0,
            (false, true) => 1.0,
            _ => 0.0,
        };
    }
}
