
use ggez::event::{Keycode, Mod};

/// **********************************************************************
/// `InputState` keeps track of the user's input state in order to make
/// keyboard events state-based and device-independent.
/// **********************************************************************
#[derive(Debug)]
pub struct InputState {
    pub xaxis: f32,
    pub yaxis: f32,
    pub fire: bool,
    pub quit: bool,
}

impl Default for InputState {
    fn default() -> Self {
        InputState {
            xaxis: 0.0,
            yaxis: 0.0,
            fire: false,
            quit: false,
        }
    }
}

impl InputState {

    pub fn handle_key_down(&mut self, keycode: Keycode, _keymod: Mod) {
        match keycode {
            Keycode::Up => {
                self.yaxis = 1.0;
            }
            Keycode::Left => {
                self.xaxis = -1.0;
            }
            Keycode::Right => {
                self.xaxis = 1.0;
            }
            Keycode::Space => {
                self.fire = true;
            }
            Keycode::Escape => {
                self.quit = true;
            },
            _ => (),
        }
    }

    pub fn handle_key_up(&mut self, keycode: Keycode, _keymod: Mod) {
        match keycode {
            Keycode::Up => {
                self.yaxis = 0.0;
            }
            Keycode::Left | Keycode::Right => {
                self.xaxis = 0.0;
            }
            Keycode::Space => {
                self.fire = false;
            }
            _ => (),
        }
    }
}
