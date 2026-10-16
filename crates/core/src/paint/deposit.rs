use super::window::{ActiveWindow, WINDOW_SIZE};
use crate::canvas::{Canvas, PAINT_CHANNELS};
use crate::brush::{Brush, SAMPLES_PER_BRISTLE};
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};

const WORKGROUP_SIZE: u32 = 128;
const CONTACT_HEIGHT: f32 = crate::units::cm(0.08);
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
    brush_travel: f32,
    rigidity: f32,
    _padding: [f32; 3],
}

pub struct Deposit {
    uniform: wgpu::Buffer,
    base: DepositUniform,
    medium: f32,
    film: wgpu::Buffer,
    pickup_demand: wgpu::Buffer,
    displacement: wgpu::Buffer,
    coarse_marks: wgpu::Buffer,
    previous_curves: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
    replenish: wgpu::ComputePipeline,
    apply_film: wgpu::ComputePipeline,
    grant_pickups: wgpu::ComputePipeline,
    apply_pickups: wgpu::ComputePipeline,
    grant_displacement: wgpu::ComputePipeline,
    snapshot_columns: wgpu::ComputePipeline,
    sample_count: u32,
    bristle_count: u32,
}

impl Deposit {
    pub fn new(gpu: &Gpu, brush: &Brush, window: &ActiveWindow, canvas: &Canvas, stroke_base: &wgpu::Buffer, dt: f32) -> Self {
        let preset = brush.preset();
        let cadence = dt / crate::engine::SIMULATION_DT;
        let sample_count = preset.bristle_count * SAMPLES_PER_BRISTLE;
        let uniform = gpu.uniform_buffer("deposit uniform", std::mem::size_of::<DepositUniform>() as u64);
        let base = DepositUniform {
            sample_count,
            samples_per_bristle: SAMPLES_PER_BRISTLE,
            deposit_rate: preset.deposit_rate * cadence,
            pickup_rate: preset.pickup_rate * cadence,
            sample_capacity: preset.sample_capacity,
            contact_height: CONTACT_HEIGHT,
            dt,
            brush_travel: 0.0,
            rigidity: if preset.is_rigid() { 1.0 } else { 0.0 },
            _padding: [0.0; 3],
        };
        gpu.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&base));
        let film = gpu.storage_buffer("brush film", u64::from(WINDOW_SIZE.x * WINDOW_SIZE.y) * PAINT_CHANNELS * 4);
        let columns = u64::from(WINDOW_SIZE.x * WINDOW_SIZE.y);
        let pickup_demand = gpu.storage_buffer("pickup demand", columns * 4);
        let pickup_grant = gpu.storage_buffer("pickup grant", columns * 16);
        let pickup_grant_scattering = gpu.storage_buffer("pickup grant scattering", columns * 16);
        let sample_request = gpu.storage_buffer("sample pickup request", u64::from(sample_count) * 8);
        let displacement = gpu.storage_buffer("displacement marks", columns * 20);
        let previous_curves = gpu.storage_buffer("deposit previous curves", brush.curves.size());
        let coarse_marks = gpu.storage_buffer("coarse displacement marks", u64::from(WINDOW_SIZE.x.div_ceil(16) * WINDOW_SIZE.y.div_ceil(16)) * 4);
        let column_state = gpu.storage_buffer("column snapshot", columns * 16);
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
                buffer_layout_entry(15, compute, read_write),
                buffer_layout_entry(16, compute, read_write),
                buffer_layout_entry(17, compute, read_only),
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
                buffer_binding(6, &previous_curves),
                buffer_binding(7, &window.uniform),
                buffer_binding(8, stroke_base),
                buffer_binding(9, &film),
                buffer_binding(10, &pickup_demand),
                buffer_binding(11, &pickup_grant),
                buffer_binding(12, &pickup_grant_scattering),
                buffer_binding(13, &sample_request),
                buffer_binding(14, &displacement),
                buffer_binding(15, &column_state),
                buffer_binding(16, &coarse_marks),
                buffer_binding(17, &canvas.ground),
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
            displacement,
            coarse_marks,
            previous_curves,
            bind_group,
            pipeline: compute_pipeline("step"),
            replenish: compute_pipeline("replenish"),
            apply_film: compute_pipeline("apply_film"),
            grant_pickups: compute_pipeline("grant_pickups"),
            apply_pickups: compute_pipeline("apply_pickups"),
            grant_displacement: compute_pipeline("grant_displacement"),
            snapshot_columns: compute_pipeline("snapshot_columns"),
            sample_count,
            bristle_count: preset.bristle_count,
        }
    }

    pub fn set_medium(&mut self, medium: f32) {
        self.medium = medium.clamp(0.0, 1.0);
    }

    pub fn encode(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, brush: &Brush, brush_travel: f32) {
        let flow = 1.0 + FLOW_PER_MEDIUM * self.medium;
        let uniform = DepositUniform { deposit_rate: self.base.deposit_rate * flow, brush_travel, ..self.base };
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        encoder.clear_buffer(&self.film, 0, None);
        encoder.clear_buffer(&self.pickup_demand, 0, None);
        encoder.clear_buffer(&self.displacement, 0, None);
        encoder.clear_buffer(&self.coarse_marks, 0, None);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("deposit"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.snapshot_columns);
        pass.dispatch_workgroups(WINDOW_SIZE.x.div_ceil(16), WINDOW_SIZE.y.div_ceil(16), 1);
        pass.set_pipeline(&self.pipeline);
        pass.dispatch_workgroups(self.sample_count.div_ceil(WORKGROUP_SIZE), 1, 1);
        pass.set_pipeline(&self.grant_pickups);
        pass.dispatch_workgroups(WINDOW_SIZE.x.div_ceil(16), WINDOW_SIZE.y.div_ceil(16), 1);
        pass.set_pipeline(&self.apply_pickups);
        pass.dispatch_workgroups(self.sample_count.div_ceil(WORKGROUP_SIZE), 1, 1);
        pass.set_pipeline(&self.grant_displacement);
        pass.dispatch_workgroups(WINDOW_SIZE.x.div_ceil(16), WINDOW_SIZE.y.div_ceil(16), 1);
        pass.set_pipeline(&self.apply_film);
        pass.dispatch_workgroups(WINDOW_SIZE.x.div_ceil(16), WINDOW_SIZE.y.div_ceil(16), 1);
        pass.set_pipeline(&self.replenish);
        pass.dispatch_workgroups(self.bristle_count.div_ceil(64), 1, 1);
        drop(pass);
        encoder.copy_buffer_to_buffer(&brush.curves, 0, &self.previous_curves, 0, brush.curves.size());
    }
}
