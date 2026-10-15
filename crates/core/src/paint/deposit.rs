use super::fluid::Fluid;
use super::window::ActiveWindow;
use crate::brush::{Brush, SAMPLES_PER_BRISTLE};
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};

const WORKGROUP_SIZE: u32 = 128;
const CONTACT_HEIGHT: f32 = crate::units::cm(0.06);
const FLOW_PER_MEDIUM: f32 = 1.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DepositUniform {
    sample_count: u32,
    samples_per_bristle: u32,
    deposit_rate: f32,
    pickup_rate: f32,
    sample_capacity: f32,
    contact_height: f32,
    dt: f32,
    _padding: f32,
}

pub struct Deposit {
    uniform: wgpu::Buffer,
    base: DepositUniform,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
    replenish: wgpu::ComputePipeline,
    sample_count: u32,
    bristle_count: u32,
}

impl Deposit {
    pub fn new(gpu: &Gpu, brush: &Brush, window: &ActiveWindow, fluid: &Fluid) -> Self {
        let preset = brush.preset();
        let sample_count = preset.bristle_count * SAMPLES_PER_BRISTLE;
        let uniform = gpu.uniform_buffer("deposit uniform", std::mem::size_of::<DepositUniform>() as u64);
        let base = DepositUniform {
            sample_count,
            samples_per_bristle: SAMPLES_PER_BRISTLE,
            deposit_rate: preset.deposit_rate,
            pickup_rate: preset.pickup_rate,
            sample_capacity: preset.sample_capacity,
            contact_height: CONTACT_HEIGHT,
            dt: crate::engine::SIMULATION_DT,
            _padding: 0.0,
        };
        gpu.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&base));
        let compute = wgpu::ShaderStages::COMPUTE;
        let read_only = wgpu::BufferBindingType::Storage { read_only: true };
        let read_write = wgpu::BufferBindingType::Storage { read_only: false };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deposit"),
            entries: &[
                buffer_layout_entry(0, compute, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(1, compute, read_only),
                buffer_layout_entry(2, compute, read_write),
                buffer_layout_entry(3, compute, read_write),
                buffer_layout_entry(4, compute, read_write),
                buffer_layout_entry(5, compute, read_write),
                buffer_layout_entry(6, compute, read_only),
                buffer_layout_entry(7, compute, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(8, compute, read_only),
            ],
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deposit"),
            layout: &layout,
            entries: &[
                buffer_binding(0, &uniform),
                buffer_binding(1, &brush.curves),
                buffer_binding(2, &brush.sample_load),
                buffer_binding(3, &brush.sample_absorption),
                buffer_binding(4, &brush.sample_scattering),
                buffer_binding(5, &window.volume),
                buffer_binding(6, &brush.previous_curves),
                buffer_binding(7, &window.uniform),
                buffer_binding(8, &fluid.particle_volume),
            ],
        });
        let shader = gpu.shader(
            "deposit",
            &[
                include_str!("../../shaders/units.wgsl"),
                include_str!("../../shaders/hermite.wgsl"),
                include_str!("../../shaders/window.wgsl"),
                include_str!("../../shaders/deposit.wgsl"),
            ],
        );
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("deposit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let compute_pipeline = |entry: &str| {
            gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            uniform,
            base,
            bind_group,
            pipeline: compute_pipeline("step"),
            replenish: compute_pipeline("replenish"),
            sample_count,
            bristle_count: preset.bristle_count,
        }
    }

    pub fn set_medium(&self, gpu: &Gpu, medium: f32) {
        let flow = 1.0 + FLOW_PER_MEDIUM * medium.clamp(0.0, 1.0);
        let uniform = DepositUniform { deposit_rate: self.base.deposit_rate * flow, ..self.base };
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("deposit"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.dispatch_workgroups(self.sample_count.div_ceil(WORKGROUP_SIZE), 1, 1);
        pass.set_pipeline(&self.replenish);
        pass.dispatch_workgroups(self.bristle_count.div_ceil(64), 1, 1);
    }
}
