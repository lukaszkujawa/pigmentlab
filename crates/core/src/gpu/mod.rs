mod compute;
mod readback;

pub use compute::{Binding, ComputePass};
pub use readback::{read_buffer, read_rgba8};

#[derive(Clone)]
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn shared(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self { device: device.clone(), queue: queue.clone() }
    }

    pub fn headless() -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .expect("no compatible GPU adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("simpaint headless"),
            required_limits: required_limits(),
            ..Default::default()
        }))
        .expect("failed to create GPU device");
        Self { device, queue }
    }

    pub fn shader(&self, label: &str, sources: &[&str]) -> wgpu::ShaderModule {
        let source = std::iter::once(shared_constants()).chain(sources.iter().map(|s| s.to_string())).collect::<Vec<_>>().join("\n");
        self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) })
    }

    pub fn storage_buffer(&self, label: &str, size: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    }

    pub fn indirect_buffer(&self, label: &str, size: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub fn uniform_buffer(&self, label: &str, size: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
}

pub fn required_limits() -> wgpu::Limits {
    wgpu::Limits { max_storage_buffers_per_shader_stage: 16, ..wgpu::Limits::default() }
}

pub fn buffer_layout_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    ty: wgpu::BufferBindingType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

pub fn buffer_binding(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
}

fn shared_constants() -> String {
    format!(
        "const PIXELS_PER_CM: f32 = {:?};\nconst PAINT_SCALE: f32 = {:?};\nconst CHANNELS: u32 = {}u;\nconst FULL_CELL: f32 = {:?};\nconst BRISTLE_VERTICES: u32 = {}u;\nconst OPTICAL_THICKNESS_PER_PIXEL: f32 = {:?};\nconst MEDIUM_THINNING: f32 = {:?};",
        crate::units::PIXELS_PER_CM,
        crate::canvas::PAINT_SCALE,
        crate::canvas::PAINT_CHANNELS,
        crate::canvas::FULL_CELL,
        crate::brush::VERTICES_PER_BRISTLE,
        crate::pigment::OPTICAL_THICKNESS_PER_PIXEL,
        crate::paint::MEDIUM_THINNING,
    )
}
