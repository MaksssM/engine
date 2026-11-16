use bevy_ecs::world::World;
use serde::{Deserialize, Serialize};

use crate::core::components::Visible;
use crate::core::uid::Uid;

/// Marker bundle for spawning a minimal "game object".
///
/// In UE5 terms this corresponds to an Actor with a root
/// SceneComponent.  Unlike UE5 we use ECS composition, so
/// `GameObject` is just a convenience group of the components
/// every spawned entity should have.
pub struct GameObject;

impl GameObject {
    /// Spawn a new game object with a unique id, a name,
    /// default Transform, and Visible(true).
    pub fn spawn(world: &mut World, name: impl Into<String>) -> bevy_ecs::entity::Entity {
        use crate::ecs::Name;
        use crate::transform::Transform;

        let entity = world
            .spawn((
                Uid::new(),
                Name(name.into()),
                Transform::default(),
                Visible::default(),
            ))
            .id();
        let uid = *world
            .get::<Uid>(entity)
            .expect("spawned GameObject has Uid");
        if let Some(mut registry) =
            world.get_resource_mut::<crate::core::resources::PersistentIdRegistry>()
        {
            registry
                .register(uid, entity)
                .expect("generated GameObject UID must be unique");
        }
        entity
    }

    /// Spawn with explicit transform.
    pub fn spawn_with_transform(
        world: &mut World,
        name: impl Into<String>,
        transform: crate::transform::Transform,
    ) -> bevy_ecs::entity::Entity {
        use crate::ecs::Name;

        let entity = world
            .spawn((Uid::new(), Name(name.into()), transform, Visible::default()))
            .id();
        let uid = *world
            .get::<Uid>(entity)
            .expect("spawned GameObject has Uid");
        if let Some(mut registry) =
            world.get_resource_mut::<crate::core::resources::PersistentIdRegistry>()
        {
            registry
                .register(uid, entity)
                .expect("generated GameObject UID must be unique");
        }
        entity
    }
}

/// Classification of engine object roles — extensible via the Tag component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectRole {
    StaticMesh,
    DynamicMesh,
    Light,
    Camera,
    Audio,
    Empty,
    Group,
    Custom,
}

impl Default for ObjectRole {
    fn default() -> Self {
        Self::Empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_object_spawn_has_uid() {
        let mut world = World::new();
        let e = GameObject::spawn(&mut world, "Test Actor");

        assert!(world.get::<Uid>(e).is_some());
        assert!(world.get::<crate::ecs::Name>(e).is_some());
        assert!(world.get::<crate::transform::Transform>(e).is_some());
        assert!(world.get::<Visible>(e).is_some());
    }

    #[test]
    fn game_object_spawn_with_transform() {
        use crate::transform::Transform;
        use glam::Vec3;

        let mut world = World::new();
        let pos = Vec3::new(1.0, 2.0, 3.0);
        let e = GameObject::spawn_with_transform(
            &mut world,
            "Positioned",
            Transform::from_translation(pos),
        );
        let t = world.get::<Transform>(e).unwrap();
        assert_eq!(t.translation, pos);
    }
}
