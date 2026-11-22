//! # Undo/Redo Command System
//!
//! Provides a Command Pattern implementation for undoable operations in the editor.
//! Each command stores enough information to execute, undo, and redo an action.

use crate::core::components::{FolderGroup, Locked, Visible};
use crate::core::resources::PersistentIdRegistry;
use crate::core::uid::Uid;
use crate::ecs::Name;
use crate::transform::{Parent, Transform};
use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use glam::Vec3;

/// Maximum number of commands to keep in history by default.
pub const DEFAULT_HISTORY_CAPACITY: usize = 256;

/// Trait for all undoable commands.
///
/// Each command knows how to:
/// - Execute (perform) the action
/// - Undo (reverse) the action
/// - Optionally provide custom redo logic
pub trait Command: Send + Sync {
    /// Human-readable name of the command for UI display.
    fn name(&self) -> &str;

    /// Brief description of what the command does.
    fn description(&self) -> &str {
        ""
    }

    /// Execute the command on the given world.
    fn execute(&mut self, world: &mut World);

    /// Undo the command, restoring the previous state.
    fn undo(&mut self, world: &mut World);

    /// Redo the command. Defaults to re-executing.
    fn redo(&mut self, world: &mut World) {
        self.execute(world);
    }

    /// Downcast to Any for accessing command-specific data.
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Centralized history manager for undo/redo operations.
///
/// Uses two separate stacks:
/// - **Undo Stack**: contains executed commands that can be undone
/// - **Redo Stack**: contains commands that were undone and can be redone
pub struct HistoryManager {
    /// Stack of executed commands that can be undone (LIFO).
    undo_stack: Vec<Box<dyn Command>>,
    /// Stack of undone commands that can be redone (LIFO).
    redo_stack: Vec<Box<dyn Command>>,
    /// Maximum number of commands to keep in each stack.
    capacity: usize,
}

impl Default for HistoryManager {
    fn default() -> Self {
        Self::new(DEFAULT_HISTORY_CAPACITY)
    }
}

impl HistoryManager {
    /// Create a new HistoryManager with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            undo_stack: Vec::with_capacity(capacity),
            redo_stack: Vec::new(),
            capacity,
        }
    }

    /// Push a new command to the history.
    /// This executes the command and clears the redo stack.
    pub fn push(&mut self, mut command: Box<dyn Command>, world: &mut World) {
        command.execute(world);

        self.undo_stack.push(command);

        // Clear redo stack when a new command is executed
        self.redo_stack.clear();

        // Trim if exceeding capacity
        if self.undo_stack.len() > self.capacity {
            self.undo_stack.remove(0);
        }
    }

    /// Undo the last command.
    /// Returns the name of the undone command, or None if nothing to undo.
    pub fn undo(&mut self, world: &mut World) -> Option<String> {
        let mut command = self.undo_stack.pop()?;
        let name = command.name().to_string();
        command.undo(world);
        self.redo_stack.push(command);
        Some(name)
    }

    /// Redo the last undone command.
    /// Returns the name of the redone command, or None if nothing to redo.
    pub fn redo(&mut self, world: &mut World) -> Option<String> {
        let mut command = self.redo_stack.pop()?;
        let name = command.name().to_string();
        command.redo(world);
        self.undo_stack.push(command);
        Some(name)
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Get the name of the next command that would be undone.
    pub fn next_undo_name(&self) -> Option<&str> {
        self.undo_stack.last().map(|c| c.name())
    }

    /// Get the name of the next command that would be redone.
    pub fn next_redo_name(&self) -> Option<&str> {
        self.redo_stack.last().map(|c| c.name())
    }

    /// Set a new capacity (truncates if new capacity is smaller).
    pub fn set_capacity(&mut self, new_capacity: usize) {
        self.capacity = new_capacity;
        while self.undo_stack.len() > self.capacity {
            self.undo_stack.remove(0);
        }
    }

    /// Get the current capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get the number of commands in the undo stack.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Get the number of commands in the redo stack.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Get the name of the next command that would be undone.
    pub fn undo_name(&self) -> Option<&str> {
        self.next_undo_name()
    }

    /// Get the name of the next command that would be redone.
    pub fn redo_name(&self) -> Option<&str> {
        self.next_redo_name()
    }

    /// Get a reference to the undo stack for UI display.
    pub fn undo_stack(&self) -> &Vec<Box<dyn Command>> {
        &self.undo_stack
    }

    /// Get a reference to the redo stack for UI display.
    pub fn redo_stack(&self) -> &Vec<Box<dyn Command>> {
        &self.redo_stack
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Check if an entity exists in the world.
fn entity_exists(world: &World, entity: Entity) -> bool {
    world.get::<Transform>(entity).is_some() || world.get::<crate::ecs::Name>(entity).is_some()
}

/// Check if an entity exists and has a Transform component.
fn entity_has_transform(world: &World, entity: Entity) -> bool {
    world.get::<Transform>(entity).is_some()
}

/// Check if an entity has a Name component.
fn entity_has_name(world: &World, entity: Entity) -> bool {
    world.get::<crate::ecs::Name>(entity).is_some()
}

/// Command to set an entity's transform.
pub struct SetTransformCommand {
    entity: Entity,
    old_transform: Transform,
    new_transform: Transform,
}

impl SetTransformCommand {
    pub fn new(world: &World, entity: Entity, new_transform: Transform) -> Option<Self> {
        let old_transform = world.get::<Transform>(entity).copied()?;
        if old_transform == new_transform {
            return None;
        }
        Some(Self {
            entity,
            old_transform,
            new_transform,
        })
    }

    /// Build a command from an already-known before/after pair.
    ///
    /// Used by gestures (such as a gizmo drag) that mutate the transform in
    /// place and only know the starting value afterwards.
    pub fn from_pair(
        entity: Entity,
        old_transform: Transform,
        new_transform: Transform,
    ) -> Option<Self> {
        if old_transform == new_transform {
            return None;
        }
        Some(Self {
            entity,
            old_transform,
            new_transform,
        })
    }
}

impl Command for SetTransformCommand {
    fn name(&self) -> &str {
        "Set Transform"
    }

    fn execute(&mut self, world: &mut World) {
        if let Some(mut t) = world.get_mut::<Transform>(self.entity) {
            *t = self.new_transform;
        }
    }

    fn undo(&mut self, world: &mut World) {
        if let Some(mut t) = world.get_mut::<Transform>(self.entity) {
            *t = self.old_transform;
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Command to spawn a new entity with specified components.
pub struct SpawnEntityCommand {
    entity: Option<Entity>,
    uid: Option<Uid>,
    name: Option<String>,
    transform: Option<Transform>,
    visible: bool,
    locked: bool,
    group: Option<String>,
    parent: Option<Entity>,
    components: crate::scene::SerializedComponents,
    executed: bool,
}

impl SpawnEntityCommand {
    /// Create a new spawn command with all necessary data
    pub fn new(
        name: impl Into<String>,
        transform: Transform,
        parent: Option<Entity>,
        components: crate::scene::SerializedComponents,
        visible: bool,
        locked: bool,
        group: Option<String>,
    ) -> Self {
        Self {
            entity: None,
            uid: Some(Uid::new()),
            name: Some(name.into()),
            transform: Some(transform),
            visible,
            locked,
            group,
            parent,
            components,
            executed: false,
        }
    }

    /// Simple constructor for backwards compatibility
    pub fn simple<F>(_components: F) -> Self
    where
        F: FnOnce(&mut World) -> Entity + Send + Sync + 'static,
    {
        Self {
            entity: None,
            uid: None,
            name: None,
            transform: None,
            visible: true,
            locked: false,
            group: None,
            parent: None,
            components: crate::scene::SerializedComponents::default(),
            executed: false,
        }
    }

    /// Get the spawned entity after execution
    pub fn entity(&self) -> Option<Entity> {
        self.entity
    }
}

impl Command for SpawnEntityCommand {
    fn name(&self) -> &str {
        "Spawn Entity"
    }

    fn description(&self) -> &str {
        "Create a new entity in the scene"
    }

    fn execute(&mut self, world: &mut World) {
        if self.executed {
            return;
        }

        let mut cmd = world.spawn_empty();
        cmd.insert(Transform::default());

        if let Some(uid) = self.uid {
            cmd.insert(uid);
        }

        if let Some(name) = &self.name {
            cmd.insert(Name(name.clone()));
        }
        if let Some(t) = self.transform {
            cmd.insert(t);
        }
        if self.visible {
            cmd.insert(Visible(true));
        } else {
            cmd.insert(Visible(false));
        }
        if self.locked {
            cmd.insert(Locked(true));
        } else {
            cmd.insert(Locked(false));
        }
        if let Some(group) = &self.group {
            cmd.insert(FolderGroup(group.clone()));
        }

        // Restore components
        if let Some(mesh_filter) = &self.components.mesh_filter {
            cmd.insert(mesh_filter.clone());
        }
        if let Some(mesh_renderer) = &self.components.mesh_renderer {
            cmd.insert(mesh_renderer.clone());
        }
        if let Some(camera) = self.components.camera {
            cmd.insert(camera);
        }
        if let Some(light) = self.components.light {
            cmd.insert(light);
        }

        let new_entity = cmd.id();

        // Set parent
        if let Some(parent) = self.parent {
            world.entity_mut(new_entity).insert(Parent(parent));
        }

        // Register in PersistentIdRegistry
        if let Some(uid) = self.uid {
            if let Some(mut registry) = world.get_resource_mut::<PersistentIdRegistry>() {
                let _ = registry.register(uid, new_entity);
            }
        }

        self.entity = Some(new_entity);
        self.executed = true;
    }

    fn undo(&mut self, world: &mut World) {
        if !self.executed {
            return;
        }
        if let Some(entity) = self.entity {
            if let Some(uid) = self.uid {
                if let Some(mut registry) = world.get_resource_mut::<PersistentIdRegistry>() {
                    registry.unregister(uid);
                }
            }
            world.despawn(entity);
            self.entity = None;
        }
        self.executed = false;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ============================================================================
// Spawn Model Hierarchy Command
// ============================================================================

/// Node description for spawning a multi-entity model hierarchy
#[derive(Clone, Debug)]
pub struct ModelNodeDesc {
    pub name: String,
    pub uid: Uid,
    pub transform: Transform,
    pub parent_index: Option<usize>,
    pub components: crate::scene::SerializedComponents,
}

/// Command to spawn an imported model's entity hierarchy as a single undoable action.
pub struct SpawnModelHierarchyCommand {
    name: String,
    nodes: Vec<ModelNodeDesc>,
    spawned_entities: Vec<Entity>,
    executed: bool,
}

impl SpawnModelHierarchyCommand {
    pub fn new(model_name: impl Into<String>, nodes: Vec<ModelNodeDesc>) -> Self {
        Self {
            name: format!("Spawn Model '{}'", model_name.into()),
            nodes,
            spawned_entities: Vec::new(),
            executed: false,
        }
    }

    pub fn spawned_entities(&self) -> &[Entity] {
        &self.spawned_entities
    }
}

impl Command for SpawnModelHierarchyCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Spawn imported model hierarchy into the scene"
    }

    fn execute(&mut self, world: &mut World) {
        if self.executed {
            return;
        }

        self.spawned_entities.clear();
        let mut new_entities = Vec::with_capacity(self.nodes.len());

        for node in &self.nodes {
            let uid = node.uid;
            let mut cmd = world.spawn_empty();
            cmd.insert((
                Name(node.name.clone()),
                uid,
                node.transform,
                Visible(true),
                Locked(false),
            ));

            if let Some(mf) = &node.components.mesh_filter {
                cmd.insert(mf.clone());
            }
            if let Some(mr) = &node.components.mesh_renderer {
                cmd.insert(mr.clone());
            }
            if let Some(cam) = node.components.camera {
                cmd.insert(cam);
            }
            if let Some(light) = node.components.light {
                cmd.insert(light);
            }

            let entity = cmd.id();
            if let Some(mut registry) = world.get_resource_mut::<PersistentIdRegistry>() {
                let _ = registry.register(uid, entity);
            }
            new_entities.push(entity);
        }

        // Hook up parent-child relationships
        for (i, node) in self.nodes.iter().enumerate() {
            if let Some(parent_idx) = node.parent_index {
                if parent_idx < new_entities.len() {
                    let parent_entity = new_entities[parent_idx];
                    world
                        .entity_mut(new_entities[i])
                        .insert(Parent(parent_entity));
                }
            }
        }

        self.spawned_entities = new_entities;
        self.executed = true;
    }

    fn undo(&mut self, world: &mut World) {
        if !self.executed {
            return;
        }

        // Despawn in reverse order (children before parents)
        for (i, &entity) in self.spawned_entities.iter().enumerate().rev() {
            if let Some(mut registry) = world.get_resource_mut::<PersistentIdRegistry>() {
                let uid = self.nodes[i].uid;
                registry.unregister(uid);
            }
            world.despawn(entity);
        }

        self.spawned_entities.clear();
        self.executed = false;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Helper to construct a `Vec<ModelNodeDesc>` from an imported 3D model (glTF, GLB, or OBJ)
pub fn build_model_hierarchy(path: &std::path::Path) -> Result<Vec<ModelNodeDesc>, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Model");
    let mut nodes = Vec::new();

    if ext == "gltf" || ext == "glb" {
        let (document, buffers, _) = gltf::import(path).map_err(|e| e.to_string())?;

        // Root entity for the model
        let root_index = nodes.len();
        nodes.push(ModelNodeDesc {
            name: file_stem.to_string(),
            uid: Uid::new(),
            transform: Transform::default(),
            parent_index: None,
            components: crate::scene::SerializedComponents::default(),
        });

        let scene = document
            .default_scene()
            .or_else(|| document.scenes().next());
        if let Some(scene) = scene {
            fn add_gltf_node(
                node: &gltf::Node,
                parent_idx: usize,
                path_str: &str,
                _buffers: &[gltf::buffer::Data],
                nodes: &mut Vec<ModelNodeDesc>,
            ) {
                let node_name = node.name().unwrap_or("Node").to_string();
                let (translation, rotation, scale) = match node.transform() {
                    gltf::scene::Transform::Decomposed {
                        translation,
                        rotation,
                        scale,
                    } => (
                        Vec3::from_array(translation),
                        glam::Quat::from_array(rotation),
                        Vec3::from_array(scale),
                    ),
                    gltf::scene::Transform::Matrix { matrix } => {
                        let mat = glam::Mat4::from_cols_array_2d(&matrix);
                        let (s, r, t) = mat.to_scale_rotation_translation();
                        (t, r, s)
                    }
                };
                let transform = Transform {
                    translation,
                    rotation,
                    scale,
                };
                let mut components = crate::scene::SerializedComponents::default();
                if let Some(mesh) = node.mesh() {
                    components.mesh_filter = Some(crate::core::components::MeshFilter {
                        mesh_path: format!("{}:mesh_{}", path_str, mesh.index()),
                    });
                    let mat_name = mesh
                        .primitives()
                        .next()
                        .and_then(|p| p.material().name().map(|n| n.to_string()))
                        .unwrap_or_else(|| format!("{}:mat_default", path_str));
                    components.mesh_renderer = Some(crate::core::components::MeshRenderer {
                        material_path: mat_name,
                        cast_shadows: true,
                        receive_shadows: true,
                    });
                }
                let current_idx = nodes.len();
                nodes.push(ModelNodeDesc {
                    name: node_name,
                    uid: Uid::new(),
                    transform,
                    parent_index: Some(parent_idx),
                    components,
                });
                for child in node.children() {
                    add_gltf_node(&child, current_idx, path_str, _buffers, nodes);
                }
            }

            let path_str = path.to_string_lossy();
            for node in scene.nodes() {
                add_gltf_node(&node, root_index, &path_str, &buffers, &mut nodes);
            }
        }
    } else {
        // Fallback for OBJ or generic models
        let path_str = path.to_string_lossy().to_string();
        nodes.push(ModelNodeDesc {
            name: file_stem.to_string(),
            uid: Uid::new(),
            transform: Transform::default(),
            parent_index: None,
            components: crate::scene::SerializedComponents {
                custom: Default::default(),
                mesh_filter: Some(crate::core::components::MeshFilter {
                    mesh_path: path_str.clone(),
                }),
                mesh_renderer: Some(crate::core::components::MeshRenderer {
                    material_path: String::new(),
                    cast_shadows: true,
                    receive_shadows: true,
                }),
                camera: None,
                light: None,
            },
        });
    }

    Ok(nodes)
}

/// Command to despawn an entity.
pub struct DespawnEntityCommand {
    entity: Entity,
    // Store enough to recreate the entity
    uid: Option<Uid>,
    name: Option<String>,
    transform: Option<Transform>,
    parent: Option<Entity>,
    children: Vec<Entity>,
    visible: bool,
    locked: bool,
    group: Option<String>,
    // Serialized components
    components: crate::scene::SerializedComponents,
}

impl DespawnEntityCommand {
    pub fn new(world: &mut World, entity: Entity) -> Option<Self> {
        let name = world.get::<Name>(entity).map(|n| n.0.clone());
        let transform = world.get::<Transform>(entity).copied();
        let uid = world.get::<Uid>(entity).copied();
        let parent = world.get::<Parent>(entity).map(|p| p.0);
        let visible = world.get::<Visible>(entity).map(|v| v.0).unwrap_or(true);
        let locked = world.get::<Locked>(entity).map(|l| l.0).unwrap_or(false);
        let group = world.get::<FolderGroup>(entity).map(|g| g.0.clone());

        // Collect children
        let children: Vec<Entity> = world
            .query::<(Entity, &Parent)>()
            .iter(world)
            .filter_map(|(child, child_parent)| (child_parent.0 == entity).then_some(child))
            .collect();

        // Serialize components using scene serialization
        let mut components = crate::scene::SerializedComponents::default();
        if let Some(mesh_filter) = world.get::<crate::core::components::MeshFilter>(entity) {
            components.mesh_filter = Some(mesh_filter.clone());
        }
        if let Some(mesh_renderer) = world.get::<crate::core::components::MeshRenderer>(entity) {
            components.mesh_renderer = Some(mesh_renderer.clone());
        }
        if let Some(camera) = world.get::<crate::camera::Camera3D>(entity) {
            components.camera = Some(*camera);
        }
        if let Some(light) = world.get::<crate::core::components::Light>(entity) {
            components.light = Some(*light);
        }

        Some(Self {
            entity,
            uid,
            name,
            transform,
            parent,
            children,
            visible,
            locked,
            group,
            components,
        })
    }
}

impl Command for DespawnEntityCommand {
    fn name(&self) -> &str {
        "Despawn Entity"
    }

    fn description(&self) -> &str {
        "Delete an entity and all its children from the scene"
    }

    fn execute(&mut self, world: &mut World) {
        // Despawn children first
        for child in &self.children {
            world.despawn(*child);
        }
        world.despawn(self.entity);
    }

    fn undo(&mut self, world: &mut World) {
        // Recreate the entity with the same UID
        let mut cmd = world.spawn_empty();
        cmd.insert(Transform::default());

        if let Some(uid) = self.uid {
            cmd.insert(uid);
        }

        if let Some(name) = &self.name {
            cmd.insert(Name(name.clone()));
        }
        if let Some(t) = self.transform {
            cmd.insert(t);
        }
        if self.visible {
            cmd.insert(Visible(true));
        } else {
            cmd.insert(Visible(false));
        }
        if self.locked {
            cmd.insert(Locked(true));
        } else {
            cmd.insert(Locked(false));
        }
        if let Some(group) = &self.group {
            cmd.insert(FolderGroup(group.clone()));
        }

        // Restore components
        if let Some(mesh_filter) = &self.components.mesh_filter {
            cmd.insert(mesh_filter.clone());
        }
        if let Some(mesh_renderer) = &self.components.mesh_renderer {
            cmd.insert(mesh_renderer.clone());
        }
        if let Some(camera) = self.components.camera {
            cmd.insert(camera);
        }
        if let Some(light) = self.components.light {
            cmd.insert(light);
        }

        let new_entity = cmd.id();

        // Register in PersistentIdRegistry
        if let Some(uid) = self.uid {
            if let Some(mut registry) = world.get_resource_mut::<PersistentIdRegistry>() {
                let _ = registry.register(uid, new_entity);
            }
        }

        // Restore parent relationship
        if let Some(parent) = self.parent {
            world.entity_mut(new_entity).insert(Parent(parent));
        }

        // Recreate children - for now we just mark that they need recreation
        // A full implementation would recursively restore children
        // For now, the children are lost on delete but the parent is restored
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Command to rename an entity.
pub struct RenameEntityCommand {
    entity: Entity,
    old_name: String,
    new_name: String,
}

impl RenameEntityCommand {
    pub fn new(world: &World, entity: Entity, new_name: String) -> Option<Self> {
        let old_name = world.get::<Name>(entity)?.0.clone();
        if old_name == new_name {
            return None;
        }
        Some(Self {
            entity,
            old_name,
            new_name,
        })
    }
}

impl Command for RenameEntityCommand {
    fn name(&self) -> &str {
        "Rename Entity"
    }

    fn description(&self) -> &str {
        "Rename an entity in the scene"
    }

    fn execute(&mut self, world: &mut World) {
        if let Some(mut name) = world.get_mut::<Name>(self.entity) {
            name.0 = self.new_name.clone();
        }
    }

    fn undo(&mut self, world: &mut World) {
        if let Some(mut name) = world.get_mut::<Name>(self.entity) {
            name.0 = self.old_name.clone();
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ============================================================================
// Add / Remove Component Commands
// ============================================================================

/// Command to add a component to an entity.
///
/// Stores the component so it can be removed on undo.
pub struct AddComponentCommand {
    entity: Entity,
    component: AddableComponent,
    added: bool,
}

/// A component that can be added/removed via the command system.
///
/// Only supports the component types the editor can currently serialize.
#[derive(Clone)]
pub enum AddableComponent {
    FolderGroup(String),
    Visible(bool),
    Locked(bool),
    Parent(Entity),
    MeshFilter(crate::core::components::MeshFilter),
    MeshRenderer(crate::core::components::MeshRenderer),
    Light(crate::core::components::Light),
    Camera(crate::camera::Camera3D),
    Collider(crate::core::components::Collider),
    RigidBody(crate::core::components::RigidBody),
    AudioSource(crate::core::components::AudioSource),
    Tag(crate::core::components::Tag),
}

impl AddComponentCommand {
    pub fn new(entity: Entity, component: AddableComponent) -> Self {
        Self {
            entity,
            component,
            added: false,
        }
    }
}

impl Command for AddComponentCommand {
    fn name(&self) -> &str {
        "Add Component"
    }

    fn description(&self) -> &str {
        "Add a component to an entity"
    }

    fn execute(&mut self, world: &mut World) {
        if self.added {
            return;
        }
        insert_component(world, self.entity, self.component.clone());
        self.added = true;
    }

    fn undo(&mut self, world: &mut World) {
        if !self.added {
            return;
        }
        remove_component(world, self.entity, &self.component);
        self.added = false;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Command to remove a component from an entity.
///
/// Stores the removed component so it can be restored on undo.
pub struct RemoveComponentCommand {
    entity: Entity,
    component: Option<AddableComponent>,
    removed: bool,
}

impl RemoveComponentCommand {
    pub fn new(world: &World, entity: Entity, component: AddableComponent) -> Option<Self> {
        if !has_component(world, entity, &component) {
            return None;
        }
        Some(Self {
            entity,
            component: Some(component),
            removed: false,
        })
    }
}

impl Command for RemoveComponentCommand {
    fn name(&self) -> &str {
        "Remove Component"
    }

    fn description(&self) -> &str {
        "Remove a component from an entity"
    }

    fn execute(&mut self, world: &mut World) {
        if self.removed {
            return;
        }
        if let Some(component) = self.component.take() {
            remove_component(world, self.entity, &component);
            self.removed = true;
        }
    }

    fn undo(&mut self, world: &mut World) {
        if !self.removed {
            return;
        }
        if let Some(component) = self.component.take() {
            insert_component(world, self.entity, component);
            self.removed = false;
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn has_component(world: &World, entity: Entity, component: &AddableComponent) -> bool {
    match component {
        AddableComponent::FolderGroup(_) => world.get::<FolderGroup>(entity).is_some(),
        AddableComponent::Visible(_) => world.get::<Visible>(entity).is_some(),
        AddableComponent::Locked(_) => world.get::<Locked>(entity).is_some(),
        AddableComponent::Parent(_) => world.get::<Parent>(entity).is_some(),
        AddableComponent::MeshFilter(_) => world
            .get::<crate::core::components::MeshFilter>(entity)
            .is_some(),
        AddableComponent::MeshRenderer(_) => world
            .get::<crate::core::components::MeshRenderer>(entity)
            .is_some(),
        AddableComponent::Light(_) => world
            .get::<crate::core::components::Light>(entity)
            .is_some(),
        AddableComponent::Camera(_) => world.get::<crate::camera::Camera3D>(entity).is_some(),
        AddableComponent::Collider(_) => world
            .get::<crate::core::components::Collider>(entity)
            .is_some(),
        AddableComponent::RigidBody(_) => world
            .get::<crate::core::components::RigidBody>(entity)
            .is_some(),
        AddableComponent::AudioSource(_) => world
            .get::<crate::core::components::AudioSource>(entity)
            .is_some(),
        AddableComponent::Tag(_) => world.get::<crate::core::components::Tag>(entity).is_some(),
    }
}

fn insert_component(world: &mut World, entity: Entity, component: AddableComponent) {
    match component {
        AddableComponent::FolderGroup(name) => {
            world.entity_mut(entity).insert(FolderGroup(name));
        }
        AddableComponent::Visible(v) => {
            world.entity_mut(entity).insert(Visible(v));
        }
        AddableComponent::Locked(v) => {
            world.entity_mut(entity).insert(Locked(v));
        }
        AddableComponent::Parent(parent) => {
            world.entity_mut(entity).insert(Parent(parent));
        }
        AddableComponent::MeshFilter(mf) => {
            world.entity_mut(entity).insert(mf);
        }
        AddableComponent::MeshRenderer(mr) => {
            world.entity_mut(entity).insert(mr);
        }
        AddableComponent::Light(l) => {
            world.entity_mut(entity).insert(l);
        }
        AddableComponent::Camera(c) => {
            world.entity_mut(entity).insert(c);
        }
        AddableComponent::Collider(c) => {
            world.entity_mut(entity).insert(c);
        }
        AddableComponent::RigidBody(rb) => {
            world.entity_mut(entity).insert(rb);
        }
        AddableComponent::AudioSource(a) => {
            world.entity_mut(entity).insert(a);
        }
        AddableComponent::Tag(t) => {
            world.entity_mut(entity).insert(t);
        }
    }
}

fn remove_component(world: &mut World, entity: Entity, component: &AddableComponent) {
    match component {
        AddableComponent::FolderGroup(_) => {
            world.entity_mut(entity).remove::<FolderGroup>();
        }
        AddableComponent::Visible(_) => {
            world.entity_mut(entity).remove::<Visible>();
        }
        AddableComponent::Locked(_) => {
            world.entity_mut(entity).remove::<Locked>();
        }
        AddableComponent::Parent(_) => {
            world.entity_mut(entity).remove::<Parent>();
        }
        AddableComponent::MeshFilter(_) => {
            world
                .entity_mut(entity)
                .remove::<crate::core::components::MeshFilter>();
        }
        AddableComponent::MeshRenderer(_) => {
            world
                .entity_mut(entity)
                .remove::<crate::core::components::MeshRenderer>();
        }
        AddableComponent::Light(_) => {
            world
                .entity_mut(entity)
                .remove::<crate::core::components::Light>();
        }
        AddableComponent::Camera(_) => {
            world.entity_mut(entity).remove::<crate::camera::Camera3D>();
        }
        AddableComponent::Collider(_) => {
            world
                .entity_mut(entity)
                .remove::<crate::core::components::Collider>();
        }
        AddableComponent::RigidBody(_) => {
            world
                .entity_mut(entity)
                .remove::<crate::core::components::RigidBody>();
        }
        AddableComponent::AudioSource(_) => {
            world
                .entity_mut(entity)
                .remove::<crate::core::components::AudioSource>();
        }
        AddableComponent::Tag(_) => {
            world
                .entity_mut(entity)
                .remove::<crate::core::components::Tag>();
        }
    }
}

// ============================================================================
// Reparent Command
// ============================================================================

/// Command to change an entity's parent.
///
/// Stores the old and new parent so the operation can be reversed.
pub struct ReparentEntityCommand {
    entity: Entity,
    old_parent: Option<Entity>,
    new_parent: Option<Entity>,
    old_index: Option<usize>,
    new_index: Option<usize>,
    applied: bool,
}

impl ReparentEntityCommand {
    pub fn new(world: &mut World, entity: Entity, new_parent: Option<Entity>) -> Option<Self> {
        if entity == new_parent.unwrap_or(entity) {
            return None;
        }
        // Reject cycles: new parent must not be a descendant of entity.
        if let Some(parent) = new_parent {
            if is_descendant(world, parent, entity) {
                return None;
            }
        }
        let old_parent = world.get::<Parent>(entity).map(|p| p.0);

        // Calculate old and new indices among siblings
        let old_index = old_parent.and_then(|p| get_child_index(world, p, entity));
        let new_index = new_parent.and_then(|p| get_child_index(world, p, entity));

        Some(Self {
            entity,
            old_parent,
            new_parent,
            old_index,
            new_index,
            applied: false,
        })
    }
}

fn is_descendant(world: &World, entity: Entity, ancestor: Entity) -> bool {
    let mut current = entity;
    loop {
        if current == ancestor {
            return true;
        }
        match world.get::<Parent>(current) {
            Some(parent) => current = parent.0,
            None => return false,
        }
    }
}

fn get_child_index(world: &mut World, parent: Entity, child: Entity) -> Option<usize> {
    let children: Vec<Entity> = world
        .query::<(Entity, &Parent)>()
        .iter(world)
        .filter_map(|(c, p)| (p.0 == parent).then_some(c))
        .collect();
    children.iter().position(|&c| c == child)
}

impl Command for ReparentEntityCommand {
    fn name(&self) -> &str {
        "Reparent Entity"
    }

    fn description(&self) -> &str {
        "Change an entity's parent in the hierarchy"
    }

    fn execute(&mut self, world: &mut World) {
        if self.applied {
            return;
        }
        set_parent(world, self.entity, self.new_parent);
        self.applied = true;
    }

    fn undo(&mut self, world: &mut World) {
        if !self.applied {
            return;
        }
        set_parent(world, self.entity, self.old_parent);
        self.applied = false;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn set_parent(world: &mut World, entity: Entity, parent: Option<Entity>) {
    let mut e = world.entity_mut(entity);
    match parent {
        Some(p) => {
            e.insert(Parent(p));
        }
        None => {
            e.remove::<Parent>();
        }
    }
}

// ============================================================================
// Composite Command (Transaction)
// ============================================================================

/// Command that groups multiple commands into a single undoable operation.
///
/// All sub-commands are executed in order. Undo reverses them in reverse order.
pub struct CompositeCommand {
    name: String,
    commands: Vec<Box<dyn Command>>,
    executed: bool,
}

impl CompositeCommand {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            commands: Vec::new(),
            executed: false,
        }
    }

    pub fn add(&mut self, command: Box<dyn Command>) {
        self.commands.push(command);
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

impl Command for CompositeCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "A group of operations treated as a single undoable action"
    }

    fn execute(&mut self, world: &mut World) {
        if self.executed {
            return;
        }
        for cmd in &mut self.commands {
            cmd.execute(world);
        }
        self.executed = true;
    }

    fn undo(&mut self, world: &mut World) {
        if !self.executed {
            return;
        }
        for cmd in self.commands.iter_mut().rev() {
            cmd.undo(world);
        }
        self.executed = false;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;

    #[test]
    fn spawn_model_hierarchy_execute_undo_redo() {
        let mut world = World::new();
        world.insert_resource(PersistentIdRegistry::default());

        let uid0 = Uid::new();
        let uid1 = Uid::new();
        let uid2 = Uid::new();

        let nodes = vec![
            ModelNodeDesc {
                name: "Root".to_string(),
                uid: uid0,
                transform: Transform::default(),
                parent_index: None,
                components: crate::scene::SerializedComponents::default(),
            },
            ModelNodeDesc {
                name: "Child".to_string(),
                uid: uid1,
                transform: Transform::from_translation(glam::vec3(0.0, 1.0, 0.0)),
                parent_index: Some(0),
                components: crate::scene::SerializedComponents::default(),
            },
            ModelNodeDesc {
                name: "Grandchild".to_string(),
                uid: uid2,
                transform: Transform::from_translation(glam::vec3(0.0, 2.0, 0.0)),
                parent_index: Some(1),
                components: crate::scene::SerializedComponents::default(),
            },
        ];

        let mut cmd = SpawnModelHierarchyCommand::new("TestHierarchy", nodes);

        // 1. Execute
        cmd.execute(&mut world);
        assert_eq!(cmd.spawned_entities().len(), 3);
        let e0 = cmd.spawned_entities()[0];
        let e1 = cmd.spawned_entities()[1];
        let e2 = cmd.spawned_entities()[2];

        // Verify entities and names
        assert_eq!(world.get::<Name>(e0).map(|n| n.0.as_str()), Some("Root"));
        assert_eq!(world.get::<Name>(e1).map(|n| n.0.as_str()), Some("Child"));
        assert_eq!(
            world.get::<Name>(e2).map(|n| n.0.as_str()),
            Some("Grandchild")
        );

        // Verify parent hierarchy
        assert_eq!(world.get::<Parent>(e0), None);
        assert_eq!(world.get::<Parent>(e1), Some(&Parent(e0)));
        assert_eq!(world.get::<Parent>(e2), Some(&Parent(e1)));

        // Verify registry UIDs
        {
            let reg = world.resource::<PersistentIdRegistry>();
            assert_eq!(reg.resolve(uid0), Some(e0));
            assert_eq!(reg.resolve(uid1), Some(e1));
            assert_eq!(reg.resolve(uid2), Some(e2));
        }

        // 2. Undo
        cmd.undo(&mut world);
        assert!(world.get_entity(e0).is_err());
        assert!(world.get_entity(e1).is_err());
        assert!(world.get_entity(e2).is_err());

        // Registry should no longer have the UIDs
        {
            let reg = world.resource::<PersistentIdRegistry>();
            assert_eq!(reg.resolve(uid0), None);
            assert_eq!(reg.resolve(uid1), None);
            assert_eq!(reg.resolve(uid2), None);
        }

        // 3. Redo
        cmd.redo(&mut world);
        assert_eq!(cmd.spawned_entities().len(), 3);
        let new_e0 = cmd.spawned_entities()[0];
        let new_e1 = cmd.spawned_entities()[1];
        let new_e2 = cmd.spawned_entities()[2];

        assert_eq!(
            world.get::<Name>(new_e0).map(|n| n.0.as_str()),
            Some("Root")
        );
        assert_eq!(
            world.get::<Name>(new_e1).map(|n| n.0.as_str()),
            Some("Child")
        );
        assert_eq!(
            world.get::<Name>(new_e2).map(|n| n.0.as_str()),
            Some("Grandchild")
        );

        assert_eq!(world.get::<Parent>(new_e0), None);
        assert_eq!(world.get::<Parent>(new_e1), Some(&Parent(new_e0)));
        assert_eq!(world.get::<Parent>(new_e2), Some(&Parent(new_e1)));

        {
            let reg = world.resource::<PersistentIdRegistry>();
            assert_eq!(reg.resolve(uid0), Some(new_e0));
            assert_eq!(reg.resolve(uid1), Some(new_e1));
            assert_eq!(reg.resolve(uid2), Some(new_e2));
        }
    }
}
