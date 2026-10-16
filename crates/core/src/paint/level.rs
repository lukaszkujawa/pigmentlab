use super::window::{ActiveWindow, WINDOW_SIZE};
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::render::HeightMap;

pub struct Levelling {
    snapshot: ComputePass,
    window: ComputePass,
    store_even: ComputePass,
    store_odd: ComputePass,
    dry_volume: ComputePass,
    dry_store: ComputePass,
    canvas_groups: [u32; 3],
}

impl Levelling {
    pub fn new(gpu: &Gpu, canvas: &Canvas, window: &ActiveWindow, height_map: &HeightMap) -> Self {
        let columns = u64::from(WINDOW_SIZE.x * WINDOW_SIZE.y);
        let column_heights = gpu.storage_buffer("column snapshot", columns * 16);
        let column_pigment = gpu.storage_buffer("column pigment snapshot", columns * 16);
        let shader = gpu.shader(
            "level",
            &[
                include_str!("../../shaders/units.wgsl"),
                include_str!("../../shaders/window.wgsl"),
                include_str!("../../shaders/level.wgsl"),
            ],
        );
        let bindings = [
            (0, Binding::Uniform(&window.uniform)),
            (1, Binding::ReadWrite(&window.volume)),
            (2, Binding::ReadWrite(&canvas.paint)),
            (3, Binding::ReadOnly(&height_map.heights)),
            (4, Binding::ReadWrite(&column_heights)),
            (5, Binding::ReadWrite(&column_pigment)),
        ];
        let pass = |label, entry| ComputePass::new(gpu, label, &shader, entry, &bindings);
        Self {
            snapshot: pass("snapshot columns", "snapshot_columns"),
            window: pass("level window", "level_window"),
            store_even: pass("level store even", "level_store_even"),
            store_odd: pass("level store odd", "level_store_odd"),
            dry_volume: pass("dry volume", "dry_volume"),
            dry_store: pass("dry store", "dry_store"),
            canvas_groups: [canvas.size.width.div_ceil(16), canvas.size.height.div_ceil(16), 1],
        }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let window_groups = [WINDOW_SIZE.x.div_ceil(16), WINDOW_SIZE.y.div_ceil(16), 1];
        self.snapshot.dispatch(encoder, window_groups);
        self.window.dispatch(encoder, window_groups);
        let half_groups = [self.canvas_groups[0].div_ceil(2), self.canvas_groups[1], 1];
        self.store_even.dispatch(encoder, half_groups);
        self.store_odd.dispatch(encoder, half_groups);
    }

    pub fn encode_dry(&self, encoder: &mut wgpu::CommandEncoder) {
        self.dry_volume.dispatch_linear(encoder, WINDOW_SIZE.x * WINDOW_SIZE.y * WINDOW_SIZE.z, 64);
        self.dry_store.dispatch(encoder, self.canvas_groups);
    }
}
