use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use crate::{
    assets::{AssetManager, Handle},
    camera::Camera3D,
    core::components::{CameraMarker, Light, LightKind, Visible},
    quality::{QualityPreset, ShaderVariant, ShadowQuality},
    transform::GlobalTransform,
    ui_new::{Rect, Ui, UiRenderer},
};
use bevy_ecs::{component::Component, entity::Entity, resource::Resource, world::World};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;
use winit::window::Window;

const SHADOW_SIZE: u32 = 2048;
const CUBE_VERTEX_COUNT: usize = 24;
const CUBE_INDEX_COUNT: u32 = 36;

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct ModelUniform {
    model: [[f32; 4]; 4],
    normal_matrix: [[f32; 4]; 4],
}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct LightUniform {
    view_proj: [[f32; 4]; 4],
    params: [f32; 4],
    direction: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct MaterialUniform {
    color: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
    normal: [f32; 3],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

fn cube_vertices() -> [Vertex; CUBE_VERTEX_COUNT] {
    let faces = [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let corners = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    let colors: [[f32; 3]; 6] = [
        [1.0, 0.2, 0.2],
        [0.2, 1.0, 0.2],
        [0.2, 0.2, 1.0],
        [1.0, 1.0, 0.2],
        [0.2, 1.0, 1.0],
        [1.0, 0.2, 1.0],
    ];
    let mut vertices = [Vertex::default(); CUBE_VERTEX_COUNT];
    for (face_index, (normal, tangent, bitangent)) in faces.into_iter().enumerate() {
        for (corner_index, corner) in corners.into_iter().enumerate() {
            vertices[face_index * 4 + corner_index] = Vertex {
                position: [
                    normal[0] + tangent[0] * corner[0] + bitangent[0] * corner[1],
                    normal[1] + tangent[1] * corner[0] + bitangent[1] * corner[1],
                    normal[2] + tangent[2] * corner[0] + bitangent[2] * corner[1],
                ],
                color: colors[face_index],
                normal,
            };
        }
    }
    vertices
}

fn cube_indices() -> [u16; CUBE_INDEX_COUNT as usize] {
    let mut indices = [0u16; CUBE_INDEX_COUNT as usize];
    for face in 0..6 {
        let offset = (face * 4) as u16;
        indices[face * 6..face * 6 + 6].copy_from_slice(&[
            offset,
            offset + 1,
            offset + 2,
            offset,
            offset + 2,
            offset + 3,
        ]);
    }
    indices
}

type ModelKey = (u64, u64);

pub struct MeshAsset {
    pub(crate) vertex_buffer: wgpu::Buffer,
    pub(crate) index_buffer: wgpu::Buffer,
    pub(crate) index_count: u32,
    pub(crate) vertex_count: u32,
    pub(crate) index_format: wgpu::IndexFormat,
    pub(crate) model_layout: wgpu::BindGroupLayout,
    pub(crate) model_buffers: HashMap<ModelKey, wgpu::Buffer>,
    pub(crate) model_bind_groups: HashMap<ModelKey, wgpu::BindGroup>,
}

pub struct MaterialAsset {
    pub pipeline_1x: wgpu::RenderPipeline,
    pub wireframe_pipeline_1x: wgpu::RenderPipeline,
    pub pipeline_4x: wgpu::RenderPipeline,
    pub wireframe_pipeline_4x: wgpu::RenderPipeline,
    pub color_buffer: wgpu::Buffer,
    pub color_bind_group: wgpu::BindGroup,
    pub color_bind_group_layout: wgpu::BindGroupLayout,
}

pub struct TextureAsset {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bind_group: Option<wgpu::BindGroup>,
    pub width: u32,
    pub height: u32,
}

#[derive(Component, Clone, Copy)]
pub struct Renderable {
    pub mesh: Handle<MeshAsset>,
    pub material: Handle<MaterialAsset>,
}

/// The renderer-facing view of world data. It contains only references and
/// transforms needed by the GPU passes, keeping ECS components independent of
/// wgpu resources.
#[derive(Clone, Copy)]
pub struct RenderableInstance {
    pub entity: Entity,
    pub renderable: Renderable,
    pub global_transform: glam::Mat4,
}

#[derive(Default)]
pub struct RenderSnapshot {
    pub renderables: Vec<RenderableInstance>,
    pub camera: Option<(Camera3D, GlobalTransform)>,
    pub directional_light: Option<([f32; 3], Vec3)>,
}

pub fn collect_render_snapshot(world: &mut World) -> RenderSnapshot {
    let renderables = collect_visible_renderables(world);
    let camera = world
        .query::<(
            &Camera3D,
            &GlobalTransform,
            Option<&CameraMarker>,
            Option<&Visible>,
        )>()
        .iter(world)
        .filter(|(_, _, marker, visible)| {
            marker.is_none_or(|marker| marker.is_main) && visible.is_none_or(|visible| visible.0)
        })
        .next()
        .map(|(camera, transform, _, _)| (*camera, *transform));
    let directional_light = world
        .query::<(&Light, &GlobalTransform, Option<&Visible>)>()
        .iter(world)
        .filter(|(_, _, visible)| visible.is_none_or(|visible| visible.0))
        .find_map(|(light, transform, _)| match light.kind {
            LightKind::Directional => Some((
                light.color,
                transform
                    .0
                    .transform_vector3(Vec3::NEG_Z)
                    .normalize_or_zero(),
            )),
            _ => None,
        });
    RenderSnapshot {
        renderables,
        camera,
        directional_light,
    }
}

pub fn collect_visible_renderables(world: &mut World) -> Vec<RenderableInstance> {
    let mut query = world.query::<(Entity, &Renderable, &GlobalTransform, Option<&Visible>)>();
    query
        .iter(world)
        .filter(|(_, _, _, visible)| visible.is_none_or(|visible| visible.0))
        .map(|(entity, renderable, transform, _)| RenderableInstance {
            entity,
            renderable: *renderable,
            global_transform: transform.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::Transform;

    #[test]
    fn renderer_snapshot_filters_hidden_entities() {
        let mut world = World::new();
        let visible_mesh = Handle::<MeshAsset>::from_id(1);
        let visible_material = Handle::<MaterialAsset>::from_id(2);
        let hidden_mesh = Handle::<MeshAsset>::from_id(3);
        let hidden_material = Handle::<MaterialAsset>::from_id(4);
        world.spawn((
            Renderable {
                mesh: visible_mesh,
                material: visible_material,
            },
            Transform::default(),
            GlobalTransform::default(),
            Visible(true),
        ));
        world.spawn((
            Renderable {
                mesh: hidden_mesh,
                material: hidden_material,
            },
            Transform::default(),
            GlobalTransform::default(),
            Visible(false),
        ));

        let visible = collect_visible_renderables(&mut world);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].renderable.mesh.id(), visible_mesh.id());
    }
}

#[derive(Resource)]
pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub window_size: winit::dpi::PhysicalSize<u32>,
    pub render_scale_percent: u32,
    pub post_processing: bool,
    pub msaa_samples: u32,
    pub shadow_quality: ShadowQuality,
    pub mesh_assets: AssetManager<MeshAsset>,
    pub material_assets: AssetManager<MaterialAsset>,
    pub texture_assets: AssetManager<TextureAsset>,
    pub gpu_registry: crate::assets::GpuResourceRegistry,
    pub cube_mesh: Handle<MeshAsset>,
    pub cube_material: Handle<MaterialAsset>,
    pub red_material: Handle<MaterialAsset>,
    pub green_material: Handle<MaterialAsset>,
    pub blue_material: Handle<MaterialAsset>,
    pub color_layout: wgpu::BindGroupLayout,
    pub model_layout: wgpu::BindGroupLayout,
    pub camera_buffer: wgpu::Buffer,
    pub camera_bind_group: wgpu::BindGroup,
    pub camera_layout: wgpu::BindGroupLayout,
    pub light_buffer: wgpu::Buffer,
    pub light_layout: wgpu::BindGroupLayout,
    pub light_bind_group: wgpu::BindGroup,
    pub shadow_depth_texture: wgpu::Texture,
    pub shadow_depth_view: wgpu::TextureView,
    pub shadow_sample_view: wgpu::TextureView,
    pub shadow_sampler: wgpu::Sampler,
    pub shadow_pipeline: wgpu::RenderPipeline,
    pub depth_view: Option<wgpu::TextureView>,
    pub msaa_view: Option<wgpu::TextureView>,
    pub scene_view: Option<wgpu::TextureView>,
    /// Offscreen target used to render the scene into the editor's Viewport panel.
    pub viewport_texture: Option<wgpu::Texture>,
    pub viewport_view: Option<wgpu::TextureView>,
    pub viewport_depth: Option<wgpu::TextureView>,
    pub viewport_size: (u32, u32),
    /// Offscreen target used to render asset previews in the editor.
    pub preview_texture: Option<wgpu::Texture>,
    pub preview_view: Option<wgpu::TextureView>,
    pub preview_depth: Option<wgpu::TextureView>,
    pub preview_size: (u32, u32),
    pub scene_sampler: wgpu::Sampler,
    pub post_pipeline: Option<wgpu::RenderPipeline>,
    pub post_bind_group: Option<wgpu::BindGroup>,
    pub post_shader: wgpu::ShaderModule,
    pub post_pipeline_layout: wgpu::PipelineLayout,
    pub color_targets: [Option<wgpu::ColorTargetState>; 1],
    pub camera_position: Vec3,
    pub camera_target: Vec3,
    pub camera_fov: f32,
    pub camera_near: f32,
    pub camera_far: f32,
    pub camera_angle: f32,
    pub wireframe_mode: bool,
    pub ui: Option<Ui>,
    pub ui_renderer: Option<UiRenderer>,
    pub fps: f32,
    pub entity_count: usize,
    pub renderable_count: usize,
    pub preset_name: String,
    pub menu_open: Option<usize>,
    pub show_stats: bool,
    pub frame_time_ms: f64,
    pub frame_draw_calls: u32,
    pub frame_triangles: u64,
    pub frame_vertices: u64,
    pub frame_shadow_casters: u64,
    pub frame_passes: u32,
    pub _shader_variant: PhantomData<ShaderVariant>,
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        preset: QualityPreset,
    ) -> Result<Self, wgpu::SurfaceError> {
        let settings = preset.settings();
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            flags: wgpu::InstanceFlags::empty(),
            ..wgpu::InstanceDescriptor::from_env_or_default()
        });
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|_| wgpu::SurfaceError::Other)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|_| wgpu::SurfaceError::Other)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("device"),
            required_features: adapter.features() & wgpu::Features::POLYGON_MODE_LINE,
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::default(),
        }))
        .map_err(|_| wgpu::SurfaceError::Other)?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let present_mode = if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            wgpu::PresentMode::Fifo
        } else {
            surface_caps.present_modes[0]
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let shader_source = match settings.shader_variant {
            ShaderVariant::Fast => include_str!("shader_fast.wgsl"),
            ShaderVariant::Standard => include_str!("shader_standard.wgsl"),
            ShaderVariant::Quality => include_str!("shader_quality.wgsl"),
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("light"),
            size: std::mem::size_of::<LightUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shadow_depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow depth"),
            size: wgpu::Extent3d {
                width: SHADOW_SIZE,
                height: SHADOW_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let shadow_depth_view =
            shadow_depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let shadow_sample_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow sample"),
            size: wgpu::Extent3d {
                width: SHADOW_SIZE,
                height: SHADOW_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_sample_view =
            shadow_sample_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light bind group"),
            layout: &light_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: light_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&shadow_sample_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
        });

        let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let color_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material color layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene pipeline layout"),
            bind_group_layouts: &[&camera_layout, &model_layout, &light_layout, &color_layout],
            push_constant_ranges: &[],
        });

        let depth_stencil = wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let color_targets = [Some(wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let create_scene_pipeline = |label, primitive, sample_count| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Vertex::layout()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &color_targets,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive,
                depth_stencil: Some(depth_stencil.clone()),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            })
        };
        let primitive = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        };
        let color_targets = [Some(wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];

        let pipeline_1x = create_scene_pipeline("scene pipeline 1x", primitive, 1);
        let pipeline_4x = create_scene_pipeline("scene pipeline 4x", primitive, 4);

        let wireframe_primitive = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Line,
            unclipped_depth: false,
            conservative: false,
        };

        let wireframe_pipeline_1x =
            create_scene_pipeline("wireframe pipeline 1x", wireframe_primitive, 1);
        let wireframe_pipeline_4x =
            create_scene_pipeline("wireframe pipeline 4x", wireframe_primitive, 4);

        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                buffers: &[Vertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            primitive,
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let vertex_data = cube_vertices();
        let vertex_bytes: &[u8] = bytemuck::cast_slice(&vertex_data);
        let index_data: [u16; CUBE_INDEX_COUNT as usize] = cube_indices();
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube vertices"),
            contents: vertex_bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube indices"),
            contents: bytemuck::cast_slice(&index_data),
            usage: wgpu::BufferUsages::INDEX,
        });

        let mut mesh_assets: AssetManager<MeshAsset> = AssetManager::default();
        let mut material_assets: AssetManager<MaterialAsset> = AssetManager::default();
        let cube_mesh = mesh_assets.insert(
            "builtin/cube".into(),
            MeshAsset {
                vertex_buffer,
                index_buffer,
                index_count: index_data.len() as u32,
                vertex_count: vertex_data.len() as u32,
                index_format: wgpu::IndexFormat::Uint16,
                model_layout: model_layout.clone(),
                model_buffers: HashMap::new(),
                model_bind_groups: HashMap::new(),
            },
        );

        let material_color_layout = color_layout.clone();

        let create_material = |device: &wgpu::Device,
                               queue: &wgpu::Queue,
                               color: [f32; 4],
                               pipeline_1x: &wgpu::RenderPipeline,
                               wireframe_pipeline_1x: &wgpu::RenderPipeline,
                               pipeline_4x: &wgpu::RenderPipeline,
                               wireframe_pipeline_4x: &wgpu::RenderPipeline,
                               layout: &wgpu::BindGroupLayout|
         -> MaterialAsset {
            let color_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("material color"),
                size: std::mem::size_of::<MaterialUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(
                &color_buffer,
                0,
                bytemuck::bytes_of(&MaterialUniform { color }),
            );

            let color_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material color bind group"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: color_buffer.as_entire_binding(),
                }],
            });

            MaterialAsset {
                pipeline_1x: pipeline_1x.clone(),
                wireframe_pipeline_1x: wireframe_pipeline_1x.clone(),
                pipeline_4x: pipeline_4x.clone(),
                wireframe_pipeline_4x: wireframe_pipeline_4x.clone(),
                color_buffer,
                color_bind_group,
                color_bind_group_layout: layout.clone(),
            }
        };

        let cube_material = material_assets.insert(
            "builtin/color".into(),
            create_material(
                &device,
                &queue,
                [1.0, 1.0, 1.0, 1.0],
                &pipeline_1x,
                &wireframe_pipeline_1x,
                &pipeline_4x,
                &wireframe_pipeline_4x,
                &material_color_layout,
            ),
        );
        let red_material = material_assets.insert(
            "builtin/red".into(),
            create_material(
                &device,
                &queue,
                [1.0, 0.0, 0.0, 1.0],
                &pipeline_1x,
                &wireframe_pipeline_1x,
                &pipeline_4x,
                &wireframe_pipeline_4x,
                &material_color_layout,
            ),
        );
        let green_material = material_assets.insert(
            "builtin/green".into(),
            create_material(
                &device,
                &queue,
                [0.0, 1.0, 0.0, 1.0],
                &pipeline_1x,
                &wireframe_pipeline_1x,
                &pipeline_4x,
                &wireframe_pipeline_4x,
                &material_color_layout,
            ),
        );
        let blue_material = material_assets.insert(
            "builtin/blue".into(),
            create_material(
                &device,
                &queue,
                [0.0, 0.0, 1.0, 1.0],
                &pipeline_1x,
                &wireframe_pipeline_1x,
                &pipeline_4x,
                &wireframe_pipeline_4x,
                &material_color_layout,
            ),
        );

        let scene_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("postprocess.wgsl").into()),
        });
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post pipeline layout"),
            bind_group_layouts: &[&post_layout],
            push_constant_ranges: &[],
        });
        let post_pipeline = if settings.post_processing {
            Some(
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("post pipeline"),
                    layout: Some(&post_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &post_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &post_shader,
                        entry_point: Some("fs_main"),
                        targets: &color_targets,
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                }),
            )
        } else {
            None
        };

        let ui = Ui::new();
        let ui_renderer = UiRenderer::new(&device, &queue, surface_format);

        let mut renderer = Self {
            surface,
            device,
            queue,
            config,
            window_size: size,
            render_scale_percent: settings.render_scale_percent,
            post_processing: settings.post_processing,
            msaa_samples: settings.msaa_samples,
            shadow_quality: settings.shadow_quality,
            mesh_assets,
            material_assets,
            texture_assets: AssetManager::default(),
            gpu_registry: crate::assets::GpuResourceRegistry::default(),
            cube_mesh,
            cube_material,
            red_material,
            green_material,
            blue_material,
            camera_buffer,
            camera_bind_group,
            camera_layout,
            light_buffer,
            light_layout,
            light_bind_group,
            shadow_depth_texture,
            shadow_depth_view,
            shadow_sample_view,
            shadow_sampler,
            shadow_pipeline,
            depth_view: None,
            msaa_view: None,
            scene_view: None,
            viewport_texture: None,
            viewport_view: None,
            viewport_depth: None,
            viewport_size: (1, 1),
            preview_texture: None,
            preview_view: None,
            preview_depth: None,
            preview_size: (1, 1),
            scene_sampler,
            post_pipeline,
            post_bind_group: None,
            post_shader,
            post_pipeline_layout,
            color_targets,
            camera_position: Vec3::new(0.0, 3.0, 6.0),
            camera_target: Vec3::ZERO,
            camera_fov: std::f32::consts::FRAC_PI_4,
            camera_near: 0.1,
            camera_far: 100.0,
            camera_angle: 0.0,
            wireframe_mode: false,
            ui: Some(ui),
            ui_renderer: Some(ui_renderer),
            fps: 0.0,
            entity_count: 0,
            renderable_count: 0,
            preset_name: "Balanced".to_string(),
            menu_open: None,
            show_stats: true,
            frame_time_ms: 0.0,
            frame_draw_calls: 0,
            frame_triangles: 0,
            frame_vertices: 0,
            frame_shadow_casters: 0,
            frame_passes: 0,
            color_layout,
            model_layout,
            _shader_variant: PhantomData,
        };
        // Initialize render targets (depth, MSAA, scene texture for post-processing)
        renderer.configure_targets();
        Ok(renderer)
    }

    pub fn cube_mesh(&self) -> Handle<MeshAsset> {
        self.cube_mesh
    }

    /// Current GPU resource counters. Cheap to read; no allocations.
    pub fn gpu_resource_stats(&self) -> crate::assets::GpuResourceStats {
        self.gpu_registry.stats()
    }

    /// Borrow the offscreen preview color target for egui registration.
    pub fn preview_texture(&self) -> Option<&wgpu::Texture> {
        self.preview_texture.as_ref()
    }

    /// Borrow the offscreen viewport color target for egui registration.
    pub fn viewport_texture(&self) -> Option<&wgpu::Texture> {
        self.viewport_texture.as_ref()
    }

    pub fn cube_material(&self) -> Handle<MaterialAsset> {
        self.cube_material
    }

    pub fn red_material(&self) -> Handle<MaterialAsset> {
        self.red_material
    }

    pub fn green_material(&self) -> Handle<MaterialAsset> {
        self.green_material
    }

    pub fn blue_material(&self) -> Handle<MaterialAsset> {
        self.blue_material
    }

    pub fn set_wireframe(&mut self, enabled: bool) {
        self.wireframe_mode = enabled;
    }

    pub fn set_render_scale(&mut self, scale_percent: u32) {
        self.render_scale_percent = scale_percent.clamp(50, 200);
        self.configure_targets();
    }

    pub fn set_msaa(&mut self, enabled: bool) {
        let sample_count = if enabled { 4 } else { 1 };
        if self.msaa_samples == sample_count {
            return;
        }
        self.msaa_samples = sample_count;
        self.configure_targets();
    }

    pub fn set_post_processing(&mut self, enabled: bool) {
        if enabled == self.post_processing {
            return;
        }

        if enabled && self.post_pipeline.is_none() {
            self.post_pipeline = Some(self.device.create_render_pipeline(
                &wgpu::RenderPipelineDescriptor {
                    label: Some("post pipeline"),
                    layout: Some(&self.post_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &self.post_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &self.post_shader,
                        entry_point: Some("fs_main"),
                        targets: &self.color_targets,
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                },
            ));
        }

        self.post_processing = enabled;
        self.configure_targets();
    }

    pub fn set_shadow_quality(&mut self, quality: crate::quality::ShadowQuality) {
        self.shadow_quality = quality;
    }

    pub fn msaa_samples(&self) -> u32 {
        self.msaa_samples
    }

    pub fn post_processing_enabled(&self) -> bool {
        self.post_processing
    }

    pub fn ui_renderer_mut(&mut self) -> &mut crate::ui_new::UiRenderer {
        self.ui_renderer.as_mut().expect("ui_renderer initialized")
    }

    pub fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        self.window_size = size;
        self.config.width = size.width.max(1);
        self.config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.configure_targets();
        if let Some(ui_renderer) = &mut self.ui_renderer {
            ui_renderer.resize(size.width, size.height);
        }
    }

    pub fn set_camera(&mut self, camera: &crate::camera::Camera3D, transform: &GlobalTransform) {
        self.camera_position = transform.0.transform_point3(Vec3::ZERO);
        self.camera_target = transform.0.transform_point3(Vec3::NEG_Z);
        self.camera_fov = camera.fov;
        self.camera_near = camera.near;
        self.camera_far = camera.far;
    }

    pub fn update_ui(
        &mut self,
        fps: f32,
        entity_count: usize,
        renderable_count: usize,
        frame_time_ms: f64,
        preset: &str,
    ) {
        self.fps = fps;
        self.entity_count = entity_count;
        self.renderable_count = renderable_count;
        self.preset_name = preset.to_string();
        self.frame_time_ms = frame_time_ms;
    }

    pub fn update(&mut self, snapshot: &RenderSnapshot, delta_seconds: f32) {
        let aspect = self.window_size.width as f32 / self.window_size.height.max(1) as f32;
        let projection =
            Mat4::perspective_rh(self.camera_fov, aspect, self.camera_near, self.camera_far);
        let view = Mat4::look_at_rh(self.camera_position, self.camera_target, Vec3::Y);
        let camera = CameraUniform {
            view_proj: (projection * view).to_cols_array_2d(),
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));
        let _ = delta_seconds;
        let (light_color, light_direction) = snapshot
            .directional_light
            .unwrap_or(([1.0, 1.0, 1.0], Vec3::new(-4.0, -8.0, -4.0).normalize()));
        let light_view = Mat4::look_at_rh(-light_direction * 10.0, Vec3::ZERO, Vec3::Y);
        let light_projection = Mat4::orthographic_rh(-8.0, 8.0, -8.0, 8.0, 0.1, 40.0);
        let light = LightUniform {
            view_proj: (light_projection * light_view).to_cols_array_2d(),
            params: [light_color[0], light_color[1], light_color[2], 0.0],
            direction: light_direction.extend(0.0).into(),
        };
        self.queue
            .write_buffer(&self.light_buffer, 0, bytemuck::bytes_of(&light));

        // Upload/update the per-entity model uniform for every renderable so the
        // main surface pass can draw even before the editor viewport pass runs.
        for instance in &snapshot.renderables {
            self.sync_model_buffer(
                instance.entity,
                instance.renderable,
                instance.global_transform,
            );
        }
    }

    /// Sync the model buffer for an arbitrary mesh and key ID with a given transform.
    pub fn sync_mesh_transform(
        &mut self,
        mesh_handle: Handle<MeshAsset>,
        key_id: u64,
        transform: Mat4,
    ) {
        let key = (mesh_handle.id(), key_id);
        let needs_model_buffer = self
            .mesh_assets
            .get_ref(mesh_handle)
            .is_some_and(|mesh| !mesh.model_buffers.contains_key(&key));
        if needs_model_buffer {
            let Some(model_layout) = self
                .mesh_assets
                .get_ref(mesh_handle)
                .map(|mesh| &mesh.model_layout)
            else {
                return;
            };
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("model"),
                size: std::mem::size_of::<ModelUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("model bind group"),
                layout: model_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            if let Some(mesh_mut) = self.mesh_assets.get_mut(mesh_handle) {
                mesh_mut.model_buffers.insert(key, buffer);
                mesh_mut.model_bind_groups.insert(key, bind_group);
            }
        }
        let model = ModelUniform {
            model: transform.to_cols_array_2d(),
            normal_matrix: glam::Mat4::from_mat3(
                glam::Mat3::from_mat4(transform.inverse()).transpose(),
            )
            .to_cols_array_2d(),
        };
        let buffer = self
            .mesh_assets
            .get_ref(mesh_handle)
            .and_then(|mesh| mesh.model_buffers.get(&key).cloned());
        if let Some(buffer) = buffer {
            self.queue
                .write_buffer(&buffer, 0, bytemuck::bytes_of(&model));
        }
    }

    /// Sync the per-instance model buffer for one renderable.
    fn sync_model_buffer(&mut self, entity: Entity, renderable: Renderable, transform: Mat4) {
        self.sync_mesh_transform(renderable.mesh, entity.to_bits(), transform);
    }

    /// Dynamically upload or reuse a mesh asset on the GPU based on revision.
    pub fn upload_mesh(
        &mut self,
        asset_id: crate::asset_database::AssetId,
        revision: u64,
        mesh_data: &crate::gltf_importer::MeshData,
    ) -> Handle<MeshAsset> {
        if self.gpu_registry.is_up_to_date(asset_id, revision) {
            if let Some(handle) = self.mesh_assets.get_by_asset_id(asset_id) {
                self.gpu_registry.record_cache_hit();
                return handle;
            }
        }
        self.gpu_registry.record_cache_miss();

        let vertices: Vec<Vertex> = mesh_data
            .vertices
            .iter()
            .map(|mv| Vertex {
                position: mv.position,
                color: [1.0, 1.0, 1.0],
                normal: mv.normal,
            })
            .collect();
        let vertex_bytes: &[u8] = bytemuck::cast_slice(&vertices);
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("mesh vertices {}", asset_id)),
                contents: vertex_bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });

        let fits_u16 = mesh_data.indices.iter().all(|&idx| idx <= u16::MAX as u32);
        let (index_buffer, index_format, index_bytes_len) = if fits_u16 {
            let indices_u16: Vec<u16> = mesh_data.indices.iter().map(|&i| i as u16).collect();
            let index_bytes: &[u8] = bytemuck::cast_slice(&indices_u16);
            let buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("mesh indices {}", asset_id)),
                    contents: index_bytes,
                    usage: wgpu::BufferUsages::INDEX,
                });
            (buf, wgpu::IndexFormat::Uint16, index_bytes.len())
        } else {
            let index_bytes: &[u8] = bytemuck::cast_slice(&mesh_data.indices);
            let buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("mesh indices {}", asset_id)),
                    contents: index_bytes,
                    usage: wgpu::BufferUsages::INDEX,
                });
            (buf, wgpu::IndexFormat::Uint32, index_bytes.len())
        };

        let mesh_asset = MeshAsset {
            vertex_buffer,
            index_buffer,
            index_count: mesh_data.indices.len() as u32,
            vertex_count: mesh_data.vertices.len() as u32,
            index_format,
            model_layout: self.model_layout.clone(),
            model_buffers: HashMap::new(),
            model_bind_groups: HashMap::new(),
        };

        let handle = self
            .mesh_assets
            .insert_with_id(asset_id.to_string(), asset_id, mesh_asset);
        let gpu_bytes = vertex_bytes.len() + index_bytes_len;
        self.gpu_registry.register_upload(
            asset_id,
            revision,
            crate::assets::GpuResourceKind::Mesh,
            handle.id(),
            gpu_bytes,
        );
        self.gpu_registry.record_uploaded_mesh();
        handle
    }

    /// Dynamically upload or reuse a material asset on the GPU based on revision.
    pub fn upload_material(
        &mut self,
        asset_id: crate::asset_database::AssetId,
        revision: u64,
        material_data: &crate::gltf_importer::MaterialData,
    ) -> Handle<MaterialAsset> {
        if self.gpu_registry.is_up_to_date(asset_id, revision) {
            if let Some(handle) = self.material_assets.get_by_asset_id(asset_id) {
                self.gpu_registry.record_cache_hit();
                return handle;
            }
        }
        self.gpu_registry.record_cache_miss();

        let (p1, wp1, p4, wp4) =
            if let Some(cube_mat) = self.material_assets.get(self.cube_material) {
                (
                    cube_mat.pipeline_1x.clone(),
                    cube_mat.wireframe_pipeline_1x.clone(),
                    cube_mat.pipeline_4x.clone(),
                    cube_mat.wireframe_pipeline_4x.clone(),
                )
            } else {
                panic!("Cube material missing in Renderer");
            };

        let color_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("material color {}", asset_id)),
            size: std::mem::size_of::<MaterialUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(
            &color_buffer,
            0,
            bytemuck::bytes_of(&MaterialUniform {
                color: material_data.base_color_factor,
            }),
        );
        let color_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("material bind group {}", asset_id)),
            layout: &self.color_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: color_buffer.as_entire_binding(),
            }],
        });

        let material_asset = MaterialAsset {
            pipeline_1x: p1,
            wireframe_pipeline_1x: wp1,
            pipeline_4x: p4,
            wireframe_pipeline_4x: wp4,
            color_buffer,
            color_bind_group,
            color_bind_group_layout: self.color_layout.clone(),
        };

        let handle =
            self.material_assets
                .insert_with_id(asset_id.to_string(), asset_id, material_asset);
        let gpu_bytes = std::mem::size_of::<MaterialUniform>();
        self.gpu_registry.register_upload(
            asset_id,
            revision,
            crate::assets::GpuResourceKind::Material,
            handle.id(),
            gpu_bytes,
        );
        self.gpu_registry.record_uploaded_material();
        handle
    }

    /// Dynamically upload or reuse a texture asset on the GPU based on revision.
    pub fn upload_texture(
        &mut self,
        asset_id: crate::asset_database::AssetId,
        revision: u64,
        texture_data: &crate::gltf_importer::TextureData,
    ) -> Result<Handle<TextureAsset>, String> {
        if self.gpu_registry.is_up_to_date(asset_id, revision) {
            if let Some(handle) = self.texture_assets.get_by_asset_id(asset_id) {
                self.gpu_registry.record_cache_hit();
                return Ok(handle);
            }
        }
        self.gpu_registry.record_cache_miss();

        let width = texture_data.width.max(1);
        let height = texture_data.height.max(1);
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("texture {}", asset_id)),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texture_data.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let texture_asset = TextureAsset {
            texture,
            view,
            sampler,
            bind_group: None,
            width,
            height,
        };
        let handle =
            self.texture_assets
                .insert_with_id(asset_id.to_string(), asset_id, texture_asset);
        let gpu_bytes = (width * height * 4) as usize;
        self.gpu_registry.register_upload(
            asset_id,
            revision,
            crate::assets::GpuResourceKind::Texture,
            handle.id(),
            gpu_bytes,
        );
        self.gpu_registry.record_uploaded_texture();
        Ok(handle)
    }

    /// Drop a mesh that was registered under a source-path key so the next
    /// `sync_world_renderables` re-reads the file. The GPU buffers are released
    /// when the last `Arc` drops.
    pub fn forget_mesh_path(&mut self, key: &str) {
        if let Some(handle) = self.mesh_assets.remove_key(&key.to_string()) {
            self.mesh_assets.remove(handle);
            self.gpu_registry.mark_stale(handle.asset_id());
        }
    }

    /// Forget every path-keyed mesh whose key references `source` (the reimported
    /// file), so the next sync re-reads the updated geometry.
    pub fn forget_meshes_for_source(&mut self, source: &std::path::Path) {
        let prefix = source.to_string_lossy();
        let keys: Vec<String> = self
            .mesh_assets
            .keys()
            .filter(|k| k.starts_with(prefix.as_ref()))
            .cloned()
            .collect();
        for key in keys {
            self.forget_mesh_path(&key);
        }
    }

    /// Mark an asset's GPU representation as stale upon reimport or modification.
    /// The previous wgpu buffers stay alive until the next upload replaces the
    /// `MeshAsset`/`TextureAsset` entry (drop then releases the GPU object).
    pub fn invalidate_asset(&mut self, asset_id: crate::asset_database::AssetId) {
        self.gpu_registry.mark_stale(asset_id);
        self.gpu_registry.record_reimport();
    }

    /// Completely unload an asset from GPU buffers, texture views, and asset managers.
    pub fn unload_asset(&mut self, asset_id: crate::asset_database::AssetId) {
        self.mesh_assets.remove_by_asset_id(asset_id);
        self.material_assets.remove_by_asset_id(asset_id);
        self.texture_assets.remove_by_asset_id(asset_id);
        self.gpu_registry.mark_unloaded(asset_id);
    }

    /// Render a hardware-accelerated preview of a mesh with a material into an offscreen texture.
    pub fn render_preview_mesh(
        &mut self,
        mesh_handle: Handle<MeshAsset>,
        material_handle: Handle<MaterialAsset>,
        width: u32,
        height: u32,
        camera_yaw: f32,
        camera_pitch: f32,
        camera_distance: f32,
    ) -> Option<wgpu::TextureView> {
        let width = width.max(1);
        let height = height.max(1);
        if self.preview_texture.is_none() || self.preview_size != (width, height) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("preview texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let depth = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("preview depth"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.preview_view = Some(texture.create_view(&wgpu::TextureViewDescriptor::default()));
            self.preview_depth = Some(depth.create_view(&wgpu::TextureViewDescriptor::default()));
            self.preview_texture = Some(texture);
            self.preview_size = (width, height);
        }

        let eye = Vec3::new(
            camera_distance * camera_pitch.cos() * camera_yaw.sin(),
            camera_distance * camera_pitch.sin(),
            camera_distance * camera_pitch.cos() * camera_yaw.cos(),
        );
        let view_mat = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        let proj_mat = Mat4::perspective_rh(
            std::f32::consts::FRAC_PI_4,
            width as f32 / height.max(1) as f32,
            0.01,
            100.0,
        );
        let cam = CameraUniform {
            view_proj: (proj_mat * view_mat).to_cols_array_2d(),
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&cam));

        let light_direction = Vec3::new(-1.0, -2.0, -1.0).normalize();
        let light_view = Mat4::look_at_rh(-light_direction * 10.0, Vec3::ZERO, Vec3::Y);
        let light_projection = Mat4::orthographic_rh(-8.0, 8.0, -8.0, 8.0, 0.1, 40.0);
        let light = LightUniform {
            view_proj: (light_projection * light_view).to_cols_array_2d(),
            params: [1.0, 1.0, 1.0, 0.0],
            direction: light_direction.extend(0.0).into(),
        };
        self.queue
            .write_buffer(&self.light_buffer, 0, bytemuck::bytes_of(&light));

        self.sync_mesh_transform(mesh_handle, 0, Mat4::IDENTITY);

        let color_view = self.preview_view.as_ref()?.clone();
        let depth_view = self.preview_depth.as_ref()?.clone();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("preview pass"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("preview scene pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.12,
                            g: 0.13,
                            b: 0.16,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            let key = (mesh_handle.id(), 0u64);
            if let (Some(mesh), Some(material)) = (
                self.mesh_assets.get(mesh_handle),
                self.material_assets.get(material_handle),
            ) {
                if let Some(model_group) = mesh.model_bind_groups.get(&key) {
                    pass.set_pipeline(&material.pipeline_1x);
                    pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    pass.set_bind_group(1, model_group, &[]);
                    pass.set_bind_group(2, &self.light_bind_group, &[]);
                    pass.set_bind_group(3, &material.color_bind_group, &[]);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), mesh.index_format);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        self.preview_view.clone()
    }

    /// Synchronize entities in the world that have `MeshFilter` and `MeshRenderer`
    /// components with GPU-ready `Renderable` components. If a mesh hasn't been uploaded
    /// yet or is stale, it will be uploaded to the GPU using its registered `AssetId`.
    pub fn sync_world_renderables(&mut self, world: &mut World) {
        let mut to_sync = Vec::new();
        for (entity, mf, mr, renderable) in world
            .query::<(
                Entity,
                &crate::core::components::MeshFilter,
                &crate::core::components::MeshRenderer,
                Option<&Renderable>,
            )>()
            .iter(world)
        {
            to_sync.push((entity, mf.clone(), mr.clone(), renderable.copied()));
        }

        for (entity, mf, mr, renderable) in to_sync {
            let mut mesh_handle = renderable.map(|r| r.mesh);
            let mut material_handle = renderable.map(|r| r.material);

            let is_stale = mesh_handle
                .and_then(|h| self.gpu_registry.get(h.asset_id()))
                .is_some_and(|e| e.status == crate::assets::GpuResourceStatus::Stale);

            if mesh_handle.is_none() || is_stale {
                if let Some(existing) = self.mesh_assets.get_by_key(&mf.mesh_path) {
                    mesh_handle = Some(existing);
                } else {
                    let clean_path = mf.mesh_path.split(':').next().unwrap_or(&mf.mesh_path);
                    let path = std::path::Path::new(clean_path);
                    if path.exists() {
                        let parsed = if path
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| {
                                e.eq_ignore_ascii_case("gltf") || e.eq_ignore_ascii_case("glb")
                            }) {
                            crate::assets_ui::ObjMesh::parse_gltf(path)
                        } else if path
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("obj"))
                        {
                            std::fs::read_to_string(path)
                                .ok()
                                .and_then(|t| crate::assets_ui::ObjMesh::parse(&t))
                        } else {
                            None
                        };

                        if let Some(mesh) = parsed {
                            let mesh_data = mesh.to_mesh_data(clean_path.to_string());
                            let asset_id = crate::asset_database::AssetId::new();
                            let handle = self.upload_mesh(asset_id, 1, &mesh_data);
                            self.mesh_assets.insert_key(mf.mesh_path.clone(), handle);
                            mesh_handle = Some(handle);
                        }
                    }
                }
            }

            if material_handle.is_none() {
                material_handle = self.material_assets.get_by_key(&mr.material_path);
            }

            let final_mesh = mesh_handle.unwrap_or(self.cube_mesh);
            let final_material = material_handle.unwrap_or(self.cube_material);

            let need_insert = match renderable {
                Some(r) => r.mesh != final_mesh || r.material != final_material,
                None => true,
            };

            if need_insert {
                world.entity_mut(entity).insert(Renderable {
                    mesh: final_mesh,
                    material: final_material,
                });
            }
        }
    }

    /// Render the scene into an offscreen texture sized for the editor's
    /// Viewport panel. Returns the texture view for egui to display.
    pub fn render_scene_to_viewport(
        &mut self,
        snapshot: &RenderSnapshot,
        width: u32,
        height: u32,
        camera: &crate::viewport::ViewportCamera,
    ) -> Option<wgpu::TextureView> {
        // Keep the offscreen target within the device limits on very large or
        // high-DPI viewport panels.
        let max_dimension = self.device.limits().max_texture_dimension_2d.max(1);
        let width = width.max(1).min(max_dimension);
        let height = height.max(1).min(max_dimension);
        if self.viewport_size != (width, height) || self.viewport_view.is_none() {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport scene"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let depth = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport depth"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.viewport_view = Some(texture.create_view(&wgpu::TextureViewDescriptor::default()));
            self.viewport_depth = Some(depth.create_view(&wgpu::TextureViewDescriptor::default()));
            self.viewport_texture = Some(texture);
            self.viewport_size = (width, height);
        }

        // Camera uniforms for this pass, built from the viewport camera so the
        // projection matches what the editor shows and picks against.
        let cam = CameraUniform {
            view_proj: camera
                .view_proj(width as f32 / height.max(1) as f32)
                .to_cols_array_2d(),
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&cam));

        let (light_color, light_direction) = snapshot
            .directional_light
            .unwrap_or(([1.0, 1.0, 1.0], Vec3::new(-4.0, -8.0, -4.0).normalize()));
        let light_view = Mat4::look_at_rh(-light_direction * 10.0, Vec3::ZERO, Vec3::Y);
        let light_projection = Mat4::orthographic_rh(-8.0, 8.0, -8.0, 8.0, 0.1, 40.0);
        let light = LightUniform {
            view_proj: (light_projection * light_view).to_cols_array_2d(),
            params: [light_color[0], light_color[1], light_color[2], 0.0],
            direction: light_direction.extend(0.0).into(),
        };
        self.queue
            .write_buffer(&self.light_buffer, 0, bytemuck::bytes_of(&light));

        for instance in &snapshot.renderables {
            self.sync_model_buffer(
                instance.entity,
                instance.renderable,
                instance.global_transform,
            );
        }

        let color_view = self.viewport_view.as_ref()?.clone();
        let depth_view = self.viewport_depth.as_ref()?.clone();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewport pass"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport scene pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            for instance in &snapshot.renderables {
                let renderable = instance.renderable;
                let key = (renderable.mesh.id(), instance.entity.to_bits());
                let (Some(mesh), Some(material)) = (
                    self.mesh_assets.get(renderable.mesh),
                    self.material_assets.get(renderable.material),
                ) else {
                    continue;
                };
                let Some(model_group) = mesh.model_bind_groups.get(&key) else {
                    continue;
                };
                pass.set_pipeline(&material.pipeline_1x);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_bind_group(1, model_group, &[]);
                pass.set_bind_group(2, &self.light_bind_group, &[]);
                pass.set_bind_group(3, &material.color_bind_group, &[]);
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), mesh.index_format);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.viewport_view.clone()
    }

    pub fn render_scene(
        &mut self,
        snapshot: &RenderSnapshot,
    ) -> Result<(wgpu::SurfaceTexture, wgpu::TextureView), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        let renderables = &snapshot.renderables;

        // Shadow pass temporarily disabled - WGPU requires separate depth texture for sampling vs attachment
        // which is not supported on this driver architecture
        let _shadow_quality = self.shadow_quality;

        // Render to the offscreen target only when a complete post-processing
        // pipeline is available. Otherwise the scene must be rendered directly
        // to the surface; rendering only to an offscreen texture leaves a black
        // window when post-processing is enabled but not initialized.
        let use_post_processing =
            self.post_processing && self.post_pipeline.is_some() && self.post_bind_group.is_some();
        let target_view = if use_post_processing {
            self.scene_view.as_ref().unwrap_or(&view)
        } else {
            &view
        };
        let resolve_target = if self.msaa_view.is_some() {
            Some(target_view)
        } else {
            None
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: if self.msaa_view.is_some() {
                    self.msaa_view.as_ref().unwrap()
                } else {
                    target_view
                },
                resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: self.depth_view.as_ref().map(|view| {
                wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let mut draw_calls = 0u32;
        let mut triangles = 0u64;
        let mut vertices = 0u64;
        let mut shadow_casters = 0u64;
        let mut passes = 1u32;

        for instance in renderables {
            let entity = instance.entity;
            let renderable = instance.renderable;
            let Some(material) = self.material_assets.get(renderable.material) else {
                continue;
            };
            let Some(mesh) = self.mesh_assets.get(renderable.mesh) else {
                continue;
            };
            let key = (renderable.mesh.id(), entity.to_bits());
            let Some(bind_group) = mesh.model_bind_groups.get(&key) else {
                continue;
            };
            pass.set_pipeline(match (self.msaa_samples, self.wireframe_mode) {
                (1, true) => &material.wireframe_pipeline_1x,
                (1, false) => &material.pipeline_1x,
                (_, true) => &material.wireframe_pipeline_4x,
                (_, false) => &material.pipeline_4x,
            });
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.set_bind_group(2, &self.light_bind_group, &[]);
            pass.set_bind_group(3, &material.color_bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), mesh.index_format);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            draw_calls = draw_calls.saturating_add(1);
            triangles = triangles.saturating_add((mesh.index_count / 3) as u64);
            vertices = vertices.saturating_add(mesh.vertex_count as u64);
            shadow_casters = shadow_casters.saturating_add(1);
        }
        drop(pass);

        if use_post_processing
            && let (Some(pipeline), Some(bind_group)) =
                (self.post_pipeline.as_ref(), self.post_bind_group.as_ref())
        {
            passes = passes.saturating_add(1);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit([encoder.finish()]);
        self.frame_draw_calls = draw_calls;
        self.frame_triangles = triangles;
        self.frame_vertices = vertices;
        self.frame_shadow_casters = shadow_casters;
        self.frame_passes = passes;
        Ok((frame, view))
    }

    /// Draw interactive UI widgets and overlay text onto the current frame, then present.
    pub fn render_ui(
        &mut self,
        frame: wgpu::SurfaceTexture,
        view: &wgpu::TextureView,
        fps: f32,
        entity_count: usize,
        _renderable_count: usize,
        frame_time_ms: f64,
        _preset_name: &str,
        render_scale_percent: u32,
        msaa_enabled: bool,
        post_processing_enabled: bool,
        wireframe_mode: bool,
        width: f32,
        height: f32,
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ui commands"),
            });

        // Get mouse state from UI before mutating it
        let (mouse_x, mouse_y, mouse_down, mouse_clicked, mouse_released) =
            if let Some(ui) = &self.ui {
                (
                    ui.mouse_x,
                    ui.mouse_y,
                    ui.mouse_down,
                    ui.mouse_clicked,
                    ui.mouse_released,
                )
            } else {
                (0.0, 0.0, false, false, false)
            };

        let mut requested_render_scale = None;
        let mut requested_msaa = None;
        let mut requested_post_processing = None;
        let mut requested_wireframe = None;
        let mut requested_preset = None;
        let mut render_scale_changed = false;
        let mut clicked_menu = None;
        let mut clicked_menu_action = None;
        let mut clicked_outside_menu = false;

        // Begin UI frame
        if let Some(ui) = &mut self.ui {
            ui.begin_frame(mouse_x, mouse_y, mouse_down, mouse_clicked, mouse_released);

            ui.theme.font_scale = 2.0;
            ui.theme.padding = 16.0;
            ui.theme.spacing = 11.0;
            ui.theme.bg = crate::ui_new::Color::rgb(0.055, 0.07, 0.10);
            ui.theme.border = crate::ui_new::Color::rgb(0.16, 0.22, 0.30);
            ui.theme.accent = crate::ui_new::Color::rgb(0.22, 0.62, 0.88);
            ui.theme.border_radius = 7.0;
            let menu_bar_height = 40.0;
            ui.draw_rect(
                Rect::new(0.0, 0.0, width, menu_bar_height),
                crate::ui_new::Color::rgb(0.035, 0.045, 0.065),
                crate::ui_new::Color::rgb(0.12, 0.16, 0.22),
                1.0,
            );
            let menus = ["File", "Edit", "Window", "Tools", "Build", "Help"];
            let mut menu_x = 14.0;
            for (index, name) in menus.iter().enumerate() {
                let (text_w, text_h, _) = crate::ui_new::text_uv_rect(name, 2.0);
                let rect = Rect::new(menu_x, 4.0, text_w + 22.0, 32.0);
                let id = ui.gen_id(name);
                let result = ui.button_behavior(id, rect, true);
                if result.clicked {
                    clicked_menu = Some(index);
                }
                let selected = self.menu_open == Some(index);
                if selected || result.hovered {
                    ui.draw_rect(
                        rect,
                        if selected {
                            ui.theme.bg_active
                        } else {
                            ui.theme.bg_hover
                        },
                        ui.theme.border,
                        1.0,
                    );
                }
                ui.draw_text(
                    rect.x + (rect.w - text_w) * 0.5,
                    rect.y + (rect.h - text_h) * 0.5,
                    name,
                    ui.theme.text,
                    2.0,
                );
                menu_x += rect.w + 2.0;
            }

            if let Some(menu_index) = self.menu_open {
                let items: &[&str] = match menu_index {
                    0 => &["Reload scene (F5)"],
                    1 => &["No edit actions"],
                    2 => &["Toggle statistics"],
                    3 => &["Toggle wireframe"],
                    4 => &["Build controls unavailable"],
                    _ => &["WASD move  |  Mouse look  |  F2 wireframe"],
                };
                let menu_rect = Rect::new(
                    14.0,
                    menu_bar_height,
                    250.0,
                    items.len() as f32 * 34.0 + 12.0,
                );
                ui.draw_rect(
                    menu_rect,
                    crate::ui_new::Color::rgb(0.055, 0.07, 0.10),
                    ui.theme.border,
                    1.0,
                );
                for (item_index, item) in items.iter().enumerate() {
                    let row = Rect::new(
                        menu_rect.x + 6.0,
                        menu_rect.y + 6.0 + item_index as f32 * 34.0,
                        menu_rect.w - 12.0,
                        30.0,
                    );
                    let id = ui.gen_id(item);
                    let result = ui.button_behavior(id, row, menu_index == 2 || menu_index == 3);
                    if result.hovered {
                        ui.draw_rect(row, ui.theme.bg_hover, ui.theme.bg_hover, 0.0);
                    }
                    let item_color = if menu_index == 4 || menu_index == 1 {
                        ui.theme.text_disabled
                    } else {
                        ui.theme.text
                    };
                    ui.draw_text(row.x + 10.0, row.y + 8.0, item, item_color, 2.0);
                    if result.clicked {
                        clicked_menu_action = Some(menu_index);
                    }
                }
                if mouse_clicked
                    && !menu_rect.contains(mouse_x, mouse_y)
                    && mouse_y > menu_bar_height
                {
                    clicked_outside_menu = true;
                }
            }

            let panel_width = (width - 32.0).clamp(320.0, 390.0).min(width - 16.0);
            let panel_x = 16.0;
            let panel_y = menu_bar_height + 12.0;
            let panel_height = 390.0_f32.min(height - panel_y - 16.0);
            ui.begin_panel(panel_x, panel_y, panel_width, panel_height);

            ui.draw_rect(
                Rect::new(panel_x + 1.0, panel_y + 1.0, 4.0, 48.0),
                ui.theme.accent,
                ui.theme.accent,
                0.0,
            );
            ui.label("RUST ENGINE");
            ui.draw_text(
                ui.current_layout.next_x,
                ui.current_layout.next_y - 4.0,
                "GRAPHICS CONTROL",
                crate::ui_new::Color::rgb(0.52, 0.63, 0.74),
                1.5,
            );
            ui.current_layout.next_y += 8.0;
            ui.separator();
            ui.spacing(2.0);
            ui.label("QUALITY PRESET");
            ui.spacing(4.0);

            let presets = ["FAST", "BALANCED", "QUALITY"];
            let preset_index = match _preset_name {
                "Performance" => 0,
                "Balanced" => 1,
                "Quality" => 2,
                _ => usize::MAX,
            };
            let gap = 6.0;
            let button_h = 36.0;
            let button_w = (ui.current_layout.width - gap * 2.0) / 3.0;
            for (i, name) in presets.iter().enumerate() {
                let rect = Rect::new(
                    ui.current_layout.next_x,
                    ui.current_layout.next_y,
                    button_w,
                    button_h,
                );
                let id = ui.gen_id(name);
                let result = ui.button_behavior(id, rect, true);
                if result.clicked {
                    requested_preset = Some(i);
                }
                let (bg_color, border_color) = if i == preset_index {
                    (ui.theme.accent, ui.theme.accent)
                } else if result.hovered {
                    (ui.theme.bg_hover, ui.theme.border_hover)
                } else {
                    (ui.theme.bg, ui.theme.border)
                };
                ui.draw_rect(rect, bg_color, border_color, ui.theme.border_width);
                let (tw, th, _) = crate::ui_new::text_uv_rect(name, ui.theme.font_scale);
                ui.draw_text(
                    rect.x + (rect.w - tw) / 2.0,
                    rect.y + (rect.h - th) / 2.0,
                    name,
                    ui.theme.text,
                    ui.theme.font_scale,
                );
                ui.current_layout.next_x += button_w + gap;
            }
            ui.current_layout.next_x = ui.current_layout.x;
            ui.current_layout.next_y += button_h + ui.theme.spacing;

            ui.spacing(3.0);
            ui.label("RENDER SCALE");
            let mut render_scale = render_scale_percent as f32;
            if ui.slider("", &mut render_scale, 50.0, 200.0).changed {
                requested_render_scale = Some(render_scale.round() as u32);
                render_scale_changed = true;
            }
            ui.spacing(2.0);
            ui.separator();
            ui.label("RENDER OPTIONS");
            let mut msaa = msaa_enabled;
            if ui.checkbox("MSAA 4X", &mut msaa).clicked {
                requested_msaa = Some(msaa);
            }
            let mut post_processing = post_processing_enabled;
            if ui.checkbox("POST FX", &mut post_processing).clicked {
                requested_post_processing = Some(post_processing);
            }
            let mut wireframe = wireframe_mode;
            if ui.checkbox("WIREFRAME", &mut wireframe).clicked {
                requested_wireframe = Some(wireframe);
            }
            ui.end_panel();

            // A compact HUD keeps the scene visible while making the key metrics
            // readable and aligned instead of stacking them across the screen.
            let stats_width = 370.0_f32.min(width - 32.0).max(0.0);
            let stats_height = 54.0_f32.min(height - 32.0).max(0.0);
            if self.show_stats && stats_width > 0.0 && stats_height > 0.0 {
                let stats_x = width - stats_width - 16.0;
                let stats_y = height - stats_height - 16.0;
                ui.begin_panel(stats_x, stats_y, stats_width, stats_height);
                let stat_y = stats_y + (stats_height - 14.0) * 0.5;
                ui.draw_text(
                    stats_x + 12.0,
                    stat_y,
                    &format!("FPS {:.0}", fps),
                    ui.theme.text,
                    2.0,
                );
                ui.draw_text(
                    stats_x + 112.0,
                    stat_y,
                    &format!("{} ENTITIES", entity_count),
                    ui.theme.text,
                    2.0,
                );
                ui.draw_text(
                    stats_x + 255.0,
                    stat_y,
                    &format!("{:.1} MS", frame_time_ms),
                    ui.theme.text,
                    2.0,
                );
                ui.end_panel();
            }

            // Get commands and draw
            let commands = ui.end_frame();

            // Draw using the UI renderer
            if let Some(ui_renderer) = &mut self.ui_renderer {
                ui_renderer.draw(&self.device, &self.queue, &mut encoder, view, &commands);
            }
        }

        if let Some(index) = clicked_menu {
            self.menu_open = if self.menu_open == Some(index) {
                None
            } else {
                Some(index)
            };
        }
        if clicked_outside_menu && clicked_menu.is_none() {
            self.menu_open = None;
        }
        if let Some(menu_index) = clicked_menu_action {
            match menu_index {
                2 => self.show_stats = !self.show_stats,
                3 => self.set_wireframe(!self.wireframe_mode),
                _ => {}
            }
            self.menu_open = None;
        }

        if render_scale_changed {
            self.preset_name = "Custom".to_string();
        }
        if let Some(scale) = requested_render_scale {
            self.set_render_scale(scale);
        }
        if let Some(enabled) = requested_msaa {
            self.set_msaa(enabled);
        }
        if let Some(enabled) = requested_post_processing {
            self.set_post_processing(enabled);
        }
        if let Some(enabled) = requested_wireframe {
            self.set_wireframe(enabled);
        }
        if let Some(index) = requested_preset {
            let preset = match index {
                0 => QualityPreset::Performance,
                1 => QualityPreset::Balanced,
                _ => QualityPreset::Quality,
            };
            let settings = preset.settings();
            self.set_render_scale(settings.render_scale_percent);
            self.set_msaa(settings.msaa_samples > 1);
            self.set_post_processing(settings.post_processing);
            self.preset_name = match preset {
                QualityPreset::Performance => "Performance",
                QualityPreset::Balanced => "Balanced",
                QualityPreset::Quality => "Quality",
            }
            .to_string();
        }

        self.queue.submit([encoder.finish()]);
        frame.present();
    }

    fn configure_targets(&mut self) {
        // The scene is rendered directly into the surface when post-processing
        // is disabled, so its depth/MSAA attachments must match the surface size.
        // The render scale only applies to the offscreen scene texture.
        let render_scale = if self.post_processing {
            self.render_scale_percent
        } else {
            100
        };
        let width = (self.window_size.width as u64 * render_scale as u64 / 100).max(1) as u32;
        let height = (self.window_size.height as u64 * render_scale as u64 / 100).max(1) as u32;
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: self.msaa_samples,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.depth_view = Some(depth.create_view(&wgpu::TextureViewDescriptor::default()));
        if self.msaa_samples > 1 {
            let msaa = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("msaa"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: self.msaa_samples,
                dimension: wgpu::TextureDimension::D2,
                format: self.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.msaa_view = Some(msaa.create_view(&wgpu::TextureViewDescriptor::default()));
        } else {
            self.msaa_view = None;
        }
        if self.post_processing {
            let scene = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scene"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let scene_view = scene.create_view(&wgpu::TextureViewDescriptor::default());
            if let Some(pipeline) = self.post_pipeline.as_ref() {
                let layout = pipeline.get_bind_group_layout(0);
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("post bind group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&scene_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.scene_sampler),
                        },
                    ],
                });
                self.post_bind_group = Some(bind_group);
            }
            self.scene_view = Some(scene_view);
        } else {
            self.scene_view = None;
        }
    }
}

fn build_debug_commands(
    _fps: f32,
    _entity_count: usize,
    _renderable_count: usize,
    wireframe_mode: bool,
    _preset: &str,
    _frame_time_ms: f64,
    _window_width: f32,
    _window_height: f32,
) -> Vec<crate::ui::UiCommand> {
    let mut commands = Vec::new();
    let padding = 10.0;
    let line_height = 20.0;
    let mut y = padding;
    let x = padding;

    // Background panel
    commands.push(crate::ui::UiCommand {
        rect: [x - 5.0, y - 5.0, 280.0, 220.0],
        color: [0.0, 0.0, 0.0, 0.7],
    });

    // FPS
    commands.push(crate::ui::UiCommand {
        rect: [x, y, 20.0, line_height],
        color: [0.2, 1.0, 0.2, 1.0],
    });
    y += line_height;

    // Entity count
    commands.push(crate::ui::UiCommand {
        rect: [x, y, 20.0, line_height],
        color: [1.0, 0.8, 0.2, 1.0],
    });
    y += line_height;

    // Renderable count
    commands.push(crate::ui::UiCommand {
        rect: [x, y, 20.0, line_height],
        color: [0.2, 0.8, 1.0, 1.0],
    });
    y += line_height;

    // Wireframe indicator
    commands.push(crate::ui::UiCommand {
        rect: [x, y, 20.0, line_height],
        color: if wireframe_mode {
            [1.0, 0.2, 0.2, 1.0]
        } else {
            [0.5, 0.5, 0.5, 1.0]
        },
    });
    y += line_height;

    // Preset
    commands.push(crate::ui::UiCommand {
        rect: [x, y, 20.0, line_height],
        color: [0.8, 0.8, 0.8, 1.0],
    });
    y += line_height;

    // Frame time
    commands.push(crate::ui::UiCommand {
        rect: [x, y, 20.0, line_height],
        color: [1.0, 1.0, 1.0, 1.0],
    });

    commands
}

fn create_shadow_view(device: &wgpu::Device) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow map"),
        size: wgpu::Extent3d {
            width: SHADOW_SIZE,
            height: SHADOW_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
