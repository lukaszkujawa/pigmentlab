use crate::canvas::{Canvas, PAINT_CHANNELS};
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};
use glam::{IVec2, UVec3, Vec2};

pub const WINDOW_SIZE: UVec3 = UVec3::new(384, 384, 16);
const RELOCATE_MARGIN: f32 = crate::units::cm(1.0);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WindowUniform {
    origin: [i32; 2],
    canvas_width: u32,
    canvas_height: u32,
    width: u32,
    height: u32,
    depth: u32,
    _padding: u32,
}

pub struct ActiveWindow {
    origin: IVec2,
    canvas_size: Vec2,
    pub uniform: wgpu::Buffer,
    pub volume: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    bake: wgpu::ComputePipeline,
    unbake: wgpu::ComputePipeline,
}

impl ActiveWindow {
    pub fn new(gpu: &Gpu, canvas: &Canvas) -> Self {
        let cells = u64::from(WINDOW_SIZE.x) * u64::from(WINDOW_SIZE.y) * u64::from(WINDOW_SIZE.z);
        let volume = gpu.storage_buffer("active window volume", cells * PAINT_CHANNELS * 4);
        let uniform = gpu.uniform_buffer("active window uniform", std::mem::size_of::<WindowUniform>() as u64);
        let compute = wgpu::ShaderStages::COMPUTE;
        let read_write = wgpu::BufferBindingType::Storage { read_only: false };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("active window transfer"),
            entries: &[
                buffer_layout_entry(0, compute, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(1, compute, read_write),
                buffer_layout_entry(2, compute, read_write),
            ],
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("active window transfer"),
            layout: &layout,
            entries: &[buffer_binding(0, &uniform), buffer_binding(1, &volume), buffer_binding(2, &canvas.paint)],
        });
        let shader = gpu.shader(
            "active window transfer",
            &[include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/window_transfer.wgsl")],
        );
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("active window transfer"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let canvas_size = Vec2::new(canvas.size.width as f32, canvas.size.height as f32);
        let window = Self {
            origin: IVec2::MAX,
            canvas_size,
            uniform,
            volume,
            bind_group,
            bake: pipeline("bake"),
            unbake: pipeline("unbake"),
        };
        window.write_uniform(gpu, canvas);
        window
    }

    fn write_uniform(&self, gpu: &Gpu, canvas: &Canvas) {
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&WindowUniform {
                origin: self.origin.to_array(),
                canvas_width: canvas.size.width,
                canvas_height: canvas.size.height,
                width: WINDOW_SIZE.x,
                height: WINDOW_SIZE.y,
                depth: WINDOW_SIZE.z,
                _padding: 0,
            }),
        );
    }

    pub fn origin(&self) -> IVec2 {
        self.origin
    }

    pub fn needs_relocation(&self, focus: Vec2) -> bool {
        let half = Vec2::new(WINDOW_SIZE.x as f32, WINDOW_SIZE.y as f32) * 0.5;
        let drift = (focus - (self.origin.as_vec2() + half)).abs();
        self.origin == IVec2::MAX || drift.x >= half.x - RELOCATE_MARGIN || drift.y >= half.y - RELOCATE_MARGIN
    }

    pub fn follow(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, canvas: &Canvas, focus: Vec2) -> bool {
        let half = Vec2::new(WINDOW_SIZE.x as f32, WINDOW_SIZE.y as f32) * 0.5;
        if !self.needs_relocation(focus) {
            return false;
        }
        let desired = (focus - half).clamp(-half, self.canvas_size - half).round().as_ivec2();
        if self.origin != IVec2::MAX {
            let mut bake_encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("bake before relocation") });
            self.dispatch(&mut bake_encoder, &self.bake);
            gpu.queue.submit(Some(bake_encoder.finish()));
        }
        self.origin = desired;
        self.write_uniform(gpu, canvas);
        self.dispatch(encoder, &self.unbake);
        true
    }

    pub fn encode_bake(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.origin != IVec2::MAX {
            self.dispatch(encoder, &self.bake);
        }
    }

    pub fn encode_unbake(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.origin != IVec2::MAX {
            self.dispatch(encoder, &self.unbake);
        }
    }

    fn dispatch(&self, encoder: &mut wgpu::CommandEncoder, pipeline: &wgpu::ComputePipeline) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("active window transfer"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.dispatch_workgroups(WINDOW_SIZE.x.div_ceil(16), WINDOW_SIZE.y.div_ceil(16), 1);
    }
}
