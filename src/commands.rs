//! # Undo/Redo Command System
//!
//! Provides a Command Pattern implementation for undoable operations in the editor.
//! Each command stores enough information to execute, undo, and redo an action.

use bevy_ecs::world::World;
use bevy_ecs::entity::Entity;

use crate::transform::Transform;

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