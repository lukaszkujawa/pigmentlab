use super::{ACCUMULATORS, Watercolour};
use crate::brush::{Brush, SAMPLES_PER_BRISTLE};
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BrushWaterUniform {
    sample_count: u32,
    samples_per_bristle: u32,
    sample_capacity: f32,
    dt: f32,
    cells: [u32; 2],
    canvas: [u32; 2],
}

pub struct WaterBrush {
    previous_chain: wgpu::Buffer,
    exchange: ComputePass,
    grant_lift: ComputePass,
    apply_lift: ComputePass,
    wick: ComputePass,
    sample_count: u32,
    bristle_count: u32,
    cells: [u32; 2],
}

impl WaterBrush {
    pub fn new(gpu: &Gpu, brush: &Brush, water: &Watercolour, canvas: &Canvas, dt: f32) -> Self {
        let preset = brush.preset();
        let sample_count = preset.bristle_count * SAMPLES_PER_BRISTLE;
        let cells = u64::from(water.cells.x * water.cells.y);
        let uniform = gpu.uniform_buffer("water brush uniform", std::mem::size_of::<BrushWaterUniform>() as u64);
        gpu.queue.write_buffer(
            &uniform,
            0,
            bytemuck::bytes_of(&BrushWaterUniform {
                sample_count,
                samples_per_bristle: SAMPLES_PER_BRISTLE,
                sample_capacity: preset.sample_capacity,
                dt,
                cells: water.cells.to_array(),
                canvas: [canvas.size.width, canvas.size.height],
            }),
        );
        let previous_chain = gpu.storage_buffer("water brush previous chain", brush.chain.size());
        let lift_demand = gpu.storage_buffer("water brush lift demand", cells * 4);
        let lift = gpu.storage_buffer("water brush lift", cells * 48);
        let sample_request = gpu.storage_buffer("water brush sample request", u64::from(sample_count) * 8);
        let start_concentration = gpu.storage_buffer("water brush start concentration", u64::from(sample_count) * 4);
        let shader = gpu.shader(
            "watercolour brush",
            &[
                include_str!("../../shaders/hermite.wgsl"),
                include_str!("../../shaders/watercolour_brush.wgsl"),
                include_str!("../../shaders/chain.wgsl"),
            ],
        );
        let bindings = [
            (0, Binding::Uniform(&uniform)),
            (1, Binding::ReadOnly(&brush.chain)),
            (2, Binding::ReadOnly(&previous_chain)),
            (3, Binding::ReadWrite(&brush.sample_load)),
            (4, Binding::ReadWrite(&brush.sample_absorption)),
            (5, Binding::ReadWrite(&brush.sample_scattering)),
            (6, Binding::ReadWrite(&brush.sample_behaviour)),
            (7, Binding::ReadWrite(&water.accumulators)),
            (8, Binding::ReadOnly(&water.water)),
            (9, Binding::ReadOnly(&water.suspended)),
            (10, Binding::ReadWrite(&lift_demand)),
            (11, Binding::ReadWrite(&lift)),
            (12, Binding::ReadWrite(&sample_request)),
            (13, Binding::ReadOnly(&canvas.ground)),
            (14, Binding::ReadWrite(&start_concentration)),
            (15, Binding::ReadOnly(&water.deposited)),
        ];
        debug_assert_eq!(ACCUMULATORS, 13);
        let pass = |label, entry| ComputePass::new(gpu, label, &shader, entry, &bindings);
        let [exchange, grant_lift, apply_lift, wick] = [
            pass("water brush exchange", "exchange"),
            pass("water brush grant lift", "grant_lift"),
            pass("water brush apply lift", "apply_lift"),
            pass("water brush wick", "wick"),
        ];
        Self {
            previous_chain,
            exchange,
            grant_lift,
            apply_lift,
            wick,
            sample_count,
            bristle_count: preset.bristle_count,
            cells: water.cells.to_array(),
        }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, brush: &Brush) {
        self.exchange.dispatch_linear(encoder, self.sample_count, 128);
        self.grant_lift.dispatch(encoder, [self.cells[0].div_ceil(16), self.cells[1].div_ceil(16), 1]);
        self.apply_lift.dispatch_linear(encoder, self.sample_count, 128);
        self.wick.dispatch_linear(encoder, self.bristle_count, 64);
        encoder.copy_buffer_to_buffer(&brush.chain, 0, &self.previous_chain, 0, brush.chain.size());
    }

    pub fn start_stroke(&self, encoder: &mut wgpu::CommandEncoder, brush: &Brush) {
        encoder.copy_buffer_to_buffer(&brush.chain, 0, &self.previous_chain, 0, brush.chain.size());
    }
}
