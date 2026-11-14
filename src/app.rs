use std::{sync::Arc, time::Duration};

use glam::Vec3;
use tracing::{info, warn};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    window::{Window, WindowId},
};

use crate::{
    camera::{Camera3D, CameraController},
    ecs::Ecs,
    events::Events,
    input::InputState,
    quality::QualityPreset,
    renderer::{Renderable, Renderer},
    time::{Clock, FixedTimestep, FpsMeter},
    transform::{GlobalTransform, Transform, propagate_transforms},
    ui::MouseInput,
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
    scene_manager: crate::scene::SceneManager,
    debug_overlay: bool,
    wireframe_mode: bool,
    mouse: MouseInput,
    show_debug_panel: bool,
    show_quality_panel: bool,
    show_camera_panel: bool,
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
                        let red_material = renderer.red_material();
                        let green_material = renderer.green_material();
                        let blue_material = renderer.blue_material();
                        let mut startup_scene = crate::scene::Scene::new();
                        startup_scene.add_entity("ground", glam::Vec3::new(0.0, -1.0, 0.0), None);
                        startup_scene.add_entity(
                            "central-cube",
                            glam::Vec3::new(0.0, 0.5, 0.0),
                            None,
                        );
                        startup_scene.add_entity(
                            "left-cube",
                            glam::Vec3::new(-2.5, 0.5, -1.0),
                            None,
                        );
                        startup_scene.add_entity(
                            "right-cube",
                            glam::Vec3::new(2.5, 0.5, 1.0),
                            None,
                        );
                        startup_scene.add_entity(
                            "back-cube",
                            glam::Vec3::new(0.0, 0.5, -2.5),
                            None,
                        );
                        self.scene_manager.insert("startup", startup_scene);
                        self.scene_manager.activate("startup");
                        self.renderer = Some(renderer);
                        let entities = self
                            .scene_manager
                            .spawn_active(&mut self.ecs.world)
                            .unwrap_or_default()
                            .to_vec();
                        for (index, entity) in entities.iter().enumerate() {
                            let mut transform =
                                self.ecs.world.get::<Transform>(*entity).copied().unwrap();
                            if index == 0 {
                                transform.scale = glam::Vec3::new(4.0, 0.2, 4.0);
                            } else if index == 1 {
                                transform.scale = glam::Vec3::ONE;
                            } else {
                                transform.scale = glam::Vec3::splat(0.6);
                            }
                            let material = match index {
                                0 => cube_material,
                                1 => blue_material,
                                2 => red_material,
                                3 => green_material,
                                _ => cube_material,
                            };
                            self.ecs.world.entity_mut(*entity).insert((
                                crate::ecs::Name(format!("scene-{index}")),
                                transform,
                                Renderable {
                                    mesh: cube_mesh,
                                    material,
                                },
                            ));
                        }
                        let camera_position = glam::Vec3::new(0.0, 3.0, 6.0);
                        let camera = self.ecs.spawn((
                            Camera3D::default(),
                            CameraController::default(),
                            Transform::from_translation(camera_position),
                        ));
                        info!(?camera, "ECS camera spawned");
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
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.mouse.left_down = matches!(state, ElementState::Pressed);
                if matches!(state, ElementState::Released) {
                    self.mouse.left_clicked = true;
                } else {
                    self.mouse.left_clicked = false;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse.x = position.x as f32;
                self.mouse.y = position.y as f32;
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
    fn update_camera_controller(&mut self, delta_seconds: f32) {
        let mut query = self
            .ecs
            .world
            .query::<(&mut Transform, &mut CameraController)>();
        let Ok((mut transform, mut controller)) = query.single_mut(&mut self.ecs.world) else {
            return;
        };
        let mut movement = Vec3::ZERO;
        if self.input.pressed(winit::keyboard::KeyCode::KeyW) {
            movement += controller.forward();
        }
        if self.input.pressed(winit::keyboard::KeyCode::KeyS) {
            movement -= controller.forward();
        }
        if self.input.pressed(winit::keyboard::KeyCode::KeyD) {
            movement += controller.right();
        }
        if self.input.pressed(winit::keyboard::KeyCode::KeyA) {
            movement -= controller.right();
        }
        if self.input.pressed(winit::keyboard::KeyCode::KeyQ) {
            movement.y -= 1.0;
        }
        if self.input.pressed(winit::keyboard::KeyCode::KeyE) {
            movement.y += 1.0;
        }
        if movement.length_squared() > 0.0 {
            transform.translation +=
                movement.normalize() * controller.movement_speed * delta_seconds;
        }
        if self.input.pressed(winit::keyboard::KeyCode::ArrowLeft) {
            controller.yaw -= 1.5 * delta_seconds;
        }
        if self.input.pressed(winit::keyboard::KeyCode::ArrowRight) {
            controller.yaw += 1.5 * delta_seconds;
        }
        if self.input.pressed(winit::keyboard::KeyCode::ArrowUp) {
            controller.pitch = (controller.pitch + delta_seconds).clamp(-1.4, 1.4);
        }
        if self.input.pressed(winit::keyboard::KeyCode::ArrowDown) {
            controller.pitch = (controller.pitch - delta_seconds).clamp(-1.4, 1.4);
        }
        let rotation = glam::Quat::from_rotation_y(controller.yaw)
            * glam::Quat::from_rotation_x(controller.pitch);
        transform.rotation = rotation;
    }

    fn reload_active_scene(&mut self) {
        self.scene_manager.unload_active(&mut self.ecs.world);
        let Some(cube_mesh) = self.renderer.as_ref().map(Renderer::cube_mesh) else {
            return;
        };
        let Some(cube_material) = self.renderer.as_ref().map(Renderer::cube_material) else {
            return;
        };
        let entities = self
            .scene_manager
            .spawn_active(&mut self.ecs.world)
            .unwrap_or_default()
            .to_vec();
        for (index, entity) in entities.iter().enumerate() {
            let mut transform = self.ecs.world.get::<Transform>(*entity).copied().unwrap();
            transform.scale = if index == 0 {
                glam::Vec3::new(4.0, 0.2, 4.0)
            } else if index == 1 {
                glam::Vec3::ONE
            } else {
                glam::Vec3::splat(0.6)
            };
            self.ecs.world.entity_mut(*entity).insert((
                crate::ecs::Name(format!("scene-{index}")),
                transform,
                Renderable {
                    mesh: cube_mesh,
                    material: cube_material,
                },
            ));
        }
        info!(count = entities.len(), "active scene reloaded");
    }

    fn process_events(&mut self) {
        let mut reload_requested = false;
        for event in self.events.drain() {
            if let WindowEvent::KeyboardInput { event, .. } = event
                && let PhysicalKey::Code(key) = event.physical_key
            {
                match event.state {
                    ElementState::Pressed => {
                        if self.input.pressed(key) {
                            continue;
                        }
                        self.input.press(key);
                        match key {
                            winit::keyboard::KeyCode::F1 => {
                                self.debug_overlay = !self.debug_overlay;
                                info!(enabled = self.debug_overlay, "debug overlay toggled");
                            }
                            winit::keyboard::KeyCode::F2 => {
                                self.wireframe_mode = !self.wireframe_mode;
                                if let Some(renderer) = self.renderer.as_mut() {
                                    renderer.set_wireframe(self.wireframe_mode);
                                }
                                info!(enabled = self.wireframe_mode, "wireframe toggled");
                            }
                            winit::keyboard::KeyCode::F5 => reload_requested = true,
                            _ => {}
                        }
                    }
                    ElementState::Released => self.input.release(key),
                }
            }
        }
        if reload_requested {
            self.reload_active_scene();
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
            self.update_camera_controller(step_seconds);
            for _ in 0..updates {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.update(&mut self.ecs.world, step_seconds);
                }
            }
        }

        propagate_transforms(&mut self.ecs.world);
        if let Some(renderer) = self.renderer.as_mut() {
            let camera_data = self
                .ecs
                .world
                .query::<(&Camera3D, &GlobalTransform)>()
                .iter(&self.ecs.world)
                .next()
                .map(|(camera, transform)| (*camera, *transform));
            if let Some((camera, transform)) = camera_data {
                renderer.set_camera(&camera, &transform);
            }
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
            let entity_count = self
                .ecs
                .world
                .query::<bevy_ecs::entity::Entity>()
                .iter(&self.ecs.world)
                .count();
            let renderable_count = self
                .ecs
                .world
                .query::<&Renderable>()
                .iter(&self.ecs.world)
                .count();
            let fixed_step_ms = self
                .timestep
                .as_ref()
                .expect("timestep initialized with clock")
                .step()
                .as_secs_f64()
                * 1000.0;
            let fps = self.fps.fps();
            let preset = QualityPreset::from_environment();
            let preset_settings = preset.settings();
            let preset_name = match preset_settings.render_scale_percent {
                75 => "Performance",
                100 if !preset_settings.post_processing => "Balanced",
                _ => "Quality",
            }
            .to_string();
            let frame_time_ms = frame_time.as_secs_f64() * 1000.0;
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.update_ui(
                    fps as f32,
                    entity_count,
                    renderable_count,
                    frame_time_ms,
                    &preset_name,
                );
            }
            info!(
                fps,
                entities = entity_count,
                renderables = renderable_count,
                frame_ms = frame_time_ms,
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
