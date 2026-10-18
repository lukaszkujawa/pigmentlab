use super::window::ActiveWindow;
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::render::HeightMap;
use glam::IVec2;

const NEIGHBOUR_REACH: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WetBounds {
    pub min: IVec2,
    pub max: IVec2,
}

impl WetBounds {
    pub fn around(centre: IVec2, radius: i32) -> Self {
        Self { min: centre - radius, max: centre + radius }
    }

    pub fn include(self, other: Self) -> Self {
        Self { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    pub fn expand(self, margin: i32) -> Self {
        Self { min: self.min - margin, max: self.max + margin }
    }

    pub fn clamped(self, width: u32, height: u32) -> (IVec2, IVec2) {
        let min = self.min.clamp(IVec2::ZERO, IVec2::new(width as i32, height as i32));
        let max = self.max.clamp(min, IVec2::new(width as i32, height as i32));
        (min, max - min)
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WetUniform {
    origin: [i32; 2],
    size: [u32; 2],
}

pub struct Levelling {
    snapshot: ComputePass,
    window: ComputePass,
    store_even: ComputePass,
    store_odd: ComputePass,
    wet_uniform: wgpu::Buffer,
    region_uniform: wgpu::Buffer,
    canvas_size: (u32, u32),
}

impl Levelling {
    pub fn new(gpu: &Gpu, canvas: &Canvas, window: &ActiveWindow, height_map: &HeightMap) -> Self {
        let columns = window.columns();
        let column_heights = gpu.storage_buffer("column snapshot", columns * 16);
        let column_pigment = gpu.storage_buffer("column pigment snapshot", columns * 16);
        let wet_uniform = gpu.uniform_buffer("wet bounds", std::mem::size_of::<WetUniform>() as u64);
        let region_uniform = gpu.uniform_buffer("levelling window region", std::mem::size_of::<WetUniform>() as u64);
        let shader = gpu.shader(
            "level",
            &[
                include_str!("../../shaders/window.wgsl"),
                include_str!("../../shaders/level.wgsl"),
            ],
        );
        let bindings = [
            (0, Binding::Uniform(&window.uniform)),
            (1, Binding::ReadWrite(&window.volume)),
            (2, Binding::ReadWrite(&canvas.paint)),
            (3, Binding::ReadOnly(&height_map.heights)),
            (4, Binding::ReadWrite(&column_heights)),
            (5, Binding::ReadWrite(&column_pigment)),
            (6, Binding::ReadOnly(&canvas.ground)),
            (7, Binding::Uniform(&wet_uniform)),
            (8, Binding::Uniform(&region_uniform)),
        ];
        let pass = |label, entry| ComputePass::new(gpu, label, &shader, entry, &bindings);
        Self {
            snapshot: pass("snapshot columns", "snapshot_columns"),
            window: pass("level window", "level_window"),
            store_even: pass("level store even", "level_store_even"),
            store_odd: pass("level store odd", "level_store_odd"),
            wet_uniform,
            region_uniform,
            canvas_size: (canvas.size.width, canvas.size.height),
        }
    }

    pub fn encode(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, window: &ActiveWindow, wet: WetBounds) {
        let region = window.region_of(wet.min - NEIGHBOUR_REACH, wet.max + NEIGHBOUR_REACH);
        let (origin, size) = wet.clamped(self.canvas_size.0, self.canvas_size.1);
        gpu.queue.write_buffer(&self.region_uniform, 0, bytemuck::bytes_of(&WetUniform { origin: region.origin.as_ivec2().to_array(), size: region.size.to_array() }));
        gpu.queue.write_buffer(&self.wet_uniform, 0, bytemuck::bytes_of(&WetUniform { origin: origin.to_array(), size: size.as_uvec2().to_array() }));
        if !region.is_empty() {
            self.snapshot.dispatch(encoder, region.groups());
            self.window.dispatch(encoder, region.groups());
        }
        if size.x == 0 || size.y == 0 {
            return;
        }
        let store_groups = [(size.x as u32).div_ceil(32), (size.y as u32).div_ceil(16), 1];
        self.store_even.dispatch(encoder, store_groups);
        self.store_odd.dispatch(encoder, store_groups);
    }
}
