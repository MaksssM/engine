use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use crate::{
    assets::{AssetManager, Handle},
    quality::{QualityPreset, ShaderVariant, ShadowQuality},
    transform::GlobalTransform,
};
use bevy_ecs::{component::Component, entity::Entity, world::World};
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
    pub pipeline: wgpu::RenderPipeline,
}

#[derive(Component, Clone, Copy)]
pub struct Renderable {
    pub mesh: Handle<MeshAsset>,
    pub material: Handle<MaterialAsset>,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    window_size: winit::dpi::PhysicalSize<u32>,
    render_scale_percent: u32,
    post_processing: bool,
    msaa_samples: u32,
    shadow_quality: ShadowQuality,
    mesh_assets: AssetManager<MeshAsset>,
    material_assets: AssetManager<MaterialAsset>,
    cube_mesh: Handle<MeshAsset>,
    cube_material: Handle<MaterialAsset>,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    camera_layout: wgpu::BindGroupLayout,
    light_buffer: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    shadow_pipeline: wgpu::RenderPipeline,
    depth_view: Option<wgpu::TextureView>,
    msaa_view: Option<wgpu::TextureView>,
    scene_view: Option<wgpu::TextureView>,
    scene_sampler: wgpu::Sampler,
    post_pipeline: Option<wgpu::RenderPipeline>,
    post_bind_group: Option<wgpu::BindGroup>,
    camera_position: Vec3,
    camera_angle: f32,
    _shader_variant: PhantomData<ShaderVariant>,
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        preset: QualityPreset,
    ) -> Result<Self, wgpu::SurfaceError> {
        let settings = preset.settings();
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
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
            required_features: wgpu::Features::empty(),
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
        let shadow_view = create_shadow_view(&device);
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
                    resource: wgpu::BindingResource::TextureView(&shadow_view),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene pipeline layout"),
            bind_group_layouts: &[&camera_layout, &model_layout, &light_layout],
            push_constant_ranges: &[],
        });

        let depth_stencil = wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let multisample = wgpu::MultisampleState {
            count: settings.msaa_samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
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

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene pipeline"),
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
            multisample,
            multiview: None,
            cache: None,
        });

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
        let cube_material =
            material_assets.insert("builtin/color".into(), MaterialAsset { pipeline });

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
            camera_buffer,
            camera_bind_group,
            camera_layout,
            light_buffer,
            light_bind_group,
            shadow_view,
            shadow_sampler,
            shadow_pipeline,
            depth_view: None,
            msaa_view: None,
            scene_view: None,
            scene_sampler,
            post_pipeline,
            post_bind_group: None,
            camera_position: Vec3::new(0.0, 1.5, 6.0),
            camera_angle: 0.0,
            _shader_variant: PhantomData,
        };
        renderer.resize(size);
        Ok(renderer)
    }

    pub fn cube_mesh(&self) -> Handle<MeshAsset> {
        self.cube_mesh
    }

    pub fn cube_material(&self) -> Handle<MaterialAsset> {
        self.cube_material
    }

    pub fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        self.window_size = size;
        self.config.width = size.width.max(1);
        self.config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.configure_targets();
    }

    pub fn update(&mut self, world: &mut World, delta_seconds: f32) {
        self.camera_angle += delta_seconds;
        self.camera_position = Vec3::new(
            self.camera_angle.sin() * 6.0,
            1.5,
            self.camera_angle.cos() * 6.0,
        );
        let aspect = self.window_size.width as f32 / self.window_size.height.max(1) as f32;
        let projection = Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, aspect, 0.1, 100.0);
        let view = Mat4::look_at_rh(self.camera_position, Vec3::ZERO, Vec3::Y);
        let camera = CameraUniform {
            view_proj: (projection * view).to_cols_array_2d(),
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));
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

    pub fn render(&mut self, world: &mut World) -> Result<(), wgpu::SurfaceError> {
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

        if self.shadow_quality != ShadowQuality::Disabled {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(2, &self.light_bind_group, &[]);
            for (entity, renderable, _) in &renderables {
                let key = (renderable.mesh.id(), entity.to_bits());
                let Some(mesh) = self.mesh_assets.get(renderable.mesh) else {
                    continue;
                };
                let Some(bind_group) = mesh.model_bind_groups.get(&key) else {
                    continue;
                };
                pass.set_bind_group(1, bind_group, &[]);
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        let scene_target = self.scene_view.as_ref();
        let target_view = scene_target.unwrap_or(&view);
        let resolve_target = if self.msaa_view.is_some() {
            scene_target
        } else {
            None
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.msaa_view.as_ref().unwrap_or(target_view),
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
            pass.set_pipeline(&material.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.set_bind_group(2, &self.light_bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
        drop(pass);

        if self.post_processing
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
        frame.present();
        Ok(())
    }

    fn configure_targets(&mut self) {
        let width =
            (self.window_size.width as f32 * self.render_scale_percent as f32 / 100.0) as u32;
        let height =
            (self.window_size.height as f32 * self.render_scale_percent as f32 / 100.0) as u32;
        let width = width.max(1);
        let height = height.max(1);
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
