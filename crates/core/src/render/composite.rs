use crate::canvas::Canvas;
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};
use crate::watercolour::Watercolour;
use super::height_map::HeightMap;
use super::present::{Present, PresentInputs};
use super::view::{Frame, ViewFrames};
use crate::paint::ActiveWindow;

pub const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const PRESENTED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

pub fn create_presented_texture(gpu: &Gpu, label: &str, size: wgpu::Extent3d, usage: wgpu::TextureUsages) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PRESENTED_FORMAT,
        usage,
        view_formats: &[DISPLAY_FORMAT],
    })
}

pub fn display_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor { format: Some(DISPLAY_FORMAT), ..Default::default() })
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeUniform {
    canvas_width: u32,
    canvas_height: u32,
    light_azimuth: f32,
    light_elevation: f32,
    gloss: f32,
    ambient: f32,
    shadow_strength: f32,
    padding: f32,
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
    watercolour: Option<(wgpu::RenderPipeline, wgpu::BindGroup)>,
    shader: wgpu::ShaderModule,
    uniform: wgpu::Buffer,
    canvas: CanvasExtent,
    albedo: wgpu::TextureView,
    finish: wgpu::TextureView,
    present: Present,
}

#[derive(Clone, Copy)]
struct CanvasExtent {
    width: u32,
    height: u32,
}

impl Composite {
    pub fn new(gpu: &Gpu, canvas: &Canvas, window: &ActiveWindow, height_map: &HeightMap, frames: &ViewFrames) -> Self {
        let shader = gpu.shader(
            "composite",
            &[
                include_str!("../../shaders/window.wgsl"),
                include_str!("../../shaders/km.wgsl"),
                include_str!("../../shaders/lighting.wgsl"),
                include_str!("../../shaders/composite.wgsl"),
                include_str!("../../shaders/wash_outline.wgsl"),
                include_str!("../../shaders/wash_bridge.wgsl"),
                include_str!("../../shaders/wash_pigment.wgsl"),
                include_str!("../../shaders/composite_watercolour.wgsl"),
            ],
        );
        let uniform = gpu.uniform_buffer("composite uniform", std::mem::size_of::<CompositeUniform>() as u64);
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[
                buffer_layout_entry(1, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_layout_entry(2, fragment, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(3, fragment, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(4, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_layout_entry(5, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_layout_entry(6, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_layout_entry(7, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_layout_entry(8, fragment, wgpu::BufferBindingType::Storage { read_only: true }),
            ],
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &layout,
            entries: &[
                buffer_binding(1, &canvas.paint),
                buffer_binding(2, &uniform),
                buffer_binding(3, &window.uniform),
                buffer_binding(4, &window.volume),
                buffer_binding(5, &height_map.heights),
                buffer_binding(6, &height_map.smooth_surface),
                buffer_binding(7, &height_map.smooth_paint),
                buffer_binding(8, &canvas.dry),
            ],
        });
        let pipeline = render_pipeline(gpu, &shader, &layout, "fs_main");
        let albedo = surface_map(gpu, "albedo map", canvas);
        let finish = surface_map(gpu, "finish map", canvas);
        let present = Present::new(gpu, PresentInputs { albedo: &albedo, finish: &finish, surface: &height_map.surface, lighting: &uniform }, frames);
        let composite = Self {
            pipeline,
            bind_group,
            watercolour: None,
            shader,
            uniform,
            canvas: CanvasExtent { width: canvas.size.width, height: canvas.size.height },
            albedo,
            finish,
            present,
        };
        composite.set_lighting(gpu, Lighting::default());
        composite
    }

    pub fn attach_watercolour(&mut self, gpu: &Gpu, canvas: &Canvas, height_map: &HeightMap, water: Option<&Watercolour>) {
        self.watercolour = water.map(|water| {
            let fragment = wgpu::ShaderStages::FRAGMENT;
            let read_only = wgpu::BufferBindingType::Storage { read_only: true };
            let kinds = [
                (2, wgpu::BufferBindingType::Uniform),
                (6, read_only),
                (8, read_only),
                (9, wgpu::BufferBindingType::Uniform),
                (10, read_only),
                (11, read_only),
                (12, read_only),
                (13, read_only),
                (14, read_only),
                (15, read_only),
                (16, read_only),
            ];
            let entries: Vec<_> = kinds.iter().map(|(binding, kind)| buffer_layout_entry(*binding, fragment, *kind)).collect();
            let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("watercolour composite"), entries: &entries });
            let buffers = [
                (2, &self.uniform),
                (6, &height_map.smooth_surface),
                (8, &canvas.dry),
                (9, &water.uniform),
                (10, &water.water),
                (11, &water.suspended),
                (12, &water.deposited),
                (13, &water.paper),
                (14, &canvas.relief),
                (15, &water.touched),
                (16, &water.concentration),
            ];
            let bindings: Vec<_> = buffers.iter().map(|(binding, buffer)| buffer_binding(*binding, buffer)).collect();
            let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("watercolour composite"), layout: &layout, entries: &bindings });
            (render_pipeline(gpu, &self.shader, &layout, "fs_watercolour"), bind_group)
        });
    }

    pub fn set_lighting(&self, gpu: &Gpu, lighting: Lighting) {
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&CompositeUniform {
                canvas_width: self.canvas.width,
                canvas_height: self.canvas.height,
                light_azimuth: lighting.azimuth_degrees,
                light_elevation: lighting.elevation_degrees,
                gloss: lighting.gloss,
                ambient: lighting.ambient,
                shadow_strength: lighting.shadow_strength,
                padding: 0.0,
            }),
        );
    }

    pub fn encode_surface(&self, encoder: &mut wgpu::CommandEncoder) {
        let target = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite"),
            color_attachments: &[target(&self.albedo), target(&self.finish)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let (pipeline, bind_group) = self.watercolour.as_ref().map_or((&self.pipeline, &self.bind_group), |(pipeline, group)| (pipeline, group));
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, frame: Frame) {
        self.present.encode(encoder, target, frame);
    }
}

fn surface_map(gpu: &Gpu, label: &str, canvas: &Canvas) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: canvas.size.extent(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SURFACE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn render_pipeline(gpu: &Gpu, shader: &wgpu::ShaderModule, layout: &wgpu::BindGroupLayout, fragment: &str) -> wgpu::RenderPipeline {
    let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(fragment),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(SURFACE_FORMAT.into()), Some(SURFACE_FORMAT.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
