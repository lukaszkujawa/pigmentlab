use crate::canvas::Canvas;
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};
use super::height_map::HeightMap;
use crate::paint::{ActiveWindow, Fluid};
use crate::pigment::OPTICAL_THICKNESS_PER_PIXEL;

pub const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeUniform {
    canvas_width: u32,
    canvas_height: u32,
    optical_thickness_per_pixel: f32,
    light_azimuth: f32,
    light_elevation: f32,
    gloss: f32,
    ambient: f32,
    shadow_strength: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    pub azimuth_degrees: f32,
    pub elevation_degrees: f32,
    pub gloss: f32,
    pub ambient: f32,
    pub shadow_strength: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Self { azimuth_degrees: 225.0, elevation_degrees: 40.0, gloss: 0.5, ambient: 0.45, shadow_strength: 0.6 }
    }
}

pub struct Composite {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    canvas: CanvasExtent,
}

#[derive(Clone, Copy)]
struct CanvasExtent {
    width: u32,
    height: u32,
}

impl Composite {
    pub fn new(gpu: &Gpu, canvas: &Canvas, window: &ActiveWindow, fluid: &Fluid, height_map: &HeightMap) -> Self {
        let shader = gpu.shader(
            "composite",
            &[include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/composite.wgsl")],
        );
        let uniform = gpu.uniform_buffer("composite uniform", std::mem::size_of::<CompositeUniform>() as u64);
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: fragment,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                buffer_layout_entry(1, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_layout_entry(2, fragment, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(3, fragment, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(4, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_layout_entry(5, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_layout_entry(6, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
            ],
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &canvas.substrate.create_view(&wgpu::TextureViewDescriptor::default()),
                    ),
                },
                buffer_binding(1, &canvas.paint),
                buffer_binding(2, &uniform),
                buffer_binding(3, &window.uniform),
                buffer_binding(4, &window.volume),
                buffer_binding(5, &fluid.particle_volume),
                buffer_binding(6, &height_map.heights),
            ],
        });
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
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
        let composite = Self {
            pipeline,
            bind_group,
            uniform,
            canvas: CanvasExtent { width: canvas.size.width, height: canvas.size.height },
        };
        composite.set_lighting(gpu, Lighting::default());
        composite
    }

    pub fn set_lighting(&self, gpu: &Gpu, lighting: Lighting) {
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&CompositeUniform {
                canvas_width: self.canvas.width,
                canvas_height: self.canvas.height,
                optical_thickness_per_pixel: OPTICAL_THICKNESS_PER_PIXEL,
                light_azimuth: lighting.azimuth_degrees,
                light_elevation: lighting.elevation_degrees,
                gloss: lighting.gloss,
                ambient: lighting.ambient,
                shadow_strength: lighting.shadow_strength,
            }),
        );
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
