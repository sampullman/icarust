use ggez::input::keyboard::{KeyCode, KeyInput};
use ggez::winit::keyboard::PhysicalKey;
use sim::PlayerInput;

#[derive(Debug, Default)]
pub struct InputState {
    pub xaxis: f32,
    pub yaxis: f32,
    pub fire: bool,
    pub quit: bool,
    left_held: bool,
    right_held: bool,
}

impl InputState {
    pub fn handle_key_down(&mut self, input: KeyInput) {
        let Some(code) = key_code(&input) else { return };
        match code {
            KeyCode::ArrowUp => self.yaxis = 1.0,
            KeyCode::ArrowLeft => {
                self.left_held = true;
                self.recompute_xaxis();
            }
            KeyCode::ArrowRight => {
                self.right_held = true;
                self.recompute_xaxis();
            }
            KeyCode::Space => self.fire = true,
            KeyCode::Escape => self.quit = true,
            _ => {}
        }
    }

    pub fn handle_key_up(&mut self, input: KeyInput) {
        let Some(code) = key_code(&input) else { return };
        match code {
            KeyCode::ArrowUp => self.yaxis = 0.0,
            KeyCode::ArrowLeft => {
                self.left_held = false;
                self.recompute_xaxis();
            }
            KeyCode::ArrowRight => {
                self.right_held = false;
                self.recompute_xaxis();
            }
            KeyCode::Space => self.fire = false,
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

    pub fn to_player_input(&self) -> PlayerInput {
        PlayerInput {
            xaxis: self.xaxis,
            yaxis: self.yaxis,
            fire: self.fire,
        }
    }
}

fn key_code(input: &KeyInput) -> Option<KeyCode> {
    match input.event.physical_key {
        PhysicalKey::Code(c) => Some(c),
        PhysicalKey::Unidentified(_) => None,
    }
}
