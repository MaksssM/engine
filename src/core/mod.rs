//! Core Engine foundation.
//!
//! This module provides the runtime-facing building blocks shared by the
//! editor and future standalone game runtime: stable object ids, ECS
//! components/resources, a staged system scheduler, and a frame lifecycle.

pub mod components;
pub mod game_object;
pub mod lifecycle;
pub mod resources;
pub mod systems;
pub mod uid;

#[allow(unused_imports)]
pub mod prelude {
    pub use super::components::*;
    pub use super::game_object::{GameObject, ObjectRole};
    pub use super::lifecycle::GameLoop;
    pub use super::resources::{EngineConfig, EngineInput, EngineMode, EngineTime};
    pub use super::systems::{FnSystem, System, SystemScheduler, SystemStage};
    pub use super::uid::Uid;
}
