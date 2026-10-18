use super::composite::DISPLAY_FORMAT;
use super::view::{Frame, ViewFrames};
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};

pub struct PresentInputs<'a> {
    pub albedo: &'a wgpu::TextureView,
    pub finish: &'a wgpu::TextureView,
    pub surface: &'a wgpu::Buffer,
    pub lighting: &'a wgpu::Buffer,
}

pub struct Present {
    pipeline: wgpu::RenderPipeline,
    canvas: wgpu::BindGroup,
    stage: wgpu::BindGroup,
}

impl Present {
    pub fn new(gpu: &Gpu, inputs: PresentInputs, frames: &ViewFrames) -> Self {
        let shader = gpu.shader("present", &[include_str!("../../shaders/view.wgsl"), include_str!("../../shaders/lighting.wgsl"), include_str!("../../shaders/present.wgsl")]);
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                buffer_layout_entry(2, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_layout_entry(3, fragment, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(4, fragment, wgpu::BufferBindingType::Uniform),
            ],
        });
        let bind_group = |frame: Frame| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("present"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(inputs.albedo) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(inputs.finish) },
                    buffer_binding(2, inputs.surface),
                    buffer_binding(3, inputs.lighting),
                    buffer_binding(4, frames.buffer(frame)),
                ],
            })
        };
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("present"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(DISPLAY_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, canvas: bind_group(Frame::Canvas), stage: bind_group(Frame::Stage) }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, frame: Frame) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(
            0,
            match frame {
                Frame::Canvas => &self.canvas,
                Frame::Stage => &self.stage,
            },
            &[],
        );
        pass.draw(0..3, 0..1);
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    }
}
