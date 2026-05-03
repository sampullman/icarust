use ggez::input::keyboard::KeyInput;
use ggez::winit::keyboard::{Key, NamedKey};

#[derive(Debug, Default)]
pub struct InputState {
    pub xaxis: f32,
    pub yaxis: f32,
    pub fire: bool,
    pub quit: bool,
}

impl InputState {
    pub fn handle_key_down(&mut self, input: KeyInput) {
        match input.event.logical_key {
            Key::Named(NamedKey::ArrowUp) => self.yaxis = 1.0,
            Key::Named(NamedKey::ArrowLeft) => self.xaxis = -1.0,
            Key::Named(NamedKey::ArrowRight) => self.xaxis = 1.0,
            Key::Named(NamedKey::Space) => self.fire = true,
            Key::Named(NamedKey::Escape) => self.quit = true,
            _ => {}
        }
    }

    pub fn handle_key_up(&mut self, input: KeyInput) {
        match input.event.logical_key {
            Key::Named(NamedKey::ArrowUp) => self.yaxis = 0.0,
            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => self.xaxis = 0.0,
            Key::Named(NamedKey::Space) => self.fire = false,
            _ => {}
        }
    }
}
