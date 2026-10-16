use super::window::ActiveWindow;
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::render::HeightMap;

pub struct StoreLevelling {
    even: ComputePass,
    odd: ComputePass,
    dry: ComputePass,
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
        let bindings = [
            (0, Binding::Uniform(&window.uniform)),
            (1, Binding::ReadWrite(&canvas.paint)),
            (2, Binding::ReadOnly(&height_map.heights)),
        ];
        Self {
            even: ComputePass::new(gpu, "level store even", &shader, "level_even", &bindings),
            odd: ComputePass::new(gpu, "level store odd", &shader, "level_odd", &bindings),
            dry: ComputePass::new(gpu, "dry store", &shader, "dry_all", &bindings),
            width: canvas.size.width,
            height: canvas.size.height,
        }
    }

    pub fn encode_dry(&self, encoder: &mut wgpu::CommandEncoder) {
        self.dry.dispatch(encoder, [self.width.div_ceil(16), self.height.div_ceil(16), 1]);
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let groups = [self.width.div_ceil(32), self.height.div_ceil(16), 1];
        self.even.dispatch(encoder, groups);
        self.odd.dispatch(encoder, groups);
    }
}
