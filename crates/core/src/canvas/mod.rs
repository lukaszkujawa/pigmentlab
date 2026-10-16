mod substrate;

use crate::gpu::{Binding, ComputePass, Gpu};

pub const PAINT_CHANNELS: u64 = 8;
pub const PAINT_SCALE: f32 = 65536.0;
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
    _padding: f32,
}

pub struct Canvas {
    pub size: CanvasSize,
    pub substrate: wgpu::Texture,
    pub paint: wgpu::Buffer,
    pub dry: wgpu::Buffer,
    pub relief: wgpu::Buffer,
    pub ground: wgpu::Buffer,
    bake_dry: ComputePass,
    refresh_ground: ComputePass,
    relief_pattern: Vec<f32>,
    tooth: f32,
}

impl Canvas {
    pub fn new(gpu: &Gpu, size: CanvasSize, seed: u64) -> Self {
        let substrate = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("substrate"),
            size: size.extent(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let pixels = substrate::paper(size, seed);
        gpu.queue.write_texture(
            substrate.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 4),
                rows_per_image: Some(size.height),
            },
            size.extent(),
        );
        let paint = gpu.storage_buffer("canvas paint", size.pixel_count() * PAINT_CHANNELS * 4);
        let dry = gpu.storage_buffer("canvas dry paint", size.pixel_count() * PAINT_CHANNELS * 4);
        let relief = gpu.storage_buffer("canvas relief", size.pixel_count() * 4);
        let ground = gpu.storage_buffer("canvas ground", size.pixel_count() * 4);
        let uniform = gpu.uniform_buffer("canvas uniform", std::mem::size_of::<CanvasUniform>() as u64);
        gpu.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&CanvasUniform { width: size.width, height: size.height, shrink: DRY_SHRINK, _padding: 0.0 }));
        let shader = gpu.shader("canvas", &[include_str!("../../shaders/units.wgsl"), include_str!("../../shaders/canvas.wgsl")]);
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
            substrate,
            paint,
            dry,
            relief,
            ground,
            bake_dry,
            refresh_ground,
            relief_pattern: substrate::relief(size, seed),
            tooth: DEFAULT_TOOTH,
        };
        canvas.set_tooth(gpu, DEFAULT_TOOTH);
        canvas
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

    pub fn encode_clear(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.paint, 0, None);
        encoder.clear_buffer(&self.dry, 0, None);
        self.encode_refresh_ground(encoder);
    }

    pub fn encode_refresh_ground_after_undo(&self, encoder: &mut wgpu::CommandEncoder) {
        self.encode_refresh_ground(encoder);
    }

    fn encode_refresh_ground(&self, encoder: &mut wgpu::CommandEncoder) {
        self.refresh_ground.dispatch(encoder, self.groups());
    }

    fn groups(&self) -> [u32; 3] {
        [self.size.width.div_ceil(16), self.size.height.div_ceil(16), 1]
    }
}
