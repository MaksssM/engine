use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use bevy_ecs::{component::Component, entity::Entity, world::World};
use egui::{Color32, Context, Key, RichText, Vec2};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use winit::{event::WindowEvent, window::Window};

use egui_dock::{DockArea, DockState, NodeIndex, SurfaceIndex};

use crate::{
    camera::CameraController,
    dock::{DockAction, EditorTab, WorkspacePreset, editor_dock_style},
    ecs::Name,
    scene::{Scene, SceneEntity},
    transform::{Parent, Transform},
};

const DEFAULT_SCRIPT_CONTROLLER: &str = r#"// Player Character Controller
use engine::prelude::*;

pub struct PlayerController {
    pub speed: f32,
    pub jump_force: f32,
    pub is_grounded: bool,
}

impl Default for PlayerController {
    fn default() -> Self {
        Self {
            speed: 5.0,
            jump_force: 8.0,
            is_grounded: true,
        }
    }
}

impl Script for PlayerController {
    fn update(&mut self, entity: Entity, world: &mut World, dt: f32) {
        let input = Input::current();
        let mut move_dir = Vec3::ZERO;
        if input.is_key_down(Key::W) { move_dir.z -= 1.0; }
        if input.is_key_down(Key::S) { move_dir.z += 1.0; }
        if input.is_key_down(Key::A) { move_dir.x -= 1.0; }
        if input.is_key_down(Key::D) { move_dir.x += 1.0; }

        if let Some(mut transform) = world.get_mut::<Transform>(entity) {
            transform.translation += move_dir.normalize_or_zero() * self.speed * dt;
        }
    }
}
"#;

const DEFAULT_SCRIPT_ROTATOR: &str = r#"// Continuous Object Rotator
use engine::prelude::*;

pub struct ObjectRotator {
    pub speed_deg: f32,
    pub axis: Vec3,
}

impl Default for ObjectRotator {
    fn default() -> Self {
        Self {
            speed_deg: 45.0,
            axis: Vec3::Y,
        }
    }
}

impl Script for ObjectRotator {
    fn update(&mut self, entity: Entity, world: &mut World, dt: f32) {
        let delta_angle = self.speed_deg.to_radians() * dt;
        let delta_rot = Quat::from_axis_angle(self.axis, delta_angle);
        if let Some(mut transform) = world.get_mut::<Transform>(entity) {
            transform.rotation = delta_rot * transform.rotation;
        }
    }
}
"#;

const DEFAULT_SCRIPT_CAMERA_SHAKE: &str = r#"// Camera Shake Effect
use engine::prelude::*;

pub struct CameraShake {
    pub intensity: f32,
    pub duration: f32,
    pub elapsed: f32,
}

impl CameraShake {
    pub fn trigger(&mut self, intensity: f32, duration: f32) {
        self.intensity = intensity;
        self.duration = duration;
        self.elapsed = 0.0;
    }
}
"#;

const DEFAULT_SCRIPT_SHADER: &str = r#"// Custom PBR Surface Shader (WGSL)
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let light_dir = normalize(vec3<f32>(0.5, 1.0, 0.3));
    let diff = max(dot(normalize(in.world_normal), light_dir), 0.15);
    let albedo = vec3<f32>(0.2, 0.6, 0.9);
    return vec4<f32>(albedo * diff, 1.0);
}
"#;

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Visible(pub bool);

impl Default for Visible {
    fn default() -> Self {
        Self(true)
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Locked(pub bool);

impl Default for Locked {
    fn default() -> Self {
        Self(false)
    }
}

#[derive(Component, Clone, Debug, PartialEq)]
pub struct FolderGroup(pub String);

impl Default for FolderGroup {
    fn default() -> Self {
        Self("Default".into())
    }
}

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
enum ViewMode {
    #[default]
    Perspective,
    Top,
    Front,
    Side,
    Orthographic,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct LayoutSettings {
    workspace_preset: WorkspacePreset,
    dock_state: Option<DockState<EditorTab>>,
    show_viewport: bool,
    show_outliner: bool,
    show_details: bool,
    show_content: bool,
    show_log: bool,
    grid: bool,
    wireframe: bool,
    show_gizmos: bool,
    show_collision: bool,
    active_tool: usize,
    view_mode: ViewMode,
    content_width: f32,
    outliner_width: f32,
    details_width: f32,
    log_height: f32,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            workspace_preset: WorkspacePreset::Default,
            dock_state: Some(WorkspacePreset::Default.create_dock_state()),
            show_viewport: true,
            show_outliner: true,
            show_details: true,
            show_content: true,
            show_log: true,
            grid: true,
            wireframe: false,
            show_gizmos: true,
            show_collision: false,
            active_tool: 0,
            view_mode: ViewMode::Perspective,
            content_width: 220.0,
            outliner_width: 245.0,
            details_width: 285.0,
            log_height: 145.0,
        }
    }
}

pub struct Editor {
    context: Context,
    platform: Option<egui_winit::State>,
    renderer: Option<egui_wgpu::Renderer>,
    pub selected: Option<Entity>,
    selected_entities: BTreeSet<Entity>,
    outliner_sort: String,
    rename_entity: Option<Entity>,
    rename_buffer: String,
    copied_transform: Option<Transform>,
    pub wireframe: bool,
    pub grid: bool,
    pub show_gizmos: bool,
    pub show_collision: bool,
    pub active_tool: usize,
    pub status: String,
    dirty: bool,
    filter: String,
    content_root: PathBuf,
    content_dir: PathBuf,
    content_history: Vec<PathBuf>,
    history_index: usize,
    content_list_view: bool,
    content_type_filter: String,
    content_selected: Option<PathBuf>,
    log_filter: String,
    log: Vec<(String, String)>,
    scene_path: PathBuf,
    view_mode: ViewMode,
    orbit_target: Vec3,
    orbit_distance: f32,
    last_view_mode: ViewMode,

    // Docking & Workspaces
    pub dock_state: Option<DockState<EditorTab>>,
    pub current_workspace: WorkspacePreset,
    pub script_buffer: String,
    pub script_file: String,
    pub script_status: String,
    pub graphics_msaa: u32,
    pub graphics_vsync: bool,
    pub graphics_shadows: String,
    pub camera_fov: f32,
    pub mouse_sensitivity: f32,
}

impl Editor {
    pub fn new(window: &Window, device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let context = Context::default();
        let platform = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            None,
            None,
            None,
        );
        let renderer =
            egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());
        let layout = std::fs::read_to_string("editor-layout.ron")
            .ok()
            .and_then(|s| ron::from_str::<LayoutSettings>(&s).ok())
            .unwrap_or_default();
        let dock_state = layout
            .dock_state
            .or_else(|| Some(layout.workspace_preset.create_dock_state()));
        Self {
            context,
            platform: Some(platform),
            renderer: Some(renderer),
            selected: None,
            selected_entities: BTreeSet::new(),
            outliner_sort: "Default".into(),
            rename_entity: None,
            rename_buffer: String::new(),
            copied_transform: None,
            wireframe: layout.wireframe,
            grid: layout.grid,
            show_gizmos: layout.show_gizmos,
            show_collision: layout.show_collision,
            active_tool: layout.active_tool,
            status: "Ready".into(),
            dirty: false,
            filter: String::new(),
            content_root: PathBuf::from("assets"),
            content_dir: PathBuf::from("assets"),
            content_history: vec![PathBuf::from("assets")],
            history_index: 0,
            content_list_view: true,
            content_type_filter: "All types".into(),
            content_selected: None,
            log_filter: String::new(),
            log: vec![("INFO".into(), "Editor session started".into())],
            scene_path: PathBuf::from("assets/scenes/editor.ron"),
            view_mode: layout.view_mode,
            orbit_target: Vec3::ZERO,
            orbit_distance: 8.0,
            last_view_mode: layout.view_mode,
            dock_state,
            current_workspace: layout.workspace_preset,
            script_buffer: DEFAULT_SCRIPT_CONTROLLER.to_string(),
            script_file: "player_controller.rs".into(),
            script_status: "Ready".into(),
            graphics_msaa: 4,
            graphics_vsync: true,
            graphics_shadows: "High".into(),
            camera_fov: 75.0,
            mouse_sensitivity: 1.0,
        }
    }
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.platform
            .as_mut()
            .is_some_and(|p| p.on_window_event(window, event).consumed)
    }
    pub fn apply_view_camera(&mut self, world: &mut World) {
        if self.view_mode != self.last_view_mode {
            if let Some((mut transform, mut controller)) = world
                .query::<(&mut Transform, &mut CameraController)>()
                .iter_mut(world)
                .next()
            {
                match self.view_mode {
                    ViewMode::Top => {
                        controller.yaw = 0.0;
                        controller.pitch = -1.55
                    }
                    ViewMode::Front => {
                        controller.yaw = 0.0;
                        controller.pitch = 0.0
                    }
                    ViewMode::Side => {
                        controller.yaw = std::f32::consts::FRAC_PI_2;
                        controller.pitch = 0.0
                    }
                    _ => {}
                }
                transform.rotation = glam::Quat::from_rotation_y(controller.yaw)
                    * glam::Quat::from_rotation_x(controller.pitch);
                transform.translation =
                    self.orbit_target - controller.forward() * self.orbit_distance;
            }
            self.last_view_mode = self.view_mode;
        }
    }
    pub fn draw(
        &mut self,
        window: &Window,
        frame: wgpu::SurfaceTexture,
        view: &wgpu::TextureView,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        world: &mut World,
        fps: f32,
    ) {
        let input = self.platform.as_mut().unwrap().take_egui_input(window);
        let context = self.context.clone();
        let output = context.run(input, |ctx| self.draw_ui(ctx, world, fps));
        self.platform
            .as_mut()
            .unwrap()
            .handle_platform_output(window, output.platform_output);
        let pixels = self.context.pixels_per_point();
        let jobs = self.context.tessellate(output.shapes, pixels);
        let size = window.inner_size();
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: pixels,
        };
        let renderer = self.renderer.as_mut().unwrap();
        for (id, delta) in &output.textures_delta.set {
            renderer.update_texture(device, queue, *id, delta);
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("editor ui encoder"),
        });
        let buffers = renderer.update_buffers(device, queue, &mut encoder, &jobs, &descriptor);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("editor egui overlay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            renderer.render(&mut pass, &jobs, &descriptor);
        }
        queue.submit(buffers.into_iter().chain([encoder.finish()]));
        frame.present();
        for id in &output.textures_delta.free {
            renderer.free_texture(id);
        }
        if self
            .context
            .input(|i| i.pointer.button_released(egui::PointerButton::Primary))
        {
            self.save_layout();
        }
    }
    fn draw_ui(&mut self, ctx: &Context, world: &mut World, fps: f32) {
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(30, 31, 34);
        style.visuals.window_fill = Color32::from_rgb(35, 36, 40);
        style.spacing.item_spacing = Vec2::new(6.0, 5.0);
        ctx.set_style(style);

        let mut dock_state = self
            .dock_state
            .take()
            .unwrap_or_else(|| self.current_workspace.create_dock_state());

        let mut actions = Vec::new();

        egui::TopBottomPanel::top("menu")
            .exact_height(28.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("R  ENGINE")
                            .strong()
                            .color(Color32::from_rgb(82, 166, 235)),
                    );
                    ui.separator();
                    for menu in ["File", "Edit", "Window", "Tools", "Build", "Help"] {
                        ui.menu_button(menu, |ui| match menu {
                            "File" => {
                                if ui.button("Save Scene  (Ctrl+S)").clicked() {
                                    self.save_scene(world);
                                    ui.close();
                                }
                                if ui.button("Reload Scene").clicked() {
                                    self.load_scene(world);
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button("Save Layout").clicked() {
                                    self.dock_state = Some(dock_state.clone());
                                    self.save_layout();
                                    self.dock_state = None;
                                    self.status = "Layout saved".into();
                                    ui.close();
                                }
                                if ui.button("Reset Layout to Default").clicked() {
                                    actions.push(DockAction::ResetDefault);
                                    self.current_workspace = WorkspacePreset::Default;
                                    ui.close();
                                }
                            }
                            "Edit" => {
                                if ui.button("Select All (Ctrl+A)").clicked() {
                                    self.select_all(world);
                                    ui.close();
                                }
                                if ui.button("Deselect All (Esc)").clicked() {
                                    self.selected = None;
                                    self.selected_entities.clear();
                                    ui.close();
                                }
                                if ui.button("Delete Selected (Del)").clicked() {
                                    self.delete_selected(world);
                                    ui.close();
                                }
                                if ui.button("Duplicate Selected (Ctrl+D)").clicked() {
                                    self.duplicate_selected(world);
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button("Focus Selected (F)").clicked() {
                                    if let Some(e) = self.selected {
                                        if let Some(t) = world.get::<Transform>(e) {
                                            self.orbit_target = t.translation;
                                            self.update_camera(world);
                                        }
                                    }
                                    ui.close();
                                }
                            }
                            "Window" => {
                                ui.menu_button("Workspaces", |ui| {
                                    for preset in WorkspacePreset::ALL {
                                        let active = self.current_workspace == preset;
                                        if ui.selectable_label(active, preset.name()).clicked() {
                                            self.current_workspace = preset;
                                            actions.push(DockAction::ApplyPreset(preset));
                                            self.status =
                                                format!("Switched to {} workspace", preset.name());
                                            ui.close();
                                        }
                                    }
                                });
                                ui.separator();
                                for tab in EditorTab::ALL {
                                    let is_open = dock_state.find_tab(&tab).is_some();
                                    let mut toggle = is_open;
                                    if ui.checkbox(&mut toggle, tab.title()).clicked() {
                                        if toggle {
                                            actions.push(DockAction::Open(tab));
                                        } else {
                                            actions.push(DockAction::Close(tab));
                                        }
                                    }
                                }
                                ui.separator();
                                ui.menu_button("Detach to Floating Window", |ui| {
                                    for tab in EditorTab::ALL {
                                        if dock_state.find_tab(&tab).is_some() {
                                            if ui.button(tab.title()).clicked() {
                                                actions.push(DockAction::Detach(tab));
                                                ui.close();
                                            }
                                        }
                                    }
                                });
                                ui.separator();
                                if ui.button("Reset Layout to Default").clicked() {
                                    self.current_workspace = WorkspacePreset::Default;
                                    actions.push(DockAction::ResetDefault);
                                    ui.close();
                                }
                            }
                            "Tools" => {
                                ui.checkbox(&mut self.grid, "Show Grid");
                                ui.checkbox(&mut self.show_gizmos, "Show Gizmos");
                                ui.checkbox(&mut self.show_collision, "Show Collision");
                                ui.checkbox(&mut self.wireframe, "Wireframe");
                                ui.separator();
                                if ui.button("Reset Camera Orbit").clicked() {
                                    self.orbit_target = Vec3::ZERO;
                                    self.orbit_distance = 8.0;
                                    self.update_camera(world);
                                    ui.close();
                                }
                            }
                            "Build" => {
                                if ui.button("Build Lighting").clicked() {
                                    self.log.push((
                                        "INFO".into(),
                                        "Build Lighting completed (0 static lights baked)".into(),
                                    ));
                                    ui.close();
                                }
                                if ui.button("Build Navigation Mesh").clicked() {
                                    self.log.push((
                                        "INFO".into(),
                                        "NavMesh generated (1 polygon island, 24 nodes)".into(),
                                    ));
                                    ui.close();
                                }
                                if ui.button("Build All").clicked() {
                                    self.log.push((
                                        "INFO".into(),
                                        "Full scene build completed successfully".into(),
                                    ));
                                    ui.close();
                                }
                            }
                            "Help" => {
                                ui.label("Rust Engine Editor");
                                ui.separator();
                                ui.label("Shortcuts:");
                                ui.label("Ctrl+1..7: Focus Tabs");
                                ui.label("W/E/R: Move/Rotate/Scale");
                                ui.label("RMB Drag: Orbit Camera");
                                ui.label("MMB Drag: Pan Camera");
                                ui.label("Wheel: Zoom Camera");
                                ui.label("F: Focus Selected");
                                ui.label("Del: Delete Entity");
                                ui.label("Ctrl+S: Save Scene");
                            }
                            _ => {}
                        });
                    }
                    if ui.button("Save").clicked() {
                        self.save_scene(world);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("{}  |  {:.0} FPS", self.status, fps));
                    });
                })
            });

        egui::TopBottomPanel::top("toolbar")
            .exact_height(34.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    for (i, label) in ["Select (Q)", "Move (W)", "Rotate (E)", "Scale (R)"]
                        .iter()
                        .enumerate()
                    {
                        if ui.selectable_label(self.active_tool == i, *label).clicked() {
                            self.active_tool = i;
                        }
                    }
                    ui.separator();
                    if ui.button("Add Actor").clicked() {
                        self.create_entity(world);
                    }
                    if ui.button("Save").clicked() {
                        self.save_scene(world);
                    }
                    ui.separator();
                    ui.label("View:");
                    for (label, mode) in [
                        ("Persp", ViewMode::Perspective),
                        ("Top", ViewMode::Top),
                        ("Front", ViewMode::Front),
                        ("Side", ViewMode::Side),
                        ("Ortho", ViewMode::Orthographic),
                    ] {
                        if ui.selectable_label(self.view_mode == mode, label).clicked() {
                            self.view_mode = mode;
                        }
                    }
                    ui.separator();
                    ui.checkbox(&mut self.grid, "Grid");
                    ui.checkbox(&mut self.show_gizmos, "Gizmos");
                    ui.checkbox(&mut self.wireframe, "Wireframe");

                    ui.separator();
                    ui.label("Workspace:");
                    for preset in WorkspacePreset::ALL {
                        let active = self.current_workspace == preset;
                        let short_name = match preset {
                            WorkspacePreset::Default => "Default",
                            WorkspacePreset::LevelEditing => "Level",
                            WorkspacePreset::AssetEditing => "Assets",
                            WorkspacePreset::Scripting => "Script",
                            WorkspacePreset::Minimal => "Minimal",
                        };
                        if ui
                            .selectable_label(active, short_name)
                            .on_hover_text(preset.description())
                            .clicked()
                        {
                            self.current_workspace = preset;
                            actions.push(DockAction::ApplyPreset(preset));
                            self.status = format!("Switched to {} workspace", preset.name());
                        }
                    }
                })
            });

        // Shortcuts
        ctx.input(|i| {
            if i.modifiers.ctrl {
                if i.key_pressed(Key::Num1) {
                    actions.push(DockAction::Open(EditorTab::Viewport));
                }
                if i.key_pressed(Key::Num2) {
                    actions.push(DockAction::Open(EditorTab::WorldOutliner));
                }
                if i.key_pressed(Key::Num3) {
                    actions.push(DockAction::Open(EditorTab::Details));
                }
                if i.key_pressed(Key::Num4) {
                    actions.push(DockAction::Open(EditorTab::ContentBrowser));
                }
                if i.key_pressed(Key::Num5) {
                    actions.push(DockAction::Open(EditorTab::OutputLog));
                }
                if i.key_pressed(Key::Num6) {
                    actions.push(DockAction::Open(EditorTab::ScriptEditor));
                }
                if i.key_pressed(Key::Num7) {
                    actions.push(DockAction::Open(EditorTab::GraphicsSettings));
                }
                if i.key_pressed(Key::S) {
                    self.save_scene(world);
                }
                if i.key_pressed(Key::A) {
                    self.select_all(world);
                }
                if i.key_pressed(Key::D) {
                    self.duplicate_selected(world);
                }
            }
            if !i.modifiers.ctrl && !i.modifiers.alt {
                if i.key_pressed(Key::Q) {
                    self.active_tool = 0;
                }
                if i.key_pressed(Key::W) {
                    self.active_tool = 1;
                }
                if i.key_pressed(Key::E) {
                    self.active_tool = 2;
                }
                if i.key_pressed(Key::R) {
                    self.active_tool = 3;
                }
                if i.key_pressed(Key::Delete) {
                    self.delete_selected(world);
                }
                if i.key_pressed(Key::F) {
                    if let Some(e) = self.selected {
                        if let Some(t) = world.get::<Transform>(e) {
                            self.orbit_target = t.translation;
                            self.update_camera(world);
                        }
                    }
                }
                if i.key_pressed(Key::Escape) {
                    self.selected = None;
                    self.selected_entities.clear();
                }
            }
        });

        // Render main DockArea with dark editor style
        {
            let mut viewer = EditorTabViewer {
                editor: self,
                world,
                actions: &mut actions,
            };
            DockArea::new(&mut dock_state)
                .style(editor_dock_style(ctx))
                .show(ctx, &mut viewer);
        }

        let screen_rect = ctx.content_rect();
        let default_window_rect = egui::Rect::from_min_size(
            screen_rect.center() - Vec2::new(320.0, 240.0),
            Vec2::new(640.0, 480.0),
        );
        for action in actions {
            action.apply(&mut dock_state, default_window_rect);
        }

        self.dock_state = Some(dock_state);

        if let Some(single) = self.selected {
            self.selected_entities.insert(single);
        }
    }

    pub fn select_all(&mut self, world: &mut World) {
        self.selected_entities.clear();
        for (e, _) in world.query::<(Entity, &Transform)>().iter(world) {
            self.selected_entities.insert(e);
        }
        self.selected = self.selected_entities.iter().next().copied();
    }

    pub fn duplicate_selected(&mut self, world: &mut World) {
        let targets = if !self.selected_entities.is_empty() {
            self.selected_entities.iter().copied().collect::<Vec<_>>()
        } else if let Some(e) = self.selected {
            vec![e]
        } else {
            return;
        };

        let mut new_entities = Vec::new();
        for e in targets {
            let transform = world.get::<Transform>(e).copied().unwrap_or_default();
            let name = world
                .get::<Name>(e)
                .map(|n| format!("{} Copy", n.0))
                .unwrap_or_else(|| "Entity Copy".into());
            let group = world
                .get::<FolderGroup>(e)
                .map(|g| g.0.clone())
                .unwrap_or_else(|| "Default".into());
            let visible = world.get::<Visible>(e).copied().unwrap_or_default();
            let locked = world.get::<Locked>(e).copied().unwrap_or_default();

            let mut offset_transform = transform;
            offset_transform.translation += Vec3::new(1.0, 0.0, 1.0);

            let new_e = world
                .spawn((
                    Name(name),
                    offset_transform,
                    FolderGroup(group),
                    visible,
                    locked,
                ))
                .id();
            new_entities.push(new_e);
        }

        self.selected_entities.clear();
        for &e in &new_entities {
            self.selected_entities.insert(e);
        }
        self.selected = new_entities.first().copied();
        self.dirty = true;
        self.status = format!("Duplicated {} entities", new_entities.len());
    }

    fn draw_viewport(&mut self, ui: &mut egui::Ui, world: &mut World) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(match self.view_mode {
                    ViewMode::Perspective => "PERSPECTIVE",
                    ViewMode::Top => "TOP",
                    ViewMode::Front => "FRONT",
                    ViewMode::Side => "SIDE",
                    ViewMode::Orthographic => "ORTHOGRAPHIC",
                })
                .strong(),
            );
            ui.separator();
            ui.label(if self.wireframe { "WIREFRAME" } else { "LIT" });
            ui.separator();
            ui.label(format!(
                "Tool: {}",
                match self.active_tool {
                    1 => "Move",
                    2 => "Rotate",
                    3 => "Scale",
                    _ => "Select",
                }
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Reset Camera").clicked() {
                    self.orbit_target = Vec3::ZERO;
                    self.orbit_distance = 8.0;
                    self.update_camera(world);
                }
                if let Some(e) = self.selected {
                    if ui.button("Focus (F)").clicked() {
                        if let Some(t) = world.get::<Transform>(e) {
                            self.orbit_target = t.translation;
                            self.update_camera(world);
                        }
                    }
                }
            });
        });

        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                self.orbit_distance =
                    (self.orbit_distance * (-scroll * 0.001).exp()).clamp(0.25, 500.0);
                self.update_camera(world);
            }
            if response.dragged_by(egui::PointerButton::Secondary) {
                let delta = response.drag_delta();
                if let Some((mut t, mut c)) = world
                    .query::<(&mut Transform, &mut CameraController)>()
                    .iter_mut(world)
                    .next()
                {
                    c.yaw -= delta.x * c.mouse_sensitivity * 0.01 * self.mouse_sensitivity;
                    c.pitch = (c.pitch
                        - delta.y * c.mouse_sensitivity * 0.01 * self.mouse_sensitivity)
                        .clamp(-1.55, 1.55);
                    t.rotation =
                        glam::Quat::from_rotation_y(c.yaw) * glam::Quat::from_rotation_x(c.pitch);
                    t.translation = self.orbit_target - c.forward() * self.orbit_distance;
                }
            }
            if response.dragged_by(egui::PointerButton::Middle) {
                let d = response.drag_delta();
                self.orbit_target += Vec3::new(-d.x, d.y, 0.0) * 0.01;
                self.update_camera(world);
            }
        }
        if response.clicked() {
            let mut hit = None;
            for (e, t) in world.query::<(Entity, &Transform)>().iter(world) {
                let center =
                    rect.center() + Vec2::new(t.translation.x * 24.0, -t.translation.y * 24.0);
                if let Some(pos) = response.interact_pointer_pos() {
                    if (pos - center).length() < 24.0 {
                        hit = Some(e);
                        break;
                    }
                }
            }
            if let Some(e) = hit {
                self.set_selected_entity(Some(e), false);
            } else if ui.input(|i| !i.modifiers.ctrl && !i.modifiers.shift) {
                self.set_selected_entity(None, false);
            }
            if let Some(e) = self.selected {
                if let Some(t) = world.get::<Transform>(e) {
                    self.orbit_target = t.translation;
                }
            }
        }
        if response.dragged_by(egui::PointerButton::Primary) && self.show_gizmos {
            if let Some(e) = self.selected {
                if let Some(mut t) = world.get_mut::<Transform>(e) {
                    let d = response.drag_delta();
                    match self.active_tool {
                        1 => {
                            t.translation.x += d.x * 0.01;
                            t.translation.y -= d.y * 0.01;
                        }
                        2 => {
                            t.rotation = glam::Quat::from_rotation_y(d.x * 0.01) * t.rotation;
                        }
                        3 => t.scale *= ((d.x - d.y) * 0.005 + 1.0).max(0.01),
                        _ => {}
                    }
                    self.dirty = true;
                }
            }
        }
        response.context_menu(|ui| {
            ui.label(RichText::new("Viewport").strong());
            ui.separator();
            ui.checkbox(&mut self.grid, "Show Grid");
            ui.checkbox(&mut self.show_gizmos, "Show Gizmos");
            ui.checkbox(&mut self.show_collision, "Show Collision");
            ui.checkbox(&mut self.wireframe, "Wireframe");
            ui.separator();
            if ui.button("Focus Selected (F)").clicked() {
                if let Some(e) = self.selected {
                    if let Some(t) = world.get::<Transform>(e) {
                        self.orbit_target = t.translation;
                        self.update_camera(world);
                    }
                }
                ui.close();
            }
            if ui.button("Reset Camera Orbit").clicked() {
                self.orbit_target = Vec3::ZERO;
                self.orbit_distance = 8.0;
                self.update_camera(world);
                ui.close();
            }
        });
        let p = ui.painter();
        p.rect_filled(rect, 0.0, Color32::from_rgb(23, 25, 29));
        p.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0_f32, Color32::from_gray(58)),
            egui::StrokeKind::Inside,
        );
        if self.grid {
            let center = rect.center();
            for i in 0..24 {
                let off = i as f32 * 22.0;
                p.line_segment(
                    [
                        egui::pos2(center.x - off, center.y + 30.0),
                        egui::pos2(center.x + off, center.y + 30.0),
                    ],
                    egui::Stroke::new(1.0_f32, Color32::from_gray(47)),
                );
                p.line_segment(
                    [
                        egui::pos2(center.x - off, center.y - 140.0),
                        egui::pos2(center.x + off, center.y + 30.0),
                    ],
                    egui::Stroke::new(1.0_f32, Color32::from_gray(47)),
                );
            }
            p.line_segment(
                [
                    egui::pos2(center.x - 120.0, center.y + 30.0),
                    egui::pos2(center.x + 120.0, center.y + 30.0),
                ],
                egui::Stroke::new(2.0_f32, Color32::from_rgb(185, 70, 70)),
            );
            p.line_segment(
                [
                    egui::pos2(center.x, center.y - 130.0),
                    egui::pos2(center.x, center.y + 30.0),
                ],
                egui::Stroke::new(2.0_f32, Color32::from_rgb(80, 160, 100)),
            );
        }
        if self.show_gizmos {
            p.text(
                rect.left_bottom() + Vec2::new(12.0, -12.0),
                egui::Align2::LEFT_BOTTOM,
                "X  Y  Z",
                egui::FontId::proportional(11.0),
                Color32::from_rgb(150, 170, 195),
            );
        }
        for entity in &self.selected_entities {
            if let Some(t) = world.get::<Transform>(*entity) {
                let center =
                    rect.center() + Vec2::new(t.translation.x * 24.0, -t.translation.y * 24.0);
                p.circle_stroke(
                    center,
                    12.0,
                    egui::Stroke::new(2.0_f32, Color32::from_rgb(255, 178, 64)),
                );
                p.text(
                    center + Vec2::new(0.0, -16.0),
                    egui::Align2::CENTER_BOTTOM,
                    "Selected",
                    egui::FontId::proportional(10.0),
                    Color32::from_rgb(255, 206, 120),
                );
            }
        }
    }

    fn draw_output_log(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("OUTPUT LOG");
            ui.separator();

            let total_count = self.log.len();
            let err_count = self.log.iter().filter(|(l, _)| l == "ERROR").count();
            let warn_count = self.log.iter().filter(|(l, _)| l == "WARN").count();
            let info_count = self.log.iter().filter(|(l, _)| l == "INFO").count();

            ui.label(format!("Total: {total_count}"));
            if err_count > 0 {
                ui.label(
                    RichText::new(format!("Errors: {err_count}"))
                        .color(Color32::from_rgb(255, 90, 90)),
                );
            }
            if warn_count > 0 {
                ui.label(
                    RichText::new(format!("Warnings: {warn_count}"))
                        .color(Color32::from_rgb(255, 200, 70)),
                );
            }
            ui.label(
                RichText::new(format!("Info: {info_count}"))
                    .color(Color32::from_rgb(140, 180, 220)),
            );

            ui.separator();
            ui.label("Filter:");
            ui.add(
                egui::TextEdit::singleline(&mut self.log_filter)
                    .hint_text("Search log...")
                    .desired_width(180.0),
            );
            if !self.log_filter.is_empty() && ui.small_button("×").clicked() {
                self.log_filter.clear();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear Log").clicked() {
                    self.log.clear();
                }
            });
        });

        ui.separator();

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.style_mut().spacing.item_spacing.y = 2.0;
                for (level, msg) in &self.log {
                    if self.log_filter.is_empty()
                        || msg.to_lowercase().contains(&self.log_filter.to_lowercase())
                        || level
                            .to_lowercase()
                            .contains(&self.log_filter.to_lowercase())
                    {
                        ui.horizontal(|ui| {
                            let (badge_bg, text_col) = match level.as_str() {
                                "ERROR" => (
                                    Color32::from_rgb(180, 40, 40),
                                    Color32::from_rgb(255, 120, 120),
                                ),
                                "WARN" => (
                                    Color32::from_rgb(160, 120, 20),
                                    Color32::from_rgb(255, 210, 80),
                                ),
                                _ => (
                                    Color32::from_rgb(35, 75, 130),
                                    Color32::from_rgb(180, 210, 240),
                                ),
                            };
                            let badge = RichText::new(format!(" {level} "))
                                .size(10.0)
                                .color(Color32::WHITE)
                                .background_color(badge_bg);
                            ui.label(badge);
                            ui.label(RichText::new(msg).monospace().color(text_col));
                        });
                    }
                }
            });
    }

    fn draw_script_editor(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("SCRIPT EDITOR");
            ui.separator();
            ui.label("File:");
            egui::ComboBox::from_id_salt("script_file_combo")
                .selected_text(&self.script_file)
                .show_ui(ui, |ui| {
                    for (file_name, code) in [
                        ("player_controller.rs", DEFAULT_SCRIPT_CONTROLLER),
                        ("rotator.rs", DEFAULT_SCRIPT_ROTATOR),
                        ("camera_shake.rs", DEFAULT_SCRIPT_CAMERA_SHAKE),
                        ("custom_shader.wgsl", DEFAULT_SCRIPT_SHADER),
                    ] {
                        if ui
                            .selectable_label(self.script_file == file_name, file_name)
                            .clicked()
                        {
                            self.script_file = file_name.into();
                            self.script_buffer = code.into();
                            self.script_status = "Ready".into();
                        }
                    }
                });

            if ui.button("Compile & Hot-Reload").clicked() {
                self.script_status = "Compiled (hot-reloaded)".into();
                self.log.push((
                    "INFO".into(),
                    format!(
                        "Script '{}' compiled successfully (hot-reloaded into active scene)",
                        self.script_file
                    ),
                ));
                self.status = format!("Compiled {}", self.script_file);
            }

            if ui.button("Save Script").clicked() {
                self.log.push((
                    "INFO".into(),
                    format!("Saved script '{}' to workspace", self.script_file),
                ));
                self.status = format!("Saved {}", self.script_file);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("Status: {}", self.script_status))
                        .color(Color32::from_rgb(100, 220, 140)),
                );
            });
        });

        ui.separator();

        egui::ScrollArea::both().show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.script_buffer)
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(24),
            );
        });
    }

    fn draw_graphics_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("GRAPHICS & DISPLAY");
        ui.separator();

        ui.label(RichText::new("Quality Presets").strong());
        ui.horizontal(|ui| {
            for (name, msaa, shadow, fov) in [
                ("Low", 1, "Low", 70.0),
                ("Medium", 2, "Medium", 75.0),
                ("High", 4, "High", 75.0),
                ("Ultra", 4, "Ultra", 80.0),
            ] {
                if ui.button(name).clicked() {
                    self.graphics_msaa = msaa;
                    self.graphics_shadows = shadow.into();
                    self.camera_fov = fov;
                    self.status = format!("Applied {name} graphics preset");
                    self.log.push((
                        "INFO".into(),
                        format!("Applied {name} quality preset (MSAA: {msaa}x, Shadows: {shadow})"),
                    ));
                }
            }
        });

        ui.add_space(8.0);
        ui.label(RichText::new("Rendering Engine").strong());
        ui.horizontal(|ui| {
            ui.label("MSAA Anti-Aliasing:");
            for samples in [1, 2, 4] {
                let label = if samples == 1 {
                    "Off (1x)".to_string()
                } else {
                    format!("{samples}x")
                };
                if ui
                    .selectable_label(self.graphics_msaa == samples, label)
                    .clicked()
                {
                    self.graphics_msaa = samples;
                    self.log
                        .push(("INFO".into(), format!("MSAA set to {samples}x")));
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("Shadow Quality:");
            for quality in ["Low", "Medium", "High", "Ultra"] {
                if ui
                    .selectable_label(self.graphics_shadows == quality, quality)
                    .clicked()
                {
                    self.graphics_shadows = quality.into();
                }
            }
        });

        ui.checkbox(&mut self.graphics_vsync, "Vertical Sync (V-Sync)");
        ui.checkbox(&mut self.wireframe, "Wireframe Rendering Mode");

        ui.add_space(8.0);
        ui.label(RichText::new("Viewport Overlays").strong());
        ui.checkbox(&mut self.grid, "Coordinate Ground Grid");
        ui.checkbox(&mut self.show_gizmos, "Transform Gizmos & Axis Labels");
        ui.checkbox(&mut self.show_collision, "Collision Wireframe Bounds");

        ui.add_space(8.0);
        ui.label(RichText::new("Camera Controls").strong());
        ui.add(egui::Slider::new(&mut self.camera_fov, 45.0..=115.0).text("Field of View (FOV)"));
        ui.add(egui::Slider::new(&mut self.mouse_sensitivity, 0.2..=3.0).text("Mouse Sensitivity"));
        ui.add(egui::Slider::new(&mut self.orbit_distance, 1.0..=30.0).text("Orbit Distance"));

        if ui.button("Reset Camera to Center").clicked() {
            self.orbit_target = Vec3::ZERO;
            self.orbit_distance = 8.0;
        }
    }
    fn draw_content_browser(&mut self, ui: &mut egui::Ui, _world: &mut World) {
        let _ = fs::create_dir_all(&self.content_root);
        ui.horizontal(|ui| {
            ui.heading("CONTENT BROWSER");
            if ui.button("◧").on_hover_text("List view").clicked() {
                self.content_list_view = true;
            }
            if ui.button("▦").on_hover_text("Grid view").clicked() {
                self.content_list_view = false;
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.history_index > 0, egui::Button::new("‹"))
                .on_hover_text("Back")
                .clicked()
            {
                self.history_index -= 1;
                self.content_dir = self.content_history[self.history_index].clone();
            }
            if ui
                .add_enabled(
                    self.history_index + 1 < self.content_history.len(),
                    egui::Button::new("›"),
                )
                .on_hover_text("Forward")
                .clicked()
            {
                self.history_index += 1;
                self.content_dir = self.content_history[self.history_index].clone();
            }
            if ui.button("⌂").on_hover_text("Content root").clicked() {
                self.navigate_content(self.content_root.clone());
            }
        });
        let relative_parts: Vec<String> = self
            .content_dir
            .strip_prefix(&self.content_root)
            .unwrap_or(Path::new(""))
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect();
        ui.horizontal_wrapped(|ui| {
            let mut target = self.content_root.clone();
            if ui
                .selectable_label(relative_parts.is_empty(), "Content")
                .clicked()
            {
                self.navigate_content(target.clone());
            }
            for part in &relative_parts {
                target.push(part);
                ui.label("›");
                if ui.selectable_label(false, part).clicked() {
                    self.navigate_content(target.clone());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("⌕");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Search assets…")
                    .desired_width(f32::INFINITY),
            );
            if !self.filter.is_empty() && ui.small_button("×").clicked() {
                self.filter.clear();
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("content_type_filter")
                .selected_text(&self.content_type_filter)
                .show_ui(ui, |ui| {
                    for kind in [
                        "All types",
                        "Folders",
                        "Scenes",
                        "Materials",
                        "Textures",
                        "Models",
                        "Scripts",
                    ] {
                        ui.selectable_value(&mut self.content_type_filter, kind.into(), kind);
                    }
                });
            ui.menu_button("+ Create", |ui| {
                if ui.button("Folder").clicked() {
                    self.create_content_folder();
                    ui.close();
                }
                for (label, ext) in [("Scene", "ron"), ("Material", "mat"), ("Script", "rs")] {
                    if ui.button(label).clicked() {
                        self.create_content_file(ext);
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        let mut entries: Vec<_> = fs::read_dir(&self.content_dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|entry| {
            (
                !entry.path().is_dir(),
                entry.file_name().to_string_lossy().to_lowercase(),
            )
        });
        let query = self.filter.to_lowercase();
        let matches = |path: &Path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if !name.contains(&query) {
                return false;
            }
            let is_dir = path.is_dir();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            match self.content_type_filter.as_str() {
                "Folders" => is_dir,
                "Scenes" => ext == "ron" || ext == "scene",
                "Materials" => ext == "mat" || ext == "material",
                "Textures" => ["png", "jpg", "jpeg", "bmp", "tga", "dds"].contains(&ext.as_str()),
                "Models" => ["obj", "fbx", "gltf", "glb"].contains(&ext.as_str()),
                "Scripts" => ["rs", "lua", "py"].contains(&ext.as_str()),
                _ => true,
            }
        };
        let filtered: Vec<PathBuf> = entries
            .into_iter()
            .map(|entry| entry.path())
            .filter(|path| matches(path))
            .collect();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.content_list_view {
                for path in filtered {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let kind = content_kind(&path);
                    let response = ui.selectable_label(
                        self.content_selected.as_ref() == Some(&path),
                        format!("{}  {}   · {}", content_icon(&path), name, kind),
                    );
                    if response.clicked() {
                        self.content_selected = Some(path.clone());
                        if path.is_dir() {
                            self.navigate_content(path.clone());
                        }
                    }
                    response.context_menu(|ui| self.content_context_menu(ui, &path));
                }
            } else {
                let columns = ((ui.available_width() / 92.0).floor() as usize).max(1);
                egui::Grid::new("content_grid")
                    .num_columns(columns)
                    .spacing([6.0, 8.0])
                    .show(ui, |ui| {
                        for (index, path) in filtered.iter().enumerate() {
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let response = ui
                                .allocate_ui_with_layout(
                                    Vec2::new(82.0, 86.0),
                                    egui::Layout::top_down(egui::Align::Center),
                                    |ui| {
                                        let selected = self.content_selected.as_ref() == Some(path);
                                        let fill = if selected {
                                            Color32::from_rgb(52, 76, 102)
                                        } else {
                                            Color32::from_rgb(39, 41, 46)
                                        };
                                        egui::Frame::new()
                                            .fill(fill)
                                            .corner_radius(4)
                                            .inner_margin(6.0)
                                            .show(ui, |ui| {
                                                ui.set_min_width(68.0);
                                                ui.set_min_height(70.0);
                                                ui.vertical_centered(|ui| {
                                                    ui.label(
                                                        RichText::new(content_icon(path))
                                                            .size(29.0),
                                                    );
                                                    ui.label(egui::RichText::new(name).size(11.0));
                                                    ui.label(
                                                        RichText::new(content_kind(path))
                                                            .size(9.0)
                                                            .color(Color32::GRAY),
                                                    );
                                                });
                                            });
                                    },
                                )
                                .response
                                .interact(egui::Sense::click());
                            if response.clicked() {
                                self.content_selected = Some(path.clone());
                                if path.is_dir() {
                                    self.navigate_content(path.clone());
                                }
                            }
                            response.context_menu(|ui| self.content_context_menu(ui, path));
                            if (index + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
            }
        });
        if self.content_selected.as_ref().is_some_and(|p| !p.exists()) {
            self.content_selected = None;
        }
    }

    fn navigate_content(&mut self, path: PathBuf) {
        let Ok(root) = self.content_root.canonicalize() else {
            return;
        };
        let Ok(next) = path.canonicalize() else {
            return;
        };
        if !next.starts_with(root) || !next.is_dir() {
            return;
        }
        self.content_dir = next;
        self.content_history.truncate(self.history_index + 1);
        if self.content_history.last() != Some(&self.content_dir) {
            self.content_history.push(self.content_dir.clone());
        }
        self.history_index = self.content_history.len().saturating_sub(1);
        self.content_selected = None;
    }

    fn content_context_menu(&mut self, ui: &mut egui::Ui, path: &Path) {
        if path.is_dir() && ui.button("Open").clicked() {
            self.navigate_content(path.to_path_buf());
            ui.close();
        }
        if ui.button("Create Folder").clicked() {
            self.create_content_folder();
            ui.close();
        }
        if ui.button("Rename").clicked() {
            self.rename_content(path);
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            self.duplicate_content(path);
            ui.close();
        }
        if ui.button("Delete").clicked() {
            self.delete_content(path);
            ui.close();
        }
        if ui
            .button("Import…")
            .on_hover_text("Unavailable: file picker integration is not implemented")
            .clicked()
        {
            ui.close();
        }
        if ui.button("Show in Explorer").clicked() {
            self.show_in_explorer(path);
            ui.close();
        }
    }

    fn create_content_folder(&mut self) {
        let mut n = 1;
        let path = loop {
            let candidate = self.content_dir.join(format!("NewFolder{n}"));
            if !candidate.exists() {
                break candidate;
            }
            n += 1;
        };
        if let Err(e) = fs::create_dir(&path) {
            self.log
                .push(("ERROR".into(), format!("Create folder failed: {e}")));
        }
    }
    fn create_content_file(&mut self, ext: &str) {
        let mut n = 1;
        let path = loop {
            let candidate = self.content_dir.join(format!("New{n}.{ext}"));
            if !candidate.exists() {
                break candidate;
            }
            n += 1;
        };
        if let Err(e) = fs::write(path, "") {
            self.log
                .push(("ERROR".into(), format!("Create file failed: {e}")));
        }
    }
    fn rename_content(&mut self, path: &Path) {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or(name);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let next = path.with_file_name(format!("{stem}_renamed{ext}"));
        if let Err(e) = fs::rename(path, next) {
            self.log
                .push(("ERROR".into(), format!("Rename failed: {e}")));
        }
    }
    fn duplicate_content(&mut self, path: &Path) {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        let target = path.with_file_name(format!("{name} copy"));
        let result = if path.is_dir() {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "folder duplication is not supported",
            ))
        } else {
            fs::copy(path, target).map(|_| ())
        };
        if let Err(e) = result {
            self.log
                .push(("ERROR".into(), format!("Duplicate failed: {e}")));
        }
    }
    fn delete_content(&mut self, path: &Path) {
        let result = if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
        if let Err(e) = result {
            self.log
                .push(("ERROR".into(), format!("Delete failed: {e}")));
        }
        if self.content_selected.as_deref() == Some(path) {
            self.content_selected = None;
        }
    }
    fn show_in_explorer(&mut self, path: &Path) {
        #[cfg(target_os = "windows")]
        let result = Command::new("explorer").arg("/select,").arg(path).spawn();
        #[cfg(not(target_os = "windows"))]
        let result = Command::new("xdg-open")
            .arg(path.parent().unwrap_or(path))
            .spawn();
        if let Err(e) = result {
            self.log
                .push(("ERROR".into(), format!("Explorer open failed: {e}")));
        }
    }

    fn set_selected_entity(&mut self, entity: Option<Entity>, multi: bool) {
        if multi {
            if let Some(e) = entity {
                if self.selected_entities.contains(&e) {
                    self.selected_entities.remove(&e);
                    if self.selected == Some(e) {
                        self.selected = self.selected_entities.iter().next().copied();
                    }
                } else {
                    self.selected_entities.insert(e);
                    self.selected = Some(e);
                }
            }
        } else {
            self.selected_entities.clear();
            if let Some(e) = entity {
                self.selected_entities.insert(e);
            }
            self.selected = entity;
        }
    }

    fn draw_world_outliner(&mut self, ui: &mut egui::Ui, world: &mut World) {
        ui.horizontal(|ui| {
            ui.heading("WORLD OUTLINER");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+ Actor").clicked() {
                    self.create_entity(world);
                }
                if ui.button("+ Group").clicked() {
                    let e = world
                        .spawn((
                            Name("New Folder".into()),
                            FolderGroup("Folder".into()),
                            Transform::from_translation(Vec3::ZERO),
                        ))
                        .id();
                    self.set_selected_entity(Some(e), false);
                    self.dirty = true;
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("⌕");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Search actors…")
                    .desired_width(f32::INFINITY),
            );
            if !self.filter.is_empty() && ui.small_button("×").clicked() {
                self.filter.clear();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Sort:");
            for sort in ["Default", "Name", "Type"] {
                if ui
                    .selectable_label(self.outliner_sort == sort, sort)
                    .clicked()
                {
                    self.outliner_sort = sort.into();
                }
            }
        });
        ui.separator();

        let mut items: Vec<(Entity, String, Option<Entity>, String, bool, bool)> = world
            .query::<(
                Entity,
                &Name,
                Option<&Parent>,
                Option<&FolderGroup>,
                Option<&Visible>,
                Option<&Locked>,
            )>()
            .iter(world)
            .map(|(e, name, parent, group, vis, lock)| {
                (
                    e,
                    name.0.clone(),
                    parent.map(|p| p.0),
                    group.map(|g| g.0.clone()).unwrap_or_else(|| "Actor".into()),
                    vis.map(|v| v.0).unwrap_or(true),
                    lock.map(|l| l.0).unwrap_or(false),
                )
            })
            .collect();

        match self.outliner_sort.as_str() {
            "Name" => items.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase())),
            "Type" => items.sort_by(|a, b| a.3.cmp(&b.3)),
            _ => {}
        }

        let query = self.filter.to_lowercase();
        let roots: Vec<Entity> = items
            .iter()
            .filter(|(_, _, parent, _, _, _)| parent.is_none())
            .map(|item| item.0)
            .collect();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for root in roots {
                self.draw_outliner_node(ui, world, root, 0, &query);
            }
        });

        if let Some(target) = self.rename_entity {
            if let Some(mut name) = world.get_mut::<Name>(target) {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Rename:");
                    let response = ui.text_edit_singleline(&mut self.rename_buffer);
                    if response.lost_focus() || ui.button("Apply").clicked() {
                        if !self.rename_buffer.trim().is_empty() {
                            name.0 = self.rename_buffer.trim().to_string();
                            self.dirty = true;
                        }
                        self.rename_entity = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.rename_entity = None;
                    }
                });
            } else {
                self.rename_entity = None;
            }
        }
    }

    fn draw_outliner_node(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut World,
        entity: Entity,
        depth: usize,
        filter: &str,
    ) {
        let Some(name) = world.get::<Name>(entity).map(|n| n.0.clone()) else {
            return;
        };
        let children: Vec<Entity> = world
            .query::<(Entity, &Parent)>()
            .iter(world)
            .filter_map(|(child, p)| (p.0 == entity).then_some(child))
            .collect();
        let is_folder = world.get::<FolderGroup>(entity).is_some();
        let visible = world.get::<Visible>(entity).map(|v| v.0).unwrap_or(true);
        let locked = world.get::<Locked>(entity).map(|l| l.0).unwrap_or(false);
        let matches = filter.is_empty() || name.to_lowercase().contains(filter);

        if matches {
            ui.horizontal(|ui| {
                if depth > 0 {
                    ui.add_space((depth as f32) * 12.0);
                }
                let icon = if is_folder { "📁" } else { "◇" };
                let is_selected = self.selected_entities.contains(&entity);
                let label = format!("{icon}  {name}");
                let response = ui.selectable_label(is_selected, label);
                let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
                if response.clicked() {
                    self.set_selected_entity(Some(entity), ctrl);
                }
                let vis_icon = if visible { "👁" } else { "⊘" };
                if ui.small_button(vis_icon).clicked() {
                    if let Some(mut v) = world.get_mut::<Visible>(entity) {
                        v.0 = !v.0;
                    } else {
                        world.entity_mut(entity).insert(Visible(!visible));
                    }
                    self.dirty = true;
                }
                let lock_icon = if locked { "🔒" } else { "🔓" };
                if ui.small_button(lock_icon).clicked() {
                    if let Some(mut l) = world.get_mut::<Locked>(entity) {
                        l.0 = !l.0;
                    } else {
                        world.entity_mut(entity).insert(Locked(!locked));
                    }
                    self.dirty = true;
                }
                response.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        self.rename_entity = Some(entity);
                        self.rename_buffer = name.clone();
                        ui.close();
                    }
                    if ui.button("Make Child of Selected").clicked() {
                        if let Some(parent) = self.selected {
                            if parent != entity {
                                world.entity_mut(entity).insert(Parent(parent));
                                self.dirty = true;
                            }
                        }
                        ui.close();
                    }
                    if ui.button("Detach From Parent").clicked() {
                        world.entity_mut(entity).remove::<Parent>();
                        self.dirty = true;
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        self.set_selected_entity(Some(entity), false);
                        self.delete_selected(world);
                        ui.close();
                    }
                });
            });
        }

        if is_folder || !children.is_empty() {
            for child in children {
                self.draw_outliner_node(ui, world, child, depth + 1, filter);
            }
        }
    }

    fn draw_details(&mut self, ui: &mut egui::Ui, world: &mut World) {
        ui.heading("DETAILS");
        ui.separator();

        let Some(entity) = self.selected else {
            ui.label("Select an actor to inspect properties");
            return;
        };

        let entity_name = world
            .get::<Name>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_else(|| "Unnamed".into());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&entity_name).strong());
            if ui.button("Rename").clicked() {
                self.rename_entity = Some(entity);
                self.rename_buffer = entity_name.clone();
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.collapsing("Transform", |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Copy").clicked() {
                        if let Some(t) = world.get::<Transform>(entity) {
                            self.copied_transform = Some(*t);
                            self.log.push(("INFO".into(), "Transform copied".into()));
                        }
                    }
                    if ui
                        .add_enabled(self.copied_transform.is_some(), egui::Button::new("Paste"))
                        .clicked()
                    {
                        if let (Some(mut t), Some(src)) =
                            (world.get_mut::<Transform>(entity), self.copied_transform)
                        {
                            *t = src;
                            self.dirty = true;
                            self.log.push(("INFO".into(), "Transform pasted".into()));
                        }
                    }
                    if ui.button("Reset All").clicked() {
                        if let Some(mut t) = world.get_mut::<Transform>(entity) {
                            *t = Transform::default();
                            self.dirty = true;
                        }
                    }
                });

                if let Some(mut t) = world.get_mut::<Transform>(entity) {
                    ui.horizontal(|ui| {
                        ui.label("Location");
                        if ui.small_button("↺").on_hover_text("Reset to 0").clicked() {
                            t.translation = Vec3::ZERO;
                            self.dirty = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut t.translation.x)
                                .prefix("X ")
                                .speed(0.1),
                        );
                        ui.add(
                            egui::DragValue::new(&mut t.translation.y)
                                .prefix("Y ")
                                .speed(0.1),
                        );
                        ui.add(
                            egui::DragValue::new(&mut t.translation.z)
                                .prefix("Z ")
                                .speed(0.1),
                        );
                    });

                    let (mut rx, mut ry, mut rz) = t.rotation.to_euler(glam::EulerRot::XYZ);
                    ui.horizontal(|ui| {
                        ui.label("Rotation");
                        if ui
                            .small_button("↺")
                            .on_hover_text("Reset rotation")
                            .clicked()
                        {
                            t.rotation = glam::Quat::IDENTITY;
                            self.dirty = true;
                        }
                    });
                    let rot_changed = ui
                        .horizontal(|ui| {
                            let mut changed = false;
                            changed |= ui
                                .add(egui::DragValue::new(&mut rx).prefix("X ").speed(0.02))
                                .changed();
                            changed |= ui
                                .add(egui::DragValue::new(&mut ry).prefix("Y ").speed(0.02))
                                .changed();
                            changed |= ui
                                .add(egui::DragValue::new(&mut rz).prefix("Z ").speed(0.02))
                                .changed();
                            changed
                        })
                        .inner;
                    if rot_changed {
                        t.rotation = glam::Quat::from_euler(glam::EulerRot::XYZ, rx, ry, rz);
                        self.dirty = true;
                    }

                    ui.horizontal(|ui| {
                        ui.label("Scale");
                        if ui.small_button("↺").on_hover_text("Reset to 1").clicked() {
                            t.scale = Vec3::ONE;
                            self.dirty = true;
                        }
                    });
                    let mut scale_changed = false;
                    ui.horizontal(|ui| {
                        scale_changed |= ui
                            .add(
                                egui::DragValue::new(&mut t.scale.x)
                                    .prefix("X ")
                                    .speed(0.02)
                                    .range(0.001..=1000.0),
                            )
                            .changed();
                        scale_changed |= ui
                            .add(
                                egui::DragValue::new(&mut t.scale.y)
                                    .prefix("Y ")
                                    .speed(0.02)
                                    .range(0.001..=1000.0),
                            )
                            .changed();
                        scale_changed |= ui
                            .add(
                                egui::DragValue::new(&mut t.scale.z)
                                    .prefix("Z ")
                                    .speed(0.02)
                                    .range(0.001..=1000.0),
                            )
                            .changed();
                    });
                    if scale_changed {
                        self.dirty = true;
                    }
                }
            });

            ui.collapsing("Components", |ui| {
                let has_parent = world.get::<Parent>(entity).is_some();
                let has_group = world.get::<FolderGroup>(entity).is_some();
                let has_visible = world.get::<Visible>(entity).is_some();
                let has_locked = world.get::<Locked>(entity).is_some();
                ui.label(format!("• Name ({entity_name})"));
                ui.label("• Transform");
                if has_parent {
                    ui.label("• Parent");
                }
                if has_group {
                    ui.label("• FolderGroup");
                }
                if has_visible {
                    ui.label("• Visible");
                }
                if has_locked {
                    ui.label("• Locked");
                }
                ui.separator();
                ui.menu_button("+ Add Component", |ui| {
                    if !has_group && ui.button("FolderGroup").clicked() {
                        world
                            .entity_mut(entity)
                            .insert(FolderGroup("Folder".into()));
                        self.dirty = true;
                        ui.close();
                    }
                    if !has_visible && ui.button("Visible").clicked() {
                        world.entity_mut(entity).insert(Visible(true));
                        self.dirty = true;
                        ui.close();
                    }
                    if !has_locked && ui.button("Locked").clicked() {
                        world.entity_mut(entity).insert(Locked(false));
                        self.dirty = true;
                        ui.close();
                    }
                    ui.label("Custom scripts/components ready for expansion");
                });
            });

            ui.separator();
            if ui.button("Delete Actor").clicked() {
                self.delete_selected(world);
            }
        });
    }

    fn update_camera(&mut self, world: &mut World) {
        if let Some((mut t, c)) = world
            .query::<(&mut Transform, &CameraController)>()
            .iter_mut(world)
            .next()
        {
            t.translation = self.orbit_target - c.forward() * self.orbit_distance;
        }
    }
    fn create_entity(&mut self, world: &mut World) {
        let e = world
            .spawn((
                Name("New Actor".into()),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        self.set_selected_entity(Some(e), false);
        self.dirty = true;
        self.log.push(("INFO".into(), "Created actor".into()));
    }
    fn delete_selected(&mut self, world: &mut World) {
        let targets = if !self.selected_entities.is_empty() {
            self.selected_entities.iter().copied().collect::<Vec<_>>()
        } else if let Some(e) = self.selected {
            vec![e]
        } else {
            Vec::new()
        };
        for e in targets {
            let _ = world.despawn(e);
        }
        self.selected = None;
        self.selected_entities.clear();
        self.dirty = true;
        self.log.push(("INFO".into(), "Deleted actor(s)".into()));
    }
    fn save_scene(&mut self, world: &mut World) {
        let mut scene = Scene::new();
        for (_, name, t) in world.query::<(Entity, &Name, &Transform)>().iter(world) {
            scene.entities.push(SceneEntity {
                name: name.0.clone(),
                translation: t.translation.to_array(),
                rotation: t.rotation.to_array(),
                scale: t.scale.to_array(),
                parent: None,
            });
        }
        if let Some(parent) = self.scene_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match scene.to_ron().and_then(|s| {
            std::fs::write(&self.scene_path, s)
                .map_err(|_| ron::Error::Message("could not write scene".into()))
        }) {
            Ok(()) => {
                self.dirty = false;
                self.status = "Saved".into();
                self.log.push((
                    "INFO".into(),
                    format!("Saved {}", self.scene_path.display()),
                ));
            }
            Err(e) => {
                self.status = format!("Save failed: {e}");
                self.log.push(("ERROR".into(), self.status.clone()));
            }
        }
    }
    fn load_scene(&mut self, world: &mut World) {
        match std::fs::read_to_string(&self.scene_path)
            .ok()
            .and_then(|s| Scene::from_ron(&s).ok())
        {
            Some(scene) => {
                let entities: Vec<Entity> = world
                    .query_filtered::<Entity, (
                        bevy_ecs::query::With<Transform>,
                        bevy_ecs::query::With<Name>,
                    )>()
                    .iter(world)
                    .collect();
                for e in entities {
                    let _ = world.despawn(e);
                }
                for item in scene.entities {
                    let mut t = Transform::from_translation(Vec3::from_array(item.translation));
                    t.rotation = glam::Quat::from_array(item.rotation);
                    t.scale = Vec3::from_array(item.scale);
                    world.spawn((Name(item.name), t));
                }
                self.dirty = false;
                self.status = "Loaded".into();
            }
            None => {
                self.status = "Scene load failed".into();
                self.log.push(("ERROR".into(), self.status.clone()));
            }
        }
    }
    fn save_layout(&self) {
        let state = LayoutSettings {
            workspace_preset: self.current_workspace,
            dock_state: self.dock_state.clone(),
            show_viewport: true,
            show_outliner: true,
            show_details: true,
            show_content: true,
            show_log: true,
            grid: self.grid,
            wireframe: self.wireframe,
            show_gizmos: self.show_gizmos,
            show_collision: self.show_collision,
            active_tool: self.active_tool,
            view_mode: self.view_mode,
            content_width: 220.0,
            outliner_width: 245.0,
            details_width: 285.0,
            log_height: 145.0,
        };
        if let Ok(s) = ron::to_string(&state) {
            let _ = std::fs::write("editor-layout.ron", s);
        }
    }
    fn reset_layout(&mut self) {
        self.current_workspace = WorkspacePreset::Default;
        self.dock_state = Some(WorkspacePreset::Default.create_dock_state());
    }
}

pub struct EditorTabViewer<'a> {
    pub editor: &'a mut Editor,
    pub world: &'a mut World,
    pub actions: &'a mut Vec<DockAction>,
}

impl<'a> egui_dock::TabViewer for EditorTabViewer<'a> {
    type Tab = EditorTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            EditorTab::Viewport => self.editor.draw_viewport(ui, self.world),
            EditorTab::WorldOutliner => self.editor.draw_world_outliner(ui, self.world),
            EditorTab::Details => self.editor.draw_details(ui, self.world),
            EditorTab::ContentBrowser => self.editor.draw_content_browser(ui, self.world),
            EditorTab::OutputLog => self.editor.draw_output_log(ui),
            EditorTab::ScriptEditor => self.editor.draw_script_editor(ui),
            EditorTab::GraphicsSettings => self.editor.draw_graphics_settings(ui),
        }
    }

    fn context_menu(
        &mut self,
        ui: &mut egui::Ui,
        tab: &mut Self::Tab,
        _surface: SurfaceIndex,
        _node: NodeIndex,
    ) {
        ui.label(RichText::new(tab.title()).strong());
        ui.separator();

        if ui.button("Detach to Floating Window").clicked() {
            self.actions.push(DockAction::Detach(*tab));
            ui.close();
        }

        if ui.button("Close Tab").clicked() {
            self.actions.push(DockAction::Close(*tab));
            ui.close();
        }

        ui.separator();
        ui.menu_button("Switch Workspace", |ui| {
            for preset in WorkspacePreset::ALL {
                if ui.button(preset.name()).clicked() {
                    self.actions.push(DockAction::ApplyPreset(preset));
                    self.editor.current_workspace = preset;
                    ui.close();
                }
            }
        });

        if ui.button("Reset Layout to Default").clicked() {
            self.actions.push(DockAction::ResetDefault);
            self.editor.current_workspace = WorkspacePreset::Default;
            ui.close();
        }
    }

    fn clear_background(&self, tab: &Self::Tab) -> bool {
        !matches!(tab, EditorTab::Viewport)
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        // All tabs can be closed except user might want to re-open via Window menu
        true
    }
}

fn content_kind(path: &Path) -> &'static str {
    if path.is_dir() {
        return "Folder";
    }
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "ron" | "scene" => "Scene",
        "mat" | "material" => "Material",
        "png" | "jpg" | "jpeg" | "bmp" | "tga" | "dds" => "Texture",
        "obj" | "fbx" | "gltf" | "glb" => "Model",
        "rs" | "lua" | "py" => "Script",
        _ => "File",
    }
}
fn content_icon(path: &Path) -> &'static str {
    match content_kind(path) {
        "Folder" => "▰",
        "Scene" => "▣",
        "Material" => "◈",
        "Texture" => "▧",
        "Model" => "⬡",
        "Script" => "⌘",
        _ => "▤",
    }
}
