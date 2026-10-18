use super::drip_overlay::Viewport;
use crate::gpu::Gpu;
use glam::UVec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Canvas,
    Stage,
}

pub struct ViewFrames {
    canvas: wgpu::Buffer,
    stage: wgpu::Buffer,
}

impl ViewFrames {
    pub fn new(gpu: &Gpu, canvas: UVec2) -> Self {
        let frames = Self { canvas: gpu.uniform_buffer("canvas view", 32), stage: gpu.uniform_buffer("stage view", 32) };
        write(gpu, &frames.canvas, Viewport::canvas(canvas));
        frames
    }

    pub fn set_stage(&self, gpu: &Gpu, view: Viewport) {
        write(gpu, &self.stage, view);
    }

    pub fn buffer(&self, frame: Frame) -> &wgpu::Buffer {
        match frame {
            Frame::Canvas => &self.canvas,
            Frame::Stage => &self.stage,
        }
    }
}

fn write(gpu: &Gpu, buffer: &wgpu::Buffer, view: Viewport) {
    gpu.queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[view.size.x as f32, view.size.y as f32, view.origin.x, view.origin.y, view.scale, 0.0, 0.0, 0.0]));
}
