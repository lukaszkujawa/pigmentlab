use super::level::WetBounds;
use super::window::ActiveWindow;
use crate::canvas::{Canvas, PAINT_CHANNELS};
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::pigment::Coefficients;
use glam::{IVec2, Vec2};

pub const MARK_CAPACITY: usize = 4096;
const TILE_CAPACITY: usize = 1 << 18;
const TILE_ROW: u32 = 65535;
pub(crate) const SMALLEST_RADIUS: f32 = 0.75;
const TILE: f32 = 16.0;
pub(crate) const EGG: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub from: Vec2,
    pub to: Vec2,
    pub radius: f32,
    pub volume: f32,
    pub paint: Coefficients,
    pub medium: f32,
    pub profile: Profile,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub flatness: f32,
    pub stiffness: f32,
    pub ragged: f32,
    pub open: [bool; 2],
    pub taper: f32,
    pub oval: bool,
}

impl Profile {
    pub const ROUND: Self = Self { flatness: 0.0, stiffness: 1.0, ragged: 0.0, open: [false; 2], taper: 1.0, oval: false };
}

impl Mark {
    pub fn dot(centre: Vec2, radius: f32, volume: f32, paint: Coefficients, medium: f32) -> Self {
        Self { from: centre, to: centre, radius, volume, paint, medium, profile: Profile::ROUND }
    }

    fn reach(&self) -> f32 {
        let egg = if self.profile.oval { 1.0 + EGG } else { 1.0 };
        (self.radius * self.profile.taper.max(1.0) * (1.0 + self.profile.ragged) * egg).max(SMALLEST_RADIUS)
    }

    fn tiles(&self) -> (u32, u32) {
        let size = self.bounds().max - self.bounds().min;
        ((size.x.max(1) as f32 / TILE).ceil() as u32, (size.y.max(1) as f32 / TILE).ceil() as u32)
    }

    fn bounds(&self) -> WetBounds {
        WetBounds { min: (self.from.min(self.to) - self.reach()).floor().as_ivec2(), max: (self.from.max(self.to) + self.reach()).ceil().as_ivec2() + IVec2::ONE }
    }

    fn gpu(&self) -> GpuMark {
        GpuMark {
            ends: [self.from.x, self.from.y, self.to.x, self.to.y],
            shape: [self.radius, self.volume, self.medium, self.profile.flatness],
            absorption: self.paint.absorption.extend(0.0).to_array(),
            scattering: self.paint.scattering.extend(0.0).to_array(),
            form: [self.radius * self.profile.taper, self.profile.ragged, f32::from(u8::from(self.profile.open[0]) + 2 * u8::from(self.profile.open[1]) + 4 * u8::from(self.profile.oval)), self.profile.stiffness],
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
    form: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MarkBatch {
    region: [u32; 4],
    count: u32,
    tiles: u32,
    padding: [u32; 2],
}

pub struct Marks {
    batch: wgpu::Buffer,
    marks: wgpu::Buffer,
    coverage: wgpu::Buffer,
    tiles: wgpu::Buffer,
    measure: ComputePass,
    spread: ComputePass,
    settle: ComputePass,
}

impl Marks {
    pub fn new(gpu: &Gpu, window: &ActiveWindow, canvas: &Canvas) -> Self {
        let batch = gpu.uniform_buffer("mark batch", std::mem::size_of::<MarkBatch>() as u64);
        let marks = gpu.storage_buffer("marks", (MARK_CAPACITY * std::mem::size_of::<GpuMark>()) as u64);
        let coverage = gpu.storage_buffer("mark coverage", (MARK_CAPACITY * 4) as u64);
        let tiles = gpu.storage_buffer("mark tiles", (TILE_CAPACITY * 8) as u64);
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
            (7, Binding::ReadOnly(&tiles)),
        ];
        Self {
            measure: ComputePass::new(gpu, "measure marks", &shader, "measure", &bindings),
            spread: ComputePass::new(gpu, "spread marks", &shader, "spread", &bindings),
            settle: ComputePass::new(gpu, "settle marks", &shader, "settle", &bindings),
            batch,
            marks,
            coverage,
            tiles,
        }
    }

    pub fn fitting(marks: &[Mark]) -> usize {
        let mut tiles = 0;
        marks.iter().take(MARK_CAPACITY).take_while(|mark| {
            let (across, down) = mark.tiles();
            tiles += (across * down) as usize;
            tiles <= TILE_CAPACITY
        }).count().max(1).min(marks.len())
    }

    pub fn encode(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, window: &ActiveWindow, marks: &[Mark]) -> Option<WetBounds> {
        let marks = &marks[..Self::fitting(marks)];
        let bounds = marks.iter().map(Mark::bounds).reduce(WetBounds::include)?;
        let tiles: Vec<[u32; 2]> = marks.iter().enumerate().flat_map(|(index, mark)| {
            let (across, down) = mark.tiles();
            (0..down).flat_map(move |y| (0..across).map(move |x| [index as u32, x | (y << 16)]))
        }).take(TILE_CAPACITY).collect();
        let region = window.region_of(bounds.min, bounds.max);
        let uniform = MarkBatch { region: [region.origin.x, region.origin.y, region.size.x, region.size.y], count: marks.len() as u32, tiles: tiles.len() as u32, padding: [0; 2] };
        let gpu_marks: Vec<GpuMark> = marks.iter().map(Mark::gpu).collect();
        gpu.queue.write_buffer(&self.batch, 0, bytemuck::bytes_of(&uniform));
        gpu.queue.write_buffer(&self.marks, 0, bytemuck::cast_slice(&gpu_marks));
        gpu.queue.write_buffer(&self.tiles, 0, bytemuck::cast_slice(&tiles));
        encoder.clear_buffer(&self.coverage, 0, Some((marks.len() * 4) as u64));
        let count = tiles.len() as u32;
        let groups = [count.min(TILE_ROW), count.div_ceil(TILE_ROW), 1];
        self.measure.dispatch(encoder, groups);
        self.spread.dispatch(encoder, groups);
        if !region.is_empty() {
            self.settle.dispatch(encoder, region.groups());
        }
        Some(bounds)
    }
}
