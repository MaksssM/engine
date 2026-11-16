#![allow(dead_code)]

mod app;
mod assets;
mod assets_ui;
mod camera;
mod content_browser;
mod core;
mod dock;
mod ecs;
mod editor;
mod events;
mod feedback;
mod icons;
mod input;
mod quality;
mod renderer;
mod scene;
mod time;
mod transform;
mod ui;
mod ui_new;

fn main() -> Result<(), winit::error::EventLoopError> {
    app::run()
}
