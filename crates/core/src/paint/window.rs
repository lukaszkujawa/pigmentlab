use crate::canvas::{Canvas, PAINT_CHANNELS};
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};
use glam::{IVec2, UVec2, Vec2};

pub const WINDOW_DEPTH: u32 = 16;
pub const SMALLEST_WINDOW: u32 = 384;
pub const LARGEST_WINDOW: u32 = 1024;
const WINDOW_STEP: u32 = 64;
const MIN_RELOCATION_STEP: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowRegion {
    pub origin: UVec2,
    pub size: UVec2,
}

impl WindowRegion {
    pub fn is_empty(&self) -> bool {
        self.size.x == 0 || self.size.y == 0
    }

    pub fn groups(&self) -> [u32; 3] {
        [self.size.x.div_ceil(16), self.size.y.div_ceil(16), 1]
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WindowUniform {
    origin: [i32; 2],
    previous_origin: [i32; 2],
    canvas_width: u32,
    canvas_height: u32,
    width: u32,
    height: u32,
    depth: u32,
    _padding: u32,
}

pub struct ActiveWindow {
    size: u32,
    origin: IVec2,
    previous_origin: IVec2,
    canvas_size: Vec2,
    pub uniform: wgpu::Buffer,
    pub volume: wgpu::Buffer,
    pub columns: wgpu::Buffer,
    previous: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    bake: wgpu::ComputePipeline,
    relocate: wgpu::ComputePipeline,
    unbake_all: wgpu::ComputePipeline,
}

impl ActiveWindow {
    pub fn new(gpu: &Gpu, canvas: &Canvas, size: u32) -> Self {
        let cells = u64::from(size) * u64::from(size) * u64::from(WINDOW_DEPTH);
        let volume = gpu.storage_buffer("active window volume", cells * PAINT_CHANNELS * 4);
        let previous = gpu.storage_buffer("active window previous volume", cells * PAINT_CHANNELS * 4);
        let columns = gpu.storage_buffer("active window column snapshot", u64::from(size) * u64::from(size) * 16);
        let uniform = gpu.uniform_buffer("active window uniform", std::mem::size_of::<WindowUniform>() as u64);
        let compute = wgpu::ShaderStages::COMPUTE;
        let read_write = wgpu::BufferBindingType::Storage { read_only: false };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("active window transfer"),
            entries: &[
                buffer_layout_entry(0, compute, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(1, compute, read_write),
                buffer_layout_entry(2, compute, read_write),
                buffer_layout_entry(3, compute, read_write),
            ],
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("active window transfer"),
            layout: &layout,
            entries: &[
                buffer_binding(0, &uniform),
                buffer_binding(1, &volume),
                buffer_binding(2, &canvas.paint),
                buffer_binding(3, &previous),
            ],
        });
        let shader = gpu.shader(
            "active window transfer",
            &[include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/km.wgsl"), include_str!("../../shaders/window_transfer.wgsl")],
        );
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("active window transfer"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let canvas_size = Vec2::new(canvas.size.width as f32, canvas.size.height as f32);
        let window = Self {
            size,
            origin: IVec2::MAX,
            previous_origin: IVec2::MAX,
            canvas_size,
            uniform,
            volume,
            columns,
            previous,
            bind_group,
            bake: pipeline("bake"),
            relocate: pipeline("relocate"),
            unbake_all: pipeline("unbake_all"),
        };
        window.write_uniform(gpu, canvas);
        window
    }

    fn write_uniform(&self, gpu: &Gpu, canvas: &Canvas) {
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&WindowUniform {
                origin: self.origin.to_array(),
                previous_origin: self.previous_origin.to_array(),
                canvas_width: canvas.size.width,
                canvas_height: canvas.size.height,
                width: self.size,
                height: self.size,
                depth: WINDOW_DEPTH,
                _padding: 0,
            }),
        );
    }

    pub fn origin(&self) -> IVec2 {
        self.origin
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn columns(&self) -> u64 {
        u64::from(self.size) * u64::from(self.size)
    }

    pub fn groups(&self) -> [u32; 3] {
        [self.size.div_ceil(16), self.size.div_ceil(16), 1]
    }

    pub fn size_for(footprint_radius: f32) -> u32 {
        let needed = ((footprint_radius + MIN_RELOCATION_STEP) * 2.0).ceil() as u32;
        (needed.div_ceil(WINDOW_STEP) * WINDOW_STEP).clamp(SMALLEST_WINDOW, LARGEST_WINDOW)
    }

    pub fn largest_footprint() -> f32 {
        LARGEST_WINDOW as f32 * 0.5 - MIN_RELOCATION_STEP
    }

    pub fn region_around(&self, centre: Vec2, radius: f32) -> WindowRegion {
        let local = centre - self.origin.as_vec2();
        let limit = IVec2::splat(self.size as i32);
        let low = (local - radius).floor().as_ivec2().clamp(IVec2::ZERO, limit);
        let high = (local + radius).ceil().as_ivec2().clamp(IVec2::ZERO, limit);
        WindowRegion { origin: low.as_uvec2(), size: (high - low).max(IVec2::ZERO).as_uvec2() }
    }

    pub fn region_of(&self, min: IVec2, max: IVec2) -> WindowRegion {
        let limit = IVec2::splat(self.size as i32);
        let low = (min - self.origin).clamp(IVec2::ZERO, limit);
        let high = (max - self.origin).clamp(IVec2::ZERO, limit);
        WindowRegion { origin: low.as_uvec2(), size: (high - low).max(IVec2::ZERO).as_uvec2() }
    }

    pub fn needs_relocation(&self, focus: Vec2, footprint_radius: f32) -> bool {
        let half = Vec2::splat(self.size as f32 * 0.5);
        let margin = footprint_radius.min(half.x - MIN_RELOCATION_STEP);
        let drift = (focus - (self.origin.as_vec2() + half)).abs();
        self.origin == IVec2::MAX || drift.x >= half.x - margin || drift.y >= half.y - margin
    }

    pub fn follow(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, canvas: &Canvas, focus: Vec2) {
        let half = Vec2::splat(self.size as f32 * 0.5);
        self.previous_origin = self.origin;
        self.origin = (focus - half).clamp(-half, self.canvas_size - half).round().as_ivec2();
        self.snapshot_volume(encoder);
        self.write_uniform(gpu, canvas);
        if self.previous_origin != IVec2::MAX {
            self.dispatch(encoder, &self.bake);
        }
        self.dispatch(encoder, &self.relocate);
    }

    pub fn encode_bake(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, canvas: &Canvas) {
        if self.origin == IVec2::MAX {
            return;
        }
        self.previous_origin = self.origin;
        self.snapshot_volume(encoder);
        self.write_uniform(gpu, canvas);
        self.dispatch(encoder, &self.bake);
    }

    pub fn encode_unbake(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.origin == IVec2::MAX {
            return;
        }
        self.dispatch(encoder, &self.unbake_all);
    }

    fn snapshot_volume(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_buffer_to_buffer(&self.volume, 0, &self.previous, 0, self.volume.size());
    }

    fn dispatch(&self, encoder: &mut wgpu::CommandEncoder, pipeline: &wgpu::ComputePipeline) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("active window transfer"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        let [x, y, z] = self.groups();
        pass.dispatch_workgroups(x, y, z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_grows_in_steps_to_hold_the_brush_and_never_beyond_its_largest_size() {
        assert_eq!(ActiveWindow::size_for(100.0), SMALLEST_WINDOW);
        assert_eq!(ActiveWindow::size_for(300.0), 640);
        assert_eq!(ActiveWindow::size_for(5000.0), LARGEST_WINDOW);
        assert!(ActiveWindow::size_for(ActiveWindow::largest_footprint()) <= LARGEST_WINDOW);
    }
}
