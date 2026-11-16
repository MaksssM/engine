use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use crate::{
    assets::{AssetManager, Handle},
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

#[derive(Component, Clone, Copy)]
pub struct Renderable {
    pub mesh: Handle<MeshAsset>,
    pub material: Handle<MaterialAsset>,
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
    pub cube_mesh: Handle<MeshAsset>,
    pub cube_material: Handle<MaterialAsset>,
    pub red_material: Handle<MaterialAsset>,
    pub green_material: Handle<MaterialAsset>,
    pub blue_material: Handle<MaterialAsset>,
    pub color_layout: wgpu::BindGroupLayout,
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
                model_layout,
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
            color_layout,
            _shader_variant: PhantomData,
        };
        // Initialize render targets (depth, MSAA, scene texture for post-processing)
        renderer.configure_targets();
        Ok(renderer)
    }

    pub fn cube_mesh(&self) -> Handle<MeshAsset> {
        self.cube_mesh
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

    pub fn update(&mut self, world: &mut World, delta_seconds: f32) {
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
        let light_view = Mat4::look_at_rh(Vec3::new(4.0, 8.0, 4.0), Vec3::ZERO, Vec3::Y);
        let light_projection = Mat4::orthographic_rh(-8.0, 8.0, -8.0, 8.0, 0.1, 40.0);
        let light = LightUniform {
            view_proj: (light_projection * light_view).to_cols_array_2d(),
            params: [0.25, 0.75, 0.0, 0.0],
            direction: Vec3::new(-4.0, -8.0, -4.0).normalize().extend(0.0).into(),
        };
        self.queue
            .write_buffer(&self.light_buffer, 0, bytemuck::bytes_of(&light));

        let mut query = world.query::<(Entity, &Renderable, &GlobalTransform)>();
        let renderables: Vec<_> = query
            .iter(world)
            .map(|(entity, renderable, transform)| (entity, *renderable, transform.0))
            .collect();
        for (entity, renderable, transform) in renderables {
            let key = (renderable.mesh.id(), entity.to_bits());
            let needs_model_buffer = self
                .mesh_assets
                .get_ref(renderable.mesh)
                .is_some_and(|mesh| !mesh.model_buffers.contains_key(&key));
            if needs_model_buffer {
                let Some(model_layout) = self
                    .mesh_assets
                    .get_ref(renderable.mesh)
                    .map(|mesh| &mesh.model_layout)
                else {
                    continue;
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
                if let Some(mesh_mut) = self.mesh_assets.get_mut(renderable.mesh) {
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
                .get_ref(renderable.mesh)
                .and_then(|mesh| mesh.model_buffers.get(&key).cloned());
            if let Some(buffer) = buffer {
                self.queue
                    .write_buffer(&buffer, 0, bytemuck::bytes_of(&model));
            }
        }
    }

    pub fn render_scene(
        &mut self,
        world: &mut World,
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

        let mut query = world.query::<(Entity, &Renderable, &GlobalTransform)>();
        let renderables: Vec<_> = query
            .iter(world)
            .map(|(entity, renderable, transform)| (entity, *renderable, transform.0))
            .collect();

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
        for (entity, renderable, _) in &renderables {
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
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
        drop(pass);

        if use_post_processing
            && let (Some(pipeline), Some(bind_group)) =
                (self.post_pipeline.as_ref(), self.post_bind_group.as_ref())
        {
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
