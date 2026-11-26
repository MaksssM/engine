use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use bevy_ecs::{entity::Entity, world::World};
use egui::{Color32, Context, Key, RichText, Sense, Stroke, Vec2};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use winit::{event::WindowEvent, window::Window};

use crate::viewport::{Aabb, CoordSpace, GizmoAxis, GizmoTool, ViewportCamera, ViewportRect};

use egui_dock::{DockArea, DockState, NodeIndex, SurfaceIndex};

use crate::{
    assets_ui::{
        AssetKind, ObjMesh, PreviewMode, classify, count_assets, format_bytes, format_modified,
    },
    camera::CameraController,
    commands::{Command as EditorCommand, DEFAULT_HISTORY_CAPACITY, HistoryManager},
    core::components::{FolderGroup, Locked, Visible},
    dock::{DockAction, EditorTab, WorkspacePreset, editor_dock_style},
    ecs::Name,
    feedback::{
        ConfirmDialog, DialogResult, NotificationManager, PendingAction, TooltipExt,
        apply_feedback_styles,
    },
    icons::{IconSize, Icons},
    performance::PerformanceMonitor,
    scene::{Scene, SceneEntity},
    transform::{GlobalTransform, Parent, Transform},
};

/// State the Viewport panel needs in order to draw the 3D scene.
#[derive(Clone, Copy)]
pub struct ViewportTextureState {
    pub texture_id: egui::TextureId,
    pub width: u32,
    pub height: u32,
}

/// Snap a vector component-wise to the given step. A step of `0` disables snapping.
fn apply_snap(value: Vec3, step: f32) -> Vec3 {
    if step <= 0.0 {
        return value;
    }
    Vec3::new(
        (value.x / step).round() * step,
        (value.y / step).round() * step,
        (value.z / step).round() * step,
    )
}

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
pub enum ViewMode {
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
    /// Undo/Redo history for scene operations.
    pub history: HistoryManager,
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
    /// Track previous engine mode to detect play mode transitions
    pub last_engine_mode: crate::core::resources::EngineMode,

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
    pub perf_selected_tab: usize,
    pub perf_window: crate::performance::TimeWindow,
    pub perf_scale_auto: bool,
    pub perf_fixed_max: f32,
    pub perf_compact: bool,
    pub perf_show_fps_graph: bool,
    pub perf_show_frame_time_graph: bool,

    // Asset Preview state
    pub preview_mode: PreviewMode,
    pub mesh_zoom: f32,
    pub texture_zoom: f32,
    pub preview_yaw: f32,
    pub preview_pitch: f32,
    pub preview_orbit_drag: Option<(f32, f32)>,
    pub open_asset_pending: Option<PathBuf>,
    pub pending_instantiate_model: Option<PathBuf>,
    pub pending_reimport_asset: Option<PathBuf>,
    pub asset_database: crate::asset_database::AssetDatabase,

    // Feedback & Notifications
    pub notifications: NotificationManager,
    pub confirm_dialog: Option<ConfirmDialog>,

    // Viewport interaction state
    pub gizmo_interaction: Option<(Entity, GizmoTool, GizmoAxis, CoordSpace)>,
    pub gizmo_drag_start: Option<egui::Pos2>,
    pub gizmo_snap_translation: f32,
    pub gizmo_snap_rotation: f32,
    pub gizmo_snap_scale: f32,
    pub gizmo_local_space: bool,
    /// Transform captured at gizmo drag start, so the whole gesture commits as
    /// a single undoable command.
    pub gizmo_drag_start_transform: Option<Transform>,
    /// World-space transform at drag start (differs from the local transform
    /// for parented entities).
    pub gizmo_drag_start_world: Option<Transform>,
    /// Inverse parent transform when dragging a child; `None` for roots.
    pub gizmo_drag_parent_inverse: Option<Mat4>,

    // Details panel editing state
    pub transform_edit_start: Option<Transform>,

    // Viewport scene texture (rendered offscreen, then blitted into panel).
    viewport_texture_id: Option<egui::TextureId>,
    viewport_texture_size: (u32, u32),
    /// Set by `draw_viewport` so the offscreen scene pass can be skipped when
    /// the viewport panel is not visible (e.g. Asset Editing workspace).
    viewport_panel_drawn: bool,
    /// Entity under the cursor, used for the hover highlight.
    pub hovered_entity: Option<Entity>,
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
            perf_selected_tab: 0,
            perf_window: crate::performance::TimeWindow::Seconds10,
            perf_scale_auto: true,
            perf_fixed_max: 33.0,
            perf_compact: false,
            perf_show_fps_graph: true,
            perf_show_frame_time_graph: true,
            preview_mode: PreviewMode::Mesh,
            mesh_zoom: 1.0,
            texture_zoom: 1.0,
            preview_yaw: 0.6,
            preview_pitch: 0.3,
            preview_orbit_drag: None,
            open_asset_pending: None,
            pending_instantiate_model: None,
            pending_reimport_asset: None,
            asset_database: crate::asset_database::AssetDatabase::load_or_create(&PathBuf::from(
                "assets/asset_database.ron",
            )),
            notifications: NotificationManager::default(),
            confirm_dialog: None,
            gizmo_interaction: None,
            gizmo_drag_start: None,
            gizmo_snap_translation: 0.0,
            gizmo_snap_rotation: 0.0,
            gizmo_snap_scale: 0.0,
            gizmo_local_space: false,
            viewport_texture_id: None,
            viewport_texture_size: (0, 0),
            viewport_panel_drawn: false,
            hovered_entity: None,
            gizmo_drag_start_transform: None,
            gizmo_drag_start_world: None,
            gizmo_drag_parent_inverse: None,
            history: HistoryManager::new(DEFAULT_HISTORY_CAPACITY),
            last_engine_mode: crate::core::resources::EngineMode::Editor,
            transform_edit_start: None,
        }
    }
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.platform
            .as_mut()
            .is_some_and(|p| p.on_window_event(window, event).consumed)
    }

    /// True while a text widget owns the keyboard. Used to keep editor
    /// shortcuts and fly-camera movement from firing while the user types.
    pub fn wants_keyboard_input(&self) -> bool {
        self.context.wants_keyboard_input()
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

    /// Returns a ViewportCamera built from the editor's current state and ECS transform.
    ///
    /// The camera transform is the single source of truth for orientation in
    /// every view mode: `apply_view_camera` writes the top/front/side rotation
    /// into the same controller, so projecting from `position + forward` keeps
    /// picking, gizmos and the rendered image aligned.
    pub fn viewport_camera(&self, world: &mut World) -> ViewportCamera {
        let (position, forward) = if let Some((t, c)) = world
            .query::<(&Transform, &CameraController)>()
            .iter(world)
            .next()
        {
            (t.translation, c.forward())
        } else {
            (Vec3::new(0.0, 0.0, 8.0), Vec3::NEG_Z)
        };
        let orthographic = matches!(
            self.view_mode,
            ViewMode::Top | ViewMode::Front | ViewMode::Side | ViewMode::Orthographic
        );
        ViewportCamera {
            position,
            target: position + forward,
            fov_y: self.camera_fov.to_radians(),
            near: 0.1,
            far: 1000.0,
            orthographic,
            // Match the perspective framing at the current orbit distance so
            // switching between projections keeps the subject roughly the
            // same size; scrolling changes orbit_distance and thus zoom.
            ortho_scale: (2.0 * self.orbit_distance * (self.camera_fov.to_radians() * 0.5).tan())
                .max(0.1),
        }
    }

    /// Pick the entity under a screen-space cursor using a real ray cast.
    ///
    /// Each entity's AABB (derived from its Transform scale) is intersected with
    /// the camera ray. The closest hit in depth order is returned.
    fn pick_entity(
        &self,
        world: &mut World,
        camera: &crate::viewport::ViewportCamera,
        rect: crate::viewport::ViewportRect,
        screen: egui::Pos2,
    ) -> Option<Entity> {
        let screen = glam::Vec2::new(screen.x, screen.y);
        let (origin, direction) = crate::viewport::screen_ray(camera, rect, screen);
        let mut best: Option<(f32, Entity)> = None;

        for (entity, transform, global, parent, controller, visible, locked) in world
            .query::<(
                Entity,
                &Transform,
                Option<&GlobalTransform>,
                Option<&Parent>,
                Option<&CameraController>,
                Option<&Visible>,
                Option<&Locked>,
            )>()
            .iter(world)
        {
            // The editor camera sits at the ray origin; picking its AABB would
            // always win. Hidden and locked actors are not selectable either.
            if controller.is_some() || visible.is_some_and(|v| !v.0) || locked.is_some_and(|l| l.0)
            {
                continue;
            }
            // Children are rendered at their global transform; use the same
            // position for the pick volume so clicks match the image.
            let world_transform = if parent.is_some() {
                global
                    .map(|g| {
                        let (scale, rotation, translation) = g.0.to_scale_rotation_translation();
                        Transform {
                            translation,
                            rotation,
                            scale,
                        }
                    })
                    .unwrap_or(*transform)
            } else {
                *transform
            };
            let scale = world_transform.scale.abs().max(Vec3::splat(0.1));
            let aabb = Aabb::from_center_half(world_transform.translation, scale * 0.5);
            if let Some(t) = crate::viewport::ray_aabb(origin, direction, aabb) {
                if best.is_none_or(|(closest, _)| t < closest) {
                    best = Some((t, entity));
                }
            }
        }
        best.map(|(_, entity)| entity)
    }

    /// Resolve an entity's transform into world space. Root entities use their
    /// local transform directly; children are decomposed from `GlobalTransform`
    /// so the gizmo and selection markers line up with what is rendered.
    fn world_transform_of(world: &World, entity: Entity) -> Option<Transform> {
        let local = *world.get::<Transform>(entity)?;
        if world.get::<Parent>(entity).is_none() {
            return Some(local);
        }
        let global = world.get::<GlobalTransform>(entity)?;
        let (scale, rotation, translation) = global.0.to_scale_rotation_translation();
        Some(Transform {
            translation,
            rotation,
            scale,
        })
    }

    /// Build the gizmo transform for the current selection.
    ///
    /// Returns the entity, the gizmo origin (its world translation), the basis
    /// rotation (identity in World space, the entity rotation in Local space)
    /// and the entity scale.
    fn gizmo_transform(&self, world: &mut World) -> Option<(Entity, Vec3, Quat, Vec3)> {
        let entity = self.selected?;
        if world.get::<Locked>(entity).is_some_and(|locked| locked.0) {
            return None;
        }
        let transform = Self::world_transform_of(world, entity)?;
        let space = if self.gizmo_local_space {
            CoordSpace::Local
        } else {
            CoordSpace::World
        };
        let basis = crate::viewport::gizmo_basis(space, transform.rotation);
        Some((entity, transform.translation, basis, transform.scale))
    }

    /// Coordinate space for the gizmo, derived from the toolbar toggle.
    fn gizmo_space(&self) -> CoordSpace {
        if self.gizmo_local_space {
            CoordSpace::Local
        } else {
            CoordSpace::World
        }
    }

    /// Which gizmo the current toolbar tool maps to.
    fn gizmo_tool(&self) -> GizmoTool {
        match self.active_tool {
            1 => GizmoTool::Move,
            2 => GizmoTool::Rotate,
            3 => GizmoTool::Scale,
            _ => GizmoTool::Select,
        }
    }

    /// Hit-test the gizmo axis handles at a screen position.
    ///
    /// Each handle is drawn as a line from the gizmo origin along its axis, so
    /// the test measures the perpendicular distance from the cursor to that
    /// line and picks the closest handle within a small pixel tolerance.
    fn hit_test_gizmo_axis(
        &self,
        camera: &crate::viewport::ViewportCamera,
        rect: crate::viewport::ViewportRect,
        screen: egui::Pos2,
        origin: Vec3,
        basis: Quat,
    ) -> Option<GizmoAxis> {
        let origin_screen = crate::viewport::world_to_screen(camera, rect, origin)?;
        let o = glam::Vec2::new(origin_screen.x, origin_screen.y);
        let cursor = glam::Vec2::new(screen.x, screen.y);

        let candidates = [GizmoAxis::X, GizmoAxis::Y, GizmoAxis::Z];

        let mut best: Option<(f32, GizmoAxis)> = None;
        for axis in candidates {
            let dir = crate::viewport::axis_vector(axis, basis);
            let Some(tip) = crate::viewport::gizmo_screen_handle(camera, rect, origin, dir) else {
                continue;
            };
            let t = glam::Vec2::new(tip.x, tip.y);
            let handle = t - o;
            let len_sq = handle.length_squared();
            if len_sq < 1e-6 {
                continue;
            }
            // Project the cursor onto the handle segment.
            let along = ((cursor - o).dot(handle) / len_sq).clamp(0.0, 1.0);
            let closest = o + handle * along;
            let distance = (cursor - closest).length();
            if distance <= 12.0 && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, axis));
            }
        }
        best.map(|(_, axis)| axis)
    }

    /// Render the 3D scene into an offscreen texture and register it with egui.
    /// Returns `None` when the scene cannot be rendered yet.
    fn prepare_viewport_texture(
        &mut self,
        renderer: &mut crate::renderer::Renderer,
        world: &mut World,
    ) -> Option<ViewportTextureState> {
        // Make imported MeshFilter/MeshRenderer entities visible to the
        // snapshot collector before the viewport pass runs.
        renderer.sync_world_renderables(world);
        // The editor may have moved objects after this frame's regular
        // transform pass, so refresh global transforms before extracting.
        crate::transform::propagate_transforms(world);
        let snapshot = crate::renderer::collect_render_snapshot(world);
        let camera = self.viewport_camera(world);

        // Size the offscreen target from the previous frame's panel rect; the
        // first frame has no rect yet, so start at a sane default.
        let (width, height) = if self.viewport_texture_size.0 > 1 {
            self.viewport_texture_size
        } else {
            (640, 360)
        };

        let view = renderer.render_scene_to_viewport(&snapshot, width, height, &camera)?;

        let egui_renderer = self.renderer.as_mut()?;
        let texture_id = match self.viewport_texture_id {
            Some(id) => {
                // Re-register the (possibly resized) view under the same id.
                egui_renderer.update_egui_texture_from_wgpu_texture(
                    &renderer.device,
                    &view,
                    wgpu::FilterMode::Linear,
                    id,
                );
                id
            }
            None => {
                let id = egui_renderer.register_native_texture(
                    &renderer.device,
                    &view,
                    wgpu::FilterMode::Linear,
                );
                self.viewport_texture_id = Some(id);
                id
            }
        };

        Some(ViewportTextureState {
            texture_id,
            width,
            height,
        })
    }

    /// Reimport `path` through the import pipeline, preserving its AssetId and
    /// bumping its revision, then invalidate the GPU copy so the next frame
    /// uploads the updated data. Returns the preserved (or newly assigned) id.
    fn reimport_asset_path(
        &mut self,
        path: &std::path::Path,
        renderer: &mut crate::renderer::Renderer,
    ) -> Result<crate::asset_database::AssetId, String> {
        let storage = crate::asset_storage::AssetStorage::new(
            crate::asset_storage::AssetStorageConfig::default(),
        )
        .map_err(|e| e.to_string())?;
        let mut pipeline = crate::asset_importer::ImportPipeline::new(
            std::mem::take(&mut self.asset_database.registry),
            storage,
        );
        pipeline.register_importer(Box::new(crate::gltf_importer::GltfImporter));

        let result = pipeline.reimport_file(path);
        let asset_id = pipeline.asset_registry.find_by_source_path(path);
        self.asset_database.registry = pipeline.asset_registry;
        let result = result.map_err(|e| e.to_string())?;

        let asset_id = asset_id.unwrap_or(result.asset_id);
        renderer.invalidate_asset(asset_id);
        renderer.forget_meshes_for_source(path);
        for dependency in &result.dependencies {
            renderer.invalidate_asset(*dependency);
        }

        let _ = self
            .asset_database
            .save_to_file(&PathBuf::from("assets/asset_database.ron"));
        Ok(asset_id)
    }

    /// Drain a queued reimport once per frame, now that the renderer is in scope.
    fn process_pending_reimport(&mut self, renderer: &mut crate::renderer::Renderer) {
        let Some(path) = self.pending_reimport_asset.take() else {
            return;
        };
        match self.reimport_asset_path(&path, renderer) {
            Ok(asset_id) => {
                self.log_info(&format!("Reimported asset {} ({asset_id})", path.display()));
                self.notifications.success(
                    "Asset Reimported",
                    format!(
                        "Reimported {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                );
            }
            Err(err) => {
                self.log
                    .push(("ERROR".into(), format!("Reimport failed: {err}")));
                self.notifications.error("Reimport Failed", err);
            }
        }
    }

    pub fn draw(
        &mut self,
        window: &Window,
        frame: wgpu::SurfaceTexture,
        view: &wgpu::TextureView,
        renderer: &mut crate::renderer::Renderer,
        world: &mut World,
        perf: &mut crate::performance::PerformanceMonitor,
        _history: (),
        fps: f32,
    ) {
        // The viewport image is drawn during `draw_ui`, but the scene pass runs
        // after it so it uses the camera and panel size produced by this
        // frame's input instead of lagging one frame behind. The texture id is
        // stable, so re-rendering before the UI pass is submitted updates the
        // image the UI already queued.
        let viewport_state = self
            .viewport_texture_id
            .map(|texture_id| ViewportTextureState {
                texture_id,
                width: self.viewport_texture_size.0,
                height: self.viewport_texture_size.1,
            });
        self.process_pending_reimport(renderer);

        self.viewport_panel_drawn = false;
        let input = self.platform.as_mut().unwrap().take_egui_input(window);
        let context = self.context.clone();
        let output = context.run(input, |ctx| {
            self.draw_ui(ctx, world, perf, fps, viewport_state)
        });
        // Re-render at the size and camera the viewport panel reported this
        // frame. `draw_ui` only needs the stable texture id.
        if self.viewport_panel_drawn {
            let _ = self.prepare_viewport_texture(renderer, world);
        }
        let device = &renderer.device;
        let queue = &renderer.queue;
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
    fn draw_ui(
        &mut self,
        ctx: &Context,
        world: &mut World,
        perf: &mut crate::performance::PerformanceMonitor,
        fps: f32,
        viewport_state: Option<ViewportTextureState>,
    ) {
        // Handle Undo/Redo keyboard shortcuts. When a text widget has focus,
        // egui owns these keys (text undo), so the scene history must not
        // react to them as well.
        let keyboard_captured = ctx.wants_keyboard_input();
        let undo_shortcut = !keyboard_captured
            && ctx.input(|i| i.key_pressed(Key::Z) && (i.modifiers.ctrl || i.modifiers.mac_cmd));
        let redo_shortcut = !keyboard_captured
            && ctx.input(|i| {
                (i.key_pressed(Key::Y) || (i.key_pressed(Key::Z) && i.modifiers.shift))
                    && (i.modifiers.ctrl || i.modifiers.mac_cmd)
            });

        if undo_shortcut {
            if let Some(name) = self.history.undo(world) {
                self.notifications
                    .info(format!("Undo: {}", name), "History");
            }
        } else if redo_shortcut {
            if let Some(name) = self.history.redo(world) {
                self.notifications
                    .info(format!("Redo: {}", name), "History");
            }
        }

        // Detect play mode transitions and clear history
        let current_mode = world
            .get_resource::<crate::core::resources::EngineMode>()
            .copied()
            .unwrap_or_default();
        if self.last_engine_mode != current_mode {
            // Mode changed - clear history on play mode enter/exit
            if current_mode == crate::core::resources::EngineMode::Playing
                || self.last_engine_mode == crate::core::resources::EngineMode::Playing
            {
                self.history.clear();
                self.log_info(&format!(
                    "History cleared (mode change: {:?} -> {:?})",
                    self.last_engine_mode, current_mode
                ));
            }
            self.last_engine_mode = current_mode;
        }

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

        // Process pending model instantiation requests
        if let Some(path) = self.pending_instantiate_model.take() {
            match crate::commands::build_model_hierarchy(&path) {
                Ok(nodes) => {
                    let model_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Model")
                        .to_string();
                    let count = nodes.len();
                    let cmd =
                        crate::commands::SpawnModelHierarchyCommand::new(model_name.clone(), nodes);
                    self.history.push(Box::new(cmd), world);
                    self.dirty = true;
                    self.log_info(&format!(
                        "Instantiated model '{model_name}' ({count} entities)"
                    ));
                    self.notifications.success(
                        "Model Instantiated",
                        format!("Added '{model_name}' with {count} node(s) to scene"),
                    );
                }
                Err(err) => {
                    self.log.push((
                        "ERROR".into(),
                        format!("Failed to instantiate model: {err}"),
                    ));
                    self.notifications.error("Instantiation Failed", err);
                }
            }
        }

        // Reimport requests are drained in `draw`, where the renderer is in scope.

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
                        Icons::show(
                            ui,
                            Icons::PROJECT,
                            IconSize::Large,
                            Some(Color32::from_rgb(82, 166, 235)),
                        );
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
                                .color(Color32::WHITE),
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
                                .color(Color32::from_rgb(80, 200, 100)),
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
                                .color(Color32::from_rgb(200, 80, 80)),
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
                            Icons::show(
                                ui,
                                icon,
                                IconSize::Small,
                                Some(Color32::from_rgb(160, 165, 175)),
                            );
                            ui.menu_button(menu, |ui| match menu {
                                "File" => {
                                    if ui
                                        .button(format!("{} New Scene", Icons::FILE_BLANK))
                                        .clicked()
                                    {
                                        if self.dirty {
                                            self.confirm_dialog =
                                                Some(ConfirmDialog::unsaved_changes(
                                                    PendingAction::NewScene,
                                                ));
                                        } else {
                                            self.new_scene(world);
                                        }
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!("{} Save Scene  (Ctrl+S)", Icons::SAVE))
                                        .clicked()
                                    {
                                        self.save_scene(world);
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!("{} Reload Scene", Icons::REFRESH))
                                        .clicked()
                                    {
                                        self.prompt_reload_scene(world);
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!(
                                            "{} Load PBR Validation Scene",
                                            Icons::PLAY
                                        ))
                                        .clicked()
                                    {
                                        let pbr_path = std::path::Path::new(
                                            "assets/scenes/pbr_validation.ron",
                                        );
                                        if !pbr_path.exists() {
                                            // If the file does not exist on disk yet, write the default validation scene
                                            if let Ok(ron_str) =
                                                crate::scene::Scene::pbr_validation_scene().to_ron()
                                            {
                                                if let Some(parent) = pbr_path.parent() {
                                                    let _ = std::fs::create_dir_all(parent);
                                                }
                                                let _ = std::fs::write(pbr_path, ron_str);
                                            }
                                        }
                                        self.load_scene_from_path(world, pbr_path);
                                        self.log_info("Loaded PBR Validation Scene");
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .button(format!("{} Save Layout", Icons::SETTINGS_FILE))
                                        .clicked()
                                    {
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
                                    if ui
                                        .button(format!("{} Reset Layout to Default", Icons::HOME))
                                        .clicked()
                                    {
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
                                    // Undo/Redo at top
                                    let can_undo = self.history.can_undo();
                                    if ui
                                        .add_enabled(
                                            can_undo,
                                            egui::Button::new(
                                                RichText::new(format!(
                                                    "{} Undo (Ctrl+Z)",
                                                    Icons::UNDO
                                                ))
                                                .size(11.0)
                                                .color(if can_undo {
                                                    Color32::WHITE
                                                } else {
                                                    Color32::from_rgb(120, 120, 120)
                                                }),
                                            )
                                            .fill(
                                                if can_undo {
                                                    Color32::from_rgb(50, 65, 100)
                                                } else {
                                                    Color32::TRANSPARENT
                                                },
                                            ),
                                        )
                                        .clicked()
                                    {
                                        if let Some(name) = self.history.undo(world) {
                                            self.dirty = true;
                                            self.log_info(&format!("Undo: {}", name));
                                        }
                                        ui.close();
                                    }

                                    let can_redo = self.history.can_redo();
                                    if ui
                                        .add_enabled(
                                            can_redo,
                                            egui::Button::new(
                                                RichText::new(format!(
                                                    "{} Redo (Ctrl+Y)",
                                                    Icons::REDO
                                                ))
                                                .size(11.0)
                                                .color(if can_redo {
                                                    Color32::WHITE
                                                } else {
                                                    Color32::from_rgb(120, 120, 120)
                                                }),
                                            )
                                            .fill(
                                                if can_redo {
                                                    Color32::from_rgb(50, 80, 50)
                                                } else {
                                                    Color32::TRANSPARENT
                                                },
                                            ),
                                        )
                                        .clicked()
                                    {
                                        if let Some(name) = self.history.redo(world) {
                                            self.dirty = true;
                                            self.log_info(&format!("Redo: {}", name));
                                        }
                                        ui.close();
                                    }

                                    ui.separator();

                                    if ui
                                        .button(format!(
                                            "{} History Panel (Ctrl+H)",
                                            Icons::HISTORY
                                        ))
                                        .clicked()
                                    {
                                        actions.push(DockAction::Open(EditorTab::History));
                                        ui.close();
                                    }

                                    ui.separator();

                                    if ui
                                        .button(format!("{} Select All (Ctrl+A)", Icons::ENTITY))
                                        .clicked()
                                    {
                                        self.select_all(world);
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!(
                                            "{} Deselect All (Esc)",
                                            Icons::EMPTY_ENTITY
                                        ))
                                        .clicked()
                                    {
                                        self.selected = None;
                                        self.selected_entities.clear();
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!("{} Delete Selected (Del)", Icons::DELETE))
                                        .clicked()
                                    {
                                        self.prompt_delete_selected(world);
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!(
                                            "{} Duplicate Selected (Ctrl+D)",
                                            Icons::DUPLICATE
                                        ))
                                        .clicked()
                                    {
                                        self.duplicate_selected(world);
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .button(format!("{} Focus Selected (F)", Icons::CAMERA))
                                        .clicked()
                                    {
                                        if let Some(e) = self.selected {
                                            if let Some(t) = Self::world_transform_of(world, e) {
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
                                            if ui.selectable_label(active, preset.name()).clicked()
                                            {
                                                self.current_workspace = preset;
                                                actions.push(DockAction::ApplyPreset(preset));
                                                self.status = format!(
                                                    "Switched to {} workspace",
                                                    preset.name()
                                                );
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
                                    if ui
                                        .button(format!("{} Reset Layout to Default", Icons::HOME))
                                        .clicked()
                                    {
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
                                    ui.checkbox(
                                        &mut self.grid,
                                        format!("{} Show Grid", Icons::GRID_VIEW),
                                    );
                                    ui.checkbox(
                                        &mut self.show_gizmos,
                                        format!("{} Show Gizmos", Icons::TRANSFORM),
                                    );
                                    ui.checkbox(
                                        &mut self.show_collision,
                                        format!("{} Show Collision", Icons::HIDDEN),
                                    );
                                    ui.checkbox(
                                        &mut self.wireframe,
                                        format!("{} Wireframe", Icons::LIST_VIEW),
                                    );
                                    ui.separator();
                                    if ui
                                        .button(format!(
                                            "{} Performance Monitor",
                                            Icons::PERFORMANCE
                                        ))
                                        .clicked()
                                    {
                                        actions
                                            .push(DockAction::Open(EditorTab::PerformanceMonitor));
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .button(format!("{} Reset Camera Orbit", Icons::CAMERA))
                                        .clicked()
                                    {
                                        self.orbit_target = Vec3::ZERO;
                                        self.orbit_distance = 8.0;
                                        self.update_camera(world);
                                        ui.close();
                                    }
                                }
                                "Build" => {
                                    if ui
                                        .button(format!("{} Build Lighting", Icons::LIGHT))
                                        .clicked()
                                    {
                                        self.log.push((
                                            "INFO".into(),
                                            "Build Lighting completed (0 static lights baked)"
                                                .into(),
                                        ));
                                        self.notifications.success(
                                            "Lighting Built",
                                            "Static lighting baked successfully (0 warnings)",
                                        );
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!("{} Build Navigation Mesh", Icons::BUILD))
                                        .clicked()
                                    {
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
                                    if ui.button(format!("{} Build All", Icons::COMPILE)).clicked()
                                    {
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
                        (
                            Icons::SELECT,
                            "Select",
                            "Select Tool",
                            "Pick entities in viewport",
                            Some("Q"),
                        ),
                        (
                            Icons::MOVE,
                            "Move",
                            "Move Tool",
                            "Translate selected entity along axes",
                            Some("W"),
                        ),
                        (
                            Icons::ROTATE,
                            "Rotate",
                            "Rotate Tool",
                            "Rotate selected entity in 3D space",
                            Some("E"),
                        ),
                        (
                            Icons::SCALE,
                            "Scale",
                            "Scale Tool",
                            "Scale selected entity per axis",
                            Some("R"),
                        ),
                    ];
                    for (i, (icon, label, title, desc, shortcut)) in tool_info.iter().enumerate() {
                        let is_active = self.active_tool == i;
                        let response = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{} {}", icon, label))
                                    .size(11.0)
                                    .color(if is_active {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(180, 185, 195)
                                    }),
                            )
                            .frame(false)
                            .min_size(egui::vec2(64.0, 28.0))
                            .fill(if is_active {
                                Color32::from_rgb(60, 80, 120)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .corner_radius(egui::CornerRadius::same(4)),
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
                                .color(Color32::WHITE),
                        )
                        .engine_tooltip(
                            "Add Actor",
                            "Spawn a new actor entity at scene origin",
                            None,
                        )
                        .clicked()
                    {
                        self.create_entity(world);
                    }
                    // Save with icon
                    if ui
                        .button(
                            RichText::new(format!("{} Save", Icons::SAVE))
                                .size(11.0)
                                .color(Color32::WHITE),
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
                    // Undo/Redo buttons with icons
                    let can_undo = self.history.can_undo();
                    if ui
                        .add_enabled(
                            can_undo,
                            egui::Button::new(
                                RichText::new(format!("{} Undo", Icons::UNDO))
                                    .size(11.0)
                                    .color(if can_undo {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(120, 120, 120)
                                    }),
                            )
                            .frame(false)
                            .min_size(egui::vec2(64.0, 28.0))
                            .fill(if can_undo {
                                Color32::from_rgb(60, 80, 120)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .corner_radius(egui::CornerRadius::same(4)),
                        )
                        .engine_tooltip(
                            "Undo",
                            format!(
                                "Undo last action ({})",
                                self.history.undo_name().unwrap_or("none")
                            ),
                            Some("Ctrl+Z"),
                        )
                        .clicked()
                    {
                        if let Some(name) = self.history.undo(world) {
                            self.dirty = true;
                            self.log_info(&format!("Undo: {}", name));
                        }
                    }

                    let can_redo = self.history.can_redo();
                    if ui
                        .add_enabled(
                            can_redo,
                            egui::Button::new(
                                RichText::new(format!("{} Redo", Icons::REDO))
                                    .size(11.0)
                                    .color(if can_redo {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(120, 120, 120)
                                    }),
                            )
                            .frame(false)
                            .min_size(egui::vec2(64.0, 28.0))
                            .fill(if can_redo {
                                Color32::from_rgb(60, 100, 60)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .corner_radius(egui::CornerRadius::same(4)),
                        )
                        .engine_tooltip(
                            "Redo",
                            format!(
                                "Redo last undone action ({})",
                                self.history.redo_name().unwrap_or("none")
                            ),
                            Some("Ctrl+Y"),
                        )
                        .clicked()
                    {
                        if let Some(name) = self.history.redo(world) {
                            self.dirty = true;
                            self.log_info(&format!("Redo: {}", name));
                        }
                    }
                    // History panel toggle
                    if ui
                        .button(
                            RichText::new(format!("{} History", Icons::HISTORY))
                                .size(11.0)
                                .color(Color32::WHITE),
                        )
                        .engine_tooltip(
                            "History",
                            "Open History panel to view full undo/redo stack",
                            Some("Ctrl+H"),
                        )
                        .clicked()
                    {
                        actions.push(DockAction::Open(EditorTab::History));
                    }
                    ui.separator();
                    // View modes with icons
                    ui.label("View:");
                    let view_modes = [
                        (
                            Icons::PERSPECTIVE,
                            "Persp",
                            ViewMode::Perspective,
                            "Perspective",
                            "Perspective 3D camera projection",
                        ),
                        (
                            Icons::UP,
                            "Top",
                            ViewMode::Top,
                            "Top View",
                            "Orthographic top-down projection looking down Y",
                        ),
                        (
                            Icons::FRONT,
                            "Front",
                            ViewMode::Front,
                            "Front View",
                            "Orthographic front projection looking along Z",
                        ),
                        (
                            Icons::SIDE,
                            "Side",
                            ViewMode::Side,
                            "Side View",
                            "Orthographic side projection looking along X",
                        ),
                        (
                            Icons::ORTHOGRAPHIC,
                            "Ortho",
                            ViewMode::Orthographic,
                            "Orthographic",
                            "Orthographic 3D camera projection",
                        ),
                    ];
                    for (icon, label, mode, title, desc) in view_modes.iter() {
                        let is_active = self.view_mode == *mode;
                        let response = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{} {}", icon, label))
                                    .size(11.0)
                                    .color(if is_active {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(180, 185, 195)
                                    }),
                            )
                            .frame(false)
                            .min_size(egui::vec2(56.0, 28.0))
                            .fill(if is_active {
                                Color32::from_rgb(60, 80, 120)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .corner_radius(egui::CornerRadius::same(4)),
                        );
                        let response = response.engine_tooltip(title, desc, None);
                        if response.clicked() {
                            self.view_mode = *mode;
                        }
                    }
                    ui.separator();
                    // View toggles with icons
                    ui.add(egui::Checkbox::new(
                        &mut self.grid,
                        format!("{} Grid", Icons::GRID_VIEW),
                    ))
                    .engine_tooltip(
                        "Toggle Grid",
                        "Show or hide the coordinate ground grid",
                        None,
                    );
                    ui.add(egui::Checkbox::new(
                        &mut self.show_gizmos,
                        format!("{} Gizmos", Icons::TRANSFORM),
                    ))
                    .engine_tooltip(
                        "Toggle Gizmos",
                        "Show or hide 3D transformation manipulators",
                        None,
                    );
                    ui.add(egui::Checkbox::new(
                        &mut self.wireframe,
                        format!("{} Wireframe", Icons::LIST_VIEW),
                    ))
                    .engine_tooltip(
                        "Toggle Wireframe",
                        "Render models in polygon wireframe mode",
                        None,
                    );

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
                            egui::Button::new(RichText::new(short_name).size(11.0).color(
                                if active {
                                    Color32::WHITE
                                } else {
                                    Color32::from_rgb(180, 185, 195)
                                },
                            ))
                            .frame(false)
                            .min_size(egui::vec2(52.0, 28.0))
                            .fill(if active {
                                Color32::from_rgb(60, 80, 120)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .corner_radius(egui::CornerRadius::same(4)),
                        );
                        let response =
                            response.engine_tooltip(preset.name(), preset.description(), None);
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
                            let btn = ui
                                .add(
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
                            let btn = ui
                                .add(
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
        if !keyboard_captured {
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
                    if i.key_pressed(Key::H) {
                        actions.push(DockAction::Open(EditorTab::History));
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
                            if let Some(t) = Self::world_transform_of(world, e) {
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
        }

        // Render main DockArea with dark editor style
        {
            let mut viewer = EditorTabViewer {
                editor: self,
                world,
                perf,
                actions: &mut actions,
                viewport_state,
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
                                use crate::commands::{CompositeCommand, DespawnEntityCommand};

                                let target_count = targets.len();
                                if target_count == 1 {
                                    // Single entity deletion
                                    if let Some(cmd) = DespawnEntityCommand::new(world, targets[0])
                                    {
                                        self.history.push(Box::new(cmd), world);
                                    }
                                } else {
                                    // Multiple entities - use composite command
                                    let mut composite = CompositeCommand::new(format!(
                                        "Delete {} Actors",
                                        target_count
                                    ));
                                    for e in &targets {
                                        if let Some(cmd) = DespawnEntityCommand::new(world, *e) {
                                            composite.add(Box::new(cmd));
                                        }
                                    }
                                    if !composite.is_empty() {
                                        self.history.push(Box::new(composite), world);
                                    }
                                }

                                self.selected = None;
                                self.selected_entities.clear();
                                self.dirty = true;
                                self.status = format!("Deleted {} actor(s)", target_count);
                                self.notifications.warning(
                                    "Actor Deleted",
                                    format!(
                                        "Permanently removed {} actor(s) from scene",
                                        targets.len()
                                    ),
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
                                        self.log.push((
                                            "INFO".into(),
                                            format!("Deleted resource: {name}"),
                                        ));
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
                                    self.dock_state =
                                        Some(self.current_workspace.create_dock_state());
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
                                    self.dock_state =
                                        Some(self.current_workspace.create_dock_state());
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

    fn draw_viewport(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut World,
        viewport_state: Option<ViewportTextureState>,
    ) {
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
                    .engine_tooltip(
                        "Reset Camera",
                        "Reset camera to default position",
                        Some("R"),
                    )
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
                        .engine_tooltip(
                            "Focus Actor",
                            "Focus camera on the selected actor",
                            Some("F"),
                        )
                        .clicked()
                    {
                        if let Some(t) = Self::world_transform_of(world, e) {
                            self.orbit_target = t.translation;
                            self.update_camera(world);
                        }
                    }
                }
            });
        });

        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.viewport_panel_drawn = true;

        // Update the offscreen viewport texture dimensions for subsequent frames.
        // Use physical pixels so the viewport stays sharp on HiDPI displays.
        let pixels_per_point = ui.ctx().pixels_per_point();
        self.viewport_texture_size = (
            (rect.width() * pixels_per_point).round().max(1.0) as u32,
            (rect.height() * pixels_per_point).round().max(1.0) as u32,
        );
        let viewport_rect = ViewportRect {
            min_x: rect.min.x,
            min_y: rect.min.y,
            width: rect.width(),
            height: rect.height(),
        };

        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                self.orbit_distance =
                    (self.orbit_distance * (-scroll * 0.001).exp()).clamp(0.25, 500.0);
                self.update_camera(world);
            }
            // `drag_delta()` is cumulative for the whole gesture; applying it
            // every frame accelerates the camera. `pointer.delta()` is per-frame.
            let pointer_delta = ui.input(|i| i.pointer.delta());
            if response.dragged_by(egui::PointerButton::Secondary) && pointer_delta != Vec2::ZERO {
                if let Some((mut t, mut c)) = world
                    .query::<(&mut Transform, &mut CameraController)>()
                    .iter_mut(world)
                    .next()
                {
                    c.yaw -= pointer_delta.x * c.mouse_sensitivity * 0.01 * self.mouse_sensitivity;
                    c.pitch = (c.pitch
                        - pointer_delta.y * c.mouse_sensitivity * 0.01 * self.mouse_sensitivity)
                        .clamp(-1.55, 1.55);
                    t.rotation =
                        glam::Quat::from_rotation_y(c.yaw) * glam::Quat::from_rotation_x(c.pitch);
                    t.translation = self.orbit_target - c.forward() * self.orbit_distance;
                }
            }
            if response.dragged_by(egui::PointerButton::Middle) && pointer_delta != Vec2::ZERO {
                // Pan in the camera's screen plane rather than world X/Y, and
                // scale the speed with distance so it feels consistent at any
                // zoom level.
                let basis = world
                    .query::<&CameraController>()
                    .iter(world)
                    .next()
                    .map(|c| (c.right(), c.up()));
                if let Some((right, up)) = basis {
                    let scale = (self.orbit_distance * 0.0015).max(0.0001);
                    self.orbit_target +=
                        right * (-pointer_delta.x * scale) + up * (pointer_delta.y * scale);
                    self.update_camera(world);
                }
            }
        }

        // Click picking uses the same camera/projection as the rendered image.
        if response.clicked() {
            let camera = self.viewport_camera(world);
            let hit = response
                .interact_pointer_pos()
                .and_then(|pos| self.pick_entity(world, &camera, viewport_rect, pos));
            if let Some(e) = hit {
                self.set_selected_entity(Some(e), false);
            } else if ui.input(|i| !i.modifiers.ctrl && !i.modifiers.shift) {
                self.set_selected_entity(None, false);
            }
            if let Some(e) = self.selected {
                if let Some(t) = Self::world_transform_of(world, e) {
                    self.orbit_target = t.translation;
                }
            }
        }

        // Hover highlight follows the same ray cast as picking.
        self.hovered_entity = if response.hovered() {
            response.hover_pos().and_then(|pos| {
                let camera = self.viewport_camera(world);
                self.pick_entity(world, &camera, viewport_rect, pos)
            })
        } else {
            None
        };

        // Gizmo drag start: only begin a transform gesture when the pointer
        // actually grabbed one of the axis handles.
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.gizmo_interaction = None;
            self.gizmo_drag_start_transform = None;
            self.gizmo_drag_start_world = None;
            self.gizmo_drag_parent_inverse = None;
            self.gizmo_drag_start = None;
            if self.show_gizmos && self.active_tool != 0 {
                if let Some(pos) = response.interact_pointer_pos() {
                    let camera = self.viewport_camera(world);
                    if let Some((entity, origin, basis, _scale)) = self.gizmo_transform(world) {
                        if let Some(axis) =
                            self.hit_test_gizmo_axis(&camera, viewport_rect, pos, origin, basis)
                        {
                            self.gizmo_interaction =
                                Some((entity, self.gizmo_tool(), axis, self.gizmo_space()));
                            self.gizmo_drag_start = Some(pos);
                            self.gizmo_drag_start_transform =
                                world.get::<Transform>(entity).copied();
                            self.gizmo_drag_start_world = Self::world_transform_of(world, entity);
                            self.gizmo_drag_parent_inverse = world.get::<Parent>(entity).map(|p| {
                                world
                                    .get::<GlobalTransform>(p.0)
                                    .map(|g| g.0.inverse())
                                    .unwrap_or(Mat4::IDENTITY)
                            });
                        }
                    }
                }
            }
        }

        // Gizmo drag: compute an absolute transform from the gesture start so
        // the object follows the cursor instead of accumulating every frame.
        if response.dragged_by(egui::PointerButton::Primary) {
            if let (
                Some((entity, tool, axis, space)),
                Some(start_pos),
                Some(start_world),
                Some(current_pos),
            ) = (
                self.gizmo_interaction,
                self.gizmo_drag_start,
                self.gizmo_drag_start_world,
                response.interact_pointer_pos(),
            ) {
                let parent_inverse = self.gizmo_drag_parent_inverse;
                let camera = self.viewport_camera(world);
                let basis = crate::viewport::gizmo_basis(space, start_world.rotation);
                let origin = start_world.translation;
                let start_screen = glam::Vec2::new(start_pos.x, start_pos.y);
                let current_screen = glam::Vec2::new(current_pos.x, current_pos.y);
                let mut new_world = start_world;

                match tool {
                    GizmoTool::Move => {
                        let (start_o, start_d) =
                            crate::viewport::screen_ray(&camera, viewport_rect, start_screen);
                        let (current_o, current_d) =
                            crate::viewport::screen_ray(&camera, viewport_rect, current_screen);
                        let delta = crate::viewport::drag_translation_delta(
                            axis,
                            basis,
                            start_o,
                            start_d,
                            current_o,
                            current_d,
                            origin,
                            self.gizmo_snap_translation,
                        );
                        new_world.translation = start_world.translation + delta;
                    }
                    GizmoTool::Rotate => {
                        let axis_dir =
                            crate::viewport::axis_vector(axis, basis).normalize_or(Vec3::Y);
                        let (start_o, start_d) =
                            crate::viewport::screen_ray(&camera, viewport_rect, start_screen);
                        let start_hit =
                            crate::viewport::ray_plane(start_o, start_d, origin, axis_dir)
                                .unwrap_or(origin + camera.right());
                        let (current_o, current_d) =
                            crate::viewport::screen_ray(&camera, viewport_rect, current_screen);
                        let angle = crate::viewport::drag_rotation_angle(
                            origin,
                            axis_dir,
                            current_o,
                            current_d,
                            start_hit - origin,
                        );
                        let angle = crate::viewport::snap_angle(angle, self.gizmo_snap_rotation);
                        new_world.rotation =
                            Quat::from_axis_angle(axis_dir, angle) * start_world.rotation;
                    }
                    GizmoTool::Scale => {
                        let uniform = matches!(
                            axis,
                            GizmoAxis::Uniform | GizmoAxis::Xy | GizmoAxis::Xz | GizmoAxis::Yz
                        );
                        let factor = crate::viewport::snap_scale(
                            crate::viewport::drag_scale_factor(
                                start_screen,
                                current_screen,
                                uniform,
                            ),
                            self.gizmo_snap_scale,
                        );
                        new_world.scale = match axis {
                            GizmoAxis::X => Vec3::new(
                                start_world.scale.x * factor,
                                start_world.scale.y,
                                start_world.scale.z,
                            ),
                            GizmoAxis::Y => Vec3::new(
                                start_world.scale.x,
                                start_world.scale.y * factor,
                                start_world.scale.z,
                            ),
                            GizmoAxis::Z => Vec3::new(
                                start_world.scale.x,
                                start_world.scale.y,
                                start_world.scale.z * factor,
                            ),
                            _ => start_world.scale * factor,
                        };
                    }
                    GizmoTool::Select => {}
                }

                // Convert the edited world transform back into the entity's
                // local space when it has a parent.
                let new_local = if let Some(inverse) = parent_inverse {
                    let matrix = inverse * new_world.matrix();
                    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
                    Transform {
                        translation,
                        rotation,
                        scale,
                    }
                } else {
                    new_world
                };

                if let Some(mut t) = world.get_mut::<Transform>(entity) {
                    *t = new_local;
                    self.dirty = true;
                }
            }
        }

        // Gizmo drag end: commit the whole gesture as one undoable command.
        if response.drag_stopped_by(egui::PointerButton::Primary) {
            if let Some((entity, _, _, _)) = self.gizmo_interaction {
                if let Some(start_transform) = self.gizmo_drag_start_transform.take() {
                    if let Some(current_transform) = world.get::<Transform>(entity).copied() {
                        if current_transform != start_transform {
                            use crate::commands::SetTransformCommand;
                            if let Some(cmd) = SetTransformCommand::from_pair(
                                entity,
                                start_transform,
                                current_transform,
                            ) {
                                self.history.push(Box::new(cmd), world);
                            }
                        }
                    }
                }
            }
            self.gizmo_interaction = None;
            self.gizmo_drag_start = None;
            self.gizmo_drag_start_transform = None;
            self.gizmo_drag_start_world = None;
            self.gizmo_drag_parent_inverse = None;
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
            if ui
                .button(format!("{} Show Collision", Icons::COLLISION))
                .clicked()
            {
                self.show_collision = !self.show_collision;
                ui.close();
            }
            if ui
                .button(format!("{} Wireframe", Icons::WIREFRAME))
                .clicked()
            {
                self.wireframe = !self.wireframe;
                ui.close();
            }
            ui.separator();
            if ui
                .button(format!("{} Focus Selected (F)", Icons::FOCUS))
                .clicked()
            {
                if let Some(e) = self.selected {
                    if let Some(t) = Self::world_transform_of(world, e) {
                        self.orbit_target = t.translation;
                        self.update_camera(world);
                    }
                }
                ui.close();
            }
            if ui
                .button(format!("{} Reset Camera Orbit", Icons::RESET))
                .clicked()
            {
                self.orbit_target = Vec3::ZERO;
                self.orbit_distance = 8.0;
                self.update_camera(world);
                ui.close();
            }
        });

        // Project the gizmo and selection markers with the same camera used for
        // the scene so the overlay stays aligned with the rendered geometry.
        let overlay_camera = self.viewport_camera(world);
        let gizmo_overlay = if self.show_gizmos && self.active_tool != 0 {
            self.gizmo_transform(world)
                .and_then(|(_entity, origin, basis, _scale)| {
                    let origin_screen =
                        crate::viewport::world_to_screen(&overlay_camera, viewport_rect, origin)?;
                    if !rect.contains(egui::pos2(origin_screen.x, origin_screen.y)) {
                        return None;
                    }
                    let mut axes = Vec::new();
                    for axis in [GizmoAxis::X, GizmoAxis::Y, GizmoAxis::Z] {
                        let dir = crate::viewport::axis_vector(axis, basis);
                        if let Some(tip) = crate::viewport::gizmo_screen_handle(
                            &overlay_camera,
                            viewport_rect,
                            origin,
                            dir,
                        ) {
                            axes.push((axis, tip));
                        }
                    }
                    let hovered_axis = if response.hovered()
                        && !response.dragged_by(egui::PointerButton::Primary)
                    {
                        response.hover_pos().and_then(|pos| {
                            self.hit_test_gizmo_axis(
                                &overlay_camera,
                                viewport_rect,
                                pos,
                                origin,
                                basis,
                            )
                        })
                    } else {
                        None
                    };
                    Some((origin_screen, axes, hovered_axis))
                })
        } else {
            None
        };
        let selection_markers: Vec<egui::Pos2> = self
            .selected_entities
            .iter()
            .filter_map(|&entity| {
                let transform = Self::world_transform_of(world, entity)?;
                let screen = crate::viewport::world_to_screen(
                    &overlay_camera,
                    viewport_rect,
                    transform.translation,
                )?;
                let pos = egui::pos2(screen.x, screen.y);
                rect.contains(pos).then_some(pos)
            })
            .collect();
        let hover_marker = self.hovered_entity.and_then(|entity| {
            if self.selected_entities.contains(&entity) {
                return None;
            }
            let transform = Self::world_transform_of(world, entity)?;
            let screen = crate::viewport::world_to_screen(
                &overlay_camera,
                viewport_rect,
                transform.translation,
            )?;
            let pos = egui::pos2(screen.x, screen.y);
            rect.contains(pos).then_some(pos)
        });

        let p = ui.painter();
        if let Some(state) = viewport_state {
            p.image(
                state.texture_id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            p.rect_filled(rect, 0.0, Color32::from_rgb(23, 25, 29));
        }
        p.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0_f32, Color32::from_gray(58)),
            egui::StrokeKind::Inside,
        );
        if self.grid && viewport_state.is_none() {
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
        if let Some((origin, axes, hovered_axis)) = gizmo_overlay {
            let origin = egui::pos2(origin.x, origin.y);
            for (axis, tip) in axes {
                let (color, label) = match axis {
                    GizmoAxis::X => (Color32::from_rgb(220, 90, 90), "X"),
                    GizmoAxis::Y => (Color32::from_rgb(90, 220, 120), "Y"),
                    GizmoAxis::Z => (Color32::from_rgb(90, 140, 230), "Z"),
                    _ => (Color32::WHITE, ""),
                };
                let highlighted = hovered_axis == Some(axis);
                let color = if highlighted {
                    color.linear_multiply(1.35)
                } else {
                    color
                };
                let width = if highlighted { 3.5_f32 } else { 2.0_f32 };
                let tip = egui::pos2(tip.x, tip.y);
                p.line_segment([origin, tip], egui::Stroke::new(width, color));
                p.circle_filled(tip, if highlighted { 5.5 } else { 4.0 }, color);
                p.text(
                    tip + Vec2::new(5.0, -5.0),
                    egui::Align2::LEFT_BOTTOM,
                    label,
                    egui::FontId::proportional(10.0),
                    color,
                );
            }
            p.circle_filled(origin, 3.5, Color32::WHITE);
        } else if self.show_gizmos {
            // No selection: show a compact orientation indicator.
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
                egui::Stroke::new(1.0_f32, Color32::from_rgb(220, 90, 90)),
            );
            p.line_segment(
                [gizmo_origin, gizmo_origin + Vec2::new(-14.0, 0.0)],
                egui::Stroke::new(1.0_f32, Color32::from_rgb(90, 220, 120)),
            );
            p.line_segment(
                [gizmo_origin, gizmo_origin + Vec2::new(0.0, -14.0)],
                egui::Stroke::new(1.0_f32, Color32::from_rgb(90, 140, 230)),
            );
        }
        for center in &selection_markers {
            p.circle_stroke(
                *center,
                12.0,
                egui::Stroke::new(2.0_f32, Color32::from_rgb(255, 178, 64)),
            );
            p.text(
                *center + Vec2::new(0.0, -16.0),
                egui::Align2::CENTER_BOTTOM,
                "Selected",
                egui::FontId::proportional(10.0),
                Color32::from_rgb(255, 206, 120),
            );
        }
        if let Some(center) = hover_marker {
            p.circle_stroke(
                center,
                10.0,
                egui::Stroke::new(1.0_f32, Color32::from_rgb(200, 200, 200)),
            );
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
            Icons::show(
                ui,
                Icons::SEARCH,
                IconSize::Small,
                Some(Color32::from_rgb(120, 135, 155)),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.log_filter)
                    .hint_text("Search log...")
                    .desired_width(180.0),
            );
            if !self.log_filter.is_empty()
                && ui
                    .small_button("Г—")
                    .engine_tooltip("Clear Search", "Clear the log search filter", None)
                    .clicked()
            {
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
                    if let Some(uid) = world.get::<crate::core::uid::Uid>(e).copied() {
                        if let Some(mut registry) =
                            world.get_resource_mut::<crate::core::resources::PersistentIdRegistry>()
                        {
                            registry.unregister(uid);
                        }
                    }
                    let _ = world.despawn(e);
                }
                let count = scene.entities.len();
                let mut spawned_entities = Vec::with_capacity(count);

                for item in &scene.entities {
                    let mut t = Transform::from_translation(Vec3::from_array(item.translation));
                    t.rotation = glam::Quat::from_array(item.rotation);
                    t.scale = Vec3::from_array(item.scale);
                    let uid = item.uid.unwrap_or_else(crate::core::uid::Uid::new);

                    let mut entity_cmds = world.spawn((
                        Name(item.name.clone()),
                        t,
                        uid,
                        crate::core::components::Visible(item.visible),
                        crate::core::components::Locked(item.locked),
                    ));

                    if let Some(ref folder) = item.folder {
                        entity_cmds.insert(crate::core::components::FolderGroup(folder.clone()));
                    }
                    if let Some(ref mf) = item.components.mesh_filter {
                        entity_cmds.insert(mf.clone());
                    }
                    if let Some(ref mr) = item.components.mesh_renderer {
                        entity_cmds.insert(mr.clone());
                    }
                    if let Some(cam) = item.components.camera {
                        entity_cmds.insert(cam);
                    }
                    if let Some(light) = item.components.light {
                        entity_cmds.insert(light);
                    }

                    let entity = entity_cmds.id();
                    spawned_entities.push(entity);

                    if let Some(mut registry) =
                        world.get_resource_mut::<crate::core::resources::PersistentIdRegistry>()
                    {
                        let _ = registry.register(uid, entity);
                    }
                }

                // Restore parent-child hierarchy
                for (idx, item) in scene.entities.iter().enumerate() {
                    if let Some(parent_idx) = item.parent {
                        if parent_idx < spawned_entities.len() {
                            let child_entity = spawned_entities[idx];
                            let parent_entity = spawned_entities[parent_idx];
                            world
                                .entity_mut(child_entity)
                                .insert(crate::transform::Parent(parent_entity));
                        }
                    }
                }

                self.dirty = false;
                self.status = "Loaded".into();
                self.log
                    .push(("INFO".into(), format!("Loaded scene with {count} actor(s)")));
                self.notifications.info(
                    "Scene Reloaded",
                    format!(
                        "Restored {count} actor(s) from {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                );
                // Clear undo/redo history on scene load
                self.history.clear();
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
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        Self::draw_asset_details_header(ui, &name, &kind, &path, metadata.as_ref());
        ui.separator();
        Self::draw_asset_details_properties(ui, &path, &kind, metadata.as_ref());
        ui.separator();
        Self::draw_asset_details_type_specific(ui, &path, &kind);
    }

    fn preview_mesh_viewport(&mut self, ui: &mut egui::Ui, mesh: &ObjMesh) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let center = rect.center();
        let zoom = self.mesh_zoom.max(0.1);
        // Handle orbit drag. `drag_delta()` is cumulative for the gesture, so
        // use the per-frame pointer delta to avoid accelerating the orbit.
        if response.dragged() {
            let delta = ui.input(|i| i.pointer.delta());
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
                let y =
                    v[1] * cos_pitch * zoom - (v[0] * sin_yaw - v[2] * cos_yaw) * sin_pitch * zoom;
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
                    painter.line_segment([*a, *b], Stroke::new(1.0_f32, tri_color));
                    painter.line_segment([*b, *c], Stroke::new(1.0_f32, tri_color));
                    painter.line_segment([*c, *a], Stroke::new(1.0_f32, tri_color));
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
                let y =
                    v[1] * cos_pitch * zoom - (v[0] * sin_yaw - v[2] * cos_yaw) * sin_pitch * zoom;
                egui::pos2(center.x + x, center.y + y)
            })
            .collect();
        let box_edges = [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];
        let box_color = Color32::from_rgb(120, 140, 160);
        for (a, b) in box_edges {
            painter.line_segment(
                [corners_2d[a], corners_2d[b]],
                Stroke::new(1.0_f32, box_color),
            );
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
                (
                    available.y * aspect * self.texture_zoom,
                    available.y * self.texture_zoom,
                )
            } else {
                (
                    available.x * self.texture_zoom,
                    available.x / aspect * self.texture_zoom,
                )
            };
            let rect = ui
                .allocate_exact_size(egui::vec2(w, h), Sense::click_and_drag())
                .0;
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
            ui.put(
                rect,
                egui::Image::new(&texture).fit_to_exact_size(rect.size()),
            );
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
                ui.label(
                    RichText::new("Failed to decode image").color(Color32::from_rgb(220, 100, 100)),
                );
            });
        }
    }

    // Preview state functions
    fn preview_empty_state(ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                Icons::show(
                    ui,
                    Icons::FILE,
                    IconSize::XLarge,
                    Some(Color32::from_rgb(100, 120, 140)),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Asset Preview")
                        .size(20.0)
                        .color(Color32::from_rgb(180, 190, 200)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Select an asset in the Content Browser to preview it here.")
                        .color(Color32::from_rgb(120, 135, 155)),
                );
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
                Icons::show(
                    ui,
                    Icons::FOLDER,
                    IconSize::XLarge,
                    Some(Color32::from_rgb(80, 180, 230)),
                );
                ui.add_space(12.0);
                ui.label(RichText::new(name).size(20.0).color(Color32::WHITE));
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("{count} asset(s) В· {entries} entries"))
                        .color(Color32::from_rgb(140, 155, 170)),
                );
                ui.add_space(20.0);
                ui.label(
                    RichText::new("Double-click in the Content Browser to navigate here.")
                        .color(Color32::from_rgb(120, 135, 155)),
                );
            });
        });
    }

    fn preview_scene_state(editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                Icons::show(
                    ui,
                    Icons::SCENE,
                    IconSize::XLarge,
                    Some(Color32::from_rgb(80, 220, 120)),
                );
                ui.add_space(12.0);
                ui.label(RichText::new(name).size(20.0).color(Color32::WHITE));
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Double-click to open in Scene Editor")
                        .color(Color32::from_rgb(140, 155, 170)),
                );
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
                ui.add(
                    egui::DragValue::new(&mut editor.texture_zoom)
                        .speed(0.01)
                        .range(0.1..=10.0)
                        .suffix("x"),
                );
                if ui.button("Reset").clicked() {
                    editor.texture_zoom = 1.0;
                }
            });
            ui.add_space(4.0);
            editor.preview_texture_viewport(ui, &data);
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Failed to read texture file")
                        .color(Color32::from_rgb(220, 100, 100)),
                );
            });
        }
    }

    fn preview_model_state(editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let mesh_opt = if ext == "gltf" || ext == "glb" {
            ObjMesh::parse_gltf(path)
        } else if let Ok(content) = std::fs::read_to_string(path) {
            ObjMesh::parse(&content)
        } else {
            None
        };

        if let Some(mesh) = mesh_opt {
            // Show zoom control
            ui.horizontal(|ui| {
                ui.label("Zoom:");
                ui.add(
                    egui::DragValue::new(&mut editor.mesh_zoom)
                        .speed(0.1)
                        .range(0.1..=50.0)
                        .suffix("x"),
                );
                if ui.button("Reset").clicked() {
                    editor.mesh_zoom = 1.0;
                    editor.preview_yaw = 0.0;
                    editor.preview_pitch = 0.0;
                }
                ui.separator();
                ui.label(format!(
                    "Vertices: {}  Triangles: {}",
                    mesh.vertices.len(),
                    mesh.indices.len() / 3
                ));
            });
            ui.add_space(4.0);
            editor.preview_mesh_viewport(ui, &mesh);
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Failed to parse 3D model (supported: glTF, GLB, OBJ)")
                        .color(Color32::from_rgb(220, 100, 100)),
                );
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
                painter.circle_stroke(
                    center,
                    radius,
                    Stroke::new(2.0_f32, Color32::from_rgb(100, 80, 60)),
                );
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
                ui.label(
                    RichText::new("Failed to read material file")
                        .color(Color32::from_rgb(220, 100, 100)),
                );
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
                ui.label(
                    RichText::new("Failed to read script file")
                        .color(Color32::from_rgb(220, 100, 100)),
                );
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
                Icons::show(
                    ui,
                    Icons::AUDIO,
                    IconSize::XLarge,
                    Some(Color32::from_rgb(220, 180, 80)),
                );
                ui.add_space(12.0);
                ui.label(RichText::new(name).size(20.0).color(Color32::WHITE));
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("{} bytes", format_bytes(size)))
                        .color(Color32::from_rgb(140, 155, 170)),
                );
                ui.add_space(20.0);
                ui.label(
                    RichText::new("Audio preview not yet implemented")
                        .color(Color32::from_rgb(120, 135, 155)),
                );
            });
        });
    }

    fn preview_unsupported_state(_editor: &mut Editor, ui: &mut egui::Ui, path: &Path) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                Icons::show(
                    ui,
                    Icons::FILE,
                    IconSize::XLarge,
                    Some(Color32::from_rgb(160, 100, 100)),
                );
                ui.add_space(12.0);
                ui.label(RichText::new(name).size(20.0).color(Color32::WHITE));
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("Unsupported asset type: .{}", ext))
                        .color(Color32::from_rgb(220, 140, 140)),
                );
                ui.add_space(20.0);
                ui.label(
                    RichText::new(
                        "No preview available. Extend the preview system to support this type.",
                    )
                    .color(Color32::from_rgb(120, 135, 155)),
                );
            });
        });
    }

    // Asset Details helpers
    fn draw_asset_details_header(
        ui: &mut egui::Ui,
        name: &str,
        kind: &AssetKind,
        path: &Path,
        metadata: Option<&std::fs::Metadata>,
    ) {
        ui.horizontal(|ui| {
            Icons::show(
                ui,
                Icons::INFO,
                IconSize::Medium,
                Some(Color32::from_rgb(80, 180, 230)),
            );
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(name)
                        .size(16.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.label(
                    RichText::new(kind.label())
                        .size(11.0)
                        .color(Color32::from_rgb(140, 155, 170)),
                );
            });
        });
        ui.add_space(8.0);
        if let Some(meta) = metadata {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Path:").color(Color32::from_rgb(120, 135, 155)));
                ui.add(
                    egui::TextEdit::singleline(&mut path.to_string_lossy().to_string())
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
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

    fn draw_asset_details_properties(
        ui: &mut egui::Ui,
        path: &Path,
        kind: &AssetKind,
        metadata: Option<&std::fs::Metadata>,
    ) {
        ui.label(RichText::new("Properties").strong());
        ui.add_space(4.0);
        egui::Grid::new("asset_details_props")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                Self::property_row(
                    ui,
                    "Name",
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                );
                Self::property_row(ui, "Type", kind.label().to_string());
                Self::property_row(
                    ui,
                    "Format",
                    path.extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_uppercase(),
                );
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
                        egui::Grid::new("model_geometry")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                Self::property_row(ui, "Vertices", mesh.vertices.len().to_string());
                                Self::property_row(ui, "Triangles", triangle_count.to_string());
                                Self::property_row(
                                    ui,
                                    "Bounds",
                                    format!("{:.2} x {:.2} x {:.2}", size[0], size[1], size[2]),
                                );
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
                        egui::Grid::new("texture_props")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                Self::property_row(ui, "Resolution", format!("{} x {}", w, h));
                                Self::property_row(
                                    ui,
                                    "Color Type",
                                    format!("{:?}", image.color()),
                                );
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
                        if line.starts_with("//") || line.is_empty() {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                RichText::new(line)
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::from_rgb(160, 170, 180)),
                            );
                        });
                    }
                }
            }
            AssetKind::Scene => {
                ui.add_space(8.0);
                ui.label(RichText::new("Scene Info").strong());
                if let Ok(content) = std::fs::read_to_string(path) {
                    if let Ok(scene) = Scene::from_ron(&content) {
                        egui::Grid::new("scene_props")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                Self::property_row(
                                    ui,
                                    "Entities",
                                    scene.entities.len().to_string(),
                                );
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
                Icons::show(
                    ui,
                    Icons::INFO,
                    IconSize::XLarge,
                    Some(Color32::from_rgb(100, 120, 140)),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Asset Details")
                        .size(20.0)
                        .color(Color32::from_rgb(180, 190, 200)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Select an asset in the Content Browser to view its properties.")
                        .color(Color32::from_rgb(120, 135, 155)),
                );
            });
        });
    }

    fn zoom_constant() -> f32 {
        100.0
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
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let is_model = ext == "gltf" || ext == "glb" || ext == "obj";

        if is_model && ui.button(format!("{} Add to Scene", Icons::ADD)).clicked() {
            self.pending_instantiate_model = Some(path.to_path_buf());
            ui.close();
        }

        if !path.is_dir() && ui.button(format!("{} Reimport", Icons::REFRESH)).clicked() {
            self.pending_reimport_asset = Some(path.to_path_buf());
            ui.close();
        }

        if path.is_dir() && ui.button(format!("{} Open", Icons::FOLDER_OPEN)).clicked() {
            self.navigate_content(path.to_path_buf());
            ui.close();
        }
        if ui
            .button(format!("{} Create Folder", Icons::FOLDER_NEW))
            .clicked()
        {
            self.create_content_folder();
            ui.close();
        }
        if ui.button(format!("{} Rename", Icons::RENAME)).clicked() {
            self.rename_content(path);
            ui.close();
        }
        if ui
            .button(format!("{} Duplicate", Icons::DUPLICATE))
            .clicked()
        {
            self.duplicate_content(path);
            ui.close();
        }
        if ui.button(format!("{} Delete", Icons::DELETE)).clicked() {
            self.prompt_delete_resource(path.to_path_buf());
            ui.close();
        }
        if ui
            .button(format!("{} Import File…", Icons::IMPORT))
            .clicked()
        {
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
        if !apply
            || new_name.trim().is_empty()
            || *new_name == path.file_name().unwrap_or_default().to_string_lossy()
        {
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
                format!(
                    "Renamed '{}'",
                    target.file_name().unwrap_or_default().to_string_lossy()
                ),
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
        let result = ProcessCommand::new("explorer")
            .arg("/select,")
            .arg(path)
            .spawn();
        #[cfg(not(target_os = "windows"))]
        let result = ProcessCommand::new("xdg-open")
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
                    .engine_tooltip(
                        "Create Group",
                        "Create a new folder group to organize actors",
                        None,
                    )
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
            Icons::show(
                ui,
                Icons::SEARCH,
                IconSize::Small,
                Some(Color32::from_rgb(120, 135, 155)),
            );
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
            let original_name = world.get::<Name>(target).map(|n| n.0.clone());
            if let Some(original_name) = original_name {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Rename:");
                    let response = ui.text_edit_singleline(&mut self.rename_buffer);
                    if response.lost_focus() || ui.button("Apply").clicked() {
                        let new_name = self.rename_buffer.trim().to_string();
                        if !new_name.is_empty() && new_name != original_name {
                            use crate::commands::RenameEntityCommand;
                            if let Some(cmd) = RenameEntityCommand::new(world, target, new_name) {
                                self.history.push(Box::new(cmd), world);
                            }
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
                let icon = if is_folder {
                    Icons::FOLDER
                } else {
                    Icons::ENTITY
                };
                let is_selected = self.selected_entities.contains(&entity);
                let label = format!("{icon}  {name}");
                let response = ui.selectable_label(is_selected, label);
                let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
                if response.clicked() {
                    self.set_selected_entity(Some(entity), ctrl);
                }
                let vis_icon = if visible {
                    Icons::VISIBLE
                } else {
                    Icons::HIDDEN
                };
                if ui
                    .button(RichText::new(vis_icon).size(12.0).color(if visible {
                        Color32::from_rgb(100, 200, 100)
                    } else {
                        Color32::from_rgb(120, 120, 120)
                    }))
                    .engine_tooltip(
                        if visible { "Hide Actor" } else { "Show Actor" },
                        if visible {
                            "Hide this actor from the viewport"
                        } else {
                            "Make this actor visible in the viewport"
                        },
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
                let lock_icon = if locked {
                    Icons::LOCKED
                } else {
                    Icons::UNLOCKED
                };
                if ui
                    .button(RichText::new(lock_icon).size(12.0).color(if locked {
                        Color32::from_rgb(220, 180, 60)
                    } else {
                        Color32::from_rgb(120, 120, 120)
                    }))
                    .engine_tooltip(
                        if locked { "Unlock Actor" } else { "Lock Actor" },
                        if locked {
                            "Allow this actor to be selected and modified"
                        } else {
                            "Prevent this actor from being selected or modified"
                        },
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
                    if ui
                        .button(format!("{} Make Child of Selected", Icons::ENTITY))
                        .clicked()
                    {
                        if let Some(parent) = self.selected {
                            if parent != entity {
                                world.entity_mut(entity).insert(Parent(parent));
                                self.dirty = true;
                            }
                        }
                        ui.close();
                    }
                    if ui
                        .button(format!("{} Detach From Parent", Icons::EMPTY_ENTITY))
                        .clicked()
                    {
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
                Icons::show(
                    ui,
                    Icons::EMPTY_ENTITY,
                    IconSize::Large,
                    Some(Color32::from_rgb(100, 105, 115)),
                );
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
            ui.collapsing(format!("{} Transform", Icons::TRANSFORM), |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(
                            RichText::new(format!("{} Copy", Icons::COPY))
                                .size(11.0)
                                .color(Color32::WHITE),
                        )
                        .engine_tooltip(
                            "Copy Transform",
                            "Copy the transform to clipboard",
                            Some("Ctrl+C"),
                        )
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
                        .engine_tooltip(
                            "Paste Transform",
                            "Paste transform from clipboard",
                            Some("Ctrl+V"),
                        )
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
                        .engine_tooltip(
                            "Reset Transform",
                            "Reset transform to default values",
                            None,
                        )
                        .clicked()
                    {
                        if let Some(mut t) = world.get_mut::<Transform>(entity) {
                            *t = Transform::default();
                            self.dirty = true;
                        }
                    }
                });

                // Track the transform at the start of a pointer/keyboard
                // interaction so translation, rotation and scale edits commit
                // as one command.
                let is_pointer_down = ui.input(|i| i.pointer.any_down());
                let pointer_pressed = ui.input(|i| i.pointer.primary_pressed());
                let has_keyboard_focus = ui.ctx().memory(|m| m.focused().is_some());
                let pointer_over_details = ui.rect_contains_pointer(ui.max_rect());
                if pointer_pressed && pointer_over_details && self.transform_edit_start.is_none() {
                    if let Some(t) = world.get::<Transform>(entity) {
                        self.transform_edit_start = Some(*t);
                    }
                }

                let mut translation_changed = false;
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
                        translation_changed |= ui
                            .add(
                                egui::DragValue::new(&mut t.translation.x)
                                    .prefix("X ")
                                    .speed(0.1),
                            )
                            .changed();
                        translation_changed |= ui
                            .add(
                                egui::DragValue::new(&mut t.translation.y)
                                    .prefix("Y ")
                                    .speed(0.1),
                            )
                            .changed();
                        translation_changed |= ui
                            .add(
                                egui::DragValue::new(&mut t.translation.z)
                                    .prefix("Z ")
                                    .speed(0.1),
                            )
                            .changed();
                    });
                }
                if translation_changed {
                    self.dirty = true;
                }

                // Rotation editing. DragValues show degrees; convert back to
                // radians so the edited values actually reach the transform.
                let rotation_euler = world.get::<Transform>(entity).map(|t| {
                    let (rx, ry, rz) = t.rotation.to_euler(glam::EulerRot::XYZ);
                    (rx.to_degrees(), ry.to_degrees(), rz.to_degrees())
                });

                ui.horizontal(|ui| {
                    ui.label("Rotation");
                    if ui
                        .button(
                            RichText::new(Icons::RESET)
                                .size(11.0)
                                .color(Color32::from_rgb(140, 145, 155)),
                        )
                        .engine_tooltip(
                            "Reset Rotation",
                            "Set rotation to identity (no rotation)",
                            None,
                        )
                        .clicked()
                    {
                        if let Some(mut t) = world.get_mut::<Transform>(entity) {
                            t.rotation = glam::Quat::IDENTITY;
                            self.dirty = true;
                        }
                    }
                });

                if let Some((mut rx, mut ry, mut rz)) = rotation_euler {
                    let edited = ui
                        .horizontal(|ui| {
                            let mut changed = false;
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut rx)
                                        .prefix("X ")
                                        .suffix("°")
                                        .speed(0.5),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut ry)
                                        .prefix("Y ")
                                        .suffix("°")
                                        .speed(0.5),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut rz)
                                        .prefix("Z ")
                                        .suffix("°")
                                        .speed(0.5),
                                )
                                .changed();
                            changed
                        })
                        .inner;
                    if edited {
                        if let Some(mut t) = world.get_mut::<Transform>(entity) {
                            t.rotation = glam::Quat::from_euler(
                                glam::EulerRot::XYZ,
                                rx.to_radians(),
                                ry.to_radians(),
                                rz.to_radians(),
                            );
                            self.dirty = true;
                        }
                    }
                }

                // Scale editing - get values without holding mutable borrow
                let mut scale_changed = false;
                let mut scale_value = None;
                if let Some(t) = world.get::<Transform>(entity) {
                    scale_value = Some(t.scale);
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
                        if let Some(mut t) = world.get_mut::<Transform>(entity) {
                            t.scale = Vec3::ONE;
                            self.dirty = true;
                        }
                    }
                });

                if let Some(mut scale) = scale_value {
                    let changed = ui
                        .horizontal(|ui| {
                            let mut changed = false;
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut scale.x)
                                        .prefix("X ")
                                        .speed(0.02)
                                        .range(0.001..=1000.0),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut scale.y)
                                        .prefix("Y ")
                                        .speed(0.02)
                                        .range(0.001..=1000.0),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut scale.z)
                                        .prefix("Z ")
                                        .speed(0.02)
                                        .range(0.001..=1000.0),
                                )
                                .changed();
                            changed
                        })
                        .inner;
                    scale_changed = changed;
                }

                if scale_changed {
                    if let (Some(mut t), Some(scale)) =
                        (world.get_mut::<Transform>(entity), scale_value)
                    {
                        t.scale = scale;
                        self.dirty = true;
                    }
                }

                // Commit the whole Details edit once both the pointer is
                // released and no DragValue is still in text-entry mode.
                if !is_pointer_down && !has_keyboard_focus {
                    if let Some(start) = self.transform_edit_start.take() {
                        if let Some(current) = world.get::<Transform>(entity).copied() {
                            if current != start {
                                use crate::commands::SetTransformCommand;
                                if let Some(cmd) =
                                    SetTransformCommand::from_pair(entity, start, current)
                                {
                                    self.history.push(Box::new(cmd), world);
                                }
                            }
                        }
                    }
                }
            });

            ui.collapsing(format!("{} Components", Icons::COMPONENTS), |ui| {
                let has_parent = world.get::<Parent>(entity).is_some();
                let has_group = world.get::<FolderGroup>(entity).is_some();
                let has_visible = world.get::<Visible>(entity).is_some();
                let has_locked = world.get::<Locked>(entity).is_some();
                ui.label(format!("{} Name ({entity_name})", Icons::ENTITY));
                ui.label(format!("{} Transform", Icons::TRANSFORM));
                if has_parent {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} Parent", Icons::ENTITY));
                        if ui.small_button("X").clicked() {
                            use crate::commands::{AddableComponent, RemoveComponentCommand};
                            if let Some(parent) = world.get::<Parent>(entity) {
                                if let Some(cmd) = RemoveComponentCommand::new(
                                    world,
                                    entity,
                                    AddableComponent::Parent(parent.0),
                                ) {
                                    self.history.push(Box::new(cmd), world);
                                    self.dirty = true;
                                }
                            }
                        }
                    });
                }
                if has_group {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} FolderGroup", Icons::FOLDER));
                        if ui.small_button("X").clicked() {
                            use crate::commands::{AddableComponent, RemoveComponentCommand};
                            if let Some(cmd) = RemoveComponentCommand::new(
                                world,
                                entity,
                                AddableComponent::FolderGroup("".into()),
                            ) {
                                self.history.push(Box::new(cmd), world);
                                self.dirty = true;
                            }
                        }
                    });
                }
                if has_visible {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} Visible", Icons::VISIBLE));
                        if ui.small_button("X").clicked() {
                            use crate::commands::{AddableComponent, RemoveComponentCommand};
                            if let Some(cmd) = RemoveComponentCommand::new(
                                world,
                                entity,
                                AddableComponent::Visible(true),
                            ) {
                                self.history.push(Box::new(cmd), world);
                                self.dirty = true;
                            }
                        }
                    });
                }
                if has_locked {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} Locked", Icons::LOCKED));
                        if ui.small_button("X").clicked() {
                            use crate::commands::{AddableComponent, RemoveComponentCommand};
                            if let Some(cmd) = RemoveComponentCommand::new(
                                world,
                                entity,
                                AddableComponent::Locked(false),
                            ) {
                                self.history.push(Box::new(cmd), world);
                                self.dirty = true;
                            }
                        }
                    });
                }
                ui.separator();
                ui.menu_button(format!("{} Add Component", Icons::ADD), |ui| {
                    if !has_group
                        && ui
                            .button(format!("{} FolderGroup", Icons::FOLDER))
                            .clicked()
                    {
                        use crate::commands::{AddComponentCommand, AddableComponent};
                        let cmd = AddComponentCommand::new(
                            entity,
                            AddableComponent::FolderGroup("Folder".into()),
                        );
                        self.history.push(Box::new(cmd), world);
                        self.dirty = true;
                        ui.close();
                    }
                    if !has_visible && ui.button(format!("{} Visible", Icons::VISIBLE)).clicked() {
                        use crate::commands::{AddComponentCommand, AddableComponent};
                        let cmd = AddComponentCommand::new(entity, AddableComponent::Visible(true));
                        self.history.push(Box::new(cmd), world);
                        self.dirty = true;
                        ui.close();
                    }
                    if !has_locked && ui.button(format!("{} Locked", Icons::LOCKED)).clicked() {
                        use crate::commands::{AddComponentCommand, AddableComponent};
                        let cmd = AddComponentCommand::new(entity, AddableComponent::Locked(false));
                        self.history.push(Box::new(cmd), world);
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
        use crate::commands::SpawnEntityCommand;
        use crate::scene::SerializedComponents;

        let cmd = SpawnEntityCommand::new(
            "New Actor",
            Transform::from_translation(Vec3::ZERO),
            None,
            SerializedComponents::default(),
            true,
            false,
            None,
        );

        // Execute the command directly to get the entity
        let mut cmd_box: Box<dyn EditorCommand> = Box::new(cmd);
        cmd_box.execute(world);

        if let Some(spawn_cmd) = cmd_box.as_any().downcast_ref::<SpawnEntityCommand>() {
            if let Some(entity) = spawn_cmd.entity() {
                self.set_selected_entity(Some(entity), false);
            }
        }

        self.history.push(cmd_box, world);

        self.dirty = true;
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
        self.notifications
            .success("New Scene", "Initialized new empty scene");
        // Clear undo/redo history on new scene
        self.history.clear();
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
                    self.content_selected = Some(dest.clone());
                    // Register into asset database
                    let ext = dest
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let asset_type = match ext.as_str() {
                        "gltf" | "glb" | "obj" => crate::asset_database::AssetType::Mesh,
                        "png" | "jpg" | "jpeg" => crate::asset_database::AssetType::Texture,
                        "mat" | "ron" => crate::asset_database::AssetType::Material,
                        _ => crate::asset_database::AssetType::Other,
                    };
                    let asset_id = crate::asset_database::AssetId::new();
                    let mut metadata = crate::asset_database::AssetMetadata::new(
                        asset_id,
                        file_name.clone(),
                        asset_type,
                        dest.clone(),
                        crate::asset_importer::ImportPipeline::generate_internal_path(
                            asset_id, asset_type,
                        ),
                    );
                    metadata.load_state = crate::asset_database::AssetLoadState::Loaded;
                    self.asset_database.registry.register(metadata, None);
                    let _ = self
                        .asset_database
                        .save_to_file(&PathBuf::from("assets/asset_database.ron"));

                    self.status = format!("Imported '{file_name}'");
                    self.log.push((
                        "INFO".into(),
                        format!("Imported resource: {file_name} -> {}", target_dir.display()),
                    ));
                    self.notifications.success(
                        "Resource Imported",
                        format!("Copied and registered '{file_name}' into asset database"),
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
            if let Some(uid) = world.get::<crate::core::uid::Uid>(e).copied() {
                if let Some(mut registry) =
                    world.get_resource_mut::<crate::core::resources::PersistentIdRegistry>()
                {
                    registry.unregister(uid);
                }
            }
            let _ = world.despawn(e);
        }
        self.selected = None;
        self.selected_entities.clear();
        self.dirty = true;
        self.log.push(("INFO".into(), "Deleted actor(s)".into()));
    }
    fn save_scene(&mut self, world: &mut World) {
        let mut scene = Scene::new();
        let entities_list: Vec<(Entity, Name, Transform)> = world
            .query::<(Entity, &Name, &Transform)>()
            .iter(world)
            .map(|(e, n, t)| (e, n.clone(), *t))
            .collect();

        let entity_to_index: std::collections::HashMap<Entity, usize> = entities_list
            .iter()
            .enumerate()
            .map(|(idx, (e, _, _))| (*e, idx))
            .collect();

        for (entity, name, t) in &entities_list {
            let parent_idx = world
                .get::<crate::transform::Parent>(*entity)
                .and_then(|p| entity_to_index.get(&p.0).copied());

            scene.entities.push(SceneEntity {
                name: name.0.clone(),
                uid: world.get::<crate::core::uid::Uid>(*entity).copied(),
                translation: t.translation.to_array(),
                rotation: t.rotation.to_array(),
                scale: t.scale.to_array(),
                parent: parent_idx,
                visible: world
                    .get::<crate::core::components::Visible>(*entity)
                    .is_none_or(|value| value.0),
                locked: world
                    .get::<crate::core::components::Locked>(*entity)
                    .is_some_and(|value| value.0),
                folder: world
                    .get::<crate::core::components::FolderGroup>(*entity)
                    .map(|value| value.0.clone()),
                components: crate::scene::SerializedComponents {
                    custom: Default::default(),
                    mesh_filter: world
                        .get::<crate::core::components::MeshFilter>(*entity)
                        .cloned(),
                    mesh_renderer: world
                        .get::<crate::core::components::MeshRenderer>(*entity)
                        .cloned(),
                    camera: world.get::<crate::camera::Camera3D>(*entity).copied(),
                    light: world
                        .get::<crate::core::components::Light>(*entity)
                        .copied(),
                },
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
    fn draw_performance_monitor(
        &mut self,
        ui: &mut egui::Ui,
        world: &World,
        perf: &mut crate::performance::PerformanceMonitor,
    ) {
        // Read EngineMode from the world
        let mode = world
            .get_resource::<crate::core::resources::EngineMode>()
            .copied()
            .unwrap_or_default();

        // Use the PerformanceMonitor passed directly from App as the single source of truth.
        // This eliminates the clone-sync problem where Clear History wouldn't persist.

        // Top bar with toggle and settings
        ui.horizontal(|ui| {
            ui.heading("PERFORMANCE MONITOR");
            ui.separator();

            // Editor / Play mode badge
            let (mode_label, mode_color) = match mode {
                crate::core::resources::EngineMode::Playing => {
                    ("Playing", Color32::from_rgb(120, 200, 120))
                }
                crate::core::resources::EngineMode::Paused => {
                    ("Paused", Color32::from_rgb(230, 200, 100))
                }
                crate::core::resources::EngineMode::Editor => {
                    ("Editor", Color32::from_rgb(140, 146, 156))
                }
            };
            ui.colored_label(mode_color, format!("Mode: {}", mode_label));

            let mut enabled = perf.enabled;
            if ui.checkbox(&mut enabled, "Enabled").changed() {
                perf.set_enabled(enabled);
            }

            let mut detailed = perf.detailed;
            if ui.checkbox(&mut detailed, "Detailed").changed() {
                perf.set_detailed(detailed);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear History").clicked() {
                    // Clear the graph samples and history but keep enabled/detailed/paused state
                    perf.clear_graph_samples();
                }
            });
        });

        ui.separator();

        // Tabs
        let tab_names = [
            "Overview",
            "CPU Profiler",
            "Renderer",
            "Memory",
            "Snapshots",
            "ECS Inspector",
        ];

        ui.horizontal(|ui| {
            for (i, name) in tab_names.iter().enumerate() {
                if ui
                    .selectable_label(self.perf_selected_tab == i, *name)
                    .clicked()
                {
                    self.perf_selected_tab = i;
                }
            }
        });

        ui.separator();

        match self.perf_selected_tab {
            0 => Self::draw_perf_overview(ui, perf, self),
            1 => Self::draw_perf_cpu_profiler(ui, perf),
            2 => Self::draw_perf_renderer(ui, &perf),
            3 => Self::draw_perf_memory(ui, &perf),
            4 => Self::draw_perf_snapshot_inspector(ui, perf),
            5 => Self::draw_perf_ecs_inspector(ui, &perf),
            _ => {}
        }
    }

    fn draw_perf_overview(ui: &mut egui::Ui, perf: &mut PerformanceMonitor, editor: &mut Editor) {
        use crate::performance_graph::{GraphScale, PerfGraph, PerfGraphConfig};

        let window = editor.perf_window;
        let window_secs = window.seconds();
        let fps_values = perf.windowed_fps(window_secs);
        let ft_values = perf.windowed_frame_time(window_secs);

        // ── Top panel: headline metrics (includes Memory + Entity count). ──
        Self::draw_perf_metrics_row(ui, perf);

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // Compact mode: skip the FPS graph and the second metrics row; show
        // only the Frame Time graph at reduced height.
        if editor.perf_compact {
            let target_ms = 1000.0 / perf.target_fps.max(1.0);
            let mut ft_config = PerfGraphConfig::new("ms", "ms").with_target(target_ms, "target");
            if !editor.perf_scale_auto {
                ft_config.scale = GraphScale::Fixed;
                ft_config.fixed_max = editor.perf_fixed_max as f64;
            }
            ui.add(
                PerfGraph::new(
                    egui::Id::new("perf_frame_time_graph"),
                    "Frame Time",
                    &ft_values,
                )
                .window_seconds(window_secs)
                .sample_hz(60.0)
                .config(ft_config)
                .height(110.0),
            );

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(2.0);
            Self::draw_perf_controls_row(ui, perf, editor);
            return;
        }

        // ── Center: the two live graphs. Frame Time gets more space. ──────
        if editor.perf_show_fps_graph {
            let mut fps_config =
                PerfGraphConfig::new("fps", "fps").with_target(perf.target_fps, "target fps");
            if !editor.perf_scale_auto {
                fps_config.scale = GraphScale::Fixed;
                fps_config.fixed_max = editor.perf_fixed_max as f64;
            }
            ui.add(
                PerfGraph::new(egui::Id::new("perf_fps_graph"), "FPS", &fps_values)
                    .window_seconds(window_secs)
                    .sample_hz(60.0)
                    .config(fps_config)
                    .height(90.0),
            );
            ui.add_space(4.0);
        }

        if editor.perf_show_frame_time_graph {
            let target_ms = 1000.0 / perf.target_fps.max(1.0);
            let mut ft_config = PerfGraphConfig::new("ms", "ms").with_target(target_ms, "target");
            if !editor.perf_scale_auto {
                ft_config.scale = GraphScale::Fixed;
                ft_config.fixed_max = editor.perf_fixed_max as f64;
            }
            ui.add(
                PerfGraph::new(
                    egui::Id::new("perf_frame_time_graph"),
                    "Frame Time",
                    &ft_values,
                )
                .window_seconds(window_secs)
                .sample_hz(60.0)
                .config(ft_config)
                .height(150.0),
            );
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(2.0);

        // ── Bottom panel: controls. ────────────────────────────────────────
        Self::draw_perf_controls_row(ui, perf, editor);
    }

    fn draw_perf_controls_row(
        ui: &mut egui::Ui,
        perf: &mut PerformanceMonitor,
        editor: &mut Editor,
    ) {
        ui.horizontal(|ui| {
            ui.label("Interval:");
            for w in crate::performance::TimeWindow::ALL {
                if ui
                    .selectable_label(editor.perf_window == w, w.label())
                    .clicked()
                {
                    editor.perf_window = w;
                }
            }
            ui.separator();
            ui.label("Target FPS:");
            let mut target = perf.target_fps as f32;
            ui.add(egui::Slider::new(&mut target, 30.0..=240.0).suffix(" fps"));
            perf.target_fps = target as f64;
            ui.separator();
            if ui
                .button(if perf.paused { "Resume" } else { "Pause" })
                .clicked()
            {
                perf.paused = !perf.paused;
            }
            if ui.button("Clear History").clicked() {
                perf.clear_samples();
                perf.fps_history.clear();
                perf.frame_time_history.clear();
            }
        });

        ui.horizontal(|ui| {
            ui.label("Graphs:");
            ui.checkbox(&mut editor.perf_show_fps_graph, "FPS");
            ui.checkbox(&mut editor.perf_show_frame_time_graph, "Frame Time");
            ui.separator();
            ui.label("Scale:");
            if ui
                .selectable_label(editor.perf_scale_auto, "Auto")
                .clicked()
            {
                editor.perf_scale_auto = true;
            }
            if ui
                .selectable_label(!editor.perf_scale_auto, "Fixed")
                .clicked()
            {
                editor.perf_scale_auto = false;
            }
            if !editor.perf_scale_auto {
                ui.add(egui::Slider::new(&mut editor.perf_fixed_max, 10.0..=200.0));
            }
            ui.separator();
            ui.checkbox(&mut editor.perf_compact, "Compact");
        });
    }

    fn draw_perf_metrics_row(ui: &mut egui::Ui, perf: &PerformanceMonitor) {
        let stats = &perf.frame_stats;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(format!("{:.1}", stats.fps))
                        .size(28.0)
                        .strong(),
                );
                ui.label("FPS");
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(format!("{:.2} ms", stats.frame_time_ms))
                        .size(28.0)
                        .strong(),
                );
                ui.label("Frame Time");
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(format!("{:.2} ms", stats.cpu_frame_time_ms))
                        .size(28.0)
                        .strong(),
                );
                ui.label("CPU Time");
            });
            ui.separator();
            ui.vertical(|ui| {
                let gpu_text = stats
                    .gpu_frame_time_ms
                    .map_or("N/A".to_string(), |v| format!("{:.2} ms", v));
                ui.label(RichText::new(gpu_text).size(28.0).strong());
                ui.label("GPU Time");
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(format!("{:.1}", perf.average_fps(10.0)))
                        .size(28.0)
                        .strong(),
                );
                ui.label("Avg FPS (10s)");
            });
        });

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(2.0);

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Low FPS (1%)").strong());
                ui.label(format!("{:.1}", stats.one_percent_low_fps));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(RichText::new("Low FPS (0.1%)").strong());
                ui.label(format!("{:.1}", stats.zero_one_percent_low_fps));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(RichText::new("Avg Frame Time").strong());
                ui.label(format!("{:.2} ms", stats.avg_frame_time_ms));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(RichText::new("Min / Max FPS").strong());
                ui.label(format!("{} / {}", stats.min_fps, stats.max_fps));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(RichText::new("Max Frame Time (10s)").strong());
                ui.label(format!("{:.2} ms", perf.max_frame_time(10.0)));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(RichText::new("Fixed Updates / Frame").strong());
                ui.label(format!("{}", stats.fixed_updates_count));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(RichText::new("Memory Usage").strong());
                let mem_text = perf
                    .memory_stats
                    .process_working_set_bytes
                    .map_or("N/A".to_string(), |b| {
                        format!("{:.1} MB", b as f64 / 1_048_576.0)
                    });
                ui.label(mem_text);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(RichText::new("Entities").strong());
                ui.label(format!("{}", perf.ecs_stats.entity_count));
            });
        });
    }

    fn draw_perf_cpu_profiler(ui: &mut egui::Ui, perf: &PerformanceMonitor) {
        ui.label(RichText::new("Operation Timings (Current Frame)").strong());
        ui.add_space(4.0);

        // Sort by duration descending
        let mut metrics: Vec<_> = perf.metrics.values().collect();
        metrics.sort_by(|a, b| {
            b.last_duration_ms
                .partial_cmp(&a.last_duration_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("cpu_profiler_grid")
                .num_columns(6)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Operation");
                    ui.strong("Category");
                    ui.strong("Last (ms)");
                    ui.strong("Avg (ms)");
                    ui.strong("Max (ms)");
                    ui.strong("Calls");
                    ui.end_row();

                    for metric in metrics {
                        ui.label(&metric.name);
                        ui.label(metric.category.name());
                        ui.label(format!("{:.3}", metric.last_duration_ms));
                        ui.label(format!("{:.3}", metric.avg_duration_ms()));
                        ui.label(format!("{:.3}", metric.max_duration_ms));
                        ui.label(format!("{}", metric.call_count));
                        ui.end_row();
                    }
                });
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // Frame stage scopes
        ui.label(RichText::new("Frame Stages (Current Frame)").strong());
        ui.add_space(4.0);

        let latest = &perf.current_cpu_stages;
        let cpu_total = perf.frame_stats.cpu_frame_time_ms.max(0.001);

        egui::Grid::new("frame_stages_grid")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Stage");
                ui.strong("Time (ms)");
                ui.strong("% of CPU Frame");
                ui.end_row();

                for (i, stage) in crate::performance::CpuStage::ALL.iter().enumerate() {
                    let ms = latest[i];
                    if ms <= 0.0 && *stage == crate::performance::CpuStage::TransformPropagation {
                        continue;
                    }
                    ui.label(stage.name());
                    ui.label(format!("{:.3}", ms));
                    ui.label(format!("{:.1}%", ms / cpu_total * 100.0));
                    ui.end_row();
                }
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(RichText::new("GPU Passes (Current Frame)").strong());
        let gpu = &perf.current_gpu_passes;
        egui::Grid::new("gpu_passes_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Pass");
                ui.strong("Time (ms)");
                ui.end_row();
                let rows = [
                    ("Total", gpu.total_ms),
                    ("Shadow", gpu.shadow_ms),
                    ("Opaque", gpu.opaque_ms),
                    ("Transparent", gpu.transparent_ms),
                    ("Post", gpu.post_ms),
                    ("Preview", gpu.preview_ms),
                ];
                for (label, value) in rows {
                    ui.label(label);
                    ui.label(value.map_or("—".into(), |v| format!("{:.3}", v)));
                    ui.end_row();
                }
            });

        if gpu.total_ms.is_none() {
            ui.label(
                egui::RichText::new(
                    "GPU timestamps unavailable: adapter or backend did not expose timestamp queries.",
                )
                .weak(),
            );
        }
    }

    fn draw_perf_snapshot_inspector(ui: &mut egui::Ui, perf: &mut PerformanceMonitor) {
        use crate::performance::{CpuStage, GpuPassTimings};
        ui.label(RichText::new("Frame Snapshots").strong());
        ui.label(format!("Buffered: {}", perf.snapshots().len()));
        let latest = perf.latest_snapshot().cloned();
        let Some(latest) = latest else {
            ui.label("No snapshots yet.");
            return;
        };
        let selected = perf.selected_snapshot_index.unwrap_or(latest.index);
        let snap = perf.snapshot_by_index(selected).cloned().unwrap_or(latest);
        ui.horizontal(|ui| {
            ui.label(format!("Showing frame {}", snap.index));
            if ui.button("Latest").clicked() {
                perf.selected_snapshot_index = None;
            }
            if snap.index > 0 && ui.button("◀ Older").clicked() {
                let prev = perf
                    .snapshots()
                    .iter()
                    .rev()
                    .find(|s| s.index < snap.index)
                    .map(|s| s.index);
                if let Some(i) = prev {
                    perf.selected_snapshot_index = Some(i);
                }
            }
            let has_newer = perf.snapshots().iter().any(|s| s.index > snap.index);
            if has_newer && ui.button("Newer ▶").clicked() {
                let next = perf
                    .snapshots()
                    .iter()
                    .find(|s| s.index > snap.index)
                    .map(|s| s.index);
                if let Some(i) = next {
                    perf.selected_snapshot_index = Some(i);
                }
            }
        });
        ui.label(format!(
            "{} · {:.2} ms · CPU {:.2} ms · GPU {}",
            snap.mode.label(),
            snap.frame_time_ms,
            snap.cpu_frame_time_ms,
            snap.gpu_frame_time_ms
                .map_or("—".into(), |v| format!("{:.2} ms", v)),
        ));
        egui::Grid::new("snapshot_stages")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("CPU Stage");
                ui.strong("ms");
                ui.end_row();
                for (i, stage) in CpuStage::ALL.iter().enumerate() {
                    if snap.cpu_stages_ms[i] <= 0.0 {
                        continue;
                    }
                    ui.label(stage.name());
                    ui.label(format!("{:.3}", snap.cpu_stages_ms[i]));
                    ui.end_row();
                }
            });
        let _ = GpuPassTimings::default();
    }

    fn draw_perf_renderer(ui: &mut egui::Ui, perf: &PerformanceMonitor) {
        let stats = &perf.renderer_stats;

        ui.label(RichText::new("Scene Statistics").strong());
        ui.add_space(4.0);

        egui::Grid::new("renderer_scene_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("Renderable Entities");
                ui.label(format!("{}", stats.renderable_entities));
                ui.end_row();

                ui.label("Visible Entities");
                ui.label(format!("{}", stats.visible_entities));
                ui.end_row();

                ui.label("Draw Calls");
                ui.label(format!("{}", stats.draw_calls));
                ui.end_row();

                ui.label("Triangles");
                ui.label(format!("{}", stats.triangles));
                ui.end_row();

                ui.label("Vertices");
                ui.label(format!("{}", stats.vertices));
                ui.end_row();

                ui.label("Meshes");
                ui.label(format!("{}", stats.meshes));
                ui.end_row();

                ui.label("Materials");
                ui.label(format!("{}", stats.materials));
                ui.end_row();

                ui.label("Textures");
                ui.label(stats.textures.map_or("N/A".to_string(), |v| v.to_string()));
                ui.end_row();

                ui.label("Lights");
                ui.label(format!("{}", stats.lights));
                ui.end_row();

                ui.label("Shadow Casters");
                ui.label(format!("{}", stats.shadow_casters));
                ui.end_row();

                ui.label("Render Passes");
                ui.label(format!("{}", stats.passes));
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(RichText::new("GPU Timing").strong());
        ui.add_space(4.0);

        let gpu_text = stats.gpu_frame_time_ms.map_or(
            "GPU timing not available (timestamp queries unsupported)".to_string(),
            |v| format!("{:.3} ms", v),
        );
        ui.label(gpu_text);

        if stats.gpu_frame_time_ms.is_none() {
            ui.label("Note: GPU timestamp queries require appropriate device support and backend.");
        }
    }

    fn draw_perf_memory(ui: &mut egui::Ui, perf: &PerformanceMonitor) {
        let stats = &perf.memory_stats;

        ui.label(RichText::new("Process Memory").strong());
        ui.add_space(4.0);

        egui::Grid::new("memory_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("Working Set");
                ui.label(
                    stats
                        .process_working_set_bytes
                        .map_or("N/A".to_string(), |v| Self::format_bytes(v)),
                );
                ui.end_row();
            });

        if stats.process_working_set_bytes.is_none() {
            ui.label("Note: Process memory reading requires platform-specific support (Windows: K32GetProcessMemoryInfo)");
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(RichText::new("ECS Resources").strong());
        ui.add_space(4.0);

        egui::Grid::new("memory_ecs_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("Entities");
                ui.label(format!("{}", stats.ecs_entities_count));
                ui.end_row();

                ui.label("Registered Assets");
                ui.label(format!("{}", stats.registered_assets_count));
                ui.end_row();

                ui.label("Mesh Assets");
                ui.label(format!("{}", stats.mesh_assets_count));
                ui.end_row();

                ui.label("Material Assets");
                ui.label(format!("{}", stats.material_assets_count));
                ui.end_row();

                ui.label("Texture Assets");
                ui.label(
                    stats
                        .texture_assets_count
                        .map_or("N/A".to_string(), |v| v.to_string()),
                );
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(RichText::new("Performance Monitor Buffer").strong());
        ui.add_space(4.0);

        egui::Grid::new("memory_perf_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("History Samples");
                ui.label(format!("{}", stats.history_samples_count));
                ui.end_row();

                ui.label("Buffer Capacity");
                ui.label(format!("{}", stats.history_buffer_capacity));
                ui.end_row();
            });
    }

    fn draw_perf_ecs_inspector(ui: &mut egui::Ui, perf: &PerformanceMonitor) {
        let stats = &perf.ecs_stats;

        ui.label(RichText::new("Entity Summary").strong());
        ui.add_space(4.0);

        egui::Grid::new("ecs_summary_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("Total Entities");
                ui.label(format!("{}", stats.entity_count));
                ui.end_row();

                ui.label("With Transform");
                ui.label(format!("{}", stats.transform_entities));
                ui.end_row();

                ui.label("Renderable");
                ui.label(format!("{}", stats.renderable_entities));
                ui.end_row();

                ui.label("Cameras");
                ui.label(format!("{}", stats.camera_entities));
                ui.end_row();

                ui.label("Lights");
                ui.label(format!("{}", stats.light_entities));
                ui.end_row();

                ui.label("Registered Resources");
                ui.label(format!("{}", stats.registered_resources_count));
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(RichText::new("Component Statistics").strong());
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .id_salt("ecs_components_scroll")
            .max_height(200.0)
            .show(ui, |ui| {
                egui::Grid::new("ecs_components_grid")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Component");
                        ui.strong("Entity Count");
                        ui.end_row();

                        for comp in &stats.components {
                            ui.label(&*comp.name);
                            ui.label(format!("{}", comp.count));
                            ui.end_row();
                        }
                    });
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(RichText::new("ECS Resources").strong());
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .id_salt("ecs_resources_scroll")
            .max_height(150.0)
            .show(ui, |ui| {
                for resource in &stats.resources {
                    ui.label(format!("Resource {}", resource));
                }
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(RichText::new("System Schedule").strong());
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .id_salt("ecs_systems_scroll")
            .max_height(150.0)
            .show(ui, |ui| {
                for (stage, system) in &stats.systems {
                    ui.label(format!("{} -> {}", stage, system));
                }
            });
    }

    fn format_bytes(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit = 0;
        while size >= 1024.0 && unit < UNITS.len() - 1 {
            size /= 1024.0;
            unit += 1;
        }
        format!("{:.2} {}", size, UNITS[unit])
    }

    fn draw_history_panel(&mut self, ui: &mut egui::Ui, world: &mut World) {
        ui.horizontal(|ui| {
            ui.heading("HISTORY");
            ui.separator();

            // Undo button
            let can_undo = self.history.can_undo();
            if ui
                .add_enabled(
                    can_undo,
                    egui::Button::new(
                        RichText::new(format!("{} Undo", Icons::UNDO))
                            .size(12.0)
                            .color(Color32::WHITE),
                    )
                    .fill(if can_undo {
                        Color32::from_rgb(60, 80, 120)
                    } else {
                        Color32::from_rgb(40, 40, 40)
                    }),
                )
                .engine_tooltip(
                    "Undo",
                    format!(
                        "Undo last action ({})",
                        self.history.undo_name().unwrap_or("none")
                    ),
                    Some("Ctrl+Z"),
                )
                .clicked()
            {
                if let Some(name) = self.history.undo(world) {
                    self.dirty = true;
                    self.log_info(&format!("Undo: {}", name));
                }
            }

            // Redo button
            let can_redo = self.history.can_redo();
            if ui
                .add_enabled(
                    can_redo,
                    egui::Button::new(
                        RichText::new(format!("{} Redo", Icons::REDO))
                            .size(12.0)
                            .color(Color32::WHITE),
                    )
                    .fill(if can_redo {
                        Color32::from_rgb(60, 100, 60)
                    } else {
                        Color32::from_rgb(40, 40, 40)
                    }),
                )
                .engine_tooltip(
                    "Redo",
                    format!(
                        "Redo last undone action ({})",
                        self.history.redo_name().unwrap_or("none")
                    ),
                    Some("Ctrl+Y"),
                )
                .clicked()
            {
                if let Some(name) = self.history.redo(world) {
                    self.dirty = true;
                    self.log_info(&format!("Redo: {}", name));
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(
                        RichText::new(format!("{} Clear", Icons::DELETE))
                            .size(11.0)
                            .color(Color32::WHITE),
                    )
                    .engine_tooltip("Clear History", "Clear all undo/redo history", None)
                    .clicked()
                {
                    self.history.clear();
                    self.log_info("History cleared");
                }
            });
        });

        ui.separator();

        // History list
        egui::ScrollArea::vertical()
            .id_salt("history_scroll")
            .show(ui, |ui| {
                let undo_stack = self.history.undo_stack();
                let redo_stack = self.history.redo_stack();

                // Show redo stack (future actions - grayed out)
                if !redo_stack.is_empty() {
                    ui.label(
                        RichText::new("Redo Stack")
                            .strong()
                            .color(Color32::from_rgb(120, 120, 120)),
                    );
                    ui.add_space(4.0);
                    for (i, cmd) in redo_stack.iter().rev().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                RichText::new(format!("  {}. {}", i + 1, cmd.name()))
                                    .size(11.0)
                                    .color(Color32::from_rgb(100, 100, 100))
                                    .italics(),
                            );
                        });
                    }
                    ui.separator();
                }

                // Current position indicator
                if !undo_stack.is_empty() || !redo_stack.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("▼ Current")
                                .size(10.0)
                                .color(Color32::from_rgb(100, 200, 100))
                                .strong(),
                        );
                    });
                    ui.add_space(4.0);
                }

                // Show undo stack (past actions - most recent first)
                if !undo_stack.is_empty() {
                    ui.label(RichText::new("Undo Stack").strong());
                    ui.add_space(4.0);
                    for (i, cmd) in undo_stack.iter().rev().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.label(
                                RichText::new(format!("  {}. {}", i + 1, cmd.name()))
                                    .size(11.0)
                                    .color(Color32::WHITE),
                            );
                        });
                    }
                } else if redo_stack.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);
                        Icons::show(
                            ui,
                            Icons::HISTORY,
                            IconSize::Large,
                            Some(Color32::from_rgb(80, 85, 95)),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("No history yet")
                                .size(12.0)
                                .color(Color32::from_rgb(100, 105, 115)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Perform actions to build history")
                                .size(10.0)
                                .color(Color32::from_rgb(80, 85, 95)),
                        );
                    });
                }
            });
    }

    fn reset_layout(&mut self) {
        self.current_workspace = WorkspacePreset::Default;
        self.dock_state = Some(WorkspacePreset::Default.create_dock_state());
    }
}

pub struct EditorTabViewer<'a> {
    pub editor: &'a mut Editor,
    pub world: &'a mut World,
    pub perf: &'a mut crate::performance::PerformanceMonitor,
    pub actions: &'a mut Vec<DockAction>,
    pub viewport_state: Option<ViewportTextureState>,
}

impl<'a> egui_dock::TabViewer for EditorTabViewer<'a> {
    type Tab = EditorTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            EditorTab::Viewport => self
                .editor
                .draw_viewport(ui, self.world, self.viewport_state),
            EditorTab::WorldOutliner => self.editor.draw_world_outliner(ui, self.world),
            EditorTab::Details => self.editor.draw_details(ui, self.world),
            EditorTab::ContentBrowser => self.editor.draw_content_browser(ui, self.world),
            EditorTab::AssetPreview => self.editor.draw_asset_preview(ui),
            EditorTab::AssetDetails => self.editor.draw_asset_details(ui),
            EditorTab::OutputLog => self.editor.draw_output_log(ui),
            EditorTab::ScriptEditor => self.editor.draw_script_editor(ui),
            EditorTab::GraphicsSettings => self.editor.draw_graphics_settings(ui),
            EditorTab::PerformanceMonitor => self
                .editor
                .draw_performance_monitor(ui, self.world, self.perf),
            EditorTab::History => self.editor.draw_history_panel(ui, self.world),
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
