use super::window::ActiveWindow;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::pigment::Coefficients;
use glam::Vec2;

pub const NOZZLE_RADIUS: f32 = crate::units::cm(0.25);
const FLOW_PER_SECOND: f32 = crate::units::cm(0.75);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TubeUniform {
    start: [f32; 2],
    end: [f32; 2],
    radius: f32,
    height: f32,
    corner: [i32; 2],
    absorption: [f32; 4],
    scattering: [f32; 4],
}

pub struct Tube {
    uniform: wgpu::Buffer,
    squeeze: ComputePass,
    dt: f32,
}

impl Tube {
    pub fn new(gpu: &Gpu, window: &ActiveWindow, dt: f32) -> Self {
        let uniform = gpu.uniform_buffer("tube uniform", std::mem::size_of::<TubeUniform>() as u64);
        let shader = gpu.shader(
            "tube",
            &[ include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/tube.wgsl")],
        );
        let bindings = [
            (0, Binding::Uniform(&uniform)),
            (1, Binding::Uniform(&window.uniform)),
            (2, Binding::ReadWrite(&window.volume)),
        ];
        Self { squeeze: ComputePass::new(gpu, "squeeze tube", &shader, "squeeze", &bindings), uniform, dt }
    }

    pub fn encode_squeeze(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, path: [Vec2; 2], pressure: f32, paint: Coefficients, medium: f32) {
        let [from, to] = path;
        let low = (from.min(to) - NOZZLE_RADIUS).floor();
        let high = (from.max(to) + NOZZLE_RADIUS).ceil();
        let size = (high - low).as_uvec2().max(glam::UVec2::ONE);
        let uniform = TubeUniform {
            start: from.to_array(),
            end: to.to_array(),
            radius: NOZZLE_RADIUS,
            height: FLOW_PER_SECOND * pressure * self.dt,
            corner: low.as_ivec2().to_array(),
            absorption: paint.absorption.extend(medium).to_array(),
            scattering: paint.scattering.extend(0.0).to_array(),
        };
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        self.squeeze.dispatch(encoder, [size.x.div_ceil(16), size.y.div_ceil(16), 1]);
    }
}
