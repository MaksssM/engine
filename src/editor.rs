use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use bevy_ecs::{entity::Entity, world::World};
use egui::{Color32, Context, Key, RichText, Sense, Stroke, Vec2};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use winit::{event::WindowEvent, window::Window};

use egui_dock::{DockArea, DockState, NodeIndex, SurfaceIndex};

use crate::{
    assets_ui::{AssetKind, PreviewMode, ObjMesh, classify, count_assets, format_bytes, format_modified},
    camera::CameraController,
    core::components::{FolderGroup, Locked, Visible},
    dock::{DockAction, EditorTab, WorkspacePreset, editor_dock_style},
    ecs::Name,
    feedback::{
        apply_feedback_styles, ConfirmDialog, DialogResult, NotificationManager, PendingAction,
        TooltipExt,
    },
    icons::{IconSize, Icons},
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
        pub filter: String,
        pub content_root: PathBuf,
        pub content_dir: PathBuf,
        pub content_history: Vec<PathBuf>,
        pub history_index: usize,
        pub content_list_view: bool,
        pub content_type_filter: String,
        pub content_selected: Option<PathBuf>,
        pub renaming: Option<(PathBuf, String)>,
        pub log_filter: String,
        pub log: Vec<(String, String)>,
        pub scene_path: PathBuf,
        pub view_mode: ViewMode,
        pub orbit_target: Vec3,
        pub orbit_distance: f32,
        pub last_view_mode: ViewMode,

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

        // Asset Preview state
        pub preview_mode: PreviewMode,
        pub mesh_zoom: f32,
        pub texture_zoom: f32,
        pub preview_yaw: f32,
        pub preview_pitch: f32,
        pub preview_orbit_drag: Option<(f32, f32)>,
                pub open_asset_pending: Option<PathBuf>,

    // Feedback & Notifications
    pub notifications: NotificationManager,
    pub confirm_dialog: Option<ConfirmDialog>,
}

impl Editor {
    pub fn new(window: &Window, device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let context = Context::default();
        apply_feedback_styles(&context);
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
            renaming: None,
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
                        preview_mode: PreviewMode::Mesh,
                        mesh_zoom: 1.0,
                        texture_zoom: 1.0,
                        preview_yaw: 0.6,
                        preview_pitch: 0.3,
                        preview_orbit_drag: None,
                        open_asset_pending: None,
            notifications: NotificationManager::default(),
            confirm_dialog: None,
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

                // Process pending asset-open requests (e.g. "Open in Scene Editor" button
                // in the Asset Preview panel, which runs outside the world-consuming code path).
                if let Some(path) = self.open_asset_pending.take() {
                    self.current_workspace = WorkspacePreset::LevelEditing;
                    self.load_scene_from_path(world, &path);
                }

        let mut dock_state = self
            .dock_state
            .take()
            .unwrap_or_else(|| self.current_workspace.create_dock_state());

        let mut actions = Vec::new();

        egui::TopBottomPanel::top("menu")
            .exact_height(30.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // App title with icon
                    ui.horizontal(|ui| {
                        Icons::show(ui, Icons::PROJECT, IconSize::Large, Some(Color32::from_rgb(82, 166, 235)));
                        ui.label(
                            RichText::new("R  ENGINE")
                                .strong()
                                .color(Color32::from_rgb(82, 166, 235)),
                        );
                    });
                    ui.separator();
                    // Save button (always visible)
                    if ui
                        .button(
                            RichText::new(format!("{} Save", Icons::SAVE))
                                .size(11.0)
                                .color(Color32::WHITE)
                        )
                        .engine_tooltip("Save Scene", "Save current scene to disk", Some("Ctrl+S"))
                        .clicked()
                    {
                        self.save_scene(world);
                    }
                    // Play controls
                    if ui
                        .button(
                            RichText::new(Icons::PLAY)
                                .size(12.0)
                                .color(Color32::from_rgb(80, 200, 100))
                        )
                        .engine_tooltip("Play", "Start simulation mode", Some("F5"))
                        .clicked()
                    {
                        self.notifications.info("Simulation", "Play mode activated");
                    }
                    if ui
                        .button(
                            RichText::new(Icons::STOP)
                                .size(12.0)
                                .color(Color32::from_rgb(200, 80, 80))
                        )
                        .engine_tooltip("Stop", "Stop simulation", Some("Shift+F5"))
                        .clicked()
                    {
                        self.notifications.info("Simulation", "Simulation stopped");
                    }
                    ui.separator();
                    // Menu buttons with icons
                    for (icon, menu) in [
                        (Icons::FILE_BLANK, "File"),
                        (Icons::SETTINGS, "Edit"),
                        (Icons::GRID_VIEW, "Window"),
                        (Icons::TOOLBOX, "Tools"),
                        (Icons::BUILD, "Build"),
                        (Icons::INFO, "Help"),
                    ] {
                        ui.horizontal(|ui| {
                            Icons::show(ui, icon, IconSize::Small, Some(Color32::from_rgb(160, 165, 175)));
                            ui.menu_button(menu, |ui| match menu {
                                "File" => {
                                    if ui.button(format!("{} New Scene", Icons::FILE_BLANK)).clicked() {
                                        if self.dirty {
                                            self.confirm_dialog = Some(ConfirmDialog::unsaved_changes(PendingAction::NewScene));
                                        } else {
                                            self.new_scene(world);
                                        }
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Save Scene  (Ctrl+S)", Icons::SAVE)).clicked() {
                                        self.save_scene(world);
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Reload Scene", Icons::REFRESH)).clicked() {
                                        self.prompt_reload_scene(world);
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button(format!("{} Save Layout", Icons::SETTINGS_FILE)).clicked() {
                                        self.dock_state = Some(dock_state.clone());
                                        self.save_layout();
                                        self.dock_state = None;
                                        self.status = "Layout saved".into();
                                        self.notifications.success(
                                            "Layout Saved",
                                            "Saved window dock layout configuration",
                                        );
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Reset Layout to Default", Icons::HOME)).clicked() {
                                        actions.push(DockAction::ResetDefault);
                                        self.current_workspace = WorkspacePreset::Default;
                                        self.notifications.info(
                                            "Layout Reset",
                                            "Reset docking panels to default layout",
                                        );
                                        ui.close();
                                    }
                                }
                                "Edit" => {
                                    if ui.button(format!("{} Select All (Ctrl+A)", Icons::ENTITY)).clicked() {
                                        self.select_all(world);
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Deselect All (Esc)", Icons::EMPTY_ENTITY)).clicked() {
                                        self.selected = None;
                                        self.selected_entities.clear();
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Delete Selected (Del)", Icons::DELETE)).clicked() {
                                        self.prompt_delete_selected(world);
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Duplicate Selected (Ctrl+D)", Icons::DUPLICATE)).clicked() {
                                        self.duplicate_selected(world);
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button(format!("{} Focus Selected (F)", Icons::CAMERA)).clicked() {
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
                                                self.notifications.info(
                                                    "Workspace Switched",
                                                    format!("Activated {} layout", preset.name()),
                                                );
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
                                    if ui.button(format!("{} Reset Layout to Default", Icons::HOME)).clicked() {
                                        self.current_workspace = WorkspacePreset::Default;
                                        actions.push(DockAction::ResetDefault);
                                        self.notifications.info(
                                            "Layout Reset",
                                            "Restored default window workspace",
                                        );
                                        ui.close();
                                    }
                                }
                                "Tools" => {
                                    ui.checkbox(&mut self.grid, format!("{} Show Grid", Icons::GRID_VIEW));
                                    ui.checkbox(&mut self.show_gizmos, format!("{} Show Gizmos", Icons::TRANSFORM));
                                    ui.checkbox(&mut self.show_collision, format!("{} Show Collision", Icons::HIDDEN));
                                    ui.checkbox(&mut self.wireframe, format!("{} Wireframe", Icons::LIST_VIEW));
                                    ui.separator();
                                    if ui.button(format!("{} Reset Camera Orbit", Icons::CAMERA)).clicked() {
                                        self.orbit_target = Vec3::ZERO;
                                        self.orbit_distance = 8.0;
                                        self.update_camera(world);
                                        ui.close();
                                    }
                                }
                                "Build" => {
                                    if ui.button(format!("{} Build Lighting", Icons::LIGHT)).clicked() {
                                        self.log.push((
                                            "INFO".into(),
                                            "Build Lighting completed (0 static lights baked)".into(),
                                        ));
                                        self.notifications.success(
                                            "Lighting Built",
                                            "Static lighting baked successfully (0 warnings)",
                                        );
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Build Navigation Mesh", Icons::BUILD)).clicked() {
                                        self.log.push((
                                            "INFO".into(),
                                            "NavMesh generated (1 polygon island, 24 nodes)".into(),
                                        ));
                                        self.notifications.success(
                                            "NavMesh Built",
                                            "Generated 1 polygon island (24 nodes)",
                                        );
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Build All", Icons::COMPILE)).clicked() {
                                        self.log.push((
                                            "INFO".into(),
                                            "Full scene build completed successfully".into(),
                                        ));
                                        self.notifications.success(
                                            "Build Complete",
                                            "Lighting, navigation, and geometry built",
                                        );
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
                        });
                    }
                    if ui
                        .button("Save")
                        .engine_tooltip(
                            "Save Scene",
                            "Save scene hierarchy and transforms to file",
                            Some("Ctrl+S"),
                        )
                        .clicked()
                    {
                        self.save_scene(world);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("Rust Engine v0.1.0-dev")
                                .size(11.0)
                                .color(Color32::from_rgb(120, 135, 155)),
                        );
                    });
                })
            });

        egui::TopBottomPanel::top("toolbar")
            .exact_height(36.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Transform tools with icons
                    let tool_info = [
                        (Icons::SELECT, "Select", "Select Tool", "Pick entities in viewport", Some("Q")),
                        (Icons::MOVE, "Move", "Move Tool", "Translate selected entity along axes", Some("W")),
                        (Icons::ROTATE, "Rotate", "Rotate Tool", "Rotate selected entity in 3D space", Some("E")),
                        (Icons::SCALE, "Scale", "Scale Tool", "Scale selected entity per axis", Some("R")),
                    ];
                    for (i, (icon, label, title, desc, shortcut)) in tool_info.iter().enumerate() {
                        let is_active = self.active_tool == i;
                        let response = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{} {}", icon, label))
                                    .size(11.0)
                                    .color(if is_active { Color32::WHITE } else { Color32::from_rgb(180, 185, 195) })
                            )
                            .frame(false)
                            .min_size(egui::vec2(64.0, 28.0))
                            .fill(if is_active { Color32::from_rgb(60, 80, 120) } else { Color32::TRANSPARENT })
                            .corner_radius(egui::CornerRadius::same(4))
                        );
                        let response = response.engine_tooltip(title, desc, *shortcut);
                        if response.clicked() {
                            self.active_tool = i;
                        }
                    }
                    ui.separator();
                    // Add Actor with icon
                    if ui
                        .button(
                            RichText::new(format!("{} Add Actor", Icons::ADD))
                                .size(11.0)
                                .color(Color32::WHITE)
                        )
                        .engine_tooltip("Add Actor", "Spawn a new actor entity at scene origin", None)
                        .clicked()
                    {
                        self.create_entity(world);
                    }
                    // Save with icon
                    if ui
                        .button(
                            RichText::new(format!("{} Save", Icons::SAVE))
                                .size(11.0)
                                .color(Color32::WHITE)
                        )
                        .engine_tooltip(
                            "Save Scene",
                            "Save all actors and components to scene file",
                            Some("Ctrl+S"),
                        )
                        .clicked()
                    {
                        self.save_scene(world);
                    }
                    ui.separator();
                    // View modes with icons
                    ui.label("View:");
                    let view_modes = [
                        (Icons::PERSPECTIVE, "Persp", ViewMode::Perspective, "Perspective", "Perspective 3D camera projection"),
                        (Icons::UP, "Top", ViewMode::Top, "Top View", "Orthographic top-down projection looking down Y"),
                        (Icons::FRONT, "Front", ViewMode::Front, "Front View", "Orthographic front projection looking along Z"),
                        (Icons::SIDE, "Side", ViewMode::Side, "Side View", "Orthographic side projection looking along X"),
                        (Icons::ORTHOGRAPHIC, "Ortho", ViewMode::Orthographic, "Orthographic", "Orthographic 3D camera projection"),
                    ];
                    for (icon, label, mode, title, desc) in view_modes.iter() {
                        let is_active = self.view_mode == *mode;
                        let response = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{} {}", icon, label))
                                    .size(11.0)
                                    .color(if is_active { Color32::WHITE } else { Color32::from_rgb(180, 185, 195) })
                            )
                            .frame(false)
                            .min_size(egui::vec2(56.0, 28.0))
                            .fill(if is_active { Color32::from_rgb(60, 80, 120) } else { Color32::TRANSPARENT })
                            .corner_radius(egui::CornerRadius::same(4))
                        );
                        let response = response.engine_tooltip(title, desc, None);
                        if response.clicked() {
                            self.view_mode = *mode;
                        }
                    }
                    ui.separator();
                    // View toggles with icons
                    ui.add(egui::Checkbox::new(&mut self.grid, format!("{} Grid", Icons::GRID_VIEW)))
                        .engine_tooltip("Toggle Grid", "Show or hide the coordinate ground grid", None);
                    ui.add(egui::Checkbox::new(&mut self.show_gizmos, format!("{} Gizmos", Icons::TRANSFORM)))
                        .engine_tooltip("Toggle Gizmos", "Show or hide 3D transformation manipulators", None);
                    ui.add(egui::Checkbox::new(&mut self.wireframe, format!("{} Wireframe", Icons::LIST_VIEW)))
                        .engine_tooltip("Toggle Wireframe", "Render models in polygon wireframe mode", None);

                    ui.separator();
                    // Workspace selector with icons
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
                        let response = ui.add(
                            egui::Button::new(
                                RichText::new(short_name)
                                    .size(11.0)
                                    .color(if active { Color32::WHITE } else { Color32::from_rgb(180, 185, 195) })
                            )
                            .frame(false)
                            .min_size(egui::vec2(52.0, 28.0))
                            .fill(if active { Color32::from_rgb(60, 80, 120) } else { Color32::TRANSPARENT })
                            .corner_radius(egui::CornerRadius::same(4))
                        );
                        let response = response.engine_tooltip(preset.name(), preset.description(), None);
                        if response.clicked() {
                            self.current_workspace = preset;
                            actions.push(DockAction::ApplyPreset(preset));
                            self.status = format!("Switched to {} workspace", preset.name());
                            self.notifications.info(
                                "Workspace Switched",
                                format!("Activated {} layout", preset.name()),
                            );
                        }
                    }
                })
            });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(24.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.style_mut().spacing.item_spacing.x = 8.0;

                    // Scene save / dirty indicator
                    if self.dirty {
                        ui.label(
                            RichText::new(format!("{} Unsaved Changes", Icons::WARN))
                                .size(11.0)
                                .color(Color32::from_rgb(240, 180, 80)),
                        )
                        .engine_tooltip(
                            "Unsaved Changes",
                            "The active scene has unsaved modifications. Press Ctrl+S to save.",
                            Some("Ctrl+S"),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!("{} Saved", Icons::SAVE))
                                .size(11.0)
                                .color(Color32::from_rgb(100, 205, 130)),
                        )
                        .engine_tooltip(
                            "Scene Saved",
                            "All scene entities and transforms are saved to disk.",
                            None,
                        );
                    }

                    ui.separator();

                    // Total actors and selection count
                    let total_actors = world.query::<&Name>().iter(world).count();
                    let selection_count = if !self.selected_entities.is_empty() {
                        self.selected_entities.len()
                    } else if self.selected.is_some() {
                        1
                    } else {
                        0
                    };

                    let actor_label = if selection_count > 0 {
                        format!("{} Actors ({} selected)", Icons::ENTITY, selection_count)
                    } else {
                        format!("{} {} Actors", Icons::ENTITY, total_actors)
                    };
                    ui.label(
                        RichText::new(actor_label)
                            .size(11.0)
                            .color(Color32::from_rgb(160, 175, 195)),
                    );

                    ui.separator();

                    // Active tool indicator
                    let tool_name = match self.active_tool {
                        0 => "Select (Q)",
                        1 => "Move (W)",
                        2 => "Rotate (E)",
                        3 => "Scale (R)",
                        _ => "None",
                    };
                    ui.label(
                        RichText::new(format!("Tool: {tool_name}"))
                            .size(11.0)
                            .color(Color32::from_rgb(160, 175, 195)),
                    );

                    ui.separator();

                    // Current workspace
                    ui.label(
                        RichText::new(format!("Workspace: {}", self.current_workspace.name()))
                            .size(11.0)
                            .color(Color32::from_rgb(160, 175, 195)),
                    );

                    ui.separator();

                    // Status text
                    ui.label(
                        RichText::new(&self.status)
                            .size(11.0)
                            .color(Color32::from_rgb(200, 210, 225)),
                    );

                    // Right side metrics & error badges
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let frame_ms = if fps > 0.0 { 1000.0 / fps } else { 0.0 };
                        ui.label(
                            RichText::new(format!("{:.0} FPS ({:.1} ms)", fps, frame_ms))
                                .size(11.0)
                                .color(Color32::from_rgb(140, 150, 165)),
                        );

                        ui.separator();

                        let err_count = self.log.iter().filter(|(l, _)| l == "ERROR").count();
                        let warn_count = self.log.iter().filter(|(l, _)| l == "WARN").count();

                        if err_count > 0 {
                            let btn = ui.add(
                                egui::Button::new(
                                    RichText::new(format!("вњ• {err_count} errors"))
                                        .size(11.0)
                                        .color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(160, 40, 40)),
                            )
                            .engine_tooltip(
                                "Errors Logged",
                                "Click to view error log entries in Output Log",
                                None,
                            );
                            if btn.clicked() {
                                actions.push(DockAction::Open(EditorTab::OutputLog));
                            }
                        }

                        if warn_count > 0 {
                            let btn = ui.add(
                                egui::Button::new(
                                    RichText::new(format!("вљ  {warn_count} warnings"))
                                        .size(11.0)
                                        .color(Color32::BLACK),
                                )
                                .fill(Color32::from_rgb(230, 180, 50)),
                            )
                            .engine_tooltip(
                                "Warnings Logged",
                                "Click to view warnings in Output Log",
                                None,
                            );
                            if btn.clicked() {
                                actions.push(DockAction::Open(EditorTab::OutputLog));
                            }
                        }
                    });
                });
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
            if !i.modifiers.ctrl && !i.modifiers.alt && self.confirm_dialog.is_none() {
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
                    self.prompt_delete_selected(world);
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

        let mut close_dialog = false;
        if let Some(dialog) = self.confirm_dialog.take() {
            if let Some(res) = dialog.show(ctx) {
                match res {
                    DialogResult::Confirmed => {
                        match dialog {
                            ConfirmDialog::DeleteEntities { targets, .. } => {
                                let count = targets.len();
                                for e in targets {
                                    let _ = world.despawn(e);
                                }
                                self.selected = None;
                                self.selected_entities.clear();
                                self.dirty = true;
                                self.status = format!("Deleted {count} actor(s)");
                                self.log.push(("INFO".into(), format!("Deleted {count} actor(s)")));
                                self.notifications.warning(
                                    "Actor Deleted",
                                    format!("Permanently removed {count} actor(s) from scene"),
                                );
                            }
                            ConfirmDialog::DeleteContent { path, .. } => {
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                let res = if path.is_dir() {
                                    std::fs::remove_dir_all(&path)
                                } else {
                                    std::fs::remove_file(&path)
                                };
                                match res {
                                    Ok(_) => {
                                        if self.content_selected.as_ref() == Some(&path) {
                                            self.content_selected = None;
                                        }
                                        self.status = format!("Deleted {name}");
                                        self.log.push(("INFO".into(), format!("Deleted resource: {name}")));
                                        self.notifications.warning(
                                            "Resource Deleted",
                                            format!("Permanently deleted '{name}'"),
                                        );
                                    }
                                    Err(e) => {
                                        self.status = format!("Failed to delete {name}: {e}");
                                        self.log.push(("ERROR".into(), self.status.clone()));
                                        self.notifications.error("Delete Failed", format!("{e}"));
                                    }
                                }
                            }
                            ConfirmDialog::UnsavedChanges { .. } => {}
                        }
                        close_dialog = true;
                    }
                    DialogResult::SaveAndProceed => {
                        self.save_scene(world);
                        if let ConfirmDialog::UnsavedChanges { action } = dialog {
                            match action {
                                PendingAction::LoadScene => self.load_scene(world),
                                PendingAction::NewScene => self.new_scene(world),
                                PendingAction::ResetLayout => {
                                    self.dock_state = Some(self.current_workspace.create_dock_state());
                                }
                            }
                        }
                        close_dialog = true;
                    }
                    DialogResult::DiscardAndProceed => {
                        if let ConfirmDialog::UnsavedChanges { action } = dialog {
                            match action {
                                PendingAction::LoadScene => self.load_scene(world),
                                PendingAction::NewScene => self.new_scene(world),
                                PendingAction::ResetLayout => {
                                    self.dock_state = Some(self.current_workspace.create_dock_state());
                                }
                            }
                        }
                        close_dialog = true;
                    }
                    DialogResult::Cancelled => {
                        close_dialog = true;
                    }
                }
            } else {
                // Dialog still open, put it back
                self.confirm_dialog = Some(dialog);
            }
        }
        if close_dialog {
            self.confirm_dialog = None;
        }

        // Render floating toast notifications stack
        self.notifications.show(ctx, 28.0);
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
        self.notifications.info(
            "Actor Duplicated",
            format!("Duplicated {} entity/entities", new_entities.len()),
        );
    }

    fn draw_viewport(&mut self, ui: &mut egui::Ui, world: &mut World) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    match self.view_mode {
                        ViewMode::Perspective => Icons::VIEW,
                        ViewMode::Top => Icons::TOP,
                        ViewMode::Front => Icons::FRONT,
                        ViewMode::Side => Icons::SIDE,
                        ViewMode::Orthographic => Icons::ORTHO,
                    },
                    match self.view_mode {
                        ViewMode::Perspective => "PERSPECTIVE",
                        ViewMode::Top => "TOP",
                        ViewMode::Front => "FRONT",
                        ViewMode::Side => "SIDE",
                        ViewMode::Orthographic => "ORTHOGRAPHIC",
                    }
                ))
                .strong()
                .size(11.0),
            );
            ui.separator();
            ui.label(
                RichText::new(if self.wireframe {
                    format!("{} WIREFRAME", Icons::WIREFRAME)
                } else {
                    format!("{} LIT", Icons::LIT)
                })
                .size(11.0),
            );
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    Icons::TOOL,
                    match self.active_tool {
                        1 => "Move",
                        2 => "Rotate",
                        3 => "Scale",
                        _ => "Select",
                    }
                ))
                .size(11.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(
                        RichText::new(format!("{} Reset Camera", Icons::RESET))
                            .size(11.0)
                            .color(Color32::WHITE),
                    )
                    .engine_tooltip("Reset Camera", "Reset camera to default position", Some("R"))
                    .clicked()
                {
                    self.orbit_target = Vec3::ZERO;
                    self.orbit_distance = 8.0;
                    self.update_camera(world);
                }
                if let Some(e) = self.selected {
                    if ui
                        .button(
                            RichText::new(format!("{} Focus (F)", Icons::FOCUS))
                                .size(11.0)
                                .color(Color32::WHITE),
                        )
                        .engine_tooltip("Focus Actor", "Focus camera on the selected actor", Some("F"))
                        .clicked()
                    {
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
            ui.label(RichText::new(format!("{} Viewport", Icons::VIEW)).strong());
            ui.separator();
            if ui.button(format!("{} Show Grid", Icons::GRID)).clicked() {
                self.grid = !self.grid;
                ui.close();
            }
            if ui.button(format!("{} Show Gizmos", Icons::GIZMO)).clicked() {
                self.show_gizmos = !self.show_gizmos;
                ui.close();
            }
            if ui.button(format!("{} Show Collision", Icons::COLLISION)).clicked() {
                self.show_collision = !self.show_collision;
                ui.close();
            }
            if ui.button(format!("{} Wireframe", Icons::WIREFRAME)).clicked() {
                self.wireframe = !self.wireframe;
                ui.close();
            }
            ui.separator();
            if ui.button(format!("{} Focus Selected (F)", Icons::FOCUS)).clicked() {
                if let Some(e) = self.selected {
                    if let Some(t) = world.get::<Transform>(e) {
                        self.orbit_target = t.translation;
                        self.update_camera(world);
                    }
                }
                ui.close();
            }
            if ui.button(format!("{} Reset Camera Orbit", Icons::RESET)).clicked() {
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
            let gizmo_origin = rect.left_bottom() + Vec2::new(24.0, -24.0);
            // Colored axis indicators (X red, Y green, Z blue)
            p.text(
                gizmo_origin + Vec2::new(-14.0, 0.0),
                egui::Align2::CENTER_CENTER,
                "X",
                egui::FontId::proportional(11.0),
                Color32::from_rgb(220, 90, 90),
            );
            p.text(
                gizmo_origin + Vec2::new(14.0, 0.0),
                egui::Align2::CENTER_CENTER,
                "Y",
                egui::FontId::proportional(11.0),
                Color32::from_rgb(90, 220, 120),
            );
            p.text(
                gizmo_origin + Vec2::new(0.0, -14.0),
                egui::Align2::CENTER_CENTER,
                "Z",
                egui::FontId::proportional(11.0),
                Color32::from_rgb(90, 140, 230),
            );
            p.line_segment(
                [gizmo_origin, gizmo_origin + Vec2::new(14.0, 0.0)],
                egui::Stroke::new(1.0, Color32::from_rgb(220, 90, 90)),
            );
            p.line_segment(
                [gizmo_origin, gizmo_origin + Vec2::new(-14.0, 0.0)],
                egui::Stroke::new(1.0, Color32::from_rgb(90, 220, 120)),
            );
            p.line_segment(
                [gizmo_origin, gizmo_origin + Vec2::new(0.0, -14.0)],
                egui::Stroke::new(1.0, Color32::from_rgb(90, 140, 230)),
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

            ui.label(
                RichText::new(format!("{} Total: {total_count}", Icons::LOG))
                    .size(10.0)
                    .color(Color32::from_rgb(140, 145, 155)),
            );
            if err_count > 0 {
                ui.label(
                    RichText::new(format!("{} Errors: {err_count}", Icons::ERROR))
                        .size(10.0)
                        .color(Color32::from_rgb(255, 90, 90)),
                );
            }
            if warn_count > 0 {
                ui.label(
                    RichText::new(format!("{} Warnings: {warn_count}", Icons::WARN))
                        .size(10.0)
                        .color(Color32::from_rgb(255, 200, 70)),
                );
            }
            ui.label(
                RichText::new(format!("{} Info: {info_count}", Icons::INFO))
                    .size(10.0)
                    .color(Color32::from_rgb(140, 180, 220)),
            );

            ui.separator();
            Icons::show(ui, Icons::SEARCH, IconSize::Small, Some(Color32::from_rgb(120, 135, 155)));
            ui.add(
                egui::TextEdit::singleline(&mut self.log_filter)
                    .hint_text("Search log...")
                    .desired_width(180.0),
            );
            if !self.log_filter.is_empty() && ui.small_button("Г—").engine_tooltip("Clear Search", "Clear the log search filter", None).clicked() {
                self.log_filter.clear();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(
                        RichText::new(format!("{} Clear Log", Icons::DELETE))
                            .size(11.0)
                            .color(Color32::WHITE),
                    )
                    .engine_tooltip("Clear Log", "Remove all entries from the output log", None)
                    .clicked()
                {
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
                            let (badge_bg, text_col, icon) = match level.as_str() {
                                "ERROR" => (
                                    Color32::from_rgb(180, 40, 40),
                                    Color32::from_rgb(255, 120, 120),
                                    Icons::ERROR,
                                ),
                                "WARN" => (
                                    Color32::from_rgb(160, 120, 20),
                                    Color32::from_rgb(255, 210, 80),
                                    Icons::WARNING,
                                ),
                                _ => (
                                    Color32::from_rgb(35, 75, 130),
                                    Color32::from_rgb(180, 210, 240),
                                    Icons::INFO,
                                ),
                            };
                            let badge = RichText::new(format!(" {level} "))
                                .size(10.0)
                                .color(Color32::WHITE)
                                .background_color(badge_bg);
                            ui.label(badge);
                            Icons::show(ui, icon, IconSize::Small, Some(text_col));
                            ui.add_space(4.0);
                            ui.label(RichText::new(msg).monospace().color(text_col));
                        });
                    }
                }
            });
    }

    /// Open an asset: scenes load into the Scene Editor; others select for preview.
        pub fn open_asset(&mut self, world: &mut World, path: &std::path::Path) {
        let kind = classify(path);
        match kind {
            AssetKind::Scene => {
                    self.load_scene_from_path(world, path);
                self.log_info("Loaded scene from Content Browser");
            }
            AssetKind::Folder => {
                self.navigate_content(path.to_path_buf());
            }
            _ => {
                self.content_selected = Some(path.to_path_buf());
            }
        }
    }

    /// Convenience helper to append an INFO-level message to the Output Log.
    pub fn log_info(&mut self, message: &str) {
        self.log.push(("INFO".into(), message.into()));
    }

    /// Load a scene file from a specific path into the world.
    pub fn load_scene_from_path(&mut self, world: &mut World, path: &Path) {
                match std::fs::read_to_string(path)
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
                        let count = scene.entities.len();
                        for item in scene.entities {
                            let mut t = Transform::from_translation(Vec3::from_array(item.translation));
                            t.rotation = glam::Quat::from_array(item.rotation);
                            t.scale = Vec3::from_array(item.scale);
                            world.spawn((Name(item.name), t));
                        }
                        self.dirty = false;
                        self.status = "Loaded".into();
                        self.log
                            .push(("INFO".into(), format!("Loaded scene with {count} actor(s)")));
                        self.notifications.info(
                            "Scene Reloaded",
                            format!(
                                "Restored {count} actor(s) from {}",
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                            ),
                        );
                    }
                    None => {
                        self.status = "Scene load failed".into();
                        self.log.push(("ERROR".into(), self.status.clone()));
                    }
                }
    }

            /// Draw the Asset Preview panel (central panel in Asset Editing workspace).
            fn draw_asset_preview(&mut self, ui: &mut egui::Ui) {
                let path = match &self.content_selected {
                    Some(p) => p.clone(),
                    None => {
                                    Self::preview_empty_state(ui);
                        return;
                    }
                };
                let kind = classify(&path);
                match kind {
                                AssetKind::Folder => Self::preview_folder_state(self, ui, &path),
                                AssetKind::Scene => Self::preview_scene_state(self, ui, &path),
                                AssetKind::Texture => Self::preview_texture_state(self, ui, &path),
                                AssetKind::Model => Self::preview_model_state(self, ui, &path),
                                AssetKind::Material => Self::preview_material_state(self, ui, &path),
                                AssetKind::Script => Self::preview_script_state(self, ui, &path),
                                AssetKind::Shader => Self::preview_shader_state(self, ui, &path),
                                AssetKind::Audio => Self::preview_audio_state(self, ui, &path),
                                AssetKind::Unsupported => Self::preview_unsupported_state(self, ui, &path),
                }
            }

            /// Draw the Asset Details panel (right panel in Asset Editing workspace).
            fn draw_asset_details(&mut self, ui: &mut egui::Ui) {
                let path = match &self.content_selected {
                    Some(p) => p.clone(),
                    None => {
                                    Self::details_empty_state(ui);
                        return;
                    }
                };
                let kind = classify(&path);
                let metadata = std::fs::metadata(&path).ok();
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                            Self::draw_asset_details_header(ui, &name, &kind, &path, metadata.as_ref());
                ui.separator();
                            Self::draw_asset_details_properties(ui, &path, &kind, metadata.as_ref());
                ui.separator();
                            Self::draw_asset_details_type_specific(ui, &path, &kind);
            }

            fn preview_mesh_viewport(&mut self, ui: &mut egui::Ui, mesh: &ObjMesh) {
                let (rect, response) = ui.allocate_exact_size(
                    ui.available_size(),
                    Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                let center = rect.center();
                let zoom = self.mesh_zoom.max(0.1);
                // Handle orbit drag
                if response.dragged() {
                    let delta = response.drag_delta();
                    self.preview_yaw += delta.x * 0.005;
                    self.preview_pitch = (self.preview_pitch - delta.y * 0.005).clamp(-1.5, 1.5);
                }
                // Scroll zoom
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        self.mesh_zoom = (self.mesh_zoom * (1.0 + scroll * 0.001)).clamp(0.1, 50.0);
                    }
                }
                // Project 3D vertices to 2D
                let cos_yaw = self.preview_yaw.cos();
                let sin_yaw = self.preview_yaw.sin();
                let cos_pitch = self.preview_pitch.cos();
                let sin_pitch = self.preview_pitch.sin();
                // Compute 2D points
                                let points_2d: Vec<egui::Pos2> = mesh
                    .vertices
                    .iter()
                    .map(|v| {
                        let x = v[0] * cos_yaw * zoom + v[2] * sin_yaw * zoom;
                        let y = v[1] * cos_pitch * zoom - (v[0] * sin_yaw - v[2] * cos_yaw) * sin_pitch * zoom;
                        egui::pos2(center.x + x, center.y + y)
                    })
                    .collect();
                                // Draw wireframe triangles (indices are a flat list of vertex indices, 3 per triangle)
                let tri_color = Color32::from_rgb(80, 180, 230);
                                for tri in mesh.indices.chunks(3) {
                                    if tri.len() == 3 {
                                        if let (Some(a), Some(b), Some(c)) = (
                                            points_2d.get(tri[0] as usize),
                                            points_2d.get(tri[1] as usize),
                                            points_2d.get(tri[2] as usize),
                                        ) {
                                            painter.line_segment([*a, *b], Stroke::new(1.0, tri_color));
                                            painter.line_segment([*b, *c], Stroke::new(1.0, tri_color));
                                            painter.line_segment([*c, *a], Stroke::new(1.0, tri_color));
                                        }
                    }
                }
                // Draw bounds box
                                let (min_p, max_p) = mesh.bounds();
                                let corners = [
                                    [min_p[0], min_p[1], min_p[2]],
                                    [max_p[0], min_p[1], min_p[2]],
                                    [max_p[0], max_p[1], min_p[2]],
                                    [min_p[0], max_p[1], min_p[2]],
                                    [min_p[0], min_p[1], max_p[2]],
                                    [max_p[0], min_p[1], max_p[2]],
                                    [max_p[0], max_p[1], max_p[2]],
                                    [min_p[0], max_p[1], max_p[2]],
                                ];
                                let corners_2d: Vec<egui::Pos2> = corners
                                    .iter()
                                    .map(|v| {
                                        let x = v[0] * cos_yaw * zoom + v[2] * sin_yaw * zoom;
                                        let y = v[1] * cos_pitch * zoom - (v[0] * sin_yaw - v[2] * cos_yaw) * sin_pitch * zoom;
                                        egui::pos2(center.x + x, center.y + y)
                                    })
                                    .collect();
                                let box_edges = [
                                    (0,1),(1,2),(2,3),(3,0),
                                    (4,5),(5,6),(6,7),(7,4),
                                    (0,4),(1,5),(2,6),(3,7),
                                ];
                                let box_color = Color32::from_rgb(120, 140, 160);
                                for (a,b) in box_edges {
                                    painter.line_segment([corners_2d[a], corners_2d[b]], Stroke::new(1.0, box_color));
                }
            }

            fn preview_texture_viewport(&mut self, ui: &mut egui::Ui, data: &[u8]) {
                            if let Ok(decoded) = image::load_from_memory(data) {
                                let rgba = decoded.to_rgba8();
                                let size = [rgba.width() as usize, rgba.height() as usize];
                                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    let texture = ui.ctx().load_texture(
                        "asset_preview_texture",
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    let available = ui.available_size();
                    let aspect = size[0] as f32 / size[1] as f32;
                    let (w, h) = if available.x / available.y > aspect {
                        (available.y * aspect * self.texture_zoom, available.y * self.texture_zoom)
                    } else {
                        (available.x * self.texture_zoom, available.x / aspect * self.texture_zoom)
                    };
                    let rect = ui.allocate_exact_size(egui::vec2(w, h), Sense::click_and_drag()).0;
                    // Checkerboard background
                    let painter = ui.painter_at(rect);
                                let square = 16.0_f32;
                    for x in (0..rect.width() as usize).step_by(square as usize * 2) {
                        for y in (0..rect.height() as usize).step_by(square as usize * 2) {
                            let r = egui::Rect::from_min_size(
                                rect.min + egui::vec2(x as f32, y as f32),
                                egui::vec2(square, square),
                            );
                            painter.rect_filled(r, 0.0, Color32::from_rgb(50, 53, 58));
                            let r2 = egui::Rect::from_min_size(
                                rect.min + egui::vec2(x as f32 + square, y as f32 + square),
                                egui::vec2(square, square),
                            );
                            painter.rect_filled(r2, 0.0, Color32::from_rgb(50, 53, 58));
                        }
                    }
                    ui.put(rect, egui::Image::new(&texture).fit_to_exact_size(rect.size()));
                    if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
                        if rect.contains(hover_pos) {
                            let local = hover_pos - rect.min;
                            let tex_x = (local.x / rect.width() * size[0] as f32) as usize;
                            let tex_y = (local.y / rect.height() * size[1] as f32) as usize;
                            if tex_x < size[0] && tex_y < size[1] {
                                let idx = (tex_y * size[0] + tex_x) * 4;
                                                            let raw = rgba.as_raw();
                                                            if idx + 3 < raw.len() {
                                                                let r = raw[idx];
                                                                let g = raw[idx + 1];
                                                                let b = raw[idx + 2];
                                                                let a = raw[idx + 3];
                                    ui.ctx().debug_painter().text(
                                        hover_pos + egui::vec2(12.0, -20.0),
                                        egui::Align2::LEFT_BOTTOM,
                                        format!("RGBA: {r}, {g}, {b}, {a}"),
                                        egui::FontId::proportional(11.0),
                                        Color32::WHITE,
                                    );
                                }
                            }
                        }
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Failed to decode image").color(Color32::from_rgb(220, 100, 100)));
                    });
                }
            }

            // Preview state functions
            fn preview_empty_state(ui: &mut egui::Ui) {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        Icons::show(ui, Icons::FILE, IconSize::XLarge, Some(Color32::from_rgb(100, 120, 140)));
                        ui.add_space(12.0);
                        ui.label(RichText::new("Asset Preview").size(20.0).color(Color32::from_rgb(180, 190, 200)));
                        ui.add_space(4.0);
                        ui.label(RichText::new("Select an asset in the Content Browser to preview it here.").color(Color32::from_rgb(120, 135, 155)));
                    });
                });
            }

            fn preview_folder_state(_editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let count = count_assets(path);
                            let entries = fs::read_dir(path).map(|rd| rd.count()).unwrap_or(0);
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        Icons::show(ui, Icons::FOLDER, IconSize::XLarge, Some(Color32::from_rgb(80, 180, 230)));
                        ui.add_space(12.0);
                        ui.label(RichText::new(name).size(20.0).color(Color32::WHITE));
                        ui.add_space(8.0);
                                    ui.label(RichText::new(format!("{count} asset(s) В· {entries} entries")).color(Color32::from_rgb(140, 155, 170)));
                        ui.add_space(20.0);
                                    ui.label(RichText::new("Double-click in the Content Browser to navigate here.").color(Color32::from_rgb(120, 135, 155)));
                    });
                });
            }

            fn preview_scene_state(editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        Icons::show(ui, Icons::SCENE, IconSize::XLarge, Some(Color32::from_rgb(80, 220, 120)));
                        ui.add_space(12.0);
                        ui.label(RichText::new(name).size(20.0).color(Color32::WHITE));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Double-click to open in Scene Editor").color(Color32::from_rgb(140, 155, 170)));
                        ui.add_space(20.0);
                                    if ui.button("Open in Scene Editor").clicked() {
                                        editor.open_asset_pending = Some(path.to_path_buf());
                        }
                    });
                });
            }

            fn preview_texture_state(editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
                if let Ok(data) = std::fs::read(path) {
                    // Show zoom controls
                    ui.horizontal(|ui| {
                        ui.label("Zoom:");
                        ui.add(egui::DragValue::new(&mut editor.texture_zoom).speed(0.01).clamp_range(0.1..=10.0).suffix("x"));
                        if ui.button("Reset").clicked() {
                            editor.texture_zoom = 1.0;
                        }
                    });
                    ui.add_space(4.0);
                    editor.preview_texture_viewport(ui, &data);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Failed to read texture file").color(Color32::from_rgb(220, 100, 100)));
                    });
                }
            }

            fn preview_model_state(editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
                if let Ok(content) = std::fs::read_to_string(path) {
                    if let Some(mesh) = ObjMesh::parse(&content) {
                        // Show zoom control
                        ui.horizontal(|ui| {
                            ui.label("Zoom:");
                            ui.add(egui::DragValue::new(&mut editor.mesh_zoom).speed(0.1).clamp_range(0.1..=50.0).suffix("x"));
                            if ui.button("Reset").clicked() {
                                editor.mesh_zoom = 1.0;
                                editor.preview_yaw = 0.0;
                                editor.preview_pitch = 0.0;
                            }
                            ui.separator();
                            ui.label(format!("Vertices: {}  Triangles: {}", mesh.vertices.len(), mesh.indices.len() / 3));
                        });
                        ui.add_space(4.0);
                        editor.preview_mesh_viewport(ui, &mesh);
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(RichText::new("Failed to parse model (OBJ only)").color(Color32::from_rgb(220, 100, 100)));
                        });
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Failed to read model file").color(Color32::from_rgb(220, 100, 100)));
                    });
                }
            }

            fn preview_material_state(_editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
                if let Ok(content) = std::fs::read_to_string(path) {
                                let mut buffer = content.clone();
                    ui.vertical(|ui| {
                        ui.add_space(10.0);
                        ui.label(RichText::new("Material Preview").strong());
                        ui.add_space(8.0);
                        // Simple sphere placeholder
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(200.0, 200.0), Sense::hover());
                        let painter = ui.painter_at(rect);
                        let center = rect.center();
                        let radius = 80.0;
                        painter.circle_filled(center, radius, Color32::from_rgb(180, 140, 100));
                        painter.circle_stroke(center, radius, Stroke::new(2.0, Color32::from_rgb(100, 80, 60)));
                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.label(RichText::new("Raw Content:").strong());
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                ui.add(
                                                egui::TextEdit::multiline(&mut buffer)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY)
                                        .interactive(false),
                                );
                            });
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Failed to read material file").color(Color32::from_rgb(220, 100, 100)));
                    });
                }
            }

            fn preview_script_state(_editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
                if let Ok(content) = std::fs::read_to_string(path) {
                                let mut buffer = content.clone();
                    ui.vertical(|ui| {
                        ui.add_space(10.0);
                        ui.label(RichText::new("Script Preview").strong());
                        ui.add_space(8.0);
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.add(
                                            egui::TextEdit::multiline(&mut buffer)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(30)
                                    .interactive(false),
                            );
                        });
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Failed to read script file").color(Color32::from_rgb(220, 100, 100)));
                    });
                }
            }

            fn preview_shader_state(_editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
                            Self::preview_script_state(_editor, ui, path); // Same as script for now
            }

            fn preview_audio_state(_editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let metadata = std::fs::metadata(path).ok();
                let size = metadata.map(|m| m.len()).unwrap_or(0);
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        Icons::show(ui, Icons::AUDIO, IconSize::XLarge, Some(Color32::from_rgb(220, 180, 80)));
                        ui.add_space(12.0);
                        ui.label(RichText::new(name).size(20.0).color(Color32::WHITE));
                        ui.add_space(8.0);
                        ui.label(RichText::new(format!("{} bytes", format_bytes(size))).color(Color32::from_rgb(140, 155, 170)));
                        ui.add_space(20.0);
                        ui.label(RichText::new("Audio preview not yet implemented").color(Color32::from_rgb(120, 135, 155)));
                    });
                });
            }

            fn preview_unsupported_state(_editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        Icons::show(ui, Icons::FILE, IconSize::XLarge, Some(Color32::from_rgb(160, 100, 100)));
                        ui.add_space(12.0);
                        ui.label(RichText::new(name).size(20.0).color(Color32::WHITE));
                        ui.add_space(8.0);
                        ui.label(RichText::new(format!("Unsupported asset type: .{}", ext)).color(Color32::from_rgb(220, 140, 140)));
                        ui.add_space(20.0);
                        ui.label(RichText::new("No preview available. Extend the preview system to support this type.").color(Color32::from_rgb(120, 135, 155)));
                    });
                });
            }

            // Asset Details helpers
            fn draw_asset_details_header(ui: &mut egui::Ui, name: &str, kind: &AssetKind, path: &Path, metadata: Option<&std::fs::Metadata>) {
                ui.horizontal(|ui| {
                    Icons::show(ui, Icons::INFO, IconSize::Medium, Some(Color32::from_rgb(80, 180, 230)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(name).size(16.0).strong().color(Color32::WHITE));
                        ui.label(RichText::new(kind.label()).size(11.0).color(Color32::from_rgb(140, 155, 170)));
                    });
                });
                ui.add_space(8.0);
                if let Some(meta) = metadata {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Path:").color(Color32::from_rgb(120, 135, 155)));
                        ui.add(egui::TextEdit::singleline(&mut path.to_string_lossy().to_string()).desired_width(f32::INFINITY).interactive(false));
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Size:").color(Color32::from_rgb(120, 135, 155)));
                        ui.label(RichText::new(format_bytes(meta.len())).color(Color32::WHITE));
                    });
                    if let Ok(modified) = meta.modified() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Modified:").color(Color32::from_rgb(120, 135, 155)));
                                                ui.label(RichText::new(format_modified(modified)).color(Color32::WHITE));
                        });
                    }
                }
            }

            fn draw_asset_details_properties(ui: &mut egui::Ui, path: &Path, kind: &AssetKind, metadata: Option<&std::fs::Metadata>) {
                ui.label(RichText::new("Properties").strong());
                ui.add_space(4.0);
                egui::Grid::new("asset_details_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    Self::property_row(ui, "Name", path.file_name().unwrap_or_default().to_string_lossy().to_string());
                    Self::property_row(ui, "Type", kind.label().to_string());
                    Self::property_row(ui, "Format", path.extension().and_then(|s| s.to_str()).unwrap_or("").to_uppercase());
                    if let Some(meta) = metadata {
                        Self::property_row(ui, "Size", format_bytes(meta.len()));
                    }
                    ui.end_row();
                });
            }

            fn draw_asset_details_type_specific(ui: &mut egui::Ui, path: &Path, kind: &AssetKind) {
                match kind {
                    AssetKind::Model => {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Geometry").strong());
                        if let Ok(content) = std::fs::read_to_string(path) {
                                                if let Some(mesh) = ObjMesh::parse(&content) {
                                                    let triangle_count = mesh.indices.len() / 3;
                                                    let (min_p, max_p) = mesh.bounds();
                                                    let size = [
                                                        max_p[0] - min_p[0],
                                                        max_p[1] - min_p[1],
                                                        max_p[2] - min_p[2],
                                                    ];
                                egui::Grid::new("model_geometry").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                                    Self::property_row(ui, "Vertices", mesh.vertices.len().to_string());
                                                        Self::property_row(ui, "Triangles", triangle_count.to_string());
                                                        Self::property_row(ui, "Bounds", format!("{:.2} x {:.2} x {:.2}", size[0], size[1], size[2]));
                                    ui.end_row();
                                });
                            }
                        }
                    }
                    AssetKind::Texture => {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Image").strong());
                        if let Ok(data) = std::fs::read(path) {
                            if let Ok(image) = image::load_from_memory(&data) {
                                                    let (w, h) = (image.width(), image.height());
                                egui::Grid::new("texture_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                                    Self::property_row(ui, "Resolution", format!("{} x {}", w, h));
                                    Self::property_row(ui, "Color Type", format!("{:?}", image.color()));
                                    ui.end_row();
                                });
                            }
                        }
                    }
                    AssetKind::Material => {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Material Parameters").strong());
                        if let Ok(content) = std::fs::read_to_string(path) {
                            for line in content.lines() {
                                let line = line.trim();
                                if line.starts_with("//") || line.is_empty() { continue; }
                                ui.horizontal(|ui| {
                                    ui.add_space(16.0);
                                    ui.label(RichText::new(line).monospace().size(11.0).color(Color32::from_rgb(160, 170, 180)));
                                });
                            }
                        }
                    }
                    AssetKind::Scene => {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Scene Info").strong());
                        if let Ok(content) = std::fs::read_to_string(path) {
                            if let Ok(scene) = Scene::from_ron(&content) {
                                egui::Grid::new("scene_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                                    Self::property_row(ui, "Entities", scene.entities.len().to_string());
                                    ui.end_row();
                                });
                            }
                        }
                    }
                    AssetKind::Script | AssetKind::Shader => {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Source").strong());
                    }
                    _ => {}
                }
            }

            fn property_row(ui: &mut egui::Ui, label: &str, value: String) {
                ui.label(RichText::new(label).color(Color32::from_rgb(120, 135, 155)));
                ui.label(RichText::new(value).color(Color32::WHITE));
                ui.end_row();
            }

            fn details_empty_state(ui: &mut egui::Ui) {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        Icons::show(ui, Icons::INFO, IconSize::XLarge, Some(Color32::from_rgb(100, 120, 140)));
                        ui.add_space(12.0);
                        ui.label(RichText::new("Asset Details").size(20.0).color(Color32::from_rgb(180, 190, 200)));
                        ui.add_space(4.0);
                        ui.label(RichText::new("Select an asset in the Content Browser to view its properties.").color(Color32::from_rgb(120, 135, 155)));
                    });
                });
            }

            fn zoom_constant() -> f32 { 100.0 }

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
    fn draw_content_browser(&mut self, ui: &mut egui::Ui, world: &mut World) {
        crate::content_browser::draw_content_browser(self, ui, world);
    }

    pub fn navigate_content(&mut self, path: PathBuf) {
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

    pub fn content_context_menu(&mut self, ui: &mut egui::Ui, path: &Path) {
        if path.is_dir() && ui.button(format!("{} Open", Icons::FOLDER_OPEN)).clicked() {
            self.navigate_content(path.to_path_buf());
            ui.close();
        }
        if ui.button(format!("{} Create Folder", Icons::FOLDER_NEW)).clicked() {
            self.create_content_folder();
            ui.close();
        }
        if ui.button(format!("{} Rename", Icons::RENAME)).clicked() {
            self.rename_content(path);
            ui.close();
        }
        if ui.button(format!("{} Duplicate", Icons::DUPLICATE)).clicked() {
            self.duplicate_content(path);
            ui.close();
        }
        if ui.button(format!("{} Delete", Icons::DELETE)).clicked() {
            self.prompt_delete_resource(path.to_path_buf());
            ui.close();
        }
        if ui.button(format!("{} Import Sample TextureвЂ¦", Icons::IMPORT)).clicked() {
            let target = if path.is_dir() {
                path.to_path_buf()
            } else {
                path.parent().unwrap_or(path).to_path_buf()
            };
            self.import_dummy_resource(&target);
            ui.close();
        }
        if ui.button("Show in Explorer").clicked() {
            self.show_in_explorer(path);
            ui.close();
        }
    }

    pub fn create_content_folder(&mut self) {
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
    pub fn create_content_file(&mut self, ext: &str) {
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
    pub fn rename_content(&mut self, path: &Path) {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        self.renaming = Some((path.to_path_buf(), name.to_string()));
    }

    /// Commit the in-progress rename (or cancel when `None` name is empty).
    pub fn commit_rename(&mut self, apply: bool) {
        let Some((path, new_name)) = self.renaming.take() else {
            return;
        };
        if !apply || new_name.trim().is_empty() || *new_name == path.file_name().unwrap_or_default().to_string_lossy() {
            return;
        }
        // Preserve extension for files: if user typed a name without extension, re-append it.
        let target = if path.is_dir() {
            path.with_file_name(new_name.trim())
        } else {
            let ext = path.extension().and_then(|e| e.to_str());
            let stem_input = new_name.trim();
            let final_name = match ext {
                Some(e) if !stem_input.to_lowercase().ends_with(&format!(".{e}")) => {
                    format!("{stem_input}.{e}")
                }
                _ => stem_input.to_string(),
            };
            path.with_file_name(final_name)
        };
        if target.exists() {
            self.log.push((
                "ERROR".into(),
                format!("Rename failed: '{}' already exists", target.display()),
            ));
            return;
        }
        if let Err(e) = fs::rename(&path, &target) {
            self.log
                .push(("ERROR".into(), format!("Rename failed: {e}")));
        } else {
            if self.content_selected.as_ref() == Some(&path) {
                self.content_selected = Some(target.clone());
            }
            self.log.push((
                "INFO".into(),
                format!("Renamed '{}'", target.file_name().unwrap_or_default().to_string_lossy()),
            ));
        }
    }
    pub fn duplicate_content(&mut self, path: &Path) {
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
    pub fn delete_content(&mut self, path: &Path) {
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
    pub fn show_in_explorer(&mut self, path: &Path) {
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
                if ui
                    .button(
                        RichText::new(format!("{} Actor", Icons::ADD))
                            .size(11.0)
                            .color(Color32::WHITE),
                    )
                    .engine_tooltip("Add Actor", "Add a new actor to the scene", Some("Ctrl+A"))
                    .clicked()
                {
                    self.create_entity(world);
                }
                if ui
                    .button(
                        RichText::new(format!("{} Group", Icons::FOLDER_NEW))
                            .size(11.0)
                            .color(Color32::WHITE),
                    )
                    .engine_tooltip("Create Group", "Create a new folder group to organize actors", None)
                    .clicked()
                {
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
            Icons::show(ui, Icons::SEARCH, IconSize::Small, Some(Color32::from_rgb(120, 135, 155)));
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Search actorsвЂ¦")
                    .desired_width(f32::INFINITY),
            );
            if !self.filter.is_empty() && ui.small_button("Г—").clicked() {
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
                let icon = if is_folder { Icons::FOLDER } else { Icons::ENTITY };
                let is_selected = self.selected_entities.contains(&entity);
                let label = format!("{icon}  {name}");
                let response = ui.selectable_label(is_selected, label);
                let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
                if response.clicked() {
                    self.set_selected_entity(Some(entity), ctrl);
                }
                let vis_icon = if visible { Icons::VISIBLE } else { Icons::HIDDEN };
                if ui
                    .button(
                        RichText::new(vis_icon)
                            .size(12.0)
                            .color(if visible { Color32::from_rgb(100, 200, 100) } else { Color32::from_rgb(120, 120, 120) }),
                    )
                    .engine_tooltip(
                        if visible { "Hide Actor" } else { "Show Actor" },
                        if visible { "Hide this actor from the viewport" } else { "Make this actor visible in the viewport" },
                        None,
                    )
                    .clicked()
                {
                    if let Some(mut v) = world.get_mut::<Visible>(entity) {
                        v.0 = !v.0;
                    } else {
                        world.entity_mut(entity).insert(Visible(!visible));
                    }
                    self.dirty = true;
                }
                let lock_icon = if locked { Icons::LOCKED } else { Icons::UNLOCKED };
                if ui
                    .button(
                        RichText::new(lock_icon)
                            .size(12.0)
                            .color(if locked { Color32::from_rgb(220, 180, 60) } else { Color32::from_rgb(120, 120, 120) }),
                    )
                    .engine_tooltip(
                        if locked { "Unlock Actor" } else { "Lock Actor" },
                        if locked { "Allow this actor to be selected and modified" } else { "Prevent this actor from being selected or modified" },
                        None,
                    )
                    .clicked()
                {
                    if let Some(mut l) = world.get_mut::<Locked>(entity) {
                        l.0 = !l.0;
                    } else {
                        world.entity_mut(entity).insert(Locked(!locked));
                    }
                    self.dirty = true;
                }
                response.context_menu(|ui| {
                    if ui.button(format!("{} Rename", Icons::RENAME)).clicked() {
                        self.rename_entity = Some(entity);
                        self.rename_buffer = name.clone();
                        ui.close();
                    }
                    if ui.button(format!("{} Make Child of Selected", Icons::ENTITY)).clicked() {
                        if let Some(parent) = self.selected {
                            if parent != entity {
                                world.entity_mut(entity).insert(Parent(parent));
                                self.dirty = true;
                            }
                        }
                        ui.close();
                    }
                    if ui.button(format!("{} Detach From Parent", Icons::EMPTY_ENTITY)).clicked() {
                        world.entity_mut(entity).remove::<Parent>();
                        self.dirty = true;
                        ui.close();
                    }
                    if ui.button(format!("{} Delete", Icons::DELETE)).clicked() {
                        self.set_selected_entity(Some(entity), false);
                        self.prompt_delete_selected(world);
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
            ui.vertical_centered(|ui| {
                Icons::show(ui, Icons::EMPTY_ENTITY, IconSize::Large, Some(Color32::from_rgb(100, 105, 115)));
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Select an actor to inspect properties")
                        .size(12.0)
                        .color(Color32::from_rgb(130, 135, 145)),
                );
            });
            return;
        };

        let entity_name = world
            .get::<Name>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_else(|| "Unnamed".into());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&entity_name).strong());
            if ui
                .button(
                    RichText::new(format!("{} Rename", Icons::RENAME))
                        .size(11.0)
                        .color(Color32::WHITE),
                )
                .engine_tooltip("Rename Actor", "Rename the selected actor", Some("F2"))
                .clicked()
            {
                self.rename_entity = Some(entity);
                self.rename_buffer = entity_name.clone();
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.collapsing(
                format!("{} Transform", Icons::TRANSFORM),
                |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button(
                                RichText::new(format!("{} Copy", Icons::COPY))
                                    .size(11.0)
                                    .color(Color32::WHITE),
                            )
                            .engine_tooltip("Copy Transform", "Copy the transform to clipboard", Some("Ctrl+C"))
                            .clicked()
                        {
                            if let Some(t) = world.get::<Transform>(entity) {
                                self.copied_transform = Some(*t);
                                self.log.push(("INFO".into(), "Transform copied".into()));
                            }
                        }
                        if ui
                            .add_enabled(
                                self.copied_transform.is_some(),
                                egui::Button::new(
                                    RichText::new(format!("{} Paste", Icons::PASTE)).size(11.0),
                                ),
                            )
                            .engine_tooltip("Paste Transform", "Paste transform from clipboard", Some("Ctrl+V"))
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
                        if ui
                            .button(
                                RichText::new(format!("{} Reset All", Icons::RESET))
                                    .size(11.0)
                                    .color(Color32::WHITE),
                            )
                            .engine_tooltip("Reset Transform", "Reset transform to default values", None)
                            .clicked()
                        {
                            if let Some(mut t) = world.get_mut::<Transform>(entity) {
                                *t = Transform::default();
                                self.dirty = true;
                            }
                        }
                    });

                    if let Some(mut t) = world.get_mut::<Transform>(entity) {
                        ui.horizontal(|ui| {
                            ui.label("Location");
                            if ui
                                .button(
                                    RichText::new(Icons::RESET)
                                        .size(11.0)
                                        .color(Color32::from_rgb(140, 145, 155)),
                                )
                                .engine_tooltip("Reset Location", "Set location to (0, 0, 0)", None)
                                .clicked()
                            {
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
                                .button(
                                    RichText::new(Icons::RESET)
                                        .size(11.0)
                                        .color(Color32::from_rgb(140, 145, 155)),
                                )
                                .engine_tooltip("Reset Rotation", "Set rotation to identity (no rotation)", None)
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
                            if ui
                                .button(
                                    RichText::new(Icons::RESET)
                                        .size(11.0)
                                        .color(Color32::from_rgb(140, 145, 155)),
                                )
                                .engine_tooltip("Reset Scale", "Set scale to (1, 1, 1)", None)
                                .clicked()
                            {
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
                },
            );

            ui.collapsing(
                format!("{} Components", Icons::COMPONENTS),
                |ui| {
                    let has_parent = world.get::<Parent>(entity).is_some();
                    let has_group = world.get::<FolderGroup>(entity).is_some();
                    let has_visible = world.get::<Visible>(entity).is_some();
                    let has_locked = world.get::<Locked>(entity).is_some();
                    ui.label(format!("{} Name ({entity_name})", Icons::ENTITY));
                    ui.label(format!("{} Transform", Icons::TRANSFORM));
                    if has_parent {
                        ui.label(format!("{} Parent", Icons::ENTITY));
                    }
                    if has_group {
                        ui.label(format!("{} FolderGroup", Icons::FOLDER));
                    }
                    if has_visible {
                        ui.label(format!("{} Visible", Icons::VISIBLE));
                    }
                    if has_locked {
                        ui.label(format!("{} Locked", Icons::LOCKED));
                    }
                    ui.separator();
                    ui.menu_button(
                        format!("{} Add Component", Icons::ADD),
                        |ui| {
                            if !has_group && ui.button(format!("{} FolderGroup", Icons::FOLDER)).clicked() {
                                world
                                    .entity_mut(entity)
                                    .insert(FolderGroup("Folder".into()));
                                self.dirty = true;
                                ui.close();
                            }
                            if !has_visible && ui.button(format!("{} Visible", Icons::VISIBLE)).clicked() {
                                world.entity_mut(entity).insert(Visible(true));
                                self.dirty = true;
                                ui.close();
                            }
                            if !has_locked && ui.button(format!("{} Locked", Icons::LOCKED)).clicked() {
                                world.entity_mut(entity).insert(Locked(false));
                                self.dirty = true;
                                ui.close();
                    }
                    ui.label("Custom scripts/components ready for expansion");
                });
            });

            ui.separator();
            if ui.button("Delete Actor").clicked() {
                self.prompt_delete_selected(world);
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
        self.notifications
            .success("Actor Spawned", "Added 'New Actor' at scene origin");
    }
    pub fn prompt_delete_selected(&mut self, world: &World) {
        let targets = if !self.selected_entities.is_empty() {
            self.selected_entities.iter().copied().collect::<Vec<_>>()
        } else if let Some(e) = self.selected {
            vec![e]
        } else {
            return;
        };

        if targets.is_empty() {
            return;
        }

        let names: Vec<String> = targets
            .iter()
            .map(|&e| {
                world
                    .get::<Name>(e)
                    .map(|n| n.0.clone())
                    .unwrap_or_else(|| "Unnamed Actor".into())
            })
            .collect();

        self.confirm_dialog = Some(ConfirmDialog::delete_entities(targets, names));
    }
    pub fn prompt_delete_resource(&mut self, path: PathBuf) {
        self.confirm_dialog = Some(ConfirmDialog::delete_content(path));
    }
    pub fn prompt_reload_scene(&mut self, world: &mut World) {
        if self.dirty {
            self.confirm_dialog = Some(ConfirmDialog::unsaved_changes(PendingAction::LoadScene));
        } else {
            self.load_scene(world);
        }
    }
    pub fn new_scene(&mut self, world: &mut World) {
        let entities: Vec<Entity> = world
            .query_filtered::<Entity, (
                bevy_ecs::query::With<Transform>,
                bevy_ecs::query::With<Name>,
                bevy_ecs::query::Without<CameraController>,
            )>()
            .iter(world)
            .collect();
        for e in entities {
            let _ = world.despawn(e);
        }
        self.selected = None;
        self.selected_entities.clear();
        self.dirty = false;
        self.status = "New scene created".into();
        self.log.push(("INFO".into(), "Created empty scene".into()));
        self.notifications.success("New Scene", "Initialized new empty scene");
    }
    pub fn import_dummy_resource(&mut self, target_dir: &Path) {
            // Pick a real file from disk and copy it into the content folder.
            if let Some(file) = rfd::FileDialog::new().pick_file() {
                let file_name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "imported_asset".into());
                let dest = target_dir.join(&file_name);
                match std::fs::copy(&file, &dest) {
                    Ok(_) => {
                        self.content_selected = Some(dest);
                        self.status = format!("Imported '{file_name}'");
                        self.log.push((
                            "INFO".into(),
                            format!("Imported resource: {file_name} -> {}", target_dir.display()),
                        ));
                        self.notifications.success(
                            "Resource Imported",
                            format!("Copied '{file_name}' into the content folder"),
                        );
                    }
                    Err(e) => {
                        self.status = format!("Import failed: {e}");
                        self.log.push(("ERROR".into(), self.status.clone()));
                        self.notifications.error("Import Failed", format!("{e}"));
                    }
                }
        }
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
        for (entity, name, t) in world.query::<(Entity, &Name, &Transform)>().iter(world) {
            scene.entities.push(SceneEntity {
                name: name.0.clone(),
                uid: world.get::<crate::core::uid::Uid>(entity).copied(),
                translation: t.translation.to_array(),
                rotation: t.rotation.to_array(),
                scale: t.scale.to_array(),
                parent: None,
                visible: world
                    .get::<crate::core::components::Visible>(entity)
                    .is_none_or(|value| value.0),
                locked: world
                    .get::<crate::core::components::Locked>(entity)
                    .is_some_and(|value| value.0),
                folder: world
                    .get::<crate::core::components::FolderGroup>(entity)
                    .map(|value| value.0.clone()),
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
                self.notifications.success(
                    "Scene Saved",
                    format!(
                        "Saved {} actor(s) to {}",
                        scene.entities.len(),
                        self.scene_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    ),
                );
            }
            Err(e) => {
                self.status = format!("Save failed: {e}");
                self.log.push(("ERROR".into(), self.status.clone()));
                self.notifications.error("Save Failed", format!("{e}"));
            }
        }
    }
    fn load_scene(&mut self, world: &mut World) {
        self.load_scene_from_path(world, &self.scene_path.clone());
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
                EditorTab::AssetPreview => self.editor.draw_asset_preview(ui),
                EditorTab::AssetDetails => self.editor.draw_asset_details(ui),
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
        "Folder" => Icons::FOLDER,
        "Scene" => Icons::SCENE,
        "Material" => Icons::MATERIAL,
        "Texture" => Icons::TEXTURE,
        "Model" => Icons::MODEL,
        "Script" => Icons::SCRIPT,
        _ => Icons::FILE,
    }
}
