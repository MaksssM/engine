// Modern immediate-mode UI system for the engine
use bytemuck::{Pod, Zeroable};

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct UiVertex {
    position: [f32; 2],
    color: [f32; 4],
    uv: [f32; 2],
}

pub struct UiRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    vertex_capacity: usize,
    index_capacity: usize,
    screen_width: f32,
    screen_height: f32,
    // Font texture
    font_texture: Option<wgpu::Texture>,
    font_bind_group: Option<wgpu::BindGroup>,
}

impl UiRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui shader"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
struct UiVertex {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@group(0) @binding(0) var font_texture: texture_2d<f32>;
@group(0) @binding(1) var font_sampler: sampler;

@vertex
fn vs_main(input: UiVertex) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.color = input.color;
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let font_color = textureSample(font_texture, font_sampler, input.uv);
    return input.color * font_color;
}
"#
                .into(),
            ),
        });

        // Create font texture (5x7 pixel font)
        let font_texture = Self::create_font_texture(device, queue);
        let font_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("font bind group layout"),
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
        let font_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("font sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let font_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("font bind group"),
            layout: &font_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &font_texture.create_view(&wgpu::TextureViewDescriptor::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&font_sampler),
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui layout"),
            bind_group_layouts: &[&font_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<UiVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32x2],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            vertex_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui vertices"),
                size: 65536,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            index_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui indices"),
                size: 65536,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            vertex_capacity: 65536 / std::mem::size_of::<UiVertex>(),
            index_capacity: 65536 / std::mem::size_of::<u16>(),
            screen_width: 1280.0,
            screen_height: 720.0,
            font_texture: Some(font_texture),
            font_bind_group: Some(font_bind_group),
        }
    }

    fn create_font_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
        // Create a texture atlas with ASCII characters 32-126 (95 chars)
        // Each char is 5x7 pixels, arranged in a grid
        const CHAR_W: u32 = 5;
        const CHAR_H: u32 = 7;
        const CHARS_PER_ROW: u32 = 16;
        const ROWS: u32 = 6; // 95 chars / 16 = 6 rows
        const ATLAS_W: u32 = CHAR_W * CHARS_PER_ROW;
        const ATLAS_H: u32 = CHAR_H * ROWS;

        let mut pixels = vec![0u8; (ATLAS_W * ATLAS_H * 4) as usize];
        // UVs of [0, 0, 0, 0] are used by solid-color UI quads.
        pixels[0..4].copy_from_slice(&[255, 255, 255, 255]);

        // Generate glyphs for all printable ASCII
        for ch in 32..=126 {
            let idx = ch - 32;
            let col = (idx % CHARS_PER_ROW) as u32;
            let row = (idx / CHARS_PER_ROW) as u32;
            let base_x = col * CHAR_W;
            let base_y = row * CHAR_H;

            if let Some(glyph) = font_glyph(ch as u8) {
                for gy in 0..CHAR_H {
                    for gx in 0..CHAR_W {
                        let bit = (glyph[gy as usize] >> (4 - gx)) & 1;
                        let px = base_x + gx;
                        let py = base_y + gy;
                        let idx = ((py * ATLAS_W + px) * 4) as usize;
                        if bit == 1 {
                            pixels[idx] = 255;
                            pixels[idx + 1] = 255;
                            pixels[idx + 2] = 255;
                            pixels[idx + 3] = 255;
                        }
                    }
                }
            }
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_W,
                height: ATLAS_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_W * 4),
                rows_per_image: Some(ATLAS_H),
            },
            wgpu::Extent3d {
                width: ATLAS_W,
                height: ATLAS_H,
                depth_or_array_layers: 1,
            },
        );

        texture
    }

    pub fn draw(
        &mut self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        commands: &[UiCommand],
    ) {
        if commands.is_empty() {
            return;
        }
        let (vertices, indices) = build_vertices(commands, self.screen_width, self.screen_height);
        if vertices.is_empty() {
            return;
        }
        if vertices.len() > self.vertex_capacity || indices.len() > self.index_capacity {
            return;
        }
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&indices));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        if let Some(bind_group) = &self.font_bind_group {
            pass.set_bind_group(0, bind_group, &[]);
        }
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.screen_width = width as f32;
        self.screen_height = height as f32;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UiCommand {
    pub rect: [f32; 4], // x, y, w, h
    pub color: [f32; 4],
    pub uv_rect: [f32; 4], // u, v, uw, uh in atlas coords
    pub border_radius: f32,
    pub border_width: f32,
    pub border_color: [f32; 4],
}

fn build_vertices(commands: &[UiCommand], width: f32, height: f32) -> (Vec<UiVertex>, Vec<u16>) {
    let to_ndc = |x: f32, y: f32| [x / width * 2.0 - 1.0, 1.0 - y / height * 2.0];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut vertex_index: u16 = 0;

    for command in commands {
        let [x, y, w, h] = command.rect;
        let [u, v, uw, uh] = command.uv_rect;
        let color = command.color;

        // Create 4 vertices for a quad (two triangles)
        let positions = [
            to_ndc(x, y),
            to_ndc(x + w, y),
            to_ndc(x + w, y + h),
            to_ndc(x, y + h),
        ];
        let uvs = [[u, v], [u + uw, v], [u + uw, v + uh], [u, v + uh]];

        let base = vertex_index;
        for i in 0..4 {
            vertices.push(UiVertex {
                position: positions[i],
                color,
                uv: uvs[i],
            });
        }
        vertex_index += 4;

        // Two triangles
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    (vertices, indices)
}

// Font glyph data - 5x7 pixels for ASCII 32-126
fn font_glyph(ch: u8) -> Option<[u8; 7]> {
    match ch {
        b' ' => Some([0, 0, 0, 0, 0, 0, 0]),
        b'!' => Some([
            0b01110, 0b01110, 0b01110, 0b01110, 0b01110, 0b00000, 0b01110,
        ]),
        b'"' => Some([
            0b10001, 0b10001, 0b10001, 0b00000, 0b00000, 0b00000, 0b00000,
        ]),
        b'#' => Some([
            0b00100, 0b01110, 0b11111, 0b00100, 0b11111, 0b01110, 0b00100,
        ]),
        b'$' => Some([
            0b01110, 0b10010, 0b01100, 0b00110, 0b00110, 0b01001, 0b01110,
        ]),
        b'%' => Some([
            0b11000, 0b11001, 0b00010, 0b00100, 0b01000, 0b10011, 0b00011,
        ]),
        b'&' => Some([
            0b01110, 0b10010, 0b10011, 0b01101, 0b10011, 0b10100, 0b01110,
        ]),
        b'\'' => Some([
            0b01100, 0b01100, 0b01100, 0b00000, 0b00000, 0b00000, 0b00000,
        ]),
        b'(' => Some([
            0b00110, 0b01000, 0b10000, 0b10000, 0b10000, 0b01000, 0b00110,
        ]),
        b')' => Some([
            0b01100, 0b00010, 0b00001, 0b00001, 0b00001, 0b00010, 0b01100,
        ]),
        b'*' => Some([
            0b00000, 0b10010, 0b01110, 0b10101, 0b01110, 0b10010, 0b00000,
        ]),
        b'+' => Some([
            0b00000, 0b00100, 0b00100, 0b01110, 0b00100, 0b00100, 0b00000,
        ]),
        b',' => Some([
            0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100, 0b01000,
        ]),
        b'-' => Some([
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ]),
        b'.' => Some([
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ]),
        b'/' => Some([
            0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000,
        ]),
        b'0' => Some([
            0b01110, 0b10011, 0b10101, 0b11001, 0b10011, 0b10001, 0b01110,
        ]),
        b'1' => Some([
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ]),
        b'2' => Some([
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ]),
        b'3' => Some([
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ]),
        b'4' => Some([
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ]),
        b'5' => Some([
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b10001, 0b01110,
        ]),
        b'6' => Some([
            0b01110, 0b10001, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ]),
        b'7' => Some([
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ]),
        b'8' => Some([
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ]),
        b'9' => Some([
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ]),
        b':' => Some([
            0b00000, 0b01110, 0b01110, 0b00000, 0b01110, 0b01110, 0b00000,
        ]),
        b';' => Some([
            0b00000, 0b01110, 0b01110, 0b00000, 0b01100, 0b01100, 0b01000,
        ]),
        b'<' => Some([
            0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
        ]),
        b'=' => Some([
            0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000,
        ]),
        b'>' => Some([
            0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000,
        ]),
        b'?' => Some([
            0b01110, 0b10001, 0b00001, 0b00110, 0b00100, 0b00000, 0b00100,
        ]),
        b'@' => Some([
            0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110,
        ]),
        b'A' => Some([
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ]),
        b'B' => Some([
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ]),
        b'C' => Some([
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ]),
        b'D' => Some([
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ]),
        b'E' => Some([
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ]),
        b'F' => Some([
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ]),
        b'G' => Some([
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ]),
        b'H' => Some([
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ]),
        b'I' => Some([
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ]),
        b'J' => Some([
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b10100, 0b01110,
        ]),
        b'K' => Some([
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ]),
        b'L' => Some([
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ]),
        b'M' => Some([
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ]),
        b'N' => Some([
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ]),
        b'O' => Some([
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ]),
        b'P' => Some([
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ]),
        b'Q' => Some([
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b11010, 0b01110,
        ]),
        b'R' => Some([
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ]),
        b'S' => Some([
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b10001, 0b01110,
        ]),
        b'T' => Some([
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        b'U' => Some([
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ]),
        b'V' => Some([
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ]),
        b'W' => Some([
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ]),
        b'X' => Some([
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ]),
        b'Y' => Some([
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        b'Z' => Some([
            0b11111, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111, 0b11111,
        ]),
        b'[' => Some([
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ]),
        b'\\' => Some([
            0b10000, 0b10000, 0b01000, 0b00100, 0b00010, 0b00001, 0b00001,
        ]),
        b']' => Some([
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ]),
        b'^' => Some([
            0b00000, 0b01010, 0b10101, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        b'_' => Some([
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ]),
        b'`' => Some([
            0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ]),
        b'a' => Some([
            0b00000, 0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111,
        ]),
        b'b' => Some([
            0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b11110,
        ]),
        b'c' => Some([
            0b00000, 0b00000, 0b01110, 0b10001, 0b10000, 0b10001, 0b01110,
        ]),
        b'd' => Some([
            0b00001, 0b00001, 0b01101, 0b10001, 0b10001, 0b10001, 0b01111,
        ]),
        b'e' => Some([
            0b00000, 0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110,
        ]),
        b'f' => Some([
            0b00110, 0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000,
        ]),
        b'g' => Some([
            0b00000, 0b00000, 0b01110, 0b10001, 0b01111, 0b00001, 0b01110,
        ]),
        b'h' => Some([
            0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b10001,
        ]),
        b'i' => Some([
            0b01100, 0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b01110,
        ]),
        b'j' => Some([
            0b00010, 0b00000, 0b01110, 0b00010, 0b00010, 0b10010, 0b01100,
        ]),
        b'k' => Some([
            0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010,
        ]),
        b'l' => Some([
            0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ]),
        b'm' => Some([
            0b00000, 0b00000, 0b10110, 0b11011, 0b10101, 0b10101, 0b10101,
        ]),
        b'n' => Some([
            0b00000, 0b00000, 0b11110, 0b10001, 0b10001, 0b10001, 0b10001,
        ]),
        b'o' => Some([
            0b00000, 0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110,
        ]),
        b'p' => Some([
            0b00000, 0b00000, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000,
        ]),
        b'q' => Some([
            0b00000, 0b00000, 0b01111, 0b10001, 0b01111, 0b00001, 0b00001,
        ]),
        b'r' => Some([
            0b00000, 0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000,
        ]),
        b's' => Some([
            0b00000, 0b00000, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110,
        ]),
        b't' => Some([
            0b01000, 0b01000, 0b11110, 0b01000, 0b01000, 0b01001, 0b00110,
        ]),
        b'u' => Some([
            0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101,
        ]),
        b'v' => Some([
            0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ]),
        b'w' => Some([
            0b00000, 0b00000, 0b10001, 0b10101, 0b10101, 0b10101, 0b11011,
        ]),
        b'x' => Some([
            0b00000, 0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001,
        ]),
        b'y' => Some([
            0b00000, 0b00000, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110,
        ]),
        b'z' => Some([
            0b00000, 0b00000, 0b11111, 0b00010, 0b00100, 0b01000, 0b11111,
        ]),
        b'{' => Some([
            0b00110, 0b01000, 0b01000, 0b00100, 0b01000, 0b01000, 0b00110,
        ]),
        b'|' => Some([
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        b'}' => Some([
            0b01100, 0b00010, 0b00010, 0b00100, 0b00010, 0b00010, 0b01100,
        ]),
        b'~' => Some([
            0b00000, 0b00000, 0b01001, 0b10110, 0b00000, 0b00000, 0b00000,
        ]),
        _ => None,
    }
}

// ============================================================================
// IMMEDIATE MODE UI SYSTEM
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WidgetState {
    Normal,
    Hovered,
    Active,
    Focused,
    Disabled,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WidgetResult {
    pub clicked: bool,
    pub hovered: bool,
    pub focused: bool,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx <= self.x + self.w && my >= self.y && my <= self.y + self.h
    }
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn inset(&self, padding: f32) -> Rect {
        Rect::new(
            self.x + padding,
            self.y + padding,
            self.w - padding * 2.0,
            self.h - padding * 2.0,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
    pub fn to_array(&self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
    pub fn lerp(&self, other: &Color, t: f32) -> Color {
        Color {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }
}

// Theme for consistent styling
#[derive(Clone, Debug)]
pub struct Theme {
    // Colors
    pub bg: Color,
    pub bg_hover: Color,
    pub bg_active: Color,
    pub border: Color,
    pub border_hover: Color,
    pub border_active: Color,
    pub text: Color,
    pub text_disabled: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_active: Color,
    pub danger: Color,
    pub success: Color,
    pub warning: Color,

    // Dimensions
    pub button_height: f32,
    pub slider_height: f32,
    pub checkbox_size: f32,
    pub padding: f32,
    pub spacing: f32,
    pub border_radius: f32,
    pub border_width: f32,

    // Font
    pub font_size: f32,
    pub font_scale: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color::rgb(0.12, 0.13, 0.16),
            bg_hover: Color::rgb(0.18, 0.19, 0.24),
            bg_active: Color::rgb(0.25, 0.26, 0.32),
            border: Color::rgb(0.25, 0.25, 0.30),
            border_hover: Color::rgb(0.35, 0.55, 0.85),
            border_active: Color::rgb(0.45, 0.65, 0.95),
            text: Color::rgb(0.90, 0.92, 0.95),
            text_disabled: Color::rgb(0.45, 0.47, 0.52),
            accent: Color::rgb(0.25, 0.55, 0.90),
            accent_hover: Color::rgb(0.35, 0.65, 1.0),
            accent_active: Color::rgb(0.20, 0.45, 0.80),
            danger: Color::rgb(0.85, 0.35, 0.35),
            success: Color::rgb(0.30, 0.75, 0.35),
            warning: Color::rgb(0.95, 0.75, 0.20),
            button_height: 32.0,
            slider_height: 20.0,
            checkbox_size: 20.0,
            padding: 8.0,
            spacing: 8.0,
            border_radius: 4.0,
            border_width: 1.0,
            font_size: 14.0,
            font_scale: 1.0,
        }
    }
}

// UV coordinates for font atlas (95 chars in 16x6 grid)
const ATLAS_W: f32 = 16.0 * 5.0; // 80 pixels
const ATLAS_H: f32 = 6.0 * 7.0; // 42 pixels
const CHAR_W: f32 = 5.0;
const CHAR_H: f32 = 7.0;

fn char_uv(ch: char) -> [f32; 4] {
    let code = ch as u32;
    if code < 32 || code > 126 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let idx = code - 32;
    let col = (idx % 16) as f32;
    let row = (idx / 16) as f32;
    let u = (col * CHAR_W) / ATLAS_W;
    let v = (row * CHAR_H) / ATLAS_H;
    let uw = CHAR_W / ATLAS_W;
    let uh = CHAR_H / ATLAS_H;
    [u, v, uw, uh]
}

pub fn text_uv_rect(text: &str, scale: f32) -> (f32, f32, Vec<[f32; 4]>) {
    let mut uvs = Vec::new();
    let mut total_w = 0.0;
    let char_advance = CHAR_W * scale;
    let spacing = 1.0 * scale;

    for (i, ch) in text.chars().enumerate() {
        if ch == ' ' {
            total_w += char_advance * 0.5;
            uvs.push([0.0, 0.0, 0.0, 0.0]);
        } else {
            uvs.push(char_uv(ch));
            total_w += char_advance;
        }
        if i < text.len() - 1 {
            total_w += spacing;
        }
    }
    let h = CHAR_H * scale;
    (total_w, h, uvs)
}

// UI Context - holds state for immediate mode UI
pub struct Ui {
    pub theme: Theme,
    pub commands: Vec<UiCommand>,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_down: bool,
    pub mouse_clicked: bool,
    pub mouse_released: bool,
    pub hot_item: Option<u64>,
    pub active_item: Option<u64>,
    pub focus_item: Option<u64>,
    pub id_stack: Vec<u64>,
    pub next_id: u64,
    pub layout_stack: Vec<Layout>,
    pub current_layout: Layout,
    pub frame: u64,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub direction: LayoutDirection,
    pub padding: f32,
    pub spacing: f32,
    pub next_y: f32,
    pub next_x: f32,
    pub max_x: f32,
    pub max_y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutDirection {
    Vertical,
    Horizontal,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            x: 10.0,
            y: 10.0,
            width: 300.0,
            height: 400.0,
            direction: LayoutDirection::Vertical,
            padding: 8.0,
            spacing: 8.0,
            next_y: 10.0,
            next_x: 10.0,
            max_x: 0.0,
            max_y: 0.0,
        }
    }
}

impl Ui {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
            commands: Vec::new(),
            mouse_x: 0.0,
            mouse_y: 0.0,
            mouse_down: false,
            mouse_clicked: false,
            mouse_released: false,
            hot_item: None,
            active_item: None,
            focus_item: None,
            id_stack: Vec::new(),
            next_id: 1,
            layout_stack: Vec::new(),
            current_layout: Layout::default(),
            frame: 0,
        }
    }

    pub fn begin_frame(
        &mut self,
        mouse_x: f32,
        mouse_y: f32,
        mouse_down: bool,
        mouse_clicked: bool,
        mouse_released: bool,
    ) {
        self.mouse_x = mouse_x;
        self.mouse_y = mouse_y;
        self.mouse_down = mouse_down;
        self.mouse_clicked = mouse_clicked;
        self.mouse_released = mouse_released;
        self.commands.clear();
        self.frame += 1;
        self.next_id = 1;
        self.hot_item = None;
    }

    pub fn end_frame(&mut self) -> &[UiCommand] {
        &self.commands
    }

    pub fn gen_id(&mut self, label: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        // Hash label with id for stable IDs
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&(id, label), &mut hasher);
        std::hash::Hasher::finish(&hasher)
    }

    fn push_id(&mut self, id: u64) {
        self.id_stack.push(id);
    }

    fn pop_id(&mut self) {
        self.id_stack.pop();
    }

    fn current_id(&self) -> u64 {
        *self.id_stack.last().unwrap_or(&0)
    }

    // Layout helpers
    pub fn begin_panel(&mut self, x: f32, y: f32, width: f32, height: f32) -> Rect {
        let panel = Rect::new(x, y, width, height);
        // Draw panel background
        self.commands.push(UiCommand {
            rect: [x, y, width, height],
            color: self.theme.bg.to_array(),
            uv_rect: [0.0, 0.0, 0.0, 0.0],
            border_radius: self.theme.border_radius,
            border_width: self.theme.border_width,
            border_color: self.theme.border.to_array(),
        });

        // Push new layout
        self.layout_stack.push(self.current_layout.clone());
        self.current_layout = Layout {
            x: x + self.theme.padding,
            y: y + self.theme.padding,
            width: width - self.theme.padding * 2.0,
            height: height - self.theme.padding * 2.0,
            direction: LayoutDirection::Vertical,
            padding: self.theme.padding,
            spacing: self.theme.spacing,
            next_y: y + self.theme.padding,
            next_x: x + self.theme.padding,
            max_x: x + self.theme.padding,
            max_y: y + self.theme.padding,
        };
        panel
    }

    pub fn end_panel(&mut self) {
        if let Some(prev) = self.layout_stack.pop() {
            self.current_layout = prev;
        }
    }

    pub fn layout_row(&mut self, height: f32) -> Rect {
        let l = &mut self.current_layout;
        let rect = Rect::new(l.next_x, l.next_y, l.width, height);
        l.next_y += height + l.spacing;
        l.max_y = l.max_y.max(l.next_y);
        rect
    }

    pub fn layout_col(&mut self, width: f32) -> Rect {
        let l = &mut self.current_layout;
        let rect = Rect::new(l.next_x, l.next_y, width, l.height);
        l.next_x += width + l.spacing;
        l.max_x = l.max_x.max(l.next_x);
        rect
    }

    // Core widget functions
    pub fn button_behavior(&mut self, id: u64, rect: Rect, enabled: bool) -> WidgetResult {
        let hovered = enabled && rect.contains(self.mouse_x, self.mouse_y);
        let is_hot = self.hot_item == Some(id);

        if hovered && !is_hot && self.hot_item.is_none() {
            self.hot_item = Some(id);
        } else if !hovered && is_hot {
            self.hot_item = None;
        }

        let mut result = WidgetResult::default();
        result.hovered = hovered;

        if enabled && self.mouse_clicked && hovered {
            self.active_item = Some(id);
            self.focus_item = Some(id);
        }
        if enabled && self.active_item == Some(id) {
            if self.mouse_released {
                result.clicked = hovered;
                self.active_item = None;
            } else if self.mouse_down {
                result.hovered = true;
            }
        }

        result
    }

    pub fn draw_rect(&mut self, rect: Rect, color: Color, border_color: Color, border_width: f32) {
        if border_width > 0.0 {
            // Draw border as 4 rectangles
            // Top
            self.commands.push(UiCommand {
                rect: [
                    rect.x - border_width,
                    rect.y - border_width,
                    rect.w + border_width * 2.0,
                    border_width,
                ],
                color: border_color.to_array(),
                uv_rect: [0.0, 0.0, 0.0, 0.0],
                border_radius: 0.0,
                border_width: 0.0,
                border_color: [0.0, 0.0, 0.0, 0.0],
            });
            // Bottom
            self.commands.push(UiCommand {
                rect: [
                    rect.x - border_width,
                    rect.y + rect.h,
                    rect.w + border_width * 2.0,
                    border_width,
                ],
                color: border_color.to_array(),
                uv_rect: [0.0, 0.0, 0.0, 0.0],
                border_radius: 0.0,
                border_width: 0.0,
                border_color: [0.0, 0.0, 0.0, 0.0],
            });
            // Left
            self.commands.push(UiCommand {
                rect: [rect.x - border_width, rect.y, border_width, rect.h],
                color: border_color.to_array(),
                uv_rect: [0.0, 0.0, 0.0, 0.0],
                border_radius: 0.0,
                border_width: 0.0,
                border_color: [0.0, 0.0, 0.0, 0.0],
            });
            // Right
            self.commands.push(UiCommand {
                rect: [rect.x + rect.w, rect.y, border_width, rect.h],
                color: border_color.to_array(),
                uv_rect: [0.0, 0.0, 0.0, 0.0],
                border_radius: 0.0,
                border_width: 0.0,
                border_color: [0.0, 0.0, 0.0, 0.0],
            });
        }

        self.commands.push(UiCommand {
            rect: [rect.x, rect.y, rect.w, rect.h],
            color: color.to_array(),
            uv_rect: [0.0, 0.0, 0.0, 0.0],
            border_radius: self.theme.border_radius,
            border_width: 0.0,
            border_color: [0.0, 0.0, 0.0, 0.0],
        });
    }

    pub fn draw_text(&mut self, x: f32, y: f32, text: &str, color: Color, scale: f32) {
        let mut cx = x;
        let char_w = CHAR_W * scale;
        let spacing = 1.0 * scale;

        for ch in text.chars() {
            if ch == ' ' {
                cx += char_w * 0.5 + spacing;
                continue;
            }
            let uv = char_uv(ch);
            self.commands.push(UiCommand {
                rect: [cx, y, char_w, CHAR_H * scale],
                color: color.to_array(),
                uv_rect: uv,
                border_radius: 0.0,
                border_width: 0.0,
                border_color: [0.0, 0.0, 0.0, 0.0],
            });
            cx += char_w + spacing;
        }
    }

    // Public widget API
    pub fn label(&mut self, text: &str) -> WidgetResult {
        let id = self.gen_id(text);
        let (tw, th, _) = text_uv_rect(text, self.theme.font_scale);
        let rect = Rect::new(
            self.current_layout.next_x,
            self.current_layout.next_y,
            tw,
            th,
        );

        let result = self.button_behavior(id, rect, false);

        let color = if result.hovered {
            self.theme.text
        } else {
            self.theme.text
        };
        self.draw_text(
            rect.x,
            rect.y + (rect.h - th) / 2.0,
            text,
            color,
            self.theme.font_scale,
        );

        if self.current_layout.direction == LayoutDirection::Vertical {
            self.current_layout.next_y += th + self.current_layout.spacing;
        } else {
            self.current_layout.next_x += tw + self.current_layout.spacing;
        }
        self.current_layout.max_y = self.current_layout.max_y.max(self.current_layout.next_y);
        self.current_layout.max_x = self.current_layout.max_x.max(self.current_layout.next_x);

        result
    }

    pub fn button(&mut self, label: &str) -> WidgetResult {
        let id = self.gen_id(label);
        let (tw, th, _) = text_uv_rect(label, self.theme.font_scale);
        let btn_w = tw.max(80.0) + self.theme.padding * 2.0;
        let btn_h = self.theme.button_height;
        let rect = Rect::new(
            self.current_layout.next_x,
            self.current_layout.next_y,
            btn_w,
            btn_h,
        );

        let result = self.button_behavior(id, rect, true);

        let (bg_color, border_color) = if !result.hovered && self.active_item != Some(id) {
            (self.theme.bg, self.theme.border)
        } else if self.active_item == Some(id) {
            (self.theme.bg_active, self.theme.border_active)
        } else {
            (self.theme.bg_hover, self.theme.border_hover)
        };

        self.draw_rect(rect, bg_color, border_color, self.theme.border_width);

        let text_color = self.theme.text;
        let text_x = rect.x + (rect.w - tw) / 2.0;
        let text_y = rect.y + (rect.h - th) / 2.0;
        self.draw_text(text_x, text_y, label, text_color, self.theme.font_scale);

        if self.current_layout.direction == LayoutDirection::Vertical {
            self.current_layout.next_y += btn_h + self.current_layout.spacing;
        } else {
            self.current_layout.next_x += btn_w + self.current_layout.spacing;
        }
        self.current_layout.max_y = self.current_layout.max_y.max(self.current_layout.next_y);
        self.current_layout.max_x = self.current_layout.max_x.max(self.current_layout.next_x);

        result
    }

    pub fn button_ex(&mut self, label: &str, color: Color, enabled: bool) -> WidgetResult {
        let id = self.gen_id(label);
        let (tw, th, _) = text_uv_rect(label, self.theme.font_scale);
        let btn_w = tw.max(80.0) + self.theme.padding * 2.0;
        let btn_h = self.theme.button_height;
        let rect = Rect::new(
            self.current_layout.next_x,
            self.current_layout.next_y,
            btn_w,
            btn_h,
        );

        let result = self.button_behavior(id, rect, enabled);

        let (bg_color, border_color, text_color) = if !enabled {
            (self.theme.bg, self.theme.border, self.theme.text_disabled)
        } else if !result.hovered && self.active_item != Some(id) {
            (color, self.theme.border, self.theme.text)
        } else if self.active_item == Some(id) {
            (
                Color::rgba(color.r * 0.8, color.g * 0.8, color.b * 0.8, color.a),
                self.theme.border_active,
                self.theme.text,
            )
        } else {
            (
                Color::rgba(color.r * 1.2, color.g * 1.2, color.b * 1.2, color.a),
                self.theme.border_hover,
                self.theme.text,
            )
        };

        self.draw_rect(rect, bg_color, border_color, self.theme.border_width);

        let text_x = rect.x + (rect.w - tw) / 2.0;
        let text_y = rect.y + (rect.h - th) / 2.0;
        self.draw_text(text_x, text_y, label, text_color, self.theme.font_scale);

        if self.current_layout.direction == LayoutDirection::Vertical {
            self.current_layout.next_y += btn_h + self.current_layout.spacing;
        } else {
            self.current_layout.next_x += btn_w + self.current_layout.spacing;
        }
        self.current_layout.max_y = self.current_layout.max_y.max(self.current_layout.next_y);
        self.current_layout.max_x = self.current_layout.max_x.max(self.current_layout.next_x);

        result
    }

    pub fn checkbox(&mut self, label: &str, checked: &mut bool) -> WidgetResult {
        let id = self.gen_id(label);
        let size = self.theme.checkbox_size;
        let box_rect = Rect::new(
            self.current_layout.next_x,
            self.current_layout.next_y,
            size,
            size,
        );

        let result = self.button_behavior(id, box_rect, true);

        if result.clicked {
            *checked = !*checked;
        }

        let (bg_color, border_color) = if *checked {
            (self.theme.accent, self.theme.accent)
        } else if result.hovered {
            (self.theme.bg_hover, self.theme.border_hover)
        } else {
            (self.theme.bg, self.theme.border)
        };

        self.draw_rect(box_rect, bg_color, border_color, self.theme.border_width);

        // Draw checkmark
        if *checked {
            let cx = box_rect.x + size * 0.25;
            let cy = box_rect.y + size * 0.2;
            let cw = size * 0.5;
            let ch = size * 0.6;
            // Draw a simple checkmark as rectangles
            self.commands.push(UiCommand {
                rect: [cx, cy + ch * 0.5, cw * 0.3, ch * 0.5],
                color: self.theme.text.to_array(),
                uv_rect: [0.0, 0.0, 0.0, 0.0],
                border_radius: 0.0,
                border_width: 0.0,
                border_color: [0.0, 0.0, 0.0, 0.0],
            });
            self.commands.push(UiCommand {
                rect: [cx + cw * 0.7, cy, cw * 0.3, ch * 0.5],
                color: self.theme.text.to_array(),
                uv_rect: [0.0, 0.0, 0.0, 0.0],
                border_radius: 0.0,
                border_width: 0.0,
                border_color: [0.0, 0.0, 0.0, 0.0],
            });
        }

        // Label
        let label_x = box_rect.x + size + self.theme.padding;
        let (tw, th, _) = text_uv_rect(label, self.theme.font_scale);
        self.draw_text(
            label_x,
            box_rect.y + (size - th) / 2.0,
            label,
            self.theme.text,
            self.theme.font_scale,
        );

        let total_w = size + self.theme.padding + tw;
        if self.current_layout.direction == LayoutDirection::Vertical {
            self.current_layout.next_y += size.max(th) + self.current_layout.spacing;
        } else {
            self.current_layout.next_x += total_w + self.current_layout.spacing;
        }
        self.current_layout.max_y = self.current_layout.max_y.max(self.current_layout.next_y);
        self.current_layout.max_x = self.current_layout.max_x.max(self.current_layout.next_x);

        result
    }

    pub fn slider(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> WidgetResult {
        let id = self.gen_id(label);
        let slider_h = self.theme.slider_height;
        let track_w = self.current_layout.width;
        let track_rect = Rect::new(
            self.current_layout.next_x,
            self.current_layout.next_y,
            track_w,
            slider_h,
        );
        let thumb_w = 16.0;
        let thumb_h = slider_h + 4.0;

        let range = (max - min).max(f32::EPSILON);
        *value = value.clamp(min, max);
        let mut t = ((*value - min) / range).clamp(0.0, 1.0);
        let thumb_x = track_rect.x + t * (track_w - thumb_w);
        let thumb_rect = Rect::new(thumb_x, track_rect.y - 2.0, thumb_w, thumb_h);

        let track_hovered = track_rect.contains(self.mouse_x, self.mouse_y);
        let thumb_hovered = thumb_rect.contains(self.mouse_x, self.mouse_y);
        let hovered = track_hovered || thumb_hovered;

        let is_hot = self.hot_item == Some(id);

        if hovered && !is_hot && self.hot_item.is_none() {
            self.hot_item = Some(id);
        } else if !hovered && is_hot {
            self.hot_item = None;
        }

        let mut result = WidgetResult::default();
        result.hovered = hovered;

        if self.mouse_clicked && hovered {
            self.active_item = Some(id);
            result.changed = true;
        }
        if self.active_item == Some(id) {
            if self.mouse_released {
                self.active_item = None;
            } else if self.mouse_down || self.mouse_clicked {
                t = ((self.mouse_x - track_rect.x - thumb_w * 0.5) / (track_w - thumb_w).max(1.0))
                    .clamp(0.0, 1.0);
                let next_value = min + t * range;
                result.changed = (next_value - *value).abs() > f32::EPSILON;
                *value = next_value;
            }
        }

        // Draw track
        let track_color = if self.active_item == Some(id) {
            self.theme.accent
        } else if hovered {
            self.theme.bg_hover
        } else {
            self.theme.bg
        };
        self.draw_rect(
            track_rect,
            track_color,
            self.theme.border,
            self.theme.border_width,
        );

        // Draw filled portion
        let fill_w = (thumb_x + thumb_w * 0.5 - track_rect.x).clamp(0.0, track_w);
        if fill_w > 0.0 {
            let fill_rect = Rect::new(track_rect.x, track_rect.y, fill_w, track_rect.h);
            self.draw_rect(
                fill_rect,
                self.theme.accent,
                Color::rgba(0.0, 0.0, 0.0, 0.0),
                0.0,
            );
        }

        // Draw thumb
        let thumb_color = if self.active_item == Some(id) {
            self.theme.accent_active
        } else if hovered {
            self.theme.accent_hover
        } else {
            self.theme.accent
        };
        let thumb_x = track_rect.x + t * (track_w - thumb_w);
        let thumb_rect = Rect::new(thumb_x, track_rect.y - 2.0, thumb_w, thumb_h);
        self.draw_rect(
            thumb_rect,
            thumb_color,
            self.theme.border,
            self.theme.border_width,
        );

        // Label
        let label_text = format!("{}: {:.0}%", label, *value);
        let (_tw, th, _) = text_uv_rect(&label_text, self.theme.font_scale);
        self.draw_text(
            track_rect.x,
            track_rect.y - th - 2.0,
            &label_text,
            self.theme.text,
            self.theme.font_scale,
        );

        self.current_layout.next_y += slider_h + th + self.current_layout.spacing + 4.0;
        self.current_layout.max_y = self.current_layout.max_y.max(self.current_layout.next_y);

        result
    }

    pub fn separator(&mut self) {
        let sep_h = 1.0;
        let rect = Rect::new(
            self.current_layout.next_x,
            self.current_layout.next_y + self.current_layout.spacing / 2.0,
            self.current_layout.width,
            sep_h,
        );
        self.commands.push(UiCommand {
            rect: [rect.x, rect.y, rect.w, rect.h],
            color: self.theme.border.to_array(),
            uv_rect: [0.0, 0.0, 0.0, 0.0],
            border_radius: 0.0,
            border_width: 0.0,
            border_color: [0.0, 0.0, 0.0, 0.0],
        });
        self.current_layout.next_y += sep_h + self.current_layout.spacing;
        self.current_layout.max_y = self.current_layout.max_y.max(self.current_layout.next_y);
    }

    pub fn spacing(&mut self, amount: f32) {
        if self.current_layout.direction == LayoutDirection::Vertical {
            self.current_layout.next_y += amount;
        } else {
            self.current_layout.next_x += amount;
        }
    }
}

// Legacy widget structs for compatibility
pub struct RectLegacy {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectLegacy {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx <= self.x + self.w && my >= self.y && my <= self.y + self.h
    }
}

#[derive(Clone, Copy, Debug)]
pub enum WidgetStateLegacy {
    Normal,
    Hovered,
    Active,
    Disabled,
}

#[derive(Clone, Copy, Debug)]
pub struct WidgetResultLegacy {
    pub hovered: bool,
    pub clicked: bool,
}

pub struct Button<'a> {
    pub rect: RectLegacy,
    pub label: &'a str,
    pub state: WidgetStateLegacy,
}

impl<'a> Button<'a> {
    pub fn new(x: f32, y: f32, w: f32, h: f32, label: &'a str) -> Self {
        Self {
            rect: RectLegacy::new(x, y, w, h),
            label,
            state: WidgetStateLegacy::Normal,
        }
    }
    pub fn update(&mut self, mx: f32, my: f32, mouse_down: bool) -> WidgetResultLegacy {
        let hovered = self.rect.contains(mx, my);
        self.state = if hovered {
            if mouse_down {
                WidgetStateLegacy::Active
            } else {
                WidgetStateLegacy::Hovered
            }
        } else {
            WidgetStateLegacy::Normal
        };
        WidgetResultLegacy {
            hovered,
            clicked: hovered && !mouse_down,
        }
    }
}

pub struct Slider<'a> {
    pub rect: RectLegacy,
    pub label: &'a str,
    pub min: f32,
    pub max: f32,
    pub value: f32,
    pub dragging: bool,
}

impl<'a> Slider<'a> {
    pub fn new(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &'a str,
        min: f32,
        max: f32,
        value: f32,
    ) -> Self {
        Self {
            rect: RectLegacy::new(x, y, w, h),
            label,
            min,
            max,
            value,
            dragging: false,
        }
    }
    pub fn update(
        &mut self,
        mx: f32,
        my: f32,
        mouse_down: bool,
        was_dragging: bool,
    ) -> WidgetResultLegacy {
        let hovered = self.rect.contains(mx, my);
        self.dragging = if mouse_down && hovered {
            true
        } else if !mouse_down {
            false
        } else {
            was_dragging
        };
        if self.dragging {
            let t = ((mx - self.rect.x) / self.rect.w).clamp(0.0, 1.0);
            self.value = self.min + t * (self.max - self.min);
        }
        WidgetResultLegacy {
            hovered: hovered || self.dragging,
            clicked: false,
        }
    }
    pub fn track_color(&self) -> [f32; 4] {
        [0.2, 0.2, 0.25, 0.9]
    }
    pub fn thumb_color(&self) -> [f32; 4] {
        if self.dragging {
            [0.5, 0.5, 0.7, 1.0]
        } else {
            [0.35, 0.35, 0.45, 1.0]
        }
    }
    pub fn thumb_rect(&self) -> RectLegacy {
        let t = (self.value - self.min) / (self.max - self.min);
        let thumb_w = 12.0;
        let x = self.rect.x + t * (self.rect.w - thumb_w);
        RectLegacy::new(x, self.rect.y - 4.0, thumb_w, self.rect.h + 8.0)
    }
}

pub struct Checkbox<'a> {
    pub rect: RectLegacy,
    pub label: &'a str,
    pub checked: bool,
    pub state: WidgetStateLegacy,
}

impl<'a> Checkbox<'a> {
    pub fn new(x: f32, y: f32, size: f32, label: &'a str, checked: bool) -> Self {
        Self {
            rect: RectLegacy::new(x, y, size, size),
            label,
            checked,
            state: WidgetStateLegacy::Normal,
        }
    }
    pub fn update(&mut self, mx: f32, my: f32, mouse_down: bool) -> WidgetResultLegacy {
        let hovered = self.rect.contains(mx, my);
        self.state = if hovered {
            if mouse_down {
                WidgetStateLegacy::Active
            } else {
                WidgetStateLegacy::Hovered
            }
        } else {
            WidgetStateLegacy::Normal
        };
        WidgetResultLegacy {
            hovered,
            clicked: hovered && !mouse_down,
        }
    }
    pub fn toggle(&mut self) {
        self.checked = !self.checked;
    }
    pub fn box_color(&self) -> [f32; 4] {
        if self.checked {
            [0.2, 0.7, 0.3, 1.0]
        } else {
            [0.15, 0.15, 0.20, 0.9]
        }
    }
    pub fn box_border(&self) -> [f32; 4] {
        match self.state {
            WidgetStateLegacy::Hovered => [0.3, 0.6, 0.9, 1.0],
            WidgetStateLegacy::Active => [0.4, 0.7, 1.0, 1.0],
            _ => [0.25, 0.25, 0.3, 1.0],
        }
    }
}

pub struct QualityPanel {
    pub rect: RectLegacy,
    pub buttons: Vec<Button<'static>>,
    pub current: usize,
}

impl QualityPanel {
    pub fn new(_screen_width: f32, _screen_height: f32) -> Self {
        let preset_names = ["Performance", "Balanced", "Quality"];
        let mut buttons = Vec::new();
        let button_w = 90.0;
        let button_h = 28.0;
        let gap = 8.0;
        let start_x = 10.0;
        let y = 50.0;
        for (i, name) in preset_names.iter().enumerate() {
            buttons.push(Button::new(
                start_x + i as f32 * (button_w + gap),
                y,
                button_w,
                button_h,
                *name,
            ));
        }
        Self {
            rect: RectLegacy::new(start_x, 10.0, 300.0, 60.0),
            buttons,
            current: 0,
        }
    }
    pub fn preset_name(&self) -> &'static str {
        ["Performance", "Balanced", "Quality"][self.current]
    }
    pub fn update(&mut self, mx: f32, my: f32, mouse_down: bool) -> bool {
        let mut changed = false;
        for (i, btn) in self.buttons.iter_mut().enumerate() {
            let res = btn.update(mx, my, mouse_down);
            if res.clicked {
                self.current = i;
                changed = true;
            }
        }
        changed
    }
}

pub struct RenderScaleSlider {
    pub rect: RectLegacy,
    pub label: &'static str,
    pub slider: Slider<'static>,
    pub value_percent: u32,
}

impl RenderScaleSlider {
    pub fn new(_screen_width: f32, _screen_height: f32) -> Self {
        let slider = Slider::new(10.0, 140.0, 200.0, 20.0, "Render Scale", 50.0, 200.0, 100.0);
        Self {
            rect: RectLegacy::new(10.0, 120.0, 220.0, 50.0),
            label: "Render Scale",
            slider,
            value_percent: 100,
        }
    }
    pub fn update(&mut self, mx: f32, my: f32, mouse_down: bool) -> bool {
        let was_dragging = self.slider.dragging;
        let res = self.slider.update(mx, my, mouse_down, was_dragging);
        self.value_percent = self.slider.value as u32;
        res.hovered
    }
}

pub struct WireframeCheckbox {
    pub checkbox: Checkbox<'static>,
}

impl WireframeCheckbox {
    pub fn new(x: f32, y: f32, size: f32, label: &'static str, checked: bool) -> Self {
        Self {
            checkbox: Checkbox::new(x, y, size, label, checked),
        }
    }
    pub fn update(&mut self, mx: f32, my: f32, mouse_down: bool) -> WidgetResultLegacy {
        self.checkbox.update(mx, my, mouse_down)
    }
    pub fn toggle(&mut self) {
        self.checkbox.toggle();
    }
    pub fn checked(&self) -> bool {
        self.checkbox.checked
    }
}

pub struct MouseInput {
    pub x: f32,
    pub y: f32,
    pub left_down: bool,
    pub left_clicked: bool,
    pub left_released: bool,
}

impl Default for MouseInput {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            left_down: false,
            left_clicked: false,
            left_released: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_click() {
        let mut ui = Ui::new();
        ui.begin_frame(50.0, 50.0, true, true, false);
        ui.begin_panel(10.0, 10.0, 200.0, 100.0);
        assert!(!ui.button("Test").clicked);
        ui.end_panel();
        ui.end_frame();

        ui.begin_frame(50.0, 50.0, false, false, true);
        ui.begin_panel(10.0, 10.0, 200.0, 100.0);
        assert!(ui.button("Test").clicked);
        ui.end_panel();
        ui.end_frame();
    }

    #[test]
    fn checkbox_toggle() {
        let mut ui = Ui::new();
        let mut checked = false;
        ui.begin_frame(20.0, 20.0, true, true, false);
        ui.begin_panel(10.0, 10.0, 200.0, 100.0);
        ui.checkbox("Test", &mut checked);
        assert!(!checked);
        ui.end_panel();
        ui.end_frame();

        ui.begin_frame(20.0, 20.0, false, false, true);
        ui.begin_panel(10.0, 10.0, 200.0, 100.0);
        ui.checkbox("Test", &mut checked);
        assert!(checked);
        ui.end_panel();
        ui.end_frame();
    }

    #[test]
    fn slider_drag() {
        let mut ui = Ui::new();
        let mut value = 0.5;
        ui.begin_frame(150.0, 20.0, true, true, false);
        ui.begin_panel(10.0, 10.0, 200.0, 100.0);
        ui.slider("Test", &mut value, 0.0, 1.0);
        assert!(value > 0.5);
        ui.end_panel();
        ui.end_frame();
    }
}
