use super::composite::DISPLAY_FORMAT;
use super::view::{Frame, ViewFrames};
use crate::brush::{Brush, BrushPreset, VERTICES_PER_BRISTLE, blade_grid, carries_paint};
use glam::Vec4;
use crate::gpu::Gpu;

const SILICONE_FLAG: u32 = 1 << 30;
const WORKING_FACE_FLAG: u32 = 1 << 29;

pub struct BrushLines {
    lines: wgpu::RenderPipeline,
    solids: wgpu::RenderPipeline,
    canvas: wgpu::BindGroup,
    stage: wgpu::BindGroup,
    line_vertices: u32,
    solid_vertices: u32,
}

impl BrushLines {
    pub fn new(gpu: &Gpu, brush: &Brush, frames: &ViewFrames) -> Self {
        let preset = brush.preset();
        let shader = gpu.shader("brush lines", &[include_str!("../../shaders/view.wgsl"), include_str!("../../shaders/brush_lines.wgsl")]);
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("brush lines"),
            entries: &[
                buffer_entry(0, wgpu::BufferBindingType::Uniform),
                buffer_entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_entry(2, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_entry(3, wgpu::BufferBindingType::Uniform),
            ],
        });
        let silicone = if preset.deposit_rate > 0.0 { 0 } else { SILICONE_FLAG };
        let solid: Vec<u32> = if preset.is_rigid() { blade_sheet(preset).into_iter().map(|id| id | silicone | working_face(preset, brush.rest(), id)).collect() } else { Vec::new() };
        let solid_buffer = upload(gpu, "blade sheet", bytemuck::cast_slice(&solid));
        let bind_group = |frame: Frame| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("brush lines"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: brush.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: brush.positions.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: solid_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: frames.buffer(frame).as_entire_binding() },
                ],
            })
        };
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("brush lines"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, topology: wgpu::PrimitiveTopology| {
            gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(entry), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: DISPLAY_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let line_vertices = if preset.is_rigid() { 0 } else { preset.bristle_count * (VERTICES_PER_BRISTLE - 2) * 2 };
        Self {
            lines: pipeline("vs_hair", wgpu::PrimitiveTopology::LineList),
            solids: pipeline("vs_solid", wgpu::PrimitiveTopology::TriangleList),
            canvas: bind_group(Frame::Canvas),
            stage: bind_group(Frame::Stage),
            line_vertices,
            solid_vertices: solid.len() as u32,
        }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, frame: Frame) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("brush lines"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(
            0,
            match frame {
                Frame::Canvas => &self.canvas,
                Frame::Stage => &self.stage,
            },
            &[],
        );
        pass.set_pipeline(&self.solids);
        pass.draw(0..self.solid_vertices, 0..1);
        if self.line_vertices > 0 {
            pass.set_pipeline(&self.lines);
            pass.draw(0..self.line_vertices, 0..1);
        }
    }
}

fn upload(gpu: &Gpu, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    let buffer = gpu.storage_buffer(label, bytes.len().max(16) as u64);
    gpu.queue.write_buffer(&buffer, 0, bytes);
    buffer
}

fn working_face(preset: &BrushPreset, rest: &[Vec4], id: u32) -> u32 {
    let per_spine = VERTICES_PER_BRISTLE;
    let level = id % per_spine;
    let offset = rest[(id - level) as usize + 1].x;
    let along = (level - 1) as f32 / (per_spine - 2) as f32;
    let meets_paint = preset.handle_lean > 0.0 && carries_paint(offset, along, preset.length, preset.width * 0.5);
    if meets_paint { WORKING_FACE_FLAG } else { 0 }
}

fn blade_sheet(preset: &BrushPreset) -> Vec<u32> {
    let (rows, columns) = blade_grid(preset);
    let vertex = |row: u32, column: u32, level: u32| (row * columns + column) * VERTICES_PER_BRISTLE + level;
    let exists = |row: u32, column: u32| row * columns + column < preset.bristle_count;
    let mut sheet = Vec::new();
    let mut quad = |a: u32, b: u32, c: u32, d: u32| sheet.extend_from_slice(&[a, b, c, a, c, d]);
    for level in 1..VERTICES_PER_BRISTLE - 1 {
        for row in 0..rows {
            for column in 0..columns {
                if exists(row, column + 1) && column + 1 < columns {
                    quad(vertex(row, column, level), vertex(row, column + 1, level), vertex(row, column + 1, level + 1), vertex(row, column, level + 1));
                }
                if exists(row + 1, column) && row + 1 < rows {
                    quad(vertex(row, column, level), vertex(row + 1, column, level), vertex(row + 1, column, level + 1), vertex(row, column, level + 1));
                }
            }
        }
    }
    sheet
}

fn buffer_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}
