use std::{sync::Arc, time::Duration};

use tracing::{info, warn};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    window::{Window, WindowId},
};

use crate::{
    ecs::Ecs,
    events::Events,
    input::InputState,
    quality::QualityPreset,
    renderer::{Renderable, Renderer},
    time::{Clock, FixedTimestep, FpsMeter},
    transform::{Transform, propagate_transforms},
};

pub fn run() -> Result<(), winit::error::EventLoopError> {
    init_logging();
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = EngineApp::default();
    event_loop.run_app(&mut app)
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
}

#[derive(Default)]
struct EngineApp {
    window: Option<Arc<Window>>,
    window_id: Option<WindowId>,
    renderer: Option<Renderer>,
    clock: Option<Clock>,
    timestep: Option<FixedTimestep>,
    fps: FpsMeter,
    ecs: Ecs,
    input: InputState,
    events: Events<WindowEvent>,
}

impl ApplicationHandler for EngineApp {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::Init) {
            info!("starting engine");
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attributes = Window::default_attributes().with_title("Rust Engine");
        match event_loop.create_window(attributes) {
            Ok(window) => {
                self.window_id = Some(window.id());
                let window = Arc::new(window);
                let preset = QualityPreset::from_environment();
                let settings = preset.settings();
                info!(
                    ?preset,
                    render_scale = settings.render_scale_percent,
                    texture_lod_bias = settings.texture_lod_bias,
                    post_processing = settings.post_processing,
                    ?settings.shadow_quality,
                    "using render quality preset"
                );
                match pollster::block_on(Renderer::new(Arc::clone(&window), preset)) {
                    Ok(renderer) => {
                        let cube_mesh = renderer.cube_mesh();
                        let cube_material = renderer.cube_material();
                        self.renderer = Some(renderer);
                        self.ecs.spawn((
                            Transform::from_translation(glam::Vec3::new(-1.25, 0.0, 0.0)),
                            Renderable {
                                mesh: cube_mesh,
                                material: cube_material,
                            },
                        ));
                        self.ecs.spawn((
                            Transform::from_translation(glam::Vec3::new(1.25, 0.0, 0.0)),
                            Renderable {
                                mesh: cube_mesh,
                                material: cube_material,
                            },
                        ));
                        self.window = Some(window);
                        self.clock = Some(Clock::new());
                        self.timestep =
                            Some(FixedTimestep::new(Duration::from_secs_f64(1.0 / 60.0)));
                        info!("window and renderer created");
                    }
                    Err(error) => warn!(?error, "failed to create renderer"),
                }
            }
            Err(error) => warn!(?error, "failed to create window"),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window_id != Some(window_id) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                info!("shutting down engine");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size);
                }
            }
            WindowEvent::RedrawRequested => self.update(),
            other => self.events.send(other),
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl EngineApp {
    fn process_events(&mut self) {
        for event in self.events.drain() {
            if let WindowEvent::KeyboardInput { event, .. } = event
                && let PhysicalKey::Code(key) = event.physical_key
            {
                match event.state {
                    ElementState::Pressed => self.input.press(key),
                    ElementState::Released => self.input.release(key),
                }
            }
        }
    }

    fn update(&mut self) {
        let frame_time = {
            let Some(clock) = self.clock.as_mut() else {
                return;
            };
            clock.tick()
        };
        self.input.begin_frame();
        self.process_events();

        let updates = self
            .timestep
            .as_mut()
            .expect("timestep initialized with clock")
            .add_frame(frame_time);
        if updates > 0 {
            let step_seconds = self
                .timestep
                .as_ref()
                .expect("timestep initialized")
                .step()
                .as_secs_f32();
            for _ in 0..updates {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.update(&mut self.ecs.world, step_seconds);
                }
            }
        }

        propagate_transforms(&mut self.ecs.world);
        if let Some(renderer) = self.renderer.as_mut() {
            match renderer.render(&mut self.ecs.world) {
                Ok(()) => {}
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    if let Some(window) = self.window.as_ref() {
                        renderer.resize(window.inner_size());
                    }
                }
                Err(wgpu::SurfaceError::OutOfMemory) => warn!("GPU is out of memory"),
                Err(wgpu::SurfaceError::Timeout) => warn!("GPU frame timed out"),
                Err(wgpu::SurfaceError::Other) => warn!("GPU surface error"),
            }
        }

        if self.fps.record_frame(frame_time).is_some() {
            let fixed_step_ms = self
                .timestep
                .as_ref()
                .expect("timestep initialized with clock")
                .step()
                .as_secs_f64()
                * 1000.0;
            info!(
                fps = self.fps.fps(),
                frame_ms = frame_time.as_secs_f64() * 1000.0,
                fixed_step_ms,
                elapsed_seconds = self
                    .clock
                    .as_ref()
                    .expect("clock initialized")
                    .elapsed()
                    .as_secs_f64(),
                "performance"
            );
        }
    }
}
