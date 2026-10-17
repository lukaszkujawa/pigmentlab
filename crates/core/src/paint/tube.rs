use super::rheology::{strand, yield_stress};
use super::squeeze::{MAX_REACH, Squeeze, Stamp};
use super::window::ActiveWindow;
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::pigment::Coefficients;
use glam::Vec2;

pub const NOZZLE_RADIUS: f32 = crate::units::cm(0.25);
const BOX_MARGIN: f32 = 1.0;
const ROPE_HEIGHT_PER_NOZZLE_RADIUS: f32 = 1.6;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TubeUniform {
    absorption: [f32; 4],
    scattering: [f32; 4],
    path_start: [f32; 2],
    path_end: [f32; 2],
    corner: [i32; 2],
    strand: [f32; 2],
    mound: [f32; 2],
    strand_core: f32,
    strand_drape: f32,
    mound_core: f32,
    mound_drape: f32,
    arc: f32,
    memory: f32,
    squeezed: f32,
    backlog_limit: f32,
    padding: [f32; 2],
}

pub struct Tube {
    uniform: wgpu::Buffer,
    ledger: wgpu::Buffer,
    offer: ComputePass,
    plan: ComputePass,
    fill: ComputePass,
    squeeze: Squeeze,
    reach: f32,
    dt: f32,
}

impl Tube {
    pub fn new(gpu: &Gpu, window: &ActiveWindow, canvas: &Canvas, heights: &wgpu::Buffer, seed: u64, dt: f32) -> Self {
        let uniform = gpu.uniform_buffer("tube uniform", std::mem::size_of::<TubeUniform>() as u64);
        let marks = gpu.storage_buffer("tube marks", canvas.size.pixel_count() * 8);
        let planned = gpu.storage_buffer("tube plan", window.columns() * 8);
        let ledger = gpu.storage_buffer("tube ledger", 8);
        let shader = gpu.shader("tube", &[include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/tube.wgsl")]);
        let bindings = [
            (0, Binding::Uniform(&uniform)),
            (1, Binding::Uniform(&window.uniform)),
            (2, Binding::ReadWrite(&window.volume)),
            (3, Binding::ReadOnly(&canvas.ground)),
            (4, Binding::ReadOnly(heights)),
            (5, Binding::ReadOnly(&canvas.paint)),
            (6, Binding::ReadWrite(&marks)),
            (7, Binding::ReadWrite(&planned)),
            (8, Binding::ReadOnly(&canvas.relief)),
            (9, Binding::ReadWrite(&ledger)),
        ];
        Self {
            offer: ComputePass::new(gpu, "offer squeeze", &shader, "offer", &bindings),
            plan: ComputePass::new(gpu, "plan squeeze", &shader, "plan", &bindings),
            fill: ComputePass::new(gpu, "fill squeeze", &shader, "fill", &bindings),
            uniform,
            ledger,
            squeeze: Squeeze::new(seed),
            reach: 0.0,
            dt,
        }
    }

    pub fn press(&mut self, gpu: &Gpu, nozzle: Vec2) {
        self.squeeze.press(nozzle);
        self.reach = 0.0;
        gpu.queue.write_buffer(&self.ledger, 0, &[0; 8]);
    }

    pub fn reach(&self, nozzle_radius: f32, medium: f32) -> f32 {
        let full_strand = strand(std::f32::consts::PI * nozzle_radius * nozzle_radius, yield_stress(medium), ROPE_HEIGHT_PER_NOZZLE_RADIUS * nozzle_radius);
        self.reach.max(full_strand.half_width).min(MAX_REACH)
    }

    pub fn encode_squeeze(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, nozzle: Vec2, nozzle_radius: f32, pressure: f32, paint: Coefficients, medium: f32) {
        let Some(stamp) = self.squeeze.advance(nozzle, nozzle_radius, pressure, yield_stress(medium), self.dt) else { return };
        self.reach = stamp.reach();
        let low = (stamp.path[0].min(stamp.path[1]) - stamp.strand.half_width).min(stamp.path[1] - stamp.mound.radius) - BOX_MARGIN;
        let high = (stamp.path[0].max(stamp.path[1]) + stamp.strand.half_width).max(stamp.path[1] + stamp.mound.radius) + BOX_MARGIN;
        let corner = low.floor().as_ivec2();
        let size = (high.ceil().as_ivec2() - corner).max(glam::IVec2::ONE).as_uvec2();
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform(&stamp, corner, paint, medium)));
        let groups = [size.x.div_ceil(16), size.y.div_ceil(16), 1];
        self.offer.dispatch(encoder, [1, 1, 1]);
        self.plan.dispatch(encoder, groups);
        self.fill.dispatch(encoder, groups);
    }
}

fn uniform(stamp: &Stamp, corner: glam::IVec2, paint: Coefficients, medium: f32) -> TubeUniform {
    TubeUniform {
        path_start: stamp.path[0].to_array(),
        path_end: stamp.path[1].to_array(),
        corner: corner.to_array(),
        strand: [stamp.strand.half_width, stamp.strand.height],
        mound: [stamp.mound.radius, stamp.mound.height],
        absorption: paint.absorption.extend(medium).to_array(),
        scattering: paint.scattering.extend(0.0).to_array(),
        strand_core: stamp.strand_core,
        strand_drape: stamp.strand_drape,
        mound_core: stamp.mound_core,
        mound_drape: stamp.mound_drape,
        arc: stamp.arc,
        memory: stamp.memory,
        squeezed: stamp.squeezed,
        backlog_limit: stamp.backlog_limit,
        padding: [0.0; 2],
    }
}
