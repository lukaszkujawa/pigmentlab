use super::Watercolour;
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::paint::{NOZZLE_RADIUS, WetBounds};
use crate::pigment::Paint;
use bytemuck::{Pod, Zeroable};
use glam::{UVec2, Vec2};

const WATER_PER_SECOND: f32 = 1.5;
const PIGMENT_PER_WATER: f32 = 20.0;
const DILUTION_PER_MEDIUM: f32 = 0.9;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PasteUniform {
    start: [f32; 2],
    end: [f32; 2],
    corner: [i32; 2],
    canvas: [u32; 2],
    cells: [u32; 2],
    radius: f32,
    water: f32,
    pigment: f32,
    padding: [f32; 3],
    absorption: [f32; 4],
    scattering: [f32; 4],
    behaviour: [f32; 4],
}

pub struct WaterTube {
    uniform: wgpu::Buffer,
    squeeze: ComputePass,
    canvas: [u32; 2],
    cells: [u32; 2],
    dt: f32,
}

impl WaterTube {
    pub fn new(gpu: &Gpu, water: &Watercolour, canvas: &Canvas, dt: f32) -> Self {
        let uniform = gpu.uniform_buffer("watercolour tube uniform", std::mem::size_of::<PasteUniform>() as u64);
        let shader = gpu.shader(
            "watercolour tube",
            &[
                include_str!("../../shaders/wash_outline.wgsl"),
                include_str!("../../shaders/wash_accumulators.wgsl"),
                include_str!("../../shaders/watercolour_tube.wgsl"),
            ],
        );
        let bindings = [
            (0, Binding::Uniform(&uniform)),
            (1, Binding::ReadWrite(&water.accumulators)),
            (2, Binding::ReadWrite(&water.touched)),
        ];
        Self {
            squeeze: ComputePass::new(gpu, "squeeze tube onto paper", &shader, "squeeze_onto_paper", &bindings),
            uniform,
            canvas: [canvas.size.width, canvas.size.height],
            cells: water.cells.to_array(),
            dt,
        }
    }

    pub fn encode_squeeze(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, path: [Vec2; 2], pressure: f32, paint: Paint, medium: f32) -> WetBounds {
        let [from, to] = path;
        let low = (from.min(to) - NOZZLE_RADIUS).floor();
        let high = (from.max(to) + NOZZLE_RADIUS).ceil();
        let size = (high - low).as_uvec2().max(UVec2::ONE);
        let flow = WATER_PER_SECOND * pressure * self.dt;
        let uniform = PasteUniform {
            start: from.to_array(),
            end: to.to_array(),
            corner: low.as_ivec2().to_array(),
            canvas: self.canvas,
            cells: self.cells,
            radius: NOZZLE_RADIUS,
            water: flow,
            pigment: flow * PIGMENT_PER_WATER * (1.0 - DILUTION_PER_MEDIUM * medium),
            padding: [0.0; 3],
            absorption: paint.coefficients.absorption.extend(0.0).to_array(),
            scattering: paint.coefficients.scattering.extend(0.0).to_array(),
            behaviour: [paint.behaviour.density, paint.behaviour.staining, paint.behaviour.granulation, 0.0],
        };
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        self.squeeze.dispatch(encoder, [size.x.div_ceil(16), size.y.div_ceil(16), 1]);
        WetBounds { min: low.as_ivec2(), max: high.as_ivec2() }
    }
}
