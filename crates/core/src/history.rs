use crate::gpu::Gpu;

pub struct History {
    snapshots: Vec<wgpu::Buffer>,
    next: usize,
    stored: usize,
    capacity: usize,
}

impl History {
    pub fn new(gpu: &Gpu, store_size: u64, capacity: usize) -> Self {
        let snapshots = (0..capacity)
            .map(|index| {
                gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("history snapshot {index}")),
                    size: store_size,
                    usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Self { snapshots, next: 0, stored: 0, capacity }
    }

    pub fn push(&mut self, encoder: &mut wgpu::CommandEncoder, store: &wgpu::Buffer) {
        let slot = &self.snapshots[self.next];
        encoder.copy_buffer_to_buffer(store, 0, slot, 0, store.size());
        self.next = (self.next + 1) % self.capacity;
        self.stored = (self.stored + 1).min(self.capacity);
    }

    pub fn pop(&mut self, encoder: &mut wgpu::CommandEncoder, store: &wgpu::Buffer) -> bool {
        if self.stored == 0 {
            return false;
        }
        self.next = (self.next + self.capacity - 1) % self.capacity;
        self.stored -= 1;
        let slot = &self.snapshots[self.next];
        encoder.copy_buffer_to_buffer(slot, 0, store, 0, store.size());
        true
    }

    pub fn len(&self) -> usize {
        self.stored
    }

    pub fn is_empty(&self) -> bool {
        self.stored == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_only_the_latest_snapshots() {
        let gpu = Gpu::headless();
        let store = gpu.storage_buffer("store", 64);
        let mut history = History::new(&gpu, 64, 3);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        for _ in 0..5 {
            history.push(&mut encoder, &store);
        }
        assert_eq!(history.len(), 3);
        for _ in 0..3 {
            assert!(history.pop(&mut encoder, &store));
        }
        assert!(!history.pop(&mut encoder, &store));
        assert!(history.is_empty());
    }
}
