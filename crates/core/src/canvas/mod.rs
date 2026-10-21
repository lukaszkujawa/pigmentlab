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
pub const PALETTE_WIDTH: u32 = crate::units::cm(6.0) as u32;
pub const PALETTE_GAP: u32 = crate::units::cm(0.4) as u32;
const PALETTE_COLOUR: [f32; 3] = [0.5, 0.48, 0.45];

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CanvasSize {
    pub width: u32,
    pub height: u32,
}

impl CanvasSize {
    pub fn extent(self) -> wgpu::Extent3d {
        wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 }
    }

    pub fn with_palette(self) -> Self {
        Self { width: self.width + PALETTE_WIDTH, ..self }
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
    painting_width: u32,
}

pub struct Canvas {
    pub size: CanvasSize,
    pub paint: wgpu::Buffer,
    pub dry: wgpu::Buffer,
    pub relief: wgpu::Buffer,
    pub ground: wgpu::Buffer,
    bake_dry: ComputePass,
    refresh_ground: ComputePass,
    clear_painting: ComputePass,
    clear_palette: ComputePass,
    painting_width: u32,
    relief_pattern: Vec<f32>,
    surface: Surface,
    ground_colour: Option<[f32; 3]>,
    tooth: f32,
    seed: u64,
}

impl Canvas {
    pub fn new(gpu: &Gpu, size: CanvasSize, seed: u64) -> Self {
        Self::spanning(gpu, size, size, seed)
    }

    pub fn with_palette(gpu: &Gpu, painting: CanvasSize, seed: u64) -> Self {
        Self::spanning(gpu, painting, painting.with_palette(), seed)
    }

    fn spanning(gpu: &Gpu, painting: CanvasSize, size: CanvasSize, seed: u64) -> Self {
        let paint = gpu.storage_buffer("canvas paint", size.pixel_count() * PAINT_CHANNELS * 4);
        let dry = gpu.storage_buffer("canvas dry layer", size.pixel_count() * 16);
        let relief = gpu.storage_buffer("canvas relief", size.pixel_count() * 4);
        let ground = gpu.storage_buffer("canvas ground", size.pixel_count() * 4);
        let uniform = gpu.uniform_buffer("canvas uniform", std::mem::size_of::<CanvasUniform>() as u64);
        gpu.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&CanvasUniform { width: size.width, height: size.height, shrink: DRY_SHRINK, painting_width: painting.width }));
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
        let clear_painting = ComputePass::new(gpu, "clear painting", &shader, "clear_painting", &bindings);
        let clear_palette = ComputePass::new(gpu, "clear palette", &shader, "clear_palette", &bindings);
        let mut canvas = Self {
            size,
            paint,
            dry,
            relief,
            ground,
            bake_dry,
            refresh_ground,
            clear_painting,
            clear_palette,
            painting_width: painting.width,
            relief_pattern: Vec::new(),
            surface: Surface::Linen,
            ground_colour: None,
            tooth: DEFAULT_TOOTH,
            seed,
        };
        canvas.relief_pattern = canvas.relief_of(Surface::Linen);
        canvas.write_paper(gpu);
        canvas.set_tooth(gpu, DEFAULT_TOOTH);
        canvas
    }

    pub fn painting_size(&self) -> CanvasSize {
        CanvasSize { width: self.painting_width, height: self.size.height }
    }

    pub fn palette_start(&self) -> u32 {
        if self.painting_width < self.size.width { self.painting_width + PALETTE_GAP } else { self.size.width }
    }

    pub fn on_palette(&self, index: usize) -> bool {
        index as u32 % self.size.width >= self.painting_width
    }

    fn relief_of(&self, surface: Surface) -> Vec<f32> {
        let mut relief = surface.relief(self.size, self.seed);
        for (index, height) in relief.iter_mut().enumerate() {
            if self.on_palette(index) {
                *height = 0.0;
            }
        }
        relief
    }

    pub fn surface(&self) -> Surface {
        self.surface
    }

    pub fn set_surface(&mut self, gpu: &Gpu, surface: Surface) {
        self.surface = surface;
        self.relief_pattern = self.relief_of(surface);
        gpu.queue.write_buffer(&self.paint, 0, &vec![0u8; self.paint.size() as usize]);
        self.write_paper(gpu);
        self.set_tooth(gpu, surface.tooth());
    }

    pub fn set_ground_colour(&mut self, colour: Option<[f32; 3]>) {
        self.ground_colour = colour;
    }

    pub fn paper_heights(&self) -> &[f32] {
        &self.relief_pattern
    }

    fn write_paper(&self, gpu: &Gpu) {
        let mut layer = self.surface.colour_layer(self.ground_colour.unwrap_or(self.surface.colour()), &self.relief_pattern, self.size, self.seed);
        for (index, texel) in layer.iter_mut().enumerate() {
            if self.on_palette(index) {
                *texel = [0.0, PALETTE_COLOUR[0], PALETTE_COLOUR[1], PALETTE_COLOUR[2]];
            }
        }
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
        self.clear_painting.dispatch(encoder, self.groups());
        self.write_paper(gpu);
        self.encode_refresh_ground(encoder);
    }

    pub fn encode_clean_palette(&self, encoder: &mut wgpu::CommandEncoder) {
        self.clear_palette.dispatch(encoder, self.groups());
    }

    pub fn encode_refresh_ground(&self, encoder: &mut wgpu::CommandEncoder) {
        self.refresh_ground.dispatch(encoder, self.groups());
    }

    fn groups(&self) -> [u32; 3] {
        [self.size.width.div_ceil(16), self.size.height.div_ceil(16), 1]
    }
}
