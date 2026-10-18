use super::level::WetBounds;
use super::window::ActiveWindow;
use crate::canvas::{Canvas, PAINT_CHANNELS};
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::pigment::Coefficients;
use glam::{IVec2, Vec2};

pub const MARK_CAPACITY: usize = 4096;
const SMALLEST_RADIUS: f32 = 0.75;
const TILE: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub from: Vec2,
    pub to: Vec2,
    pub radius: f32,
    pub volume: f32,
    pub paint: Coefficients,
    pub medium: f32,
}

impl Mark {
    pub fn dot(centre: Vec2, radius: f32, volume: f32, paint: Coefficients, medium: f32) -> Self {
        Self { from: centre, to: centre, radius, volume, paint, medium }
    }

    fn reach(&self) -> f32 {
        self.radius.max(SMALLEST_RADIUS)
    }

    fn bounds(&self) -> WetBounds {
        WetBounds { min: (self.from.min(self.to) - self.reach()).floor().as_ivec2(), max: (self.from.max(self.to) + self.reach()).ceil().as_ivec2() + IVec2::ONE }
    }

    fn gpu(&self) -> GpuMark {
        GpuMark {
            ends: [self.from.x, self.from.y, self.to.x, self.to.y],
            shape: [self.radius, self.volume, self.medium, 0.0],
            absorption: self.paint.absorption.extend(0.0).to_array(),
            scattering: self.paint.scattering.extend(0.0).to_array(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuMark {
    ends: [f32; 4],
    shape: [f32; 4],
    absorption: [f32; 4],
    scattering: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MarkBatch {
    region: [u32; 4],
    count: u32,
    padding: [u32; 3],
}

pub struct Marks {
    batch: wgpu::Buffer,
    marks: wgpu::Buffer,
    coverage: wgpu::Buffer,
    measure: ComputePass,
    spread: ComputePass,
    settle: ComputePass,
}

impl Marks {
    pub fn new(gpu: &Gpu, window: &ActiveWindow, canvas: &Canvas) -> Self {
        let batch = gpu.uniform_buffer("mark batch", std::mem::size_of::<MarkBatch>() as u64);
        let marks = gpu.storage_buffer("marks", (MARK_CAPACITY * std::mem::size_of::<GpuMark>()) as u64);
        let coverage = gpu.storage_buffer("mark coverage", (MARK_CAPACITY * 4) as u64);
        let film = gpu.storage_buffer("mark film", window.columns() * PAINT_CHANNELS * 4);
        let shader = gpu.shader("marks", &[include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/column.wgsl"), include_str!("../../shaders/marks.wgsl")]);
        let bindings = [
            (0, Binding::Uniform(&batch)),
            (1, Binding::Uniform(&window.uniform)),
            (2, Binding::ReadOnly(&marks)),
            (3, Binding::ReadWrite(&coverage)),
            (4, Binding::ReadWrite(&film)),
            (5, Binding::ReadWrite(&canvas.paint)),
            (6, Binding::ReadWrite(&window.volume)),
        ];
        Self {
            measure: ComputePass::new(gpu, "measure marks", &shader, "measure", &bindings),
            spread: ComputePass::new(gpu, "spread marks", &shader, "spread", &bindings),
            settle: ComputePass::new(gpu, "settle marks", &shader, "settle", &bindings),
            batch,
            marks,
            coverage,
        }
    }

    pub fn encode(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, window: &ActiveWindow, marks: &[Mark]) -> Option<WetBounds> {
        let marks = &marks[..marks.len().min(MARK_CAPACITY)];
        let bounds = marks.iter().map(Mark::bounds).reduce(WetBounds::include)?;
        let widest = marks.iter().map(|mark| mark.bounds()).map(|b| (b.max - b.min).max_element()).max().unwrap_or(1);
        let tile_span = (widest as f32 / TILE).ceil() as u32;
        let region = window.region_of(bounds.min, bounds.max);
        let uniform = MarkBatch { region: [region.origin.x, region.origin.y, region.size.x, region.size.y], count: marks.len() as u32, padding: [0; 3] };
        let gpu_marks: Vec<GpuMark> = marks.iter().map(Mark::gpu).collect();
        gpu.queue.write_buffer(&self.batch, 0, bytemuck::bytes_of(&uniform));
        gpu.queue.write_buffer(&self.marks, 0, bytemuck::cast_slice(&gpu_marks));
        encoder.clear_buffer(&self.coverage, 0, Some((marks.len() * 4) as u64));
        let groups = [tile_span, tile_span, marks.len() as u32];
        self.measure.dispatch(encoder, groups);
        self.spread.dispatch(encoder, groups);
        if !region.is_empty() {
            self.settle.dispatch(encoder, region.groups());
        }
        Some(bounds)
    }
}
