#![allow(dead_code)]

mod app;
mod asset_database;
mod asset_importer;
mod asset_storage;
mod assets;
mod assets_ui;
mod benchmark;
mod camera;
mod commands;
mod content_browser;
mod core;
mod dock;
mod ecs;
mod editor;
mod events;
mod feedback;
mod frustum;
mod gltf_importer;
mod icons;
mod input;
mod performance;
mod performance_graph;
mod quality;
mod renderer;
mod scene;
mod time;
mod transform;
mod ui;
mod ui_new;
mod viewport;

// Re-export AssetData from assets module for other modules
pub use crate::assets::AssetData;

fn main() -> Result<(), winit::error::EventLoopError> {
    app::run()
}
