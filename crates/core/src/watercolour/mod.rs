mod brush;
mod tube;

pub use brush::WaterBrush;
pub use tube::WaterTube;

use crate::canvas::{Canvas, Surface};
use crate::gpu::{Binding, ComputePass, Gpu};
use crate::history::History;
use crate::paint::WetBounds;
use glam::{IVec2, UVec2};

pub const CELL_PIXELS: u32 = 2;
pub const ACCUMULATORS: u64 = 16;
pub const PIGMENT_BYTES: u64 = 64;
const REGION_MARGIN_CELLS: i32 = 24;
const UNDO_DEPTH: usize = 4;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WaterUniform {
    cells: [u32; 2],
    canvas: [u32; 2],
    region_origin: [i32; 2],
    region_size: [u32; 2],
    dt: f32,
    absorbency: f32,
    granulation: f32,
    tooth: f32,
    force_dry: u32,
    scrub: f32,
    _padding: [f32; 2],
}

pub struct Watercolour {
    pub cells: UVec2,
    pub uniform: wgpu::Buffer,
    pub paper: wgpu::Buffer,
    pub water: wgpu::Buffer,
    pub suspended: wgpu::Buffer,
    pub deposited: wgpu::Buffer,
    pub accumulators: wgpu::Buffer,
    pub touched: wgpu::Buffer,
    pub concentration: wgpu::Buffer,
    velocity_u: wgpu::Buffer,
    velocity_v: wgpu::Buffer,
    next_u: wgpu::Buffer,
    next_v: wgpu::Buffer,
    base: WaterUniform,
    region: Option<WetBounds>,
    apply_brush: ComputePass,
    update_u: ComputePass,
    update_v: ComputePass,
    limit_outflow: ComputePass,
    advect: ComputePass,
    transfer: ComputePass,
    capillary: ComputePass,
    mark_drying: ComputePass,
    dry_cells: ComputePass,
    bake_pixels: ComputePass,
    history: [History; 3],
}

impl Watercolour {
    pub fn new(gpu: &Gpu, canvas: &Canvas, dt: f32, scrub: f32) -> Self {
        let cells = UVec2::new(canvas.size.width.div_ceil(CELL_PIXELS), canvas.size.height.div_ceil(CELL_PIXELS));
        let count = u64::from(cells.x * cells.y);
        let paper = gpu.storage_buffer("watercolour paper", count * 16);
        let water = gpu.storage_buffer("watercolour water", count * 16);
        let water_next = gpu.storage_buffer("watercolour water next", count * 16);
        let velocity_u = gpu.storage_buffer("watercolour velocity u", u64::from((cells.x + 1) * cells.y) * 4);
        let velocity_v = gpu.storage_buffer("watercolour velocity v", u64::from(cells.x * (cells.y + 1)) * 4);
        let next_u = gpu.storage_buffer("watercolour next u", velocity_u.size());
        let next_v = gpu.storage_buffer("watercolour next v", velocity_v.size());
        let suspended = gpu.storage_buffer("watercolour suspended pigment", count * PIGMENT_BYTES);
        let suspended_next = gpu.storage_buffer("watercolour suspended next", count * PIGMENT_BYTES);
        let deposited = gpu.storage_buffer("watercolour deposited pigment", count * PIGMENT_BYTES);
        let impulse = gpu.storage_buffer("watercolour impulse", count * 16);
        let accumulators = gpu.storage_buffer("watercolour brush accumulators", count * ACCUMULATORS * 4);
        let touched = gpu.storage_buffer("watercolour hair contact", canvas.size.pixel_count() * 4);
        let concentration = gpu.storage_buffer("watercolour pigment per wetted pixel", count * 4);
        let uniform = gpu.uniform_buffer("watercolour uniform", std::mem::size_of::<WaterUniform>() as u64);
        let shader = gpu.shader(
            "watercolour",
            &[include_str!("../../shaders/km.wgsl"), include_str!("../../shaders/wash_outline.wgsl"), include_str!("../../shaders/wash_bridge.wgsl"), include_str!("../../shaders/wash_pigment.wgsl"), include_str!("../../shaders/wash_accumulators.wgsl"), include_str!("../../shaders/watercolour.wgsl")],
        );
        let bindings = [
            (0, Binding::Uniform(&uniform)),
            (1, Binding::ReadOnly(&paper)),
            (2, Binding::ReadWrite(&water)),
            (3, Binding::ReadWrite(&water_next)),
            (4, Binding::ReadWrite(&velocity_u)),
            (5, Binding::ReadWrite(&velocity_v)),
            (6, Binding::ReadWrite(&next_u)),
            (7, Binding::ReadWrite(&next_v)),
            (8, Binding::ReadWrite(&suspended)),
            (9, Binding::ReadWrite(&suspended_next)),
            (10, Binding::ReadWrite(&deposited)),
            (11, Binding::ReadWrite(&impulse)),
            (12, Binding::ReadWrite(&accumulators)),
            (13, Binding::ReadWrite(&canvas.dry)),
            (14, Binding::ReadOnly(&canvas.relief)),
            (15, Binding::ReadWrite(&touched)),
            (16, Binding::ReadWrite(&concentration)),
        ];
        let pass = |label, entry| ComputePass::new(gpu, label, &shader, entry, &bindings);
        let passes = [
            pass("watercolour apply brush", "apply_brush"),
            pass("watercolour update u", "update_u"),
            pass("watercolour update v", "update_v"),
            pass("watercolour limit outflow", "limit_outflow"),
            pass("watercolour advect", "advect"),
            pass("watercolour transfer", "transfer"),
            pass("watercolour capillary", "capillary"),
            pass("watercolour mark drying", "mark_drying"),
            pass("watercolour dry cells", "dry_cells"),
            pass("watercolour bake pixels", "bake_pixels"),
        ];
        let [apply_brush, update_u, update_v, limit_outflow, advect, transfer, capillary, mark_drying, dry_cells, bake_pixels] = passes;
        let history = [&water, &suspended, &deposited].map(|buffer| History::new(gpu, buffer.size(), UNDO_DEPTH));
        let base = WaterUniform {
            cells: cells.to_array(),
            canvas: [canvas.size.width, canvas.size.height],
            region_origin: [0; 2],
            region_size: [0; 2],
            dt,
            absorbency: 0.0,
            granulation: 0.0,
            tooth: 1.0,
            force_dry: 0,
            scrub,
            _padding: [0.0; 2],
        };
        let mut watercolour = Self {
            cells,
            uniform,
            paper,
            water,
            suspended,
            deposited,
            accumulators,
            touched,
            concentration,
            velocity_u,
            velocity_v,
            next_u,
            next_v,
            base,
            region: None,
            apply_brush,
            update_u,
            update_v,
            limit_outflow,
            advect,
            transfer,
            capillary,
            mark_drying,
            dry_cells,
            bake_pixels,
            history,
        };
        watercolour.set_paper(gpu, canvas);
        watercolour
    }

    pub fn set_paper(&mut self, gpu: &Gpu, canvas: &Canvas) {
        let surface = canvas.surface();
        let properties = surface.paper();
        self.base.absorbency = properties.absorbency;
        self.base.granulation = properties.granulation;
        self.base.tooth = surface.tooth().max(1e-6);
        gpu.queue.write_buffer(&self.paper, 0, bytemuck::cast_slice(&paper_cells(canvas, surface, self.cells)));
        self.clear(gpu);
    }

    pub fn set_tooth(&self, gpu: &Gpu, canvas: &Canvas) {
        gpu.queue.write_buffer(&self.paper, 0, bytemuck::cast_slice(&paper_cells(canvas, canvas.surface(), self.cells)));
    }

    pub fn clear(&mut self, gpu: &Gpu) {
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("clear watercolour") });
        for buffer in [&self.water, &self.suspended, &self.deposited, &self.velocity_u, &self.velocity_v, &self.next_u, &self.next_v, &self.accumulators, &self.touched, &self.concentration] {
            encoder.clear_buffer(buffer, 0, None);
        }
        gpu.queue.submit(Some(encoder.finish()));
        self.region = None;
        for history in &mut self.history {
            history.forget();
        }
    }

    pub fn wet(&mut self, pixels: WetBounds) {
        let cells = WetBounds { min: pixels.min / CELL_PIXELS as i32, max: pixels.max / CELL_PIXELS as i32 + IVec2::ONE }.expand(REGION_MARGIN_CELLS);
        self.region = Some(self.region.map_or(cells, |region| region.include(cells)));
    }

    pub fn is_wet(&self) -> bool {
        self.region.is_some()
    }

    pub fn encode_step(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, force_dry: bool) {
        let Some(region) = self.region else { return };
        let (origin, extent) = region.clamped(self.cells.x, self.cells.y);
        if extent.x <= 0 || extent.y <= 0 {
            return;
        }
        let uniform = WaterUniform {
            region_origin: origin.to_array(),
            region_size: [extent.x as u32, extent.y as u32],
            force_dry: u32::from(force_dry),
            ..self.base
        };
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        let cells = [(extent.x as u32).div_ceil(16), (extent.y as u32).div_ceil(16), 1];
        let faces = [(extent.x as u32 + 1).div_ceil(16), (extent.y as u32 + 1).div_ceil(16), 1];
        self.apply_brush.dispatch(encoder, cells);
        self.update_u.dispatch(encoder, faces);
        self.update_v.dispatch(encoder, faces);
        encoder.copy_buffer_to_buffer(&self.next_u, 0, &self.velocity_u, 0, self.velocity_u.size());
        encoder.copy_buffer_to_buffer(&self.next_v, 0, &self.velocity_v, 0, self.velocity_v.size());
        self.limit_outflow.dispatch(encoder, cells);
        self.advect.dispatch(encoder, cells);
        self.transfer.dispatch(encoder, cells);
        self.capillary.dispatch(encoder, cells);
        self.mark_drying.dispatch(encoder, cells);
        self.dry_cells.dispatch(encoder, cells);
        self.bake_pixels.dispatch(encoder, [(extent.x as u32 * 2).div_ceil(16), (extent.y as u32 * 2).div_ceil(16), 1]);
    }

    pub fn settle(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.region = None;
        encoder.clear_buffer(&self.touched, 0, None);
        encoder.clear_buffer(&self.concentration, 0, None);
    }

    pub fn snapshot(&mut self, encoder: &mut wgpu::CommandEncoder) {
        for (history, buffer) in self.history.iter_mut().zip([&self.water, &self.suspended, &self.deposited]) {
            history.push(encoder, buffer);
        }
    }

    pub fn undo(&mut self, encoder: &mut wgpu::CommandEncoder) -> bool {
        let mut restored = true;
        for (history, buffer) in self.history.iter_mut().zip([&self.water, &self.suspended, &self.deposited]) {
            restored &= history.pop(encoder, buffer);
        }
        encoder.clear_buffer(&self.velocity_u, 0, None);
        encoder.clear_buffer(&self.velocity_v, 0, None);
        if restored {
            self.region = Some(WetBounds { min: IVec2::ZERO, max: self.cells.as_ivec2() });
        }
        restored
    }
}

fn paper_cells(canvas: &Canvas, surface: Surface, cells: UVec2) -> Vec<[f32; 4]> {
    let reference = surface.tooth().max(1e-6);
    let heights: Vec<f32> = canvas.relief_heights().iter().map(|h| h / reference).collect();
    let (width, height) = (canvas.size.width, canvas.size.height);
    let (low, high) = surface.paper().capacity;
    let mut out = Vec::with_capacity((cells.x * cells.y) as usize);
    for cy in 0..cells.y {
        for cx in 0..cells.x {
            let mut total = 0.0;
            let mut count = 0.0;
            for k in 0..CELL_PIXELS * CELL_PIXELS {
                let (x, y) = (cx * CELL_PIXELS + k % CELL_PIXELS, cy * CELL_PIXELS + k / CELL_PIXELS);
                if x < width && y < height {
                    total += heights[(y * width + x) as usize] * 0.5 + 0.5;
                    count += 1.0;
                }
            }
            let mean = total / count;
            out.push([mean, mean * (high - low) + low, 0.0, 0.0]);
        }
    }
    out
}
