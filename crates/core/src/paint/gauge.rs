use crate::gpu::Gpu;
use crate::pigment::Coefficients;
use glam::Vec3;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const CHANNELS: usize = 7;
const IDLE: u8 = 0;
const COPIED: u8 = 1;
const MAPPING: u8 = 2;
const READY: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushContents {
    pub fill: f32,
    pub paint: Coefficients,
}

impl BrushContents {
    pub fn empty() -> Self {
        Self { fill: 0.0, paint: Coefficients { absorption: Vec3::ZERO, scattering: Vec3::ZERO } }
    }
}

pub struct Gauge {
    source: wgpu::Buffer,
    offset: u64,
    scale: f32,
    capacity: f32,
    staging: wgpu::Buffer,
    state: Arc<AtomicU8>,
    stale: AtomicBool,
}

impl Gauge {
    pub fn new(gpu: &Gpu, source: &wgpu::Buffer, offset: u64, scale: f32, capacity: f32) -> Self {
        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("brush gauge"),
            size: (CHANNELS * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self { source: source.clone(), offset, scale, capacity, staging, state: Arc::new(AtomicU8::new(IDLE)), stale: AtomicBool::new(false) }
    }

    pub fn invalidate(&self) {
        self.stale.store(self.state.load(Ordering::Acquire) != IDLE, Ordering::Release);
    }

    pub fn encode_copy(&self, encoder: &mut wgpu::CommandEncoder) -> bool {
        let idle = self.state.compare_exchange(IDLE, COPIED, Ordering::AcqRel, Ordering::Acquire).is_ok();
        if idle {
            encoder.copy_buffer_to_buffer(&self.source, self.offset, &self.staging, 0, self.staging.size());
        }
        idle
    }

    pub fn after_submit(&self) {
        if self.state.compare_exchange(COPIED, MAPPING, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            let state = self.state.clone();
            self.staging.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                state.store(if result.is_ok() { READY } else { IDLE }, Ordering::Release);
            });
        }
    }

    pub fn take(&self) -> Option<BrushContents> {
        if self.state.load(Ordering::Acquire) != READY {
            return None;
        }
        let data = self.staging.slice(..).get_mapped_range().ok()?;
        let values: [i32; CHANNELS] = bytemuck::pod_read_unaligned(&data);
        drop(data);
        self.staging.unmap();
        self.state.store(IDLE, Ordering::Release);
        if self.stale.swap(false, Ordering::AcqRel) {
            return None;
        }
        let [load, ka, kb, kc, sa, sb, sc] = values.map(|v| v as f32 / self.scale);
        let mass = load.max(1e-6);
        Some(BrushContents {
            fill: (load / self.capacity).clamp(0.0, 1.0),
            paint: Coefficients { absorption: Vec3::new(ka, kb, kc) / mass, scattering: Vec3::new(sa, sb, sc) / mass },
        })
    }
}
