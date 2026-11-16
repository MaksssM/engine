use bevy_ecs::{component::Component, entity::Entity, world::World};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::default()
        }
    }
    pub fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Parent(pub Entity);

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct GlobalTransform(pub Mat4);

impl Default for GlobalTransform {
    fn default() -> Self {
        Self(Mat4::IDENTITY)
    }
}

pub fn propagate_transforms(world: &mut World) {
    let roots: Vec<Entity> = world
        .query_filtered::<Entity, (
            bevy_ecs::query::With<Transform>,
            bevy_ecs::query::Without<Parent>,
        )>()
        .iter(world)
        .collect();
    let mut visited = std::collections::HashSet::new();
    for root in roots {
        propagate_entity(world, root, Mat4::IDENTITY, &mut visited);
    }
}

fn propagate_entity(
    world: &mut World,
    entity: Entity,
    parent: Mat4,
    visited: &mut std::collections::HashSet<Entity>,
) {
    if !visited.insert(entity) {
        return;
    }
    let Some(local) = world.get::<Transform>(entity).copied() else {
        return;
    };
    let global = parent * local.matrix();
    if let Some(mut component) = world.get_mut::<GlobalTransform>(entity) {
        component.0 = global;
    } else {
        world.entity_mut(entity).insert(GlobalTransform(global));
    }
    let children: Vec<Entity> = world
        .query::<(Entity, &Parent)>()
        .iter(world)
        .filter_map(|(child, child_parent)| (child_parent.0 == entity).then_some(child))
        .collect();
    for child in children {
        propagate_entity(world, child, global, visited);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_child_world_transform_propagates() {
        let mut world = World::new();
        let parent = world
            .spawn((Transform::from_translation(Vec3::new(10.0, 0.0, 0.0)),))
            .id();
        let child = world
            .spawn((
                Transform::from_translation(Vec3::new(2.0, 3.0, 0.0)),
                Parent(parent),
            ))
            .id();
        propagate_transforms(&mut world);
        let position = world
            .get::<GlobalTransform>(child)
            .unwrap()
            .0
            .transform_point3(Vec3::ZERO);
        assert_eq!(position, Vec3::new(12.0, 3.0, 0.0));
    }
}
