use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::paint::ActiveWindow;

pub struct HeightMap {
    pub heights: wgpu::Buffer,
    pub surface: wgpu::Buffer,
    pub smooth_surface: wgpu::Buffer,
    pub smooth_paint: wgpu::Buffer,
    pass: ComputePass,
    smooth: ComputePass,
    width: u32,
    height: u32,
}

impl HeightMap {
    pub fn new(gpu: &Gpu, canvas: &Canvas, window: &ActiveWindow) -> Self {
        let heights = gpu.storage_buffer("height map", canvas.size.pixel_count() * 4);
        let surface = gpu.storage_buffer("surface map", canvas.size.pixel_count() * 4);
        let smooth_surface = gpu.storage_buffer("smooth surface map", canvas.size.pixel_count() * 4);
        let smooth_paint = gpu.storage_buffer("smooth paint map", canvas.size.pixel_count() * 4);
        let shader = gpu.shader(
            "height map",
            &[ include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/height_map.wgsl")],
        );
        let bindings = [
            (0, Binding::Uniform(&window.uniform)),
            (1, Binding::ReadOnly(&canvas.paint)),
            (2, Binding::ReadOnly(&window.volume)),
            (3, Binding::ReadWrite(&heights)),
            (4, Binding::ReadOnly(&canvas.ground)),
            (5, Binding::ReadWrite(&surface)),
            (6, Binding::ReadWrite(&smooth_surface)),
            (7, Binding::ReadWrite(&smooth_paint)),
        ];
        let pass = ComputePass::new(gpu, "height map", &shader, "build", &bindings);
        let smooth = ComputePass::new(gpu, "smooth height map", &shader, "blur", &bindings);
        Self { heights, surface, smooth_surface, smooth_paint, pass, smooth, width: canvas.size.width, height: canvas.size.height }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let groups = [self.width.div_ceil(16), self.height.div_ceil(16), 1];
        self.pass.dispatch(encoder, groups);
        self.smooth.dispatch(encoder, groups);
    }
}
