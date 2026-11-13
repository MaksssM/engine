use std::collections::HashSet;
use winit::keyboard::KeyCode;

#[derive(Debug, Default)]
pub struct InputState {
    pressed: HashSet<KeyCode>,
    just_pressed: HashSet<KeyCode>,
    just_released: HashSet<KeyCode>,
}

impl InputState {
    pub fn begin_frame(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
    }
    pub fn press(&mut self, key: KeyCode) {
        if self.pressed.insert(key) {
            self.just_pressed.insert(key);
        }
    }
    pub fn release(&mut self, key: KeyCode) {
        if self.pressed.remove(&key) {
            self.just_released.insert(key);
        }
    }
    pub fn pressed(&self, key: KeyCode) -> bool {
        self.pressed.contains(&key)
    }
    pub fn just_pressed(&self, key: KeyCode) -> bool {
        self.just_pressed.contains(&key)
    }
    pub fn just_released(&self, key: KeyCode) -> bool {
        self.just_released.contains(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracks_edges_and_held_state() {
        let mut input = InputState::default();
        input.press(KeyCode::Space);
        assert!(input.pressed(KeyCode::Space) && input.just_pressed(KeyCode::Space));
        input.begin_frame();
        assert!(input.pressed(KeyCode::Space) && !input.just_pressed(KeyCode::Space));
        input.release(KeyCode::Space);
        assert!(!input.pressed(KeyCode::Space) && input.just_released(KeyCode::Space));
    }
}
