use bytemuck::{Pod, Zeroable};
use winit::event::ElementState;

#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
#[repr(C)]
struct UiVertex {
    position: [f32; 2],
    color: [f32; 4],
}

pub struct UiRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: usize,
    screen_width: f32,
    screen_height: f32,
}

impl UiRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui shader"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
struct UiVertex {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: UiVertex) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
"#
                .into(),
            ),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui layout"),
            bind_group_layouts: &[],
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
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
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
            vertex_capacity: 65536,
            screen_width: 1280.0,
            screen_height: 720.0,
        }
    }

    pub fn draw(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        commands: &[UiCommand],
    ) {
        if commands.is_empty() {
            return;
        }
        let vertices = build_vertices(commands, self.screen_width, self.screen_height);
        if vertices.is_empty() {
            return;
        }
        if vertices.len() > self.vertex_capacity {
            return;
        }
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
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
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }

    pub fn draw_rects(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        commands: &[UiCommand],
    ) {
        let vertices = build_vertices(commands, self.screen_width, self.screen_height);
        if vertices.is_empty() || vertices.len() > self.vertex_capacity {
            return;
        }
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui rectangles"),
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
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }

    pub fn draw_text(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        x: f32,
        y: f32,
        text: &str,
        color: [f32; 4],
        scale: f32,
    ) {
        let mut commands = Vec::new();
        let mut cx = x;
        let mut cy = y;
        let char_width = 5.0 * scale;
        let char_height = 7.0 * scale;
        let spacing = 1.0 * scale;

        for ch in text.chars() {
            if ch == '\n' {
                cx = x;
                cy += char_height + 2.0 * scale;
                continue;
            }
            if let Some(glyph) = font_glyph(ch) {
                for (gy, row) in glyph.iter().enumerate() {
                    for gx in 0..5 {
                        // Glyph data is written visually (MSB=left), but bits are LSB=right.
                        // Read bit (4 - gx) so gx=0 (leftmost) checks bit 4 (MSB).
                        if row & (1 << (4 - gx)) != 0 {
                            commands.push(UiCommand {
                                rect: [
                                    cx + gx as f32 * scale,
                                    cy + gy as f32 * scale,
                                    scale,
                                    scale,
                                ],
                                color,
                            });
                        }
                    }
                }
            }
            cx += char_width + spacing;
        }

        if commands.is_empty() {
            return;
        }
        let vertices = build_vertices(&commands, self.screen_width, self.screen_height);
        if vertices.len() > self.vertex_capacity {
            return;
        }
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui text pass"),
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
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.screen_width = width as f32;
        self.screen_height = height as f32;
    }
}

pub struct UiCommand {
    pub rect: [f32; 4],
    pub color: [f32; 4],
}

fn build_vertices(commands: &[UiCommand], width: f32, height: f32) -> Vec<UiVertex> {
    let to_ndc = |x: f32, y: f32| [x / width * 2.0 - 1.0, 1.0 - y / height * 2.0];
    let mut vertices = Vec::with_capacity(commands.len() * 6);
    for command in commands {
        let [x, y, w, h] = command.rect;
        let color = command.color;
        let positions = [
            to_ndc(x, y),
            to_ndc(x + w, y),
            to_ndc(x + w, y + h),
            to_ndc(x, y),
            to_ndc(x + w, y + h),
            to_ndc(x, y + h),
        ];
        for position in positions {
            vertices.push(UiVertex { position, color });
        }
    }
    vertices
}

fn font_glyph(ch: char) -> Option<[u8; 7]> {
    match ch {
        'A' => Some([
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ]),
        'B' => Some([
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ]),
        'C' => Some([
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ]),
        'D' => Some([
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ]),
        'E' => Some([
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ]),
        'F' => Some([
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ]),
        'G' => Some([
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ]),
        'H' => Some([
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ]),
        'I' => Some([
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ]),
        'J' => Some([
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b10100, 0b01110,
        ]),
        'K' => Some([
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ]),
        'L' => Some([
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ]),
        'M' => Some([
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ]),
        'N' => Some([
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ]),
        'O' => Some([
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ]),
        'P' => Some([
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ]),
        'Q' => Some([
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b11010, 0b01110,
        ]),
        'R' => Some([
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ]),
        'S' => Some([
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b10001, 0b01110,
        ]),
        'T' => Some([
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        'U' => Some([
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ]),
        'V' => Some([
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ]),
        'W' => Some([
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ]),
        'X' => Some([
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ]),
        'Y' => Some([
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ]),
        'Z' => Some([
            0b11111, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111, 0b11111,
        ]),
        '0' => Some([
            0b01110, 0b10011, 0b10101, 0b11001, 0b10011, 0b10001, 0b01110,
        ]),
        '1' => Some([
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ]),
        '2' => Some([
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ]),
        '3' => Some([
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ]),
        '4' => Some([
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ]),
        '5' => Some([
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b10001, 0b01110,
        ]),
        '6' => Some([
            0b01110, 0b10001, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ]),
        '7' => Some([
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ]),
        '8' => Some([
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ]),
        '9' => Some([
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ]),
        ':' => Some([
            0b00000, 0b01110, 0b01110, 0b00000, 0b01110, 0b01110, 0b00000,
        ]),
        '.' => Some([
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01110, 0b01110,
        ]),
        '-' => Some([
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ]),
        '+' => Some([
            0b00000, 0b00100, 0b00100, 0b01110, 0b00100, 0b00100, 0b00000,
        ]),
        '/' => Some([
            0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000,
        ]),
        ' ' => Some([0, 0, 0, 0, 0, 0, 0]),
        _ => None,
    }
}

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
    pub fn to_command(&self, color: [f32; 4]) -> UiCommand {
        UiCommand {
            rect: [self.x, self.y, self.w, self.h],
            color,
        }
    }
}

/// State of a UI widget: normal, hovered, pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetState {
    Normal,
    Hovered,
    Pressed,
}

/// Result of processing a widget: whether it was clicked this frame.
#[derive(Clone, Copy, Debug)]
pub struct WidgetResult {
    pub clicked: bool,
    pub hovered: bool,
}

/// A simple push button.
pub struct Button<'a> {
    pub rect: Rect,
    pub label: &'a str,
    pub state: WidgetState,
}

impl<'a> Button<'a> {
    pub fn new(x: f32, y: f32, w: f32, h: f32, label: &'a str) -> Self {
        Self {
            rect: Rect::new(x, y, w, h),
            label,
            state: WidgetState::Normal,
        }
    }

    pub fn update(&mut self, mx: f32, my: f32, mouse_down: bool) -> WidgetResult {
        let hovered = self.rect.contains(mx, my);
        let was_pressed = self.state == WidgetState::Pressed;
        self.state = if hovered && mouse_down {
            WidgetState::Pressed
        } else if hovered {
            WidgetState::Hovered
        } else {
            WidgetState::Normal
        };
        WidgetResult {
            clicked: hovered && was_pressed && !mouse_down,
            hovered,
        }
    }

    pub fn color(&self) -> [f32; 4] {
        match self.state {
            WidgetState::Normal => [0.15, 0.15, 0.20, 0.9],
            WidgetState::Hovered => [0.25, 0.25, 0.35, 0.95],
            WidgetState::Pressed => [0.35, 0.35, 0.50, 1.0],
        }
    }

    pub fn text_color(&self) -> [f32; 4] {
        match self.state {
            WidgetState::Normal => [0.9, 0.9, 0.9, 1.0],
            WidgetState::Hovered => [1.0, 1.0, 1.0, 1.0],
            WidgetState::Pressed => [1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// A slider for adjusting float values in a range.
pub struct Slider<'a> {
    pub rect: Rect,
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
            rect: Rect::new(x, y, w, h),
            label,
            min,
            max,
            value: value.clamp(min, max),
            dragging: false,
        }
    }

    pub fn update(
        &mut self,
        mx: f32,
        my: f32,
        mouse_down: bool,
        was_dragging: bool,
    ) -> WidgetResult {
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

        WidgetResult {
            clicked: false,
            hovered: hovered || self.dragging,
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

    pub fn thumb_rect(&self) -> Rect {
        let t = (self.value - self.min) / (self.max - self.min);
        let thumb_w = 12.0;
        let x = self.rect.x + t * (self.rect.w - thumb_w);
        let y = self.rect.y - 4.0;
        Rect::new(x, y, thumb_w, self.rect.h + 8.0)
    }
}

/// A checkbox for boolean values.
pub struct Checkbox<'a> {
    pub rect: Rect,
    pub label: &'a str,
    pub checked: bool,
    pub state: WidgetState,
}

impl<'a> Checkbox<'a> {
    pub fn new(x: f32, y: f32, size: f32, label: &'a str, checked: bool) -> Self {
        Self {
            rect: Rect::new(x, y, size, size),
            label,
            checked,
            state: WidgetState::Normal,
        }
    }

    pub fn update(&mut self, mx: f32, my: f32, mouse_down: bool) -> WidgetResult {
        let hovered = self.rect.contains(mx, my);
        let was_pressed = self.state == WidgetState::Pressed;
        self.state = if hovered && mouse_down {
            WidgetState::Pressed
        } else if hovered {
            WidgetState::Hovered
        } else {
            WidgetState::Normal
        };
        WidgetResult {
            clicked: hovered && was_pressed && !mouse_down,
            hovered,
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
            WidgetState::Normal => [0.4, 0.4, 0.5, 1.0],
            WidgetState::Hovered => [0.6, 0.6, 0.8, 1.0],
            WidgetState::Pressed => [0.8, 0.8, 1.0, 1.0],
        }
    }
}

/// A text label.
pub struct Label<'a> {
    pub x: f32,
    pub y: f32,
    pub text: &'a str,
    pub color: [f32; 4],
    pub scale: f32,
}

impl<'a> Label<'a> {
    pub fn new(x: f32, y: f32, text: &'a str) -> Self {
        Self {
            x,
            y,
            text,
            color: [0.9, 0.9, 0.9, 1.0],
            scale: 1.0,
        }
    }
}

/// Wireframe checkbox that toggles renderer wireframe mode.
pub struct WireframeCheckbox {
    pub rect: Rect,
    pub label: &'static str,
    pub checked: bool,
    pub state: WidgetState,
}

impl WireframeCheckbox {
    pub fn new(x: f32, y: f32, size: f32, label: &'static str, checked: bool) -> Self {
        Self {
            rect: Rect::new(x, y, size, size),
            label,
            checked,
            state: WidgetState::Normal,
        }
    }

    pub fn update(&mut self, mx: f32, my: f32, mouse_down: bool) -> WidgetResult {
        let hovered = self.rect.contains(mx, my);
        let was_pressed = self.state == WidgetState::Pressed;
        self.state = if hovered && mouse_down {
            WidgetState::Pressed
        } else if hovered {
            WidgetState::Hovered
        } else {
            WidgetState::Normal
        };
        WidgetResult {
            clicked: hovered && was_pressed && !mouse_down,
            hovered,
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
            WidgetState::Normal => [0.5, 0.5, 0.6, 1.0],
            WidgetState::Hovered => [0.7, 0.7, 0.8, 1.0],
            WidgetState::Pressed => [0.9, 0.9, 1.0, 1.0],
        }
    }
}

/// Quality preset panel showing current preset and buttons to cycle.
pub struct QualityPanel {
    pub rect: Rect,
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
            rect: Rect::new(start_x, 10.0, 300.0, 60.0),
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

/// Render scale slider (50% - 200%).
pub struct RenderScaleSlider {
    pub rect: Rect,
    pub label: &'static str,
    pub slider: Slider<'static>,
    pub value_percent: u32,
}

impl RenderScaleSlider {
    pub fn new(_screen_width: f32, _screen_height: f32) -> Self {
        let slider = Slider::new(10.0, 110.0, 200.0, 20.0, "Render Scale", 50.0, 200.0, 100.0);
        Self {
            rect: Rect::new(10.0, 90.0, 240.0, 40.0),
            label: "Render Scale",
            slider,
            value_percent: 100,
        }
    }

    pub fn update(&mut self, mx: f32, my: f32, mouse_down: bool) -> bool {
        let was_dragging = self.slider.dragging;
        let res = self.slider.update(mx, my, mouse_down, was_dragging);
        let new_percent = self.slider.value as u32;
        if new_percent != self.value_percent || res.hovered {
            self.value_percent = new_percent;
            return true;
        }
        false
    }
}

/// Collects mouse state for UI interaction.
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseInput {
    pub x: f32,
    pub y: f32,
    pub left_down: bool,
    pub left_clicked: bool,
}

impl MouseInput {
    pub fn from_winit(x: f64, y: f64, button_state: ElementState, was_down: bool) -> Self {
        let left_down = button_state == ElementState::Pressed;
        let left_clicked = was_down && !left_down;
        Self {
            x: x as f32,
            y: y as f32,
            left_down,
            left_clicked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains() {
        let r = Rect::new(10.0, 20.0, 50.0, 30.0);
        assert!(r.contains(15.0, 25.0));
        assert!(!r.contains(5.0, 25.0));
        assert!(!r.contains(15.0, 10.0));
        assert!(!r.contains(70.0, 25.0));
        assert!(!r.contains(15.0, 60.0));
    }

    #[test]
    fn button_hover_and_click() {
        let mut btn = Button::new(0.0, 0.0, 100.0, 30.0, "Test");
        let res = btn.update(50.0, 15.0, false);
        assert!(res.hovered);
        assert!(!res.clicked);
        assert_eq!(btn.state, WidgetState::Hovered);

        let res = btn.update(50.0, 15.0, true);
        assert!(res.hovered);
        assert!(!res.clicked);
        assert_eq!(btn.state, WidgetState::Pressed);

        let res = btn.update(50.0, 15.0, false);
        assert!(res.hovered);
        assert!(res.clicked);
        assert_eq!(btn.state, WidgetState::Hovered);
    }

    #[test]
    fn slider_clamp_and_drag() {
        let mut s = Slider::new(0.0, 0.0, 100.0, 20.0, "Vol", 0.0, 1.0, 0.5);
        assert!((s.value - 0.5).abs() < 0.001);
        s.dragging = true;
        s.update(100.0, 5.0, true, true);
        assert!((s.value - 1.0).abs() < 0.01);
        s.update(0.0, 5.0, true, true);
        assert!((s.value - 0.0).abs() < 0.01);
    }

    #[test]
    fn checkbox_toggle() {
        let mut cb = Checkbox::new(0.0, 0.0, 16.0, "Opt", false);
        assert!(!cb.checked);
        cb.toggle();
        assert!(cb.checked);
    }
}
