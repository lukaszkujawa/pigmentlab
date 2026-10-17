use super::window::ActiveWindow;
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::pigment::Coefficients;
use glam::Vec2;

pub const NOZZLE_RADIUS: f32 = crate::units::cm(0.25);
const PILE_PER_SECOND: f32 = crate::units::cm(0.75);
const FULL_ROPE_SPEED: f32 = crate::units::cm(4.0);
const ROPE_PROFILE_AREA: f32 = 2.0 + std::f32::consts::FRAC_PI_2;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TubeUniform {
    start: [f32; 2],
    end: [f32; 2],
    nozzle_radius: f32,
    rope_radius: f32,
    corner: [i32; 2],
    absorption: [f32; 4],
    scattering: [f32; 4],
    pile_height: f32,
    padding: [f32; 3],
}

pub struct Extrusion {
    pub rope_radius: f32,
    pub piled_share: f32,
}

pub fn extrude(nozzle_radius: f32, pressure: f32, travelled: f32, dt: f32) -> Extrusion {
    let squeezed = ROPE_PROFILE_AREA * nozzle_radius * nozzle_radius * FULL_ROPE_SPEED * pressure.clamp(0.0, 1.0) * dt;
    let full_rope = ROPE_PROFILE_AREA * nozzle_radius * nozzle_radius * travelled;
    if squeezed <= 0.0 {
        return Extrusion { rope_radius: 0.0, piled_share: 0.0 };
    }
    if squeezed >= full_rope {
        return Extrusion { rope_radius: nozzle_radius, piled_share: 1.0 - full_rope / squeezed };
    }
    Extrusion { rope_radius: (squeezed / (ROPE_PROFILE_AREA * travelled)).sqrt(), piled_share: 0.0 }
}

pub struct Tube {
    uniform: wgpu::Buffer,
    squeeze: ComputePass,
    dt: f32,
}

impl Tube {
    pub fn new(gpu: &Gpu, window: &ActiveWindow, canvas: &Canvas, stroke_base: &wgpu::Buffer, dt: f32) -> Self {
        let uniform = gpu.uniform_buffer("tube uniform", std::mem::size_of::<TubeUniform>() as u64);
        let shader = gpu.shader(
            "tube",
            &[ include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/tube.wgsl")],
        );
        let bindings = [
            (0, Binding::Uniform(&uniform)),
            (1, Binding::Uniform(&window.uniform)),
            (2, Binding::ReadWrite(&window.volume)),
            (3, Binding::ReadOnly(stroke_base)),
            (4, Binding::ReadOnly(&canvas.ground)),
        ];
        Self { squeeze: ComputePass::new(gpu, "squeeze tube", &shader, "squeeze", &bindings), uniform, dt }
    }

    pub fn encode_squeeze(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, path: [Vec2; 2], nozzle_radius: f32, pressure: f32, paint: Coefficients, medium: f32) {
        let [from, to] = path;
        let extrusion = extrude(nozzle_radius, pressure, from.distance(to), self.dt);
        let low = (from.min(to) - nozzle_radius).floor();
        let high = (from.max(to) + nozzle_radius).ceil();
        let size = (high - low).as_uvec2().max(glam::UVec2::ONE);
        let uniform = TubeUniform {
            start: from.to_array(),
            end: to.to_array(),
            nozzle_radius,
            rope_radius: extrusion.rope_radius,
            corner: low.as_ivec2().to_array(),
            absorption: paint.absorption.extend(medium).to_array(),
            scattering: paint.scattering.extend(0.0).to_array(),
            pile_height: PILE_PER_SECOND * pressure * self.dt * extrusion.piled_share,
            padding: [0.0; 3],
        };
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        self.squeeze.dispatch(encoder, [size.x.div_ceil(16), size.y.div_ceil(16), 1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_brisk_hand_draws_a_thinner_rope_and_never_piles() {
        let slow = extrude(10.0, 1.0, FULL_ROPE_SPEED * 0.01, 0.01);
        let fast = extrude(10.0, 1.0, FULL_ROPE_SPEED * 0.04, 0.01);
        assert!((slow.rope_radius - 10.0).abs() < 1e-3 && slow.piled_share < 1e-3);
        assert!((fast.rope_radius - 5.0).abs() < 1e-3 && fast.piled_share == 0.0);
    }

    #[test]
    fn a_still_nozzle_piles_everything() {
        let still = extrude(10.0, 0.8, 0.0, 0.01);
        assert_eq!(still.rope_radius, 10.0);
        assert!((still.piled_share - 1.0).abs() < 1e-6);
    }

    #[test]
    fn no_pressure_squeezes_nothing() {
        let none = extrude(10.0, 0.0, 5.0, 0.01);
        assert_eq!((none.rope_radius, none.piled_share), (0.0, 0.0));
    }
}
