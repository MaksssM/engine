#![allow(dead_code)]

mod app;
mod assets;
mod ecs;
mod events;
mod input;
mod quality;
mod renderer;
mod scene;
mod time;
mod transform;

fn main() -> Result<(), winit::error::EventLoopError> {
    app::run()
}
