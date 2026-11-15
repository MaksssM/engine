#![allow(dead_code)]

mod app;
mod assets;
mod camera;
mod dock;
mod ecs;
mod editor;
mod events;
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
