mod substrate;

use crate::gpu::Gpu;

pub const PAINT_CHANNELS: u64 = 8;
pub const PAINT_SCALE: f32 = 65536.0;

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

pub struct Canvas {
    pub size: CanvasSize,
    pub substrate: wgpu::Texture,
    pub paint: wgpu::Buffer,
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
        Self { size, substrate, paint }
    }
}
