use super::composite::DISPLAY_FORMAT;
use crate::drip::Drips;
use crate::gpu::Gpu;
use crate::units::{METRES_PER_PIXEL, pixels};
use glam::{UVec2, Vec2, Vec4};
use std::f32::consts::TAU;

const ROUND_SEGMENTS: usize = 40;
const STRING_WIDTH: f32 = 3.0;
const OUTLINE_WIDTH: f32 = 4.0;
const PIVOT_RADIUS: f32 = 10.0;
const HOOK_RADIUS: f32 = 24.0;
const SMALLEST_DOT: f32 = 1.5;
const RIG: Vec4 = Vec4::new(0.12, 0.11, 0.1, 0.75);
const SELECTED: Vec4 = Vec4::new(0.98, 0.55, 0.1, 0.95);
const GLASS: Vec4 = Vec4::new(0.85, 0.88, 0.9, 0.12);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    colour: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub size: UVec2,
    pub origin: Vec2,
    pub scale: f32,
}

impl Viewport {
    pub fn canvas(size: UVec2) -> Self {
        Self { size, origin: Vec2::ZERO, scale: 1.0 }
    }
}

pub struct DripOverlay {
    straight: wgpu::RenderPipeline,
    premultiplied: wgpu::RenderPipeline,
    frame: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertices: Option<wgpu::Buffer>,
}

impl DripOverlay {
    pub fn new(gpu: &Gpu) -> Self {
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("drip overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/drip_overlay.wgsl").into()),
        });
        let frame = gpu.uniform_buffer("drip overlay frame", 32);
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("drip overlay"),
            entries: &[crate::gpu::buffer_layout_entry(0, wgpu::ShaderStages::VERTEX, wgpu::BufferBindingType::Uniform)],
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("drip overlay"),
            layout: &layout,
            entries: &[crate::gpu::buffer_binding(0, &frame)],
        });
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("drip overlay"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = |entry: &str, blend: wgpu::BlendState| {
            let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
            gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("drip overlay"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attributes })],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: DISPLAY_FORMAT, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            straight: pipeline("fs_main", wgpu::BlendState::ALPHA_BLENDING),
            premultiplied: pipeline("fs_premultiplied", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            frame,
            bind_group,
            vertices: None,
        }
    }

    pub fn encode(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, target: &Target, drips: Option<&Drips>, hook: Option<Vec2>) {
        let vertices = drips.map(|drips| shapes(drips, hook, 1.0 / target.view.scale)).unwrap_or_default();
        let load = match target.premultiplied {
            true => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            false => wgpu::LoadOp::Load,
        };
        let bytes: &[u8] = bytemuck::cast_slice(&vertices);
        if self.vertices.as_ref().is_none_or(|buffer| buffer.size() < bytes.len() as u64) {
            self.vertices = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("drip overlay vertices"),
                size: (bytes.len() as u64).max(4).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let buffer = self.vertices.as_ref().expect("vertex buffer was just made");
        gpu.queue.write_buffer(buffer, 0, bytes);
        let view = target.view;
        gpu.queue.write_buffer(&self.frame, 0, bytemuck::cast_slice(&[view.size.x as f32, view.size.y as f32, view.origin.x, view.origin.y, view.scale, 0.0, 0.0, 0.0]));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("drip overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.texture,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if vertices.is_empty() {
            return;
        }
        pass.set_pipeline(if target.premultiplied { &self.premultiplied } else { &self.straight });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..bytes.len() as u64));
        pass.draw(0..vertices.len() as u32, 0..1);
    }
}

pub struct Target<'a> {
    pub texture: &'a wgpu::TextureView,
    pub view: Viewport,
    pub premultiplied: bool,
}

fn shapes(drips: &Drips, hook: Option<Vec2>, pixel: f32) -> Vec<Vertex> {
    let mut shapes = Shapes::default();
    if let Some(hook) = hook {
        shapes.ring(hook, HOOK_RADIUS * pixel, OUTLINE_WIDTH * pixel, SELECTED);
        shapes.disc(hook, PIVOT_RADIUS * pixel * 0.5, SELECTED);
    }
    for bottle in drips.bottles() {
        let pendulum = &bottle.pendulum;
        let pivot = pendulum.pivot.truncate() / METRES_PER_PIXEL;
        let hole = pendulum.bob.truncate() / METRES_PER_PIXEL;
        let up = (pendulum.pivot - pendulum.bob).normalize();
        let top = (pendulum.bob + up * bottle.height()).truncate() / METRES_PER_PIXEL;
        let paint = bottle.paint.opaque_reflectance().extend(0.55);
        let radius = pixels(bottle.radius);
        shapes.capsule(hole, top, radius, GLASS);
        shapes.capsule(hole, hole + (top - hole) * bottle.fill(), radius * 0.92, paint);
        let label = Vec2::from_angle(bottle.turned) * radius;
        shapes.line(hole + label * 0.45, hole + label * 0.9, OUTLINE_WIDTH * pixel, RIG);
        match drips.selected() == Some(bottle.id) {
            true => shapes.capsule_outline(hole, top, radius, OUTLINE_WIDTH * pixel * 2.0, SELECTED),
            false => shapes.capsule_outline(hole, top, radius, OUTLINE_WIDTH * pixel, RIG),
        }
        let string = if pendulum.taut() { RIG } else { RIG * Vec4::new(1.0, 1.0, 1.0, 0.4) };
        shapes.line(top, pivot, STRING_WIDTH * pixel, string);
        shapes.disc(pivot, PIVOT_RADIUS * pixel, RIG);
        if let Some(end) = bottle.thread_end() {
            shapes.line(hole, end, SMALLEST_DOT * pixel * 2.0, paint.truncate().extend(0.9));
        }
    }
    for droplet in drips.droplets() {
        let centre = droplet.position.truncate() / METRES_PER_PIXEL;
        shapes.disc(centre, pixels(droplet.diameter * 0.5).max(SMALLEST_DOT * pixel), droplet.paint.opaque_reflectance().extend(0.9));
    }
    shapes.vertices
}

#[derive(Default)]
struct Shapes {
    vertices: Vec<Vertex>,
}

impl Shapes {
    fn triangle(&mut self, corners: [Vec2; 3], colour: Vec4) {
        self.vertices.extend(corners.map(|corner| Vertex { position: corner.to_array(), colour: colour.to_array() }));
    }

    fn quad(&mut self, corners: [Vec2; 4], colour: Vec4) {
        self.triangle([corners[0], corners[1], corners[2]], colour);
        self.triangle([corners[0], corners[2], corners[3]], colour);
    }

    fn line(&mut self, from: Vec2, to: Vec2, width: f32, colour: Vec4) {
        let side = (to - from).perp().normalize_or_zero() * width * 0.5;
        self.quad([from - side, to - side, to + side, from + side], colour);
    }

    fn disc(&mut self, centre: Vec2, radius: f32, colour: Vec4) {
        for k in 0..ROUND_SEGMENTS {
            let [a, b] = [k, k + 1].map(|i| centre + Vec2::from_angle(i as f32 / ROUND_SEGMENTS as f32 * TAU) * radius);
            self.triangle([centre, a, b], colour);
        }
    }

    fn ring(&mut self, centre: Vec2, radius: f32, width: f32, colour: Vec4) {
        for k in 0..ROUND_SEGMENTS {
            let [a, b] = [k, k + 1].map(|i| centre + Vec2::from_angle(i as f32 / ROUND_SEGMENTS as f32 * TAU) * radius);
            self.line(a, b, width, colour);
        }
    }

    fn capsule(&mut self, from: Vec2, to: Vec2, radius: f32, colour: Vec4) {
        let outline = capsule_outline(from, to, radius);
        let centre = (from + to) * 0.5;
        for (k, a) in outline.iter().enumerate() {
            self.triangle([centre, *a, outline[(k + 1) % outline.len()]], colour);
        }
    }

    fn capsule_outline(&mut self, from: Vec2, to: Vec2, radius: f32, width: f32, colour: Vec4) {
        let outline = capsule_outline(from, to, radius);
        for (k, a) in outline.iter().enumerate() {
            self.line(*a, outline[(k + 1) % outline.len()], width, colour);
        }
    }
}

fn capsule_outline(from: Vec2, to: Vec2, radius: f32) -> Vec<Vec2> {
    let axis = (to - from).normalize_or(Vec2::Y);
    let start = axis.perp();
    let half = ROUND_SEGMENTS / 2;
    let arc = |centre: Vec2, facing: Vec2| (0..=half).map(move |i| centre + Vec2::from_angle(i as f32 / half as f32 * TAU * 0.5).rotate(facing) * radius);
    arc(to, -start).chain(arc(from, start)).collect()
}
