use super::window::ActiveWindow;
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::render::HeightMap;

pub struct StoreLevelling {
    pass: ComputePass,
    width: u32,
    height: u32,
}

impl StoreLevelling {
    pub fn new(gpu: &Gpu, canvas: &Canvas, window: &ActiveWindow, height_map: &HeightMap) -> Self {
        let shader = gpu.shader(
            "level store",
            &[
                include_str!("../../shaders/units.wgsl"),
                include_str!("../../shaders/window.wgsl"),
                include_str!("../../shaders/level_store.wgsl"),
            ],
        );
        let pass = ComputePass::new(
            gpu,
            "level store",
            &shader,
            "level",
            &[
                (0, Binding::Uniform(&window.uniform)),
                (1, Binding::ReadWrite(&canvas.paint)),
                (2, Binding::ReadOnly(&height_map.heights)),
            ],
        );
        Self { pass, width: canvas.size.width, height: canvas.size.height }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        self.pass.dispatch(encoder, [self.width.div_ceil(16), self.height.div_ceil(16), 1]);
    }
}
