use crate::brush::{Brush, BrushAdjustments, BrushPose, BrushPreset, preset_by_name};
use crate::canvas::{Canvas, CanvasSize, PAINT_CHANNELS, PAINT_SCALE};
use crate::gpu::{Gpu, read_buffer, read_rgba8};
use crate::history::History;
use crate::input::{PointerSample, PointerState, PointerTrack};
use crate::paint::{ActiveWindow, Deposit, Fluid, FluidSettings, StoreLevelling};
use crate::pigment::{Coefficients, Pigment};
use crate::render::{BrushLines, Composite, DISPLAY_FORMAT, HeightMap, Lighting};
use glam::{Quat, Vec2, Vec3};

pub const SIMULATION_DT: f32 = 1.0 / 240.0;
const MAX_SUBSTEPS_PER_ADVANCE: u32 = 32;
const SUBSTEPS_PER_FLUID_STEP: u32 = 8;
const MAX_BRUSH_TRAVEL_PER_FLUID_STEP: f32 = 16.0;
const MIN_SUBSTEPS_PER_FLUID_STEP: u32 = 4;
const UNDO_DEPTH: usize = 6;
const FLUID_SETTLE_SECONDS: f64 = 0.5;
const FLUID: FluidSettings = FluidSettings {
    dt: SIMULATION_DT * SUBSTEPS_PER_FLUID_STEP as f32,
    damping: 0.55,
    viscosity: 400.0,
    dryness_rate: 0.0,
    gravity: 0.0,
    yield_speed: crate::units::cm(1.5),
    particles: false,
};
const MAX_COMPRESSION: f32 = 0.4;
const LIFTED_HEIGHT: f32 = 1.25;
const MAX_TILT: f32 = 0.8;
const FOOTPRINT_SLACK: f32 = crate::units::cm(0.15);
const DILUTION_PER_MEDIUM: f32 = 0.9;

pub struct Engine {
    gpu: Gpu,
    canvas: Canvas,
    brush: Brush,
    window: ActiveWindow,
    deposit: Deposit,
    fluid: Fluid,
    height_map: HeightMap,
    store_levelling: StoreLevelling,
    composite: Composite,
    brush_lines: BrushLines,
    display: wgpu::Texture,
    pointer: PointerTrack,
    pose: BrushPose,
    show_bristles: bool,
    time: f64,
    substeps: u64,
    pose_at_last_fluid_step: BrushPose,
    substeps_since_fluid: u32,
    tuft_centre: Vec2,
    fluid_enabled: bool,
    fluid_active_until: f64,
    was_pressed: bool,
    history: History,
    seed: u64,
    base_preset: BrushPreset,
    adjustments: BrushAdjustments,
    brush_angle_degrees: f32,
    paint: PaintSettings,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintSettings {
    pub load: f32,
    pub medium: f32,
}

impl Default for PaintSettings {
    fn default() -> Self {
        Self { load: 1.0, medium: 0.0 }
    }
}

impl Engine {
    pub fn new(gpu: Gpu, size: CanvasSize, seed: u64) -> Self {
        let canvas = Canvas::new(&gpu, size, seed);
        let window = ActiveWindow::new(&gpu, &canvas);
        let brush = Brush::new(&gpu, BrushPreset::default_round(), seed, &window);
        let particle_volume = Fluid::particle_volume_buffer(&gpu, &window);
        let height_map = HeightMap::new(&gpu, &canvas, &window, &particle_volume);
        let fluid = Fluid::new(&gpu, &canvas, &window, &brush, particle_volume, &height_map.heights, FLUID);
        let deposit = Deposit::new(&gpu, &brush, &window, &fluid);
        let store_levelling = StoreLevelling::new(&gpu, &canvas, &window, &height_map);
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
            store_levelling,
            composite,
            brush_lines,
            display,
            pointer: PointerTrack::default(),
            pose: BrushPose::upright(lifted),
            show_bristles: false,
            time: 0.0,
            substeps: 0,
            pose_at_last_fluid_step: BrushPose::upright(lifted),
            substeps_since_fluid: 0,
            tuft_centre: Vec2::ZERO,
            fluid_enabled: true,
            fluid_active_until: 0.0,
            was_pressed: false,
            history,
            seed,
            base_preset: BrushPreset::default_round(),
            adjustments: BrushAdjustments::default(),
            brush_angle_degrees: 0.0,
            paint: PaintSettings::default(),
        }
    }

    pub fn canvas_size(&self) -> CanvasSize {
        self.canvas.size
    }

    pub fn brush(&self) -> &Brush {
        &self.brush
    }

    pub fn select_brush(&mut self, name: &str) -> bool {
        match preset_by_name(name) {
            Some(preset) => {
                self.base_preset = preset;
                self.rebuild_brush();
                true
            }
            None => false,
        }
    }

    pub fn set_brush_adjustments(&mut self, adjustments: BrushAdjustments) {
        if adjustments != self.adjustments {
            self.adjustments = adjustments;
            self.rebuild_brush();
        }
    }

    pub fn set_brush_angle(&mut self, degrees: f32) {
        self.brush_angle_degrees = degrees;
    }

    pub fn set_paint(&mut self, paint: PaintSettings) {
        self.paint = paint;
        self.deposit.set_medium(&self.gpu, paint.medium);
    }

    pub fn set_param(&mut self, name: &str, value: f32) -> bool {
        match name {
            "size" => self.set_brush_adjustments(BrushAdjustments { scale: value, ..self.adjustments }),
            "bristles" => self.set_brush_adjustments(BrushAdjustments { bristle_count: Some(value as u32), ..self.adjustments }),
            "stiffness" => self.set_brush_adjustments(BrushAdjustments { stiffness: value, ..self.adjustments }),
            "angle" => self.set_brush_angle(value),
            "load" => self.set_paint(PaintSettings { load: value, ..self.paint }),
            "medium" => self.set_paint(PaintSettings { medium: value, ..self.paint }),
            _ => return false,
        }
        true
    }

    fn rebuild_brush(&mut self) {
        self.set_brush(self.base_preset.adjusted(self.adjustments));
        self.deposit.set_medium(&self.gpu, self.paint.medium);
    }

    pub fn set_brush(&mut self, preset: BrushPreset) {
        self.fluid.submit_return_all_particles(&self.gpu);
        self.brush = Brush::new(&self.gpu, preset, self.seed, &self.window);
        let particle_volume = Fluid::particle_volume_buffer(&self.gpu, &self.window);
        self.height_map = HeightMap::new(&self.gpu, &self.canvas, &self.window, &particle_volume);
        self.fluid = Fluid::new(&self.gpu, &self.canvas, &self.window, &self.brush, particle_volume, &self.height_map.heights, FLUID);
        self.deposit = Deposit::new(&self.gpu, &self.brush, &self.window, &self.fluid);
        self.store_levelling = StoreLevelling::new(&self.gpu, &self.canvas, &self.window, &self.height_map);
        self.composite = Composite::new(&self.gpu, &self.canvas, &self.window, &self.fluid, &self.height_map);
        self.brush_lines = BrushLines::new(&self.gpu, &self.brush);
        self.brush.teleport(&self.gpu, &self.pose);
    }

    pub fn clear(&mut self) {
        self.fluid.submit_return_all_particles(&self.gpu);
        let mut encoder = self.encoder("clear");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.history.push(&mut encoder, &self.canvas.paint);
        encoder.clear_buffer(&self.canvas.paint, 0, None);
        self.window.encode_unbake(&self.gpu, &mut encoder, &self.canvas);
        self.fluid.clear_velocity(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
    }

    pub fn dry(&mut self) {
        self.fluid.submit_return_all_particles(&self.gpu);
        let mut encoder = self.encoder("dry");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.history.push(&mut encoder, &self.canvas.paint);
        self.fluid.encode_dry_all(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
    }

    pub fn undo(&mut self) -> bool {
        self.fluid.submit_return_all_particles(&self.gpu);
        let mut encoder = self.encoder("undo");
        let restored = self.history.pop(&mut encoder, &self.canvas.paint);
        if restored {
            self.window.encode_unbake(&self.gpu, &mut encoder, &self.canvas);
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
        let dilution = 1.0 - DILUTION_PER_MEDIUM * self.paint.medium.clamp(0.0, 1.0);
        let thinned = Pigment {
            name: pigment.name,
            coefficients: Coefficients {
                absorption: pigment.coefficients.absorption * dilution,
                scattering: pigment.coefficients.scattering * dilution,
            },
        };
        self.brush.dip(&self.gpu, thinned, self.paint.load.clamp(0.0, 1.0));
    }

    pub fn clean(&self) {
        self.brush.clean(&self.gpu);
    }

    pub fn brush_load(&self) -> f32 {
        read_buffer::<f32>(&self.gpu, &self.brush.sample_load).iter().map(|load| f64::from(*load)).sum::<f64>() as f32
    }

    pub fn paint_heights(&mut self) -> Vec<f32> {
        self.fluid.submit_return_all_particles(&self.gpu);
        let mut encoder = self.encoder("bake");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.gpu.queue.submit(Some(encoder.finish()));
        let store = read_buffer::<i32>(&self.gpu, &self.canvas.paint);
        store.chunks(PAINT_CHANNELS as usize).map(|pixel| pixel[0] as f32 / PAINT_SCALE).collect()
    }

    pub fn paint_mass(&mut self) -> f32 {
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
            let mut lifting = false;
            if let Some(state) = self.pointer.state_at(self.time) {
                lifting = self.was_pressed && !state.pressed;
                if !lifting {
                    self.pose = self.pose_for(state);
                }
                self.was_pressed = state.pressed;
                if state.pressed {
                    self.fluid_active_until = self.time + FLUID_SETTLE_SECONDS;
                }
                if state.fresh_press {
                    self.tuft_centre = self.handle_position();
                    self.brush.teleport(&self.gpu, &self.pose);
                    self.fluid.submit_return_all_particles(&self.gpu);
                    let mut encoder = self.encoder("stroke snapshot");
                    self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
                    self.history.push(&mut encoder, &self.canvas.paint);
                    self.height_map.encode(&mut encoder);
                    self.fluid.encode_capture_stroke_base(&mut encoder, &self.height_map.heights);
                    self.gpu.queue.submit(Some(encoder.finish()));
                }
            }
            let mut encoder = self
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("substep") });
            let preset = *self.brush.preset();
            let reach = ((preset.length - preset.spread()) * 0.5).max(0.0);
            let focus = self.tow_tuft_centre(reach);
            let footprint_radius = (preset.length + preset.spread()) * 0.5 + FOOTPRINT_SLACK;
            if self.window.needs_relocation(focus, footprint_radius) {
                self.fluid.submit_return_all_particles(&self.gpu);
                self.window.follow(&self.gpu, &mut encoder, &self.canvas, focus);
                self.fluid.clear_velocity(&mut encoder);
            }
            self.brush.encode_step(&self.gpu, &mut encoder, &self.pose, canvas);
            self.deposit.set_lifting(&self.gpu, lifting);
            self.deposit.encode(&mut encoder);
            self.substeps += 1;
            let fluid_due = self.fluid_enabled && self.time <= self.fluid_active_until;
            let speed = self.pose.position.distance(self.pose_at_last_fluid_step.position)
                / (SIMULATION_DT * self.substeps_since_fluid.max(1) as f32);
            let travel_per_substep = speed * SIMULATION_DT;
            let cadence = ((MAX_BRUSH_TRAVEL_PER_FLUID_STEP / travel_per_substep.max(1e-3)).floor() as u32)
                .clamp(MIN_SUBSTEPS_PER_FLUID_STEP, SUBSTEPS_PER_FLUID_STEP);
            self.substeps_since_fluid += 1;
            if fluid_due && self.substeps_since_fluid >= cadence {
                let elapsed = SIMULATION_DT * self.substeps_since_fluid as f32;
                self.fluid.encode_step(&self.gpu, &mut encoder, &self.brush, speed, elapsed);
                self.pose_at_last_fluid_step = self.pose;
                self.substeps_since_fluid = 0;
            }
            if self.substeps.is_multiple_of(u64::from(SUBSTEPS_PER_FLUID_STEP)) {
                if self.fluid_enabled {
                    self.height_map.encode(&mut encoder);
                    self.fluid.encode_levelling(&mut encoder);
                    self.store_levelling.encode(&mut encoder);
                }
            }
            self.gpu.queue.submit(Some(encoder.finish()));
        }
    }

    fn handle_position(&self) -> Vec2 {
        Vec2::new(self.pose.position.x, self.pose.position.y)
    }

    fn tow_tuft_centre(&mut self, reach: f32) -> Vec2 {
        let offset = self.handle_position() - self.tuft_centre;
        if offset.length() > reach {
            self.tuft_centre = self.handle_position() - offset.normalize() * reach;
        }
        self.tuft_centre
    }

    fn pose_for(&self, state: PointerState) -> BrushPose {
        let length = self.brush.preset().length;
        let along_handle = if state.pressed {
            length * (1.0 - MAX_COMPRESSION * state.pressure.clamp(0.0, 1.0))
        } else {
            length * LIFTED_HEIGHT
        };
        let handle = handle_direction(state.tilt);
        let spin = Quat::from_rotation_z((self.brush_angle_degrees + state.rotation).to_radians());
        BrushPose {
            position: Vec3::new(state.x, state.y, 0.0) + handle * along_handle,
            orientation: Quat::from_rotation_arc(Vec3::Z, handle) * spin,
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
