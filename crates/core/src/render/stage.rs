use super::composite::DISPLAY_FORMAT;
use super::drip_overlay::Viewport;
use crate::gpu::Gpu;
use glam::UVec2;

pub struct Stage {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    targets: Option<Targets>,
}

struct Targets {
    size: UVec2,
    layer: wgpu::TextureView,
    output: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

impl Stage {
    pub fn new(gpu: &Gpu) -> Self {
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("unpremultiply"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/unpremultiply.wgsl").into()),
        });
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("unpremultiply"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            }],
        });
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("unpremultiply"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("unpremultiply"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: DISPLAY_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, layout, targets: None }
    }

    pub fn layer(&mut self, gpu: &Gpu, view: Viewport) -> &wgpu::TextureView {
        if self.targets.as_ref().is_none_or(|targets| targets.size != view.size) {
            self.targets = Some(self.make_targets(gpu, view.size));
        }
        &self.targets.as_ref().expect("targets were just made").layer
    }

    pub fn encode_output(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(targets) = &self.targets else { return };
        let output = targets.output.create_view(&wgpu::TextureViewDescriptor::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("unpremultiply"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output,
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
        pass.set_bind_group(0, &targets.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn output(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|targets| &targets.output)
    }

    fn make_targets(&self, gpu: &Gpu, size: UVec2) -> Targets {
        let texture = |label: &str, usage: wgpu::TextureUsages| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size.x.max(1), height: size.y.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DISPLAY_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | usage,
                view_formats: &[],
            })
        };
        let layer = texture("stage layer", wgpu::TextureUsages::empty()).create_view(&wgpu::TextureViewDescriptor::default());
        let output = texture("stage", wgpu::TextureUsages::COPY_SRC);
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("unpremultiply"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&layer) }],
        });
        Targets { size, layer, output, bind_group }
    }
}
