use crate::brush::{Brush, BrushPose, BrushPreset};
use crate::canvas::{Canvas, CanvasSize, PAINT_CHANNELS, PAINT_SCALE};
use crate::gpu::{Gpu, read_buffer, read_rgba8};
use crate::history::History;
use crate::input::{PointerSample, PointerState, PointerTrack};
use crate::paint::{ActiveWindow, Deposit, Fluid, FluidSettings};
use crate::pigment::Pigment;
use crate::render::{BrushLines, Composite, DISPLAY_FORMAT, HeightMap, Lighting};
use glam::{Quat, Vec2, Vec3};

pub const SIMULATION_DT: f32 = 1.0 / 240.0;
const MAX_SUBSTEPS_PER_ADVANCE: u32 = 32;
const SUBSTEPS_PER_FLUID_STEP: u32 = 4;
const UNDO_DEPTH: usize = 12;
const FLUID_SETTLE_SECONDS: f64 = 0.5;
const FLUID: FluidSettings = FluidSettings {
    dt: SIMULATION_DT * SUBSTEPS_PER_FLUID_STEP as f32,
    damping: 0.55,
    viscosity: 400.0,
    dryness_rate: 0.0,
    gravity: 0.0,
    yield_speed: 60.0,
    particles: false,
};
const MAX_COMPRESSION: f32 = 0.4;
const LIFTED_HEIGHT: f32 = 1.25;
const MAX_TILT: f32 = 0.8;

pub struct Engine {
    gpu: Gpu,
    canvas: Canvas,
    brush: Brush,
    window: ActiveWindow,
    deposit: Deposit,
    fluid: Fluid,
    height_map: HeightMap,
    composite: Composite,
    brush_lines: BrushLines,
    display: wgpu::Texture,
    pointer: PointerTrack,
    pose: BrushPose,
    show_bristles: bool,
    time: f64,
    substeps: u64,
    pose_at_last_fluid_step: BrushPose,
    fluid_enabled: bool,
    fluid_active_until: f64,
    history: History,
    seed: u64,
}

impl Engine {
    pub fn new(gpu: Gpu, size: CanvasSize, seed: u64) -> Self {
        let canvas = Canvas::new(&gpu, size, seed);
        let window = ActiveWindow::new(&gpu, &canvas);
        let brush = Brush::new(&gpu, BrushPreset::default_round(), seed, &window);
        let fluid = Fluid::new(&gpu, &window, &brush, FLUID);
        let deposit = Deposit::new(&gpu, &brush, &window, &fluid);
        let height_map = HeightMap::new(&gpu, &canvas, &window, &fluid);
        let composite = Composite::new(&gpu, &canvas, &window, &fluid, &height_map);
        let brush_lines = BrushLines::new(&gpu, &brush);
        let display = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("display"),
            size: size.extent(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DISPLAY_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let history = History::new(&gpu, canvas.paint.size(), UNDO_DEPTH);
        let lifted = Vec3::new(size.width as f32 * 0.5, size.height as f32 * 0.5, brush.preset().length * LIFTED_HEIGHT);
        Self {
            gpu,
            canvas,
            brush,
            window,
            deposit,
            fluid,
            height_map,
            composite,
            brush_lines,
            display,
            pointer: PointerTrack::default(),
            pose: BrushPose::upright(lifted),
            show_bristles: false,
            time: 0.0,
            substeps: 0,
            pose_at_last_fluid_step: BrushPose::upright(lifted),
            fluid_enabled: true,
            fluid_active_until: 0.0,
            history,
            seed,
        }
    }

    pub fn canvas_size(&self) -> CanvasSize {
        self.canvas.size
    }

    pub fn brush(&self) -> &Brush {
        &self.brush
    }

    pub fn set_brush(&mut self, preset: BrushPreset) {
        self.fluid.submit_return_all_particles(&self.gpu);
        self.brush = Brush::new(&self.gpu, preset, self.seed, &self.window);
        self.fluid = Fluid::new(&self.gpu, &self.window, &self.brush, FLUID);
        self.deposit = Deposit::new(&self.gpu, &self.brush, &self.window, &self.fluid);
        self.height_map = HeightMap::new(&self.gpu, &self.canvas, &self.window, &self.fluid);
        self.composite = Composite::new(&self.gpu, &self.canvas, &self.window, &self.fluid, &self.height_map);
        self.brush_lines = BrushLines::new(&self.gpu, &self.brush);
        self.brush.teleport(&self.gpu, &self.pose);
    }

    pub fn clear(&mut self) {
        self.fluid.submit_return_all_particles(&self.gpu);
        let mut encoder = self.encoder("clear");
        self.window.encode_bake(&mut encoder);
        self.history.push(&mut encoder, &self.canvas.paint);
        encoder.clear_buffer(&self.canvas.paint, 0, None);
        self.window.encode_unbake(&mut encoder);
        self.fluid.clear_velocity(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
    }

    pub fn dry(&mut self) {
        self.fluid.submit_return_all_particles(&self.gpu);
        let mut encoder = self.encoder("dry");
        self.window.encode_bake(&mut encoder);
        self.history.push(&mut encoder, &self.canvas.paint);
        self.fluid.encode_dry_all(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
    }

    pub fn undo(&mut self) -> bool {
        self.fluid.submit_return_all_particles(&self.gpu);
        let mut encoder = self.encoder("undo");
        let restored = self.history.pop(&mut encoder, &self.canvas.paint);
        if restored {
            self.window.encode_unbake(&mut encoder);
            self.fluid.clear_velocity(&mut encoder);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        restored
    }

    pub fn save_png(&mut self, path: &std::path::Path) -> Result<(), image::ImageError> {
        self.render();
        self.read_back().save(path)
    }

    fn encoder(&self, label: &str) -> wgpu::CommandEncoder {
        self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
    }

    pub fn set_lighting(&self, lighting: Lighting) {
        self.composite.set_lighting(&self.gpu, lighting);
    }

    pub fn set_fluid_enabled(&mut self, enabled: bool) {
        self.fluid_enabled = enabled;
    }

    pub fn set_show_bristles(&mut self, show: bool) {
        self.show_bristles = show;
    }

    pub fn dip(&self, pigment: Pigment) {
        self.brush.dip(&self.gpu, pigment);
    }

    pub fn clean(&self) {
        self.brush.clean(&self.gpu);
    }

    pub fn brush_load(&self) -> f32 {
        read_buffer::<f32>(&self.gpu, &self.brush.sample_load).iter().map(|load| f64::from(*load)).sum::<f64>() as f32
    }

    pub fn paint_heights(&self) -> Vec<f32> {
        self.fluid.submit_return_all_particles(&self.gpu);
        let mut encoder = self.encoder("bake");
        self.window.encode_bake(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        let store = read_buffer::<i32>(&self.gpu, &self.canvas.paint);
        store.chunks(PAINT_CHANNELS as usize).map(|pixel| pixel[0] as f32 / PAINT_SCALE).collect()
    }

    pub fn paint_mass(&self) -> f32 {
        self.paint_heights().iter().map(|h| f64::from(*h)).sum::<f64>() as f32
    }

    pub fn pointer(&mut self, sample: PointerSample) {
        self.pointer.push(sample);
    }

    pub fn advance_realtime(&mut self, target_time: f64) {
        let dt = f64::from(SIMULATION_DT);
        let needed = ((target_time - self.time) / dt).floor().max(0.0) as u32;
        if needed > MAX_SUBSTEPS_PER_ADVANCE {
            self.time = target_time - dt * f64::from(MAX_SUBSTEPS_PER_ADVANCE);
        }
        self.advance_steps(needed.min(MAX_SUBSTEPS_PER_ADVANCE));
    }

    pub fn advance_exact(&mut self, target_time: f64) {
        let dt = f64::from(SIMULATION_DT);
        let needed = ((target_time - self.time) / dt).round().max(0.0) as u32;
        self.advance_steps(needed);
    }

    fn advance_steps(&mut self, steps: u32) {
        let dt = f64::from(SIMULATION_DT);
        let canvas = [self.canvas.size.width as f32, self.canvas.size.height as f32];
        for _ in 0..steps {
            self.time += dt;
            if let Some(state) = self.pointer.state_at(self.time) {
                self.pose = self.pose_for(state);
                if state.pressed {
                    self.fluid_active_until = self.time + FLUID_SETTLE_SECONDS;
                }
                if state.fresh_press {
                    self.brush.teleport(&self.gpu, &self.pose);
                    self.fluid.submit_return_all_particles(&self.gpu);
                    let mut encoder = self.encoder("stroke snapshot");
                    self.window.encode_bake(&mut encoder);
                    self.history.push(&mut encoder, &self.canvas.paint);
                    self.gpu.queue.submit(Some(encoder.finish()));
                }
            }
            let mut encoder = self
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("substep") });
            let focus = Vec2::new(self.pose.position.x, self.pose.position.y);
            if self.window.needs_relocation(focus) {
                self.fluid.submit_return_all_particles(&self.gpu);
                self.window.follow(&self.gpu, &mut encoder, &self.canvas, focus);
                self.fluid.clear_velocity(&mut encoder);
            }
            self.brush.encode_step(&self.gpu, &mut encoder, &self.pose, canvas);
            self.deposit.encode(&mut encoder);
            self.substeps += 1;
            let fluid_due = self.fluid_enabled && self.time <= self.fluid_active_until;
            if fluid_due && self.substeps.is_multiple_of(u64::from(SUBSTEPS_PER_FLUID_STEP)) {
                let speed = self.pose.position.distance(self.pose_at_last_fluid_step.position) / FLUID.dt;
                self.fluid.encode_step(&self.gpu, &mut encoder, &self.brush, speed);
                self.pose_at_last_fluid_step = self.pose;
            }
            self.gpu.queue.submit(Some(encoder.finish()));
        }
    }

    fn pose_for(&self, state: PointerState) -> BrushPose {
        let length = self.brush.preset().length;
        let along_handle = if state.pressed {
            length * (1.0 - MAX_COMPRESSION * state.pressure.clamp(0.0, 1.0))
        } else {
            length * LIFTED_HEIGHT
        };
        let handle = handle_direction(state.tilt);
        BrushPose {
            position: Vec3::new(state.x, state.y, 0.0) + handle * along_handle,
            orientation: Quat::from_rotation_arc(Vec3::Z, handle),
        }
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    pub fn render(&mut self) -> &wgpu::Texture {
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let view = self.display.create_view(&wgpu::TextureViewDescriptor::default());
        self.height_map.encode(&mut encoder);
        self.composite.encode(&mut encoder, &view);
        if self.show_bristles {
            self.brush_lines.encode(&mut encoder, &view);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        &self.display
    }

    pub fn read_back(&self) -> image::RgbaImage {
        read_rgba8(&self.gpu, &self.display)
    }
}

fn handle_direction(tilt: [f32; 2]) -> Vec3 {
    let lean = Vec2::new(tilt[0], -tilt[1]).clamp_length_max(MAX_TILT);
    Vec3::new(lean.x, lean.y, (1.0 - lean.length_squared()).max(0.05).sqrt()).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upright_pen_gives_vertical_handle() {
        assert!((handle_direction([0.0, 0.0]) - Vec3::Z).length() < 1e-6);
    }

    #[test]
    fn tilt_is_limited_and_unit_length() {
        let handle = handle_direction([1.0, 1.0]);
        assert!((handle.length() - 1.0).abs() < 1e-5);
        assert!(handle.z > 0.3, "handle should never lie flat, got {handle}");
    }
}
