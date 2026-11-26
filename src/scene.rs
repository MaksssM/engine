use crate::{
    camera::Camera3D,
    core::{
        components::{FolderGroup, Light, Locked, MeshFilter, MeshRenderer, Visible},
        uid::Uid,
    },
    ecs::Name,
    transform::Transform,
};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use glam::Vec3;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, HashMap};

fn default_rotation() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}
fn default_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}
fn default_visible() -> bool {
    true
}

const CURRENT_SCENE_VERSION: u32 = 2;

pub trait SceneComponent:
    Component + Clone + Serialize + DeserializeOwned + Send + Sync + 'static
{
    const KEY: &'static str;
}

#[derive(Default)]
pub struct SceneComponentRegistry {
    serializers: BTreeMap<
        &'static str,
        Box<dyn Fn(&bevy_ecs::world::World, Entity) -> Option<ron::Value> + Send + Sync>,
    >,
    restorers: BTreeMap<
        &'static str,
        Box<
            dyn Fn(&mut bevy_ecs::world::World, Entity, &ron::Value) -> Result<(), String>
                + Send
                + Sync,
        >,
    >,
}

impl SceneComponentRegistry {
    pub fn register<T: SceneComponent>(&mut self) {
        self.serializers.insert(
            T::KEY,
            Box::new(|world, entity| {
                world
                    .get::<T>(entity)
                    .and_then(|component| ron::from_str(&ron::to_string(component).ok()?).ok())
            }),
        );
        self.restorers.insert(
            T::KEY,
            Box::new(|world, entity, value| {
                let component: T =
                    ron::from_str(&ron::to_string(value).map_err(|error| error.to_string())?)
                        .map_err(|error| error.to_string())?;
                world.entity_mut(entity).insert(component);
                Ok(())
            }),
        );
    }

    pub fn serialize_entity(
        &self,
        world: &bevy_ecs::world::World,
        entity: Entity,
    ) -> BTreeMap<String, ron::Value> {
        self.serializers
            .iter()
            .filter_map(|(key, serializer)| {
                serializer(world, entity).map(|value| ((*key).to_string(), value))
            })
            .collect()
    }

    pub fn restore_entity(
        &self,
        world: &mut bevy_ecs::world::World,
        entity: Entity,
        values: &BTreeMap<String, ron::Value>,
    ) -> Result<(), String> {
        for (key, value) in values {
            if let Some(restorer) = self.restorers.get(key.as_str()) {
                restorer(world, entity, value)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SerializedComponents {
    #[serde(default)]
    pub custom: BTreeMap<String, ron::Value>,
    #[serde(default)]
    pub mesh_filter: Option<MeshFilter>,
    #[serde(default)]
    pub mesh_renderer: Option<MeshRenderer>,
    #[serde(default)]
    pub camera: Option<Camera3D>,
    #[serde(default)]
    pub light: Option<Light>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneEntity {
    pub name: String,
    #[serde(default)]
    pub uid: Option<Uid>,
    pub translation: [f32; 3],
    #[serde(default = "default_rotation")]
    pub rotation: [f32; 4],
    #[serde(default = "default_scale")]
    pub scale: [f32; 3],
    pub parent: Option<usize>,
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub components: SerializedComponents,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    #[serde(default = "default_scene_version")]
    pub version: u32,
    pub entities: Vec<SceneEntity>,
}

fn default_scene_version() -> u32 {
    1
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            version: CURRENT_SCENE_VERSION,
            entities: Vec::new(),
        }
    }
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
            uid: Some(Uid::new()),
            translation: translation.to_array(),
            rotation: default_rotation(),
            scale: default_scale(),
            parent,
            visible: true,
            locked: false,
            folder: None,
            components: SerializedComponents::default(),
        });
        index
    }

    /// Construct a deterministic PBR validation scene with 9 distinct material types
    /// arranged in a 3x3 grid, plus a directional key light and a fill light.
    pub fn pbr_validation_scene() -> Self {
        let mut scene = Self::new();

        // Key light
        let key_light_entity = SceneEntity {
            name: "Key Directional Light".into(),
            uid: Some(Uid::new()),
            translation: [5.0, 10.0, 7.0],
            rotation: default_rotation(),
            scale: default_scale(),
            parent: None,
            visible: true,
            locked: false,
            folder: Some("Lighting".into()),
            components: SerializedComponents {
                light: Some(Light {
                    kind: crate::core::components::LightKind::Directional,
                    color: [1.0, 0.98, 0.95],
                    intensity: 2.5,
                    range: 50.0,
                    cast_shadows: true,
                }),
                ..Default::default()
            },
        };
        scene.entities.push(key_light_entity);

        // Fill light
        let fill_light_entity = SceneEntity {
            name: "Fill Directional Light".into(),
            uid: Some(Uid::new()),
            translation: [-5.0, 5.0, -5.0],
            rotation: default_rotation(),
            scale: default_scale(),
            parent: None,
            visible: true,
            locked: false,
            folder: Some("Lighting".into()),
            components: SerializedComponents {
                light: Some(Light {
                    kind: crate::core::components::LightKind::Directional,
                    color: [0.6, 0.7, 0.85],
                    intensity: 0.8,
                    range: 50.0,
                    cast_shadows: false,
                }),
                ..Default::default()
            },
        };
        scene.entities.push(fill_light_entity);

        // 9 PBR Materials
        let pbr_configs = [
            (
                "PBR Red Plastic",
                "builtin/pbr_red_plastic",
                [-3.0, 3.0, 0.0],
            ),
            (
                "PBR Polished Metal",
                "builtin/pbr_polished_metal",
                [0.0, 3.0, 0.0],
            ),
            (
                "PBR Rough Metal",
                "builtin/pbr_rough_metal",
                [3.0, 3.0, 0.0],
            ),
            (
                "PBR Matte Dielectric",
                "builtin/pbr_matte_dielectric",
                [-3.0, 0.0, 0.0],
            ),
            (
                "PBR Normal Mapped",
                "builtin/pbr_normal_mapped",
                [0.0, 0.0, 0.0],
            ),
            ("PBR Emissive", "builtin/pbr_emissive", [3.0, 0.0, 0.0]),
            (
                "PBR Alpha Mask",
                "builtin/pbr_alpha_mask",
                [-3.0, -3.0, 0.0],
            ),
            (
                "PBR Alpha Blend",
                "builtin/pbr_alpha_blend",
                [0.0, -3.0, 0.0],
            ),
            (
                "PBR Double Sided",
                "builtin/pbr_double_sided",
                [3.0, -3.0, 0.0],
            ),
        ];

        for (name, mat_path, pos) in pbr_configs {
            let entity = SceneEntity {
                name: name.into(),
                uid: Some(Uid::new()),
                translation: pos,
                rotation: default_rotation(),
                scale: default_scale(),
                parent: None,
                visible: true,
                locked: false,
                folder: Some("Validation Objects".into()),
                components: SerializedComponents {
                    mesh_filter: Some(MeshFilter {
                        mesh_path: "builtin/sphere".into(),
                    }),
                    mesh_renderer: Some(MeshRenderer {
                        material_path: mat_path.into(),
                        cast_shadows: true,
                        receive_shadows: true,
                    }),
                    ..Default::default()
                },
            };
            scene.entities.push(entity);
        }

        scene
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
    play_snapshot: Option<Scene>,
    play_entities: Vec<Entity>,
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
        let mut seen_uids = std::collections::HashSet::with_capacity(scene.entities.len());
        for item in &scene.entities {
            let uid = item.uid.unwrap_or_default();
            if !uid.is_valid() || !seen_uids.insert(uid) {
                return None;
            }
            if world
                .get_resource::<crate::core::resources::PersistentIdRegistry>()
                .and_then(|registry| registry.resolve(uid))
                .is_some()
            {
                return None;
            }
        }
        let mut entities = Vec::with_capacity(scene.entities.len());
        for item in &scene.entities {
            let uid = item.uid.unwrap_or_default();
            let entity = world
                .spawn((
                    Name(item.name.clone()),
                    uid,
                    Transform {
                        translation: Vec3::from_array(item.translation),
                        rotation: glam::Quat::from_xyzw(
                            item.rotation[0],
                            item.rotation[1],
                            item.rotation[2],
                            item.rotation[3],
                        ),
                        scale: Vec3::from_array(item.scale),
                    },
                    Visible(item.visible),
                    Locked(item.locked),
                ))
                .id();
            if let Some(component) = &item.components.mesh_filter {
                world.entity_mut(entity).insert(component.clone());
            }
            if let Some(component) = &item.components.mesh_renderer {
                world.entity_mut(entity).insert(component.clone());
            }
            if let Some(component) = item.components.camera {
                world.entity_mut(entity).insert(component);
            }
            if let Some(component) = item.components.light {
                world.entity_mut(entity).insert(component);
            }
            if let Some(mut registry) =
                world.get_resource_mut::<crate::core::resources::PersistentIdRegistry>()
            {
                if registry.register(uid, entity).is_err() {
                    world.despawn(entity);
                    return None;
                }
            }
            entities.push(entity);
            if let Some(folder) = &item.folder {
                world.entity_mut(entity).insert(FolderGroup(folder.clone()));
            }
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
        Self::despawn_entities(world, &mut self.active_entities);
    }

    fn despawn_entities(world: &mut bevy_ecs::world::World, entities: &mut Vec<Entity>) {
        for entity in entities.drain(..) {
            if let Some(uid) = world.get::<Uid>(entity).copied() {
                if let Some(mut registry) =
                    world.get_resource_mut::<crate::core::resources::PersistentIdRegistry>()
                {
                    registry.unregister(uid);
                }
            }
            let _ = world.despawn(entity);
        }
    }

    pub fn enter_play(&mut self, world: &mut bevy_ecs::world::World) -> bool {
        if self.play_snapshot.is_some() || self.active.is_none() {
            return false;
        }
        let name = self.active.clone().unwrap();
        let snapshot = match self.scenes.get(&name) {
            Some(scene) => scene.clone(),
            None => return false,
        };
        Self::despawn_entities(world, &mut self.active_entities);
        let Some(entities) = self.spawn_into(&name, world) else {
            return false;
        };
        self.play_snapshot = Some(snapshot);
        self.play_entities = entities;
        true
    }

    pub fn stop_play(&mut self, world: &mut bevy_ecs::world::World) -> bool {
        let Some(snapshot) = self.play_snapshot.take() else {
            return false;
        };
        Self::despawn_entities(world, &mut self.play_entities);
        let Some(name) = self.active.clone() else {
            return false;
        };
        self.scenes.insert(name.clone(), snapshot);
        self.active_entities = self.spawn_into(&name, world).unwrap_or_default();
        true
    }

    pub fn is_playing(&self) -> bool {
        self.play_snapshot.is_some()
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

    #[test]
    fn scene_round_trip_preserves_hierarchy_and_components() {
        let mut manager = SceneManager::default();
        let scene = manager.create("model");
        let root = scene.add_entity("Root", Vec3::ZERO, None);
        let child = scene.add_entity("Child", Vec3::Y * 2.0, Some(root));
        let grandchild = scene.add_entity("Grandchild", Vec3::Z * 3.0, Some(child));

        scene.entities[root].components.mesh_filter = Some(MeshFilter {
            mesh_path: "meshes/root.obj".to_string(),
        });
        scene.entities[root].components.mesh_renderer = Some(MeshRenderer {
            material_path: "materials/root.mat".to_string(),
            cast_shadows: true,
            receive_shadows: false,
        });
        scene.entities[child].components.camera = Some(Camera3D::default());
        scene.entities[grandchild].components.light = Some(Light {
            color: [1.0, 0.5, 0.25],
            intensity: 3.5,
            range: 20.0,
            ..Default::default()
        });

        let serialized = scene.to_ron().unwrap();
        let mut manager2 = SceneManager::default();
        manager2.insert("copy", Scene::from_ron(&serialized).unwrap());
        assert!(manager2.get("copy").unwrap().validate().is_ok());

        let mut world = bevy_ecs::world::World::new();
        world.insert_resource(crate::core::resources::PersistentIdRegistry::default());
        manager2.activate("copy");
        let entities = manager2.spawn_active(&mut world).unwrap();
        assert_eq!(entities.len(), 3);

        let e_root = entities[root];
        let e_child = entities[child];
        let e_grandchild = entities[grandchild];

        // Hierarchy restored
        assert_eq!(world.get::<crate::transform::Parent>(e_root), None);
        assert_eq!(
            world.get::<crate::transform::Parent>(e_child),
            Some(&crate::transform::Parent(e_root))
        );
        assert_eq!(
            world.get::<crate::transform::Parent>(e_grandchild),
            Some(&crate::transform::Parent(e_child))
        );

        // Transforms restored
        assert_eq!(
            world.get::<Transform>(e_child).map(|t| t.translation),
            Some(Vec3::Y * 2.0)
        );

        // Components restored
        assert_eq!(
            world
                .get::<MeshFilter>(e_root)
                .map(|c| c.mesh_path.as_str()),
            Some("meshes/root.obj")
        );
        assert!(world.get::<MeshRenderer>(e_root).is_some());
        assert!(world.get::<Camera3D>(e_child).is_some());
        assert_eq!(
            world.get::<Light>(e_grandchild).map(|l| l.intensity),
            Some(3.5)
        );

        // UIDs registered
        {
            let registry = world.resource::<crate::core::resources::PersistentIdRegistry>();
            assert!(
                registry
                    .resolve(manager2.get("copy").unwrap().entities[root].uid.unwrap())
                    .is_some()
            );
        }
    }

    #[test]
    fn pbr_validation_scene_has_9_distinct_materials_and_lighting() {
        let scene = Scene::pbr_validation_scene();
        assert!(scene.validate().is_ok());

        // 2 lights + 9 validation spheres = 11 entities
        assert_eq!(scene.entities.len(), 11);

        let lights: Vec<_> = scene
            .entities
            .iter()
            .filter(|e| e.components.light.is_some())
            .collect();
        assert_eq!(lights.len(), 2, "must have key and fill lights");

        let pbr_objects: Vec<_> = scene
            .entities
            .iter()
            .filter(|e| e.components.mesh_renderer.is_some())
            .collect();
        assert_eq!(pbr_objects.len(), 9, "must have 9 PBR validation spheres");

        let mut material_paths = std::collections::HashSet::new();
        for obj in &pbr_objects {
            let mr = obj.components.mesh_renderer.as_ref().unwrap();
            material_paths.insert(mr.material_path.clone());
            assert_eq!(
                obj.components.mesh_filter.as_ref().unwrap().mesh_path,
                "builtin/sphere"
            );
        }
        assert_eq!(
            material_paths.len(),
            9,
            "all 9 objects must have distinct material assets"
        );

        // Serialize and deserialize roundtrip
        let ron_str = scene.to_ron().expect("serialize pbr validation scene");
        let deserialized = Scene::from_ron(&ron_str).expect("deserialize pbr validation scene");
        assert_eq!(deserialized.entities.len(), 11);

        // Ensure directory and file can be written to assets/scenes/pbr_validation.ron
        let pbr_path = std::path::Path::new("assets/scenes/pbr_validation.ron");
        if let Some(parent) = pbr_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(pbr_path, &ron_str).expect("write assets/scenes/pbr_validation.ron");
    }
}
