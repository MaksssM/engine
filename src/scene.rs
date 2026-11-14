use crate::transform::Transform;
use bevy_ecs::entity::Entity;
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneEntity {
    pub name: String,
    pub translation: [f32; 3],
    pub parent: Option<usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    pub entities: Vec<SceneEntity>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn validate(&self) -> Result<(), String> {
        for (index, entity) in self.entities.iter().enumerate() {
            if let Some(parent) = entity.parent {
                if parent >= self.entities.len() {
                    return Err(format!("entity {index} has invalid parent index {parent}"));
                }
                if parent == index {
                    return Err(format!("entity {index} cannot parent itself"));
                }
            }
        }
        for start in 0..self.entities.len() {
            let mut current = Some(start);
            let mut seen = std::collections::HashSet::new();
            while let Some(index) = current {
                if !seen.insert(index) {
                    return Err(format!("scene contains a parent cycle at entity {index}"));
                }
                current = self.entities[index].parent;
            }
        }
        Ok(())
    }
    pub fn add_entity(
        &mut self,
        name: impl Into<String>,
        translation: Vec3,
        parent: Option<usize>,
    ) -> usize {
        let index = self.entities.len();
        self.entities.push(SceneEntity {
            name: name.into(),
            translation: translation.to_array(),
            parent,
        });
        index
    }
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }
    pub fn from_ron(source: &str) -> Result<Self, ron::de::SpannedError> {
        ron::from_str(source)
    }
}

#[derive(Debug, Default)]
pub struct SceneManager {
    scenes: HashMap<String, Scene>,
    active: Option<String>,
    active_entities: Vec<Entity>,
}

impl SceneManager {
    pub fn create(&mut self, name: impl Into<String>) -> &mut Scene {
        let name = name.into();
        self.scenes.entry(name).or_default()
    }
    pub fn insert(&mut self, name: impl Into<String>, scene: Scene) {
        self.scenes.insert(name.into(), scene);
    }
    pub fn get(&self, name: &str) -> Option<&Scene> {
        self.scenes.get(name)
    }
    pub fn activate(&mut self, name: &str) -> bool {
        if self.scenes.contains_key(name) {
            self.active = Some(name.into());
            true
        } else {
            false
        }
    }
    pub fn active(&self) -> Option<&Scene> {
        self.active
            .as_deref()
            .and_then(|name| self.scenes.get(name))
    }
    pub fn spawn_into(
        &self,
        name: &str,
        world: &mut bevy_ecs::world::World,
    ) -> Option<Vec<Entity>> {
        let scene = self.scenes.get(name)?;
        if scene.validate().is_err() {
            return None;
        }
        let mut entities = Vec::with_capacity(scene.entities.len());
        for item in &scene.entities {
            entities.push(
                world
                    .spawn(Transform::from_translation(Vec3::from_array(
                        item.translation,
                    )))
                    .id(),
            );
        }
        for (index, item) in scene.entities.iter().enumerate() {
            if let Some(parent) = item.parent {
                world
                    .entity_mut(entities[index])
                    .insert(crate::transform::Parent(entities[parent]));
            }
        }
        Some(entities)
    }

    pub fn spawn_active(&mut self, world: &mut bevy_ecs::world::World) -> Option<&[Entity]> {
        let name = self.active.clone()?;
        let entities = self.spawn_into(&name, world)?;
        self.active_entities = entities;
        Some(&self.active_entities)
    }

    pub fn active_entities(&self) -> &[Entity] {
        &self.active_entities
    }

    pub fn unload_active(&mut self, world: &mut bevy_ecs::world::World) {
        for entity in self.active_entities.drain(..) {
            world.despawn(entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_scene_parents_are_rejected() {
        let mut scene = Scene::new();
        scene.add_entity("child", Vec3::ZERO, Some(1));
        assert!(scene.validate().is_err());
        scene.entities[0].parent = Some(0);
        assert!(scene.validate().is_err());
    }

    #[test]
    fn spawn_active_unloads_previous_entities() {
        let mut manager = SceneManager::default();
        let mut world = bevy_ecs::world::World::new();
        manager.create("first").add_entity("first", Vec3::X, None);
        manager.activate("first");
        let first = manager.spawn_active(&mut world).unwrap()[0];
        assert!(world.get::<Transform>(first).is_some());

        manager.create("second").add_entity("second", Vec3::Y, None);
        manager.activate("second");
        manager.unload_active(&mut world);
        assert!(world.get::<Transform>(first).is_none());
        let second = manager.spawn_active(&mut world).unwrap()[0];
        assert!(world.get::<Transform>(second).is_some());
    }

    #[test]
    fn scene_round_trip_and_activation() {
        let mut manager = SceneManager::default();
        manager.create("level").add_entity("root", Vec3::X, None);
        let source = manager.get("level").unwrap().to_ron().unwrap();
        manager.insert("copy", Scene::from_ron(&source).unwrap());
        assert!(manager.activate("copy"));
        assert_eq!(manager.active().unwrap().entities.len(), 1);
    }
}
