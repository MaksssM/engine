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
    pub delta_seconds: f32,
    pub fixed_delta_seconds: f32,
    pub frame_number: u64,
}

impl Default for EngineTime {
    fn default() -> Self {
        Self {
            clock: Clock::new(),
            delta_seconds: 0.0,
            fixed_delta_seconds: 1.0 / 60.0,
            frame_number: 0,
        }
    }
}

/// Wrapper for the input state as a Bevy ECS Resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputContext {
    Editor,
    Game,
    Both,
}

#[derive(bevy_ecs::resource::Resource)]
pub struct EngineInput {
    pub state: InputState,
    pub context: InputContext,
}

impl Default for EngineInput {
    fn default() -> Self {
        Self {
            state: InputState::default(),
            context: InputContext::Editor,
        }
    }
}

impl EngineInput {
    pub fn accepts_editor(&self) -> bool {
        matches!(self.context, InputContext::Editor | InputContext::Both)
    }
    pub fn accepts_game(&self) -> bool {
        matches!(self.context, InputContext::Game | InputContext::Both)
    }
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

/// Renderer settings shared by runtime, editor and render systems.
#[derive(bevy_ecs::resource::Resource, Clone, Debug)]
pub struct RendererConfiguration {
    pub render_scale_percent: u32,
    pub post_processing: bool,
    pub msaa_samples: u32,
    pub shadow_quality: crate::quality::ShadowQuality,
}

impl Default for RendererConfiguration {
    fn default() -> Self {
        let settings = crate::quality::QualityPreset::Balanced.settings();
        Self {
            render_scale_percent: settings.render_scale_percent,
            post_processing: settings.post_processing,
            msaa_samples: settings.msaa_samples,
            shadow_quality: settings.shadow_quality,
        }
    }
}

/// Project-wide mapping between persistent object IDs and transient ECS entities.
#[derive(bevy_ecs::resource::Resource, Default)]
pub struct PersistentIdRegistry {
    entities: std::collections::HashMap<crate::core::uid::Uid, bevy_ecs::entity::Entity>,
}

impl PersistentIdRegistry {
    pub fn register(
        &mut self,
        uid: crate::core::uid::Uid,
        entity: bevy_ecs::entity::Entity,
    ) -> Result<(), RegistryError> {
        if !uid.is_valid() {
            return Err(RegistryError::InvalidId);
        }
        if let Some(existing) = self.entities.get(&uid).copied() {
            if existing != entity {
                return Err(RegistryError::DuplicateId(uid));
            }
        }
        self.entities.insert(uid, entity);
        Ok(())
    }

    pub fn unregister(&mut self, uid: crate::core::uid::Uid) -> Option<bevy_ecs::entity::Entity> {
        self.entities.remove(&uid)
    }

    pub fn unregister_entity(&mut self, entity: bevy_ecs::entity::Entity) {
        self.entities.retain(|_, mapped| *mapped != entity);
    }

    pub fn resolve(&self, uid: crate::core::uid::Uid) -> Option<bevy_ecs::entity::Entity> {
        self.entities.get(&uid).copied()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    InvalidId,
    DuplicateId(crate::core::uid::Uid),
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

impl EngineMode {
    pub fn enter_play(&mut self) -> bool {
        if *self == Self::Editor {
            *self = Self::Playing;
            true
        } else {
            false
        }
    }

    pub fn pause(&mut self) -> bool {
        if *self == Self::Playing {
            *self = Self::Paused;
            true
        } else {
            false
        }
    }

    pub fn resume(&mut self) -> bool {
        if *self == Self::Paused {
            *self = Self::Playing;
            true
        } else {
            false
        }
    }

    pub fn stop(&mut self) -> bool {
        if matches!(*self, Self::Playing | Self::Paused) {
            *self = Self::Editor;
            true
        } else {
            false
        }
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
    if !world.contains_resource::<RendererConfiguration>() {
        world.insert_resource(RendererConfiguration::default());
    }
    if !world.contains_resource::<PersistentIdRegistry>() {
        world.insert_resource(PersistentIdRegistry::default());
    }
    if !world.contains_resource::<crate::performance::PerformanceMonitor>() {
        world.insert_resource(crate::performance::PerformanceMonitor::default());
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
        assert!(world.contains_resource::<RendererConfiguration>());
        assert!(world.contains_resource::<PersistentIdRegistry>());
    }

    #[test]
    fn persistent_registry_rejects_collisions_and_removes_entities() {
        let mut registry = PersistentIdRegistry::default();
        let uid = crate::core::uid::Uid::from_parts(1, 2);
        let first = bevy_ecs::entity::Entity::from_raw(1);
        let second = bevy_ecs::entity::Entity::from_raw(2);
        assert_eq!(registry.register(uid, first), Ok(()));
        assert_eq!(registry.resolve(uid), Some(first));
        assert_eq!(
            registry.register(uid, second),
            Err(RegistryError::DuplicateId(uid))
        );
        registry.unregister_entity(first);
        assert_eq!(registry.resolve(uid), None);
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

    #[test]
    fn mode_transitions_are_explicit_and_legal() {
        let mut mode = EngineMode::Editor;
        assert!(mode.enter_play());
        assert!(!mode.enter_play());
        assert!(mode.pause());
        assert!(mode.resume());
        assert!(mode.stop());
        assert!(!mode.stop());
    }
}
