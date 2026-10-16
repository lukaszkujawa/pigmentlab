use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::paint::{ActiveWindow, WetBounds};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RegionUniform {
    origin: [i32; 2],
    size: [u32; 2],
}

pub struct HeightMap {
    pub heights: wgpu::Buffer,
    pub surface: wgpu::Buffer,
    pub smooth_surface: wgpu::Buffer,
    pub smooth_paint: wgpu::Buffer,
    pub tuft_base: wgpu::Buffer,
    build_region: wgpu::Buffer,
    blur_region: wgpu::Buffer,
    base_region: wgpu::Buffer,
    pass: ComputePass,
    smooth: ComputePass,
    base: ComputePass,
    width: u32,
    height: u32,
}

impl HeightMap {
    pub fn new(gpu: &Gpu, canvas: &Canvas, window: &ActiveWindow) -> Self {
        let heights = gpu.storage_buffer("height map", canvas.size.pixel_count() * 4);
        let surface = gpu.storage_buffer("surface map", canvas.size.pixel_count() * 4);
        let smooth_surface = gpu.storage_buffer("smooth surface map", canvas.size.pixel_count() * 4);
        let smooth_paint = gpu.storage_buffer("smooth paint map", canvas.size.pixel_count() * 4);
        let tuft_base = gpu.storage_buffer("tuft base map", canvas.size.pixel_count() * 4);
        let build_region = gpu.uniform_buffer("height map build region", std::mem::size_of::<RegionUniform>() as u64);
        let blur_region = gpu.uniform_buffer("height map blur region", std::mem::size_of::<RegionUniform>() as u64);
        let base_region = gpu.uniform_buffer("height map base region", std::mem::size_of::<RegionUniform>() as u64);
        let shader = gpu.shader(
            "height map",
            &[include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/height_map.wgsl")],
        );
        let maps = [&heights, &surface, &smooth_surface, &smooth_paint, &tuft_base];
        let pass = ComputePass::new(gpu, "height map", &shader, "build", &bindings(window, canvas, maps, &build_region));
        let smooth = ComputePass::new(gpu, "smooth height map", &shader, "blur", &bindings(window, canvas, maps, &blur_region));
        let base = ComputePass::new(gpu, "tuft base map", &shader, "tuft_floor", &bindings(window, canvas, maps, &base_region));
        Self {
            heights,
            surface,
            smooth_surface,
            smooth_paint,
            tuft_base,
            build_region,
            blur_region,
            base_region,
            pass,
            smooth,
            base,
            width: canvas.size.width,
            height: canvas.size.height,
        }
    }

    pub fn encode_tuft_base(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, region: WetBounds) {
        if let Some(groups) = self.select(gpu, &self.base_region, region) {
            self.base.dispatch(encoder, groups);
        }
    }

    pub fn encode_build(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, region: WetBounds) {
        if let Some(groups) = self.select(gpu, &self.build_region, region) {
            self.pass.dispatch(encoder, groups);
        }
    }

    pub fn encode_blur(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, region: WetBounds) {
        if let Some(groups) = self.select(gpu, &self.blur_region, region.expand(1)) {
            self.smooth.dispatch(encoder, groups);
        }
    }

    fn select(&self, gpu: &Gpu, uniform: &wgpu::Buffer, region: WetBounds) -> Option<[u32; 3]> {
        let (origin, size) = region.clamped(self.width, self.height);
        if size.x == 0 || size.y == 0 {
            return None;
        }
        gpu.queue.write_buffer(uniform, 0, bytemuck::bytes_of(&RegionUniform { origin: origin.to_array(), size: size.as_uvec2().to_array() }));
        Some([(size.x as u32).div_ceil(16), (size.y as u32).div_ceil(16), 1])
    }
}

fn bindings<'a>(window: &'a ActiveWindow, canvas: &'a Canvas, maps: [&'a wgpu::Buffer; 5], region: &'a wgpu::Buffer) -> [(u32, Binding<'a>); 10] {
    [
        (0, Binding::Uniform(&window.uniform)),
        (1, Binding::ReadOnly(&canvas.paint)),
        (2, Binding::ReadOnly(&window.volume)),
        (3, Binding::ReadWrite(maps[0])),
        (4, Binding::ReadOnly(&canvas.ground)),
        (5, Binding::ReadWrite(maps[1])),
        (6, Binding::ReadWrite(maps[2])),
        (7, Binding::ReadWrite(maps[3])),
        (8, Binding::Uniform(region)),
        (9, Binding::ReadWrite(maps[4])),
    ]
}
