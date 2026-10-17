use super::composite::DISPLAY_FORMAT;
use crate::brush::{Brush, BrushPreset, VERTICES_PER_BRISTLE, blade_grid};
use crate::gpu::Gpu;

pub struct BrushLines {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    vertex_count: u32,
}

impl BrushLines {
    pub fn new(gpu: &Gpu, brush: &Brush) -> Self {
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("brush lines"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/brush_lines.wgsl").into()),
        });
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("brush lines"),
            entries: &[
                buffer_entry(0, wgpu::BufferBindingType::Uniform),
                buffer_entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_entry(2, wgpu::BufferBindingType::Storage { read_only: true }),
            ],
        });
        let rigid = brush.preset().is_rigid();
        let sheet: Vec<u32> = if rigid { blade_sheet(brush.preset()) } else { vec![0] };
        let sheet_buffer = gpu.storage_buffer("blade sheet", (sheet.len() * 4) as u64);
        gpu.queue.write_buffer(&sheet_buffer, 0, bytemuck::cast_slice(&sheet));
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("brush lines"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: brush.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: brush.positions.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: sheet_buffer.as_entire_binding() },
            ],
        });
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("brush lines"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("brush lines"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(if rigid { "vs_blade" } else { "vs_main" }),
                buffers: &[],
                compilation_options: Default::default(),
            },
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
            primitive: wgpu::PrimitiveState {
                topology: if rigid { wgpu::PrimitiveTopology::TriangleList } else { wgpu::PrimitiveTopology::LineList },
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let vertex_count = if rigid { sheet.len() as u32 } else { brush.preset().bristle_count * (VERTICES_PER_BRISTLE - 2) * 2 };
        Self { pipeline, bind_group, vertex_count }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..self.vertex_count, 0..1);
    }
}

fn buffer_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
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
