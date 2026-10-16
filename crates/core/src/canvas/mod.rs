mod linen;
mod noise;
mod paper;
mod surface;

pub use surface::{Medium, PaperProperties, Surface};

use crate::gpu::{Binding, ComputePass, Gpu};

pub const PAINT_CHANNELS: u64 = 8;
pub const PAINT_SCALE: f32 = 65536.0;
pub const FULL_CELL: f32 = 0.999;
pub const DEFAULT_TOOTH: f32 = crate::units::cm(0.00625);
pub const DRY_SHRINK: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CanvasSize {
    pub width: u32,
    pub height: u32,
}

impl CanvasSize {
    pub fn extent(self) -> wgpu::Extent3d {
        wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 }
    }

    pub fn pixel_count(self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CanvasUniform {
    width: u32,
    height: u32,
    shrink: f32,
    padding: f32,
}

pub struct Canvas {
    pub size: CanvasSize,
    pub paint: wgpu::Buffer,
    pub dry: wgpu::Buffer,
    pub relief: wgpu::Buffer,
    pub ground: wgpu::Buffer,
    bake_dry: ComputePass,
    refresh_ground: ComputePass,
    relief_pattern: Vec<f32>,
    surface: Surface,
    tooth: f32,
    seed: u64,
}

impl Canvas {
    pub fn new(gpu: &Gpu, size: CanvasSize, seed: u64) -> Self {
        let paint = gpu.storage_buffer("canvas paint", size.pixel_count() * PAINT_CHANNELS * 4);
        let dry = gpu.storage_buffer("canvas dry layer", size.pixel_count() * 16);
        let relief = gpu.storage_buffer("canvas relief", size.pixel_count() * 4);
        let ground = gpu.storage_buffer("canvas ground", size.pixel_count() * 4);
        let uniform = gpu.uniform_buffer("canvas uniform", std::mem::size_of::<CanvasUniform>() as u64);
        gpu.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&CanvasUniform { width: size.width, height: size.height, shrink: DRY_SHRINK, padding: 0.0 }));
        let shader = gpu.shader("canvas", &[ include_str!("../../shaders/km.wgsl"), include_str!("../../shaders/canvas.wgsl")]);
        let bindings = [
            (0, Binding::Uniform(&uniform)),
            (1, Binding::ReadWrite(&paint)),
            (2, Binding::ReadWrite(&dry)),
            (3, Binding::ReadOnly(&relief)),
            (4, Binding::ReadWrite(&ground)),
        ];
        let bake_dry = ComputePass::new(gpu, "bake dry layer", &shader, "bake_dry", &bindings);
        let refresh_ground = ComputePass::new(gpu, "refresh ground", &shader, "refresh_ground", &bindings);
        let mut canvas = Self {
            size,
            paint,
            dry,
            relief,
            ground,
            bake_dry,
            refresh_ground,
            relief_pattern: Surface::Linen.relief(size, seed),
            surface: Surface::Linen,
            tooth: DEFAULT_TOOTH,
            seed,
        };
        canvas.write_paper(gpu);
        canvas.set_tooth(gpu, DEFAULT_TOOTH);
        canvas
    }

    pub fn surface(&self) -> Surface {
        self.surface
    }

    pub fn set_surface(&mut self, gpu: &Gpu, surface: Surface) {
        self.surface = surface;
        self.relief_pattern = surface.relief(self.size, self.seed);
        gpu.queue.write_buffer(&self.paint, 0, &vec![0u8; self.paint.size() as usize]);
        self.write_paper(gpu);
        self.set_tooth(gpu, surface.tooth());
    }

    pub fn paper_heights(&self) -> &[f32] {
        &self.relief_pattern
    }

    fn write_paper(&self, gpu: &Gpu) {
        let layer = self.surface.colour_layer(&self.relief_pattern, self.size, self.seed);
        gpu.queue.write_buffer(&self.dry, 0, bytemuck::cast_slice(&layer));
    }

    pub fn tooth(&self) -> f32 {
        self.tooth
    }

    pub fn relief_heights(&self) -> Vec<f32> {
        self.relief_pattern.iter().map(|r| r * self.tooth).collect()
    }

    pub fn set_tooth(&mut self, gpu: &Gpu, tooth: f32) {
        self.tooth = tooth.max(0.0);
        gpu.queue.write_buffer(&self.relief, 0, bytemuck::cast_slice(&self.relief_heights()));
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("refresh ground") });
        self.encode_refresh_ground(&mut encoder);
        gpu.queue.submit(Some(encoder.finish()));
    }

    pub fn encode_dry(&self, encoder: &mut wgpu::CommandEncoder) {
        self.bake_dry.dispatch(encoder, self.groups());
        self.encode_refresh_ground(encoder);
    }

    pub fn clear(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.paint, 0, None);
        self.write_paper(gpu);
        self.encode_refresh_ground(encoder);
    }

    pub fn encode_refresh_ground(&self, encoder: &mut wgpu::CommandEncoder) {
        self.refresh_ground.dispatch(encoder, self.groups());
    }

    fn groups(&self) -> [u32; 3] {
        [self.size.width.div_ceil(16), self.size.height.div_ceil(16), 1]
    }
}
