use super::Change;
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::paint::{ActiveWindow, WetBounds};
use glam::IVec2;

pub const FILM_CHANGE_CAPACITY: usize = 1 << 18;
const ENTRY: usize = 9;

pub struct FilmDeltas {
    changes: wgpu::Buffer,
    batch: wgpu::Buffer,
    apply: ComputePass,
    width: u32,
}

impl FilmDeltas {
    pub fn new(gpu: &Gpu, window: &ActiveWindow, canvas: &Canvas) -> Self {
        let changes = gpu.storage_buffer("film changes", (FILM_CHANGE_CAPACITY * ENTRY * 4) as u64);
        let batch = gpu.uniform_buffer("film batch", 16);
        let shader = gpu.shader("film", &[include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/film.wgsl")]);
        let bindings = [(0, Binding::Uniform(&window.uniform)), (1, Binding::ReadWrite(&window.volume)), (2, Binding::ReadWrite(&canvas.paint)), (3, Binding::ReadOnly(&changes)), (4, Binding::Uniform(&batch))];
        Self { apply: ComputePass::new(gpu, "apply film", &shader, "apply", &bindings), changes, batch, width: canvas.size.width }
    }

    pub fn encode(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, changes: &[Change]) -> Option<WetBounds> {
        let changes = &changes[..changes.len().min(FILM_CHANGE_CAPACITY)];
        let pixel = |change: &Change| IVec2::new((change.pixel % self.width) as i32, (change.pixel / self.width) as i32);
        let low = changes.iter().map(pixel).reduce(IVec2::min)?;
        let high = changes.iter().map(pixel).reduce(IVec2::max)?;
        let flat: Vec<i32> = changes.iter().flat_map(|change| std::iter::once(change.pixel as i32).chain(change.delta)).collect();
        gpu.queue.write_buffer(&self.changes, 0, bytemuck::cast_slice(&flat));
        gpu.queue.write_buffer(&self.batch, 0, bytemuck::cast_slice(&[changes.len() as u32, 0, 0, 0]));
        self.apply.dispatch(encoder, [(changes.len() as u32).div_ceil(64), 1, 1]);
        Some(WetBounds { min: low, max: high + 1 })
    }
}
