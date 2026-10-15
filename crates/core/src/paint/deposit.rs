use super::fluid::Fluid;
use super::window::{ActiveWindow, WINDOW_SIZE};
use crate::canvas::PAINT_CHANNELS;
use crate::brush::{Brush, SAMPLES_PER_BRISTLE};
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};

const WORKGROUP_SIZE: u32 = 128;
const CONTACT_HEIGHT: f32 = crate::units::cm(0.08);
const FLOW_PER_MEDIUM: f32 = 1.0;
const LIFT_BURST: f32 = 10.0;

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
    lift_burst: f32,
}

pub struct Deposit {
    uniform: wgpu::Buffer,
    base: DepositUniform,
    medium: f32,
    film: wgpu::Buffer,
    pickup_demand: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
    replenish: wgpu::ComputePipeline,
    apply_film: wgpu::ComputePipeline,
    snapshot: wgpu::ComputePipeline,
    grant_pickups: wgpu::ComputePipeline,
    apply_pickups: wgpu::ComputePipeline,
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
            lift_burst: 0.0,
        };
        gpu.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&base));
        let film = gpu.storage_buffer("brush film", u64::from(WINDOW_SIZE.x * WINDOW_SIZE.y) * PAINT_CHANNELS * 4);
        let columns = u64::from(WINDOW_SIZE.x * WINDOW_SIZE.y);
        let column_heights = gpu.storage_buffer("deposit column heights", columns * 4);
        let pickup_demand = gpu.storage_buffer("pickup demand", columns * 4);
        let pickup_grant = gpu.storage_buffer("pickup grant", columns * 16);
        let pickup_grant_scattering = gpu.storage_buffer("pickup grant scattering", columns * 16);
        let sample_request = gpu.storage_buffer("sample pickup request", u64::from(sample_count) * 8);
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
                buffer_layout_entry(9, compute, read_write),
                buffer_layout_entry(10, compute, read_write),
                buffer_layout_entry(11, compute, read_write),
                buffer_layout_entry(12, compute, read_write),
                buffer_layout_entry(13, compute, read_write),
                buffer_layout_entry(14, compute, read_write),
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
                buffer_binding(9, &film),
                buffer_binding(10, &column_heights),
                buffer_binding(11, &pickup_demand),
                buffer_binding(12, &pickup_grant),
                buffer_binding(13, &pickup_grant_scattering),
                buffer_binding(14, &sample_request),
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
            medium: 0.0,
            film,
            pickup_demand,
            bind_group,
            pipeline: compute_pipeline("step"),
            replenish: compute_pipeline("replenish"),
            apply_film: compute_pipeline("apply_film"),
            snapshot: compute_pipeline("snapshot_columns"),
            grant_pickups: compute_pipeline("grant_pickups"),
            apply_pickups: compute_pipeline("apply_pickups"),
            sample_count,
            bristle_count: preset.bristle_count,
        }
    }

    pub fn set_medium(&mut self, gpu: &Gpu, medium: f32) {
        self.medium = medium.clamp(0.0, 1.0);
        self.write_uniform(gpu, false);
    }

    pub fn set_lifting(&self, gpu: &Gpu, lifting: bool) {
        self.write_uniform(gpu, lifting);
    }

    fn write_uniform(&self, gpu: &Gpu, lifting: bool) {
        let flow = 1.0 + FLOW_PER_MEDIUM * self.medium;
        let uniform = DepositUniform {
            deposit_rate: self.base.deposit_rate * flow,
            lift_burst: if lifting { LIFT_BURST } else { 0.0 },
            ..self.base
        };
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.film, 0, None);
        encoder.clear_buffer(&self.pickup_demand, 0, None);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("deposit"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.snapshot);
        pass.dispatch_workgroups(WINDOW_SIZE.x.div_ceil(16), WINDOW_SIZE.y.div_ceil(16), 1);
        pass.set_pipeline(&self.pipeline);
        pass.dispatch_workgroups(self.sample_count.div_ceil(WORKGROUP_SIZE), 1, 1);
        pass.set_pipeline(&self.grant_pickups);
        pass.dispatch_workgroups(WINDOW_SIZE.x.div_ceil(16), WINDOW_SIZE.y.div_ceil(16), 1);
        pass.set_pipeline(&self.apply_pickups);
        pass.dispatch_workgroups(self.sample_count.div_ceil(WORKGROUP_SIZE), 1, 1);
        pass.set_pipeline(&self.apply_film);
        pass.dispatch_workgroups(WINDOW_SIZE.x.div_ceil(16), WINDOW_SIZE.y.div_ceil(16), 1);
        pass.set_pipeline(&self.replenish);
        pass.dispatch_workgroups(self.bristle_count.div_ceil(64), 1, 1);
    }
}
