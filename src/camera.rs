use bevy_ecs::component::Component;
use glam::{Mat4, Vec3};

#[derive(Component, Clone, Copy, Debug)]
pub struct Camera3D {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera3D {
    fn default() -> Self {
        Self {
            fov: 45.0_f32.to_radians(),
            near: 0.1,
            far: 100.0,
        }
    }
}

impl Camera3D {
    pub fn view_matrix(&self, position: Vec3, target: Vec3) -> Mat4 {
        Mat4::look_at_rh(position, target, Vec3::Y)
    }

    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov, aspect.max(0.01), self.near, self.far)
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct CameraController {
    pub movement_speed: f32,
    pub mouse_sensitivity: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for CameraController {
    fn default() -> Self {
        Self {
            movement_speed: 4.0,
            mouse_sensitivity: 0.08,
            yaw: -35.0_f32.to_radians(),
            pitch: -25.0_f32.to_radians(),
        }
    }
}

impl CameraController {
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
        .normalize()
    }

    pub fn right(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, self.yaw.cos()).normalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_controller_forward_is_normalized() {
        let controller = CameraController::default();
        assert!((controller.forward().length() - 1.0).abs() < 0.0001);
    }
}
