use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};

use crate::input::InputState;
use crate::time::Clock;

// ---------------------------------------------------------------------------
// Engine-wide resources exposed through the ECS World
// ---------------------------------------------------------------------------

/// Wrapper that makes `Clock` available as a Bevy ECS Resource.
#[derive(bevy_ecs::resource::Resource)]
pub struct EngineTime {
    pub clock: Clock,
}

impl Default for EngineTime {
    fn default() -> Self {
        Self {
            clock: Clock::new(),
        }
    }
}

/// Wrapper for the input state as a Bevy ECS Resource.
#[derive(bevy_ecs::resource::Resource, Default)]
pub struct EngineInput {
    pub state: InputState,
}

/// Global engine configuration stored as a Bevy ECS Resource.
#[derive(bevy_ecs::resource::Resource, Clone, Debug, Serialize, Deserialize)]
pub struct EngineConfig {
    pub project_name: String,
    pub project_version: String,
    pub gravity: f32,
    pub fixed_timestep_hz: f64,
    pub max_fps: Option<u32>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            project_name: "Untitled Project".into(),
            project_version: "0.1.0".into(),
            gravity: -9.81,
            fixed_timestep_hz: 60.0,
            max_fps: None,
        }
    }
}

/// Current engine mode resource.
#[derive(
    bevy_ecs::resource::Resource, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum EngineMode {
    Editor,
    Playing,
    Paused,
}

impl Default for EngineMode {
    fn default() -> Self {
        Self::Editor
    }
}

/// Insert all core resources into the world with their defaults.
pub fn init_core_resources(world: &mut World) {
    if !world.contains_resource::<EngineTime>() {
        world.insert_resource(EngineTime::default());
    }
    if !world.contains_resource::<EngineInput>() {
        world.insert_resource(EngineInput::default());
    }
    if !world.contains_resource::<EngineConfig>() {
        world.insert_resource(EngineConfig::default());
    }
    if !world.contains_resource::<EngineMode>() {
        world.insert_resource(EngineMode::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_core_resources_idempotent() {
        let mut world = World::new();
        init_core_resources(&mut world);
        init_core_resources(&mut world); // must not panic on second call

        assert!(world.contains_resource::<EngineTime>());
        assert!(world.contains_resource::<EngineInput>());
        assert!(world.contains_resource::<EngineConfig>());
        assert!(world.contains_resource::<EngineMode>());
    }

    #[test]
    fn engine_config_defaults() {
        let cfg = EngineConfig::default();
        assert_eq!(cfg.gravity, -9.81);
        assert_eq!(cfg.fixed_timestep_hz, 60.0);
    }

    #[test]
    fn engine_mode_default_is_editor() {
        assert_eq!(EngineMode::default(), EngineMode::Editor);
    }
}
