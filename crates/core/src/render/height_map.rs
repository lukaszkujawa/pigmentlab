use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::paint::ActiveWindow;

pub struct HeightMap {
    pub heights: wgpu::Buffer,
    pass: ComputePass,
    width: u32,
    height: u32,
}

impl HeightMap {
    pub fn new(gpu: &Gpu, canvas: &Canvas, window: &ActiveWindow) -> Self {
        let heights = gpu.storage_buffer("height map", canvas.size.pixel_count() * 4);
        let shader = gpu.shader(
            "height map",
            &[include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/height_map.wgsl")],
        );
        let pass = ComputePass::new(
            gpu,
            "height map",
            &shader,
            "build",
            &[
                (0, Binding::Uniform(&window.uniform)),
                (1, Binding::ReadOnly(&canvas.paint)),
                (2, Binding::ReadOnly(&window.volume)),
                (3, Binding::ReadWrite(&heights)),
            ],
        );
        Self { heights, pass, width: canvas.size.width, height: canvas.size.height }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        self.pass.dispatch(encoder, [self.width.div_ceil(16), self.height.div_ceil(16), 1]);
    }
}
