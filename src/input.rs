use std::collections::HashSet;
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GamepadButton {
    South,
    East,
    West,
    North,
    Start,
    Select,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

#[derive(Debug, Default)]
pub struct InputState {
    pressed: HashSet<KeyCode>,
    just_pressed: HashSet<KeyCode>,
    just_released: HashSet<KeyCode>,
    mouse_pressed: HashSet<MouseButton>,
    mouse_just_pressed: HashSet<MouseButton>,
    mouse_just_released: HashSet<MouseButton>,
    mouse_position: (f32, f32),
    mouse_delta: (f32, f32),
}

impl InputState {
    pub fn begin_frame(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
        self.mouse_just_pressed.clear();
        self.mouse_just_released.clear();
        self.mouse_delta = (0.0, 0.0);
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
    pub fn press_mouse(&mut self, button: MouseButton) {
        if self.mouse_pressed.insert(button) {
            self.mouse_just_pressed.insert(button);
        }
    }
    pub fn release_mouse(&mut self, button: MouseButton) {
        if self.mouse_pressed.remove(&button) {
            self.mouse_just_released.insert(button);
        }
    }
    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.mouse_pressed.contains(&button)
    }
    pub fn mouse_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_just_pressed.contains(&button)
    }
    pub fn mouse_just_released(&self, button: MouseButton) -> bool {
        self.mouse_just_released.contains(&button)
    }
    pub fn set_mouse_position(&mut self, x: f32, y: f32) {
        self.mouse_delta.0 += x - self.mouse_position.0;
        self.mouse_delta.1 += y - self.mouse_position.1;
        self.mouse_position = (x, y);
    }
    pub fn mouse_position(&self) -> (f32, f32) {
        self.mouse_position
    }
    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
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

    #[test]
    fn tracks_mouse_edges_and_motion() {
        let mut input = InputState::default();
        input.set_mouse_position(10.0, 20.0);
        input.set_mouse_position(13.0, 16.0);
        input.press_mouse(MouseButton::Left);
        assert_eq!(input.mouse_delta(), (13.0, 16.0));
        assert!(input.mouse_pressed(MouseButton::Left));
        assert!(input.mouse_just_pressed(MouseButton::Left));
        input.begin_frame();
        assert_eq!(input.mouse_delta(), (0.0, 0.0));
        assert!(!input.mouse_just_pressed(MouseButton::Left));
        input.release_mouse(MouseButton::Left);
        assert!(input.mouse_just_released(MouseButton::Left));
    }
}
