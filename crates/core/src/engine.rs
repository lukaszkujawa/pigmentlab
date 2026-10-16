use crate::brush::{Brush, BrushAdjustments, BrushPose, BrushPreset, MAX_COMPRESSION, PRESETS, Touch, preset_by_name};
use crate::canvas::{Canvas, CanvasSize, Medium, PAINT_CHANNELS, PAINT_SCALE, Surface};
use crate::gpu::{Gpu, read_buffer, read_rgba8, read_texel};
use crate::history::History;
use crate::input::{PointerSample, PointerTrack};
use crate::paint::{ActiveWindow, BrushContents, Deposit, Levelling, NOZZLE_RADIUS, StrokeState, Tube, WetBounds};
use crate::pigment::{Coefficients, Paint};
use crate::watercolour::{WaterBrush, Watercolour};
use crate::render::{BrushLines, Composite, DISPLAY_FORMAT, HeightMap, Lighting};
use glam::{IVec2, Quat, Vec2, Vec3, Vec4};

pub const SIMULATION_DT: f32 = 1.0 / 240.0;
const MAX_SUBSTEPS_PER_ADVANCE: u32 = 32;
const SUBSTEPS_PER_LEVELLING: u64 = 8;
const SUBSTEPS_PER_DEPOSIT: u64 = 2;
const DEPOSIT_DT: f32 = SIMULATION_DT * SUBSTEPS_PER_DEPOSIT as f32;
const UNDO_DEPTH: usize = 6;
const LEVELLING_SETTLE_SECONDS: f64 = 2.0;
const LEVELLING_SETTLE_SECONDS_PER_MEDIUM: f64 = 4.0;
const WET_MARGIN: i32 = 48;
const DEPOSIT_REACH: i32 = 24;
const LIFTED_HEIGHT: f32 = 1.25;
const MAX_TILT: f32 = 0.8;
const FOOTPRINT_SLACK: f32 = crate::units::cm(0.15);
const DILUTION_PER_MEDIUM: f32 = 0.9;
const SUBSTEPS_PER_WATER_STEP: u64 = 4;
const WATER_DT: f32 = SIMULATION_DT * SUBSTEPS_PER_WATER_STEP as f32;
const WATER_SETTLE_SECONDS: f64 = 90.0;
const WATERCOLOUR_PIGMENT: f32 = 0.08;
const WATERCOLOUR_DILUTION: f32 = 0.95;
const WATERCOLOUR_DRIEST_LOAD: f32 = 0.2;
const WATERCOLOUR_SCRUB: f32 = 0.002;
pub const TUBE_TOOL: &str = "Paint tube";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Brush,
    Tube,
}

pub fn tool_names() -> Vec<&'static str> {
    PRESETS.iter().map(|named| named.name).chain(std::iter::once(TUBE_TOOL)).collect()
}

pub struct Engine {
    gpu: Gpu,
    canvas: Canvas,
    brush: Brush,
    window: ActiveWindow,
    stroke_base: wgpu::Buffer,
    deposit: Deposit,
    tube: Tube,
    tool: Tool,
    paint_on_tool: Coefficients,
    height_map: HeightMap,
    levelling: Levelling,
    composite: Composite,
    brush_lines: BrushLines,
    display: wgpu::Texture,
    pointer: PointerTrack,
    pose: BrushPose,
    position_at_last_deposit: Vec3,
    touch: Touch,
    watercolour: Option<Watercolour>,
    water_brush: Option<WaterBrush>,
    water_settles_at: f64,
    brush_contents: BrushContents,
    contents_version: u64,
    gauge_version: u64,
    pressed: bool,
    levelling_active_until: f64,
    show_bristles: bool,
    time: f64,
    substeps: u64,
    levelling_enabled: bool,
    wet_bounds: Option<WetBounds>,
    stale_heights: Option<WetBounds>,
    stale_blur: Option<WetBounds>,
    needs_render: bool,
    history: History,
    dry_history: History,
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
        let stroke_base = gpu.storage_buffer("stroke base heights", canvas.size.pixel_count() * 4);
        let brush = Brush::new(&gpu, BrushPreset::default_round(), seed, &window, &canvas, &stroke_base);
        let height_map = HeightMap::new(&gpu, &canvas, &window);
        let deposit = Deposit::new(&gpu, &brush, &window, &canvas, &stroke_base, DEPOSIT_DT);
        let tube = Tube::new(&gpu, &window, SIMULATION_DT);
        let levelling = Levelling::new(&gpu, &canvas, &window, &height_map);
        let composite = Composite::new(&gpu, &canvas, &window, &height_map);
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
        let dry_history = History::new(&gpu, canvas.dry.size(), UNDO_DEPTH);
        let lifted = Vec3::new(size.width as f32 * 0.5, size.height as f32 * 0.5, brush.preset().length * LIFTED_HEIGHT);
        let mut engine = Self {
            gpu,
            canvas,
            brush,
            window,
            stroke_base,
            deposit,
            tube,
            tool: Tool::Brush,
            paint_on_tool: Coefficients { absorption: Vec3::ZERO, scattering: Vec3::ZERO },
            height_map,
            levelling,
            composite,
            brush_lines,
            display,
            pointer: PointerTrack::default(),
            pose: BrushPose::upright(lifted),
            position_at_last_deposit: lifted,
            touch: Touch::default(),
            watercolour: None,
            water_brush: None,
            water_settles_at: 0.0,
            brush_contents: BrushContents::empty(),
            contents_version: 0,
            gauge_version: 0,
            pressed: false,
            levelling_active_until: 0.0,
            show_bristles: false,
            time: 0.0,
            substeps: 0,
            levelling_enabled: true,
            wet_bounds: None,
            stale_heights: None,
            stale_blur: None,
            needs_render: true,
            history,
            dry_history,
            seed,
            base_preset: BrushPreset::default_round(),
            adjustments: BrushAdjustments::default(),
            brush_angle_degrees: 0.0,
            paint: PaintSettings::default(),
        };
        engine.mark_whole_canvas_stale();
        engine
    }

    pub fn canvas_size(&self) -> CanvasSize {
        self.canvas.size
    }

    pub fn canvas_relief(&self) -> Vec<f32> {
        self.canvas.relief_heights()
    }

    pub fn dry_layer(&self) -> Vec<Vec4> {
        read_buffer::<Vec4>(&self.gpu, &self.canvas.dry)
    }

    pub fn dry_heights(&self) -> Vec<f32> {
        self.dry_layer().iter().map(|layer| layer.x).collect()
    }

    fn snapshot(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.history.push(encoder, &self.canvas.paint);
        self.dry_history.push(encoder, &self.canvas.dry);
        if let Some(watercolour) = &mut self.watercolour {
            watercolour.snapshot(encoder);
        }
    }

    pub fn watercolour(&self) -> Option<&Watercolour> {
        self.watercolour.as_ref()
    }

    pub fn medium(&self) -> Medium {
        self.canvas.surface().medium()
    }

    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    pub fn brush(&self) -> &Brush {
        &self.brush
    }

    pub fn select_brush(&mut self, name: &str) -> bool {
        self.configure_brush(name, self.adjustments)
    }

    pub fn set_brush_adjustments(&mut self, adjustments: BrushAdjustments) {
        if adjustments != self.adjustments {
            self.adjustments = adjustments;
            self.rebuild_brush();
        }
    }

    pub fn configure_brush(&mut self, name: &str, adjustments: BrushAdjustments) -> bool {
        if name.eq_ignore_ascii_case(TUBE_TOOL) {
            self.tool = Tool::Tube;
            self.adjustments = adjustments;
            return true;
        }
        let Some(preset) = preset_by_name(name) else { return false };
        self.tool = Tool::Brush;
        if preset != self.base_preset || adjustments != self.adjustments || self.brush.preset() != &preset.adjusted(adjustments) {
            self.base_preset = preset;
            self.adjustments = adjustments;
            self.rebuild_brush();
        }
        true
    }

    pub fn set_brush_angle(&mut self, degrees: f32) {
        self.brush_angle_degrees = degrees;
    }

    pub fn set_paint(&mut self, paint: PaintSettings) {
        self.paint = paint;
        self.deposit.set_medium(paint.medium);
    }

    pub fn set_param(&mut self, name: &str, value: f32) -> bool {
        match name {
            "size" => self.set_brush_adjustments(BrushAdjustments { scale: value, ..self.adjustments }),
            "bristles" => self.set_brush_adjustments(BrushAdjustments { bristle_count: Some(value as u32), ..self.adjustments }),
            "stiffness" => self.set_brush_adjustments(BrushAdjustments { stiffness: value, ..self.adjustments }),
            "angle" => self.set_brush_angle(value),
            "load" => self.set_paint(PaintSettings { load: value, ..self.paint }),
            "medium" => self.set_paint(PaintSettings { medium: value, ..self.paint }),
            "tooth" => {
                self.canvas.set_tooth(&self.gpu, value);
                if let Some(watercolour) = &mut self.watercolour {
                    watercolour.set_tooth(self.canvas.tooth());
                }
                self.mark_whole_canvas_stale();
            }
            _ => return false,
        }
        true
    }

    fn rebuild_brush(&mut self) {
        self.set_brush(self.base_preset.adjusted(self.adjustments));
        self.deposit.set_medium(self.paint.medium);
    }

    pub fn set_brush(&mut self, preset: BrushPreset) {
        self.brush = Brush::new(&self.gpu, preset, self.seed, &self.window, &self.canvas, &self.stroke_base);
        self.deposit = Deposit::new(&self.gpu, &self.brush, &self.window, &self.canvas, &self.stroke_base, DEPOSIT_DT);
        self.brush_lines = BrushLines::new(&self.gpu, &self.brush);
        self.brush.teleport(&self.gpu, &self.pose);
        self.rebuild_water_brush();
        self.brush_contents = BrushContents::empty();
        self.gauge_version = self.contents_version;
    }

    pub fn surface(&self) -> Surface {
        self.canvas.surface()
    }

    pub fn set_surface(&mut self, surface: Surface) {
        self.canvas.set_surface(&self.gpu, surface);
        match surface.medium() {
            Medium::Watercolour => match &mut self.watercolour {
                Some(watercolour) => watercolour.set_paper(&self.gpu, &self.canvas),
                None => self.watercolour = Some(Watercolour::new(&self.gpu, &self.canvas, WATER_DT, WATERCOLOUR_SCRUB)),
            },
            Medium::Oil => self.watercolour = None,
        }
        self.rebuild_water_brush();
        self.composite.attach_watercolour(&self.gpu, &self.canvas, &self.height_map, self.watercolour.as_ref());
        let mut encoder = self.encoder("new sheet");
        self.window.encode_unbake(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.history.forget();
        self.dry_history.forget();
        self.wet_bounds = None;
        self.mark_whole_canvas_stale();
    }

    fn rebuild_water_brush(&mut self) {
        self.water_brush = self.watercolour.as_ref().map(|water| WaterBrush::new(&self.gpu, &self.brush, water, &self.canvas, DEPOSIT_DT));
    }

    pub fn clear(&mut self) {
        if let Some(watercolour) = &mut self.watercolour {
            watercolour.clear(&self.gpu);
        }
        let mut encoder = self.encoder("clear");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.snapshot(&mut encoder);
        self.canvas.clear(&self.gpu, &mut encoder);
        self.window.encode_unbake(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.wet_bounds = None;
        self.mark_whole_canvas_stale();
    }

    pub fn dry(&mut self) {
        let mut encoder = self.encoder("dry");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.snapshot(&mut encoder);
        if let Some(watercolour) = &mut self.watercolour {
            watercolour.encode_step(&self.gpu, &mut encoder, true);
            watercolour.settle();
        }
        self.canvas.encode_dry(&mut encoder);
        self.window.encode_unbake(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.wet_bounds = None;
        self.mark_whole_canvas_stale();
    }

    pub fn undo(&mut self) -> bool {
        let mut encoder = self.encoder("undo");
        let restored = self.history.pop(&mut encoder, &self.canvas.paint);
        if restored {
            self.dry_history.pop(&mut encoder, &self.canvas.dry);
            if let Some(watercolour) = &mut self.watercolour {
                watercolour.undo(&mut encoder);
                self.water_settles_at = self.time + WATER_SETTLE_SECONDS;
            }
            self.canvas.encode_refresh_ground(&mut encoder);
            self.window.encode_unbake(&mut encoder);
            self.wet_bounds = Some(self.whole_canvas());
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        self.mark_whole_canvas_stale();
        restored
    }

    pub fn save_png(&mut self, path: &std::path::Path) -> Result<(), image::ImageError> {
        self.render();
        self.read_back().save(path)
    }

    fn encoder(&self, label: &str) -> wgpu::CommandEncoder {
        self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
    }

    pub fn set_lighting(&mut self, lighting: Lighting) {
        self.composite.set_lighting(&self.gpu, lighting);
        self.needs_render = true;
    }

    pub fn set_levelling_enabled(&mut self, enabled: bool) {
        self.levelling_enabled = enabled;
    }

    pub fn set_show_bristles(&mut self, show: bool) {
        self.show_bristles = show;
        self.needs_render = true;
    }

    pub fn dip(&mut self, paint: impl Into<Paint>) {
        let paint = paint.into();
        let load = self.paint.load.clamp(0.0, 1.0);
        let medium = self.paint.medium.clamp(0.0, 1.0);
        if self.watercolour.is_some() {
            let concentration = WATERCOLOUR_PIGMENT * (1.0 - WATERCOLOUR_DILUTION * medium) / load.max(WATERCOLOUR_DRIEST_LOAD);
            self.brush.dip_watercolour(&self.gpu, paint, load, concentration);
            self.deposit.discard_brush_reading();
            self.brush_contents = BrushContents { fill: load, paint: paint.coefficients };
            return;
        }
        let paint = paint.coefficients;
        let dilution = 1.0 - DILUTION_PER_MEDIUM * medium;
        let thinned = Coefficients { absorption: paint.absorption * dilution, scattering: paint.scattering * dilution };
        self.paint_on_tool = thinned;
        self.brush.dip(&self.gpu, thinned, load, medium);
        self.deposit.discard_brush_reading();
        self.brush_contents = BrushContents { fill: load, paint: thinned };
    }

    pub fn dip_water(&mut self) {
        let load = self.paint.load.clamp(0.0, 1.0);
        let clear = Paint::from(Coefficients { absorption: glam::Vec3::ZERO, scattering: glam::Vec3::ZERO });
        if self.watercolour.is_some() {
            self.brush.dip_watercolour(&self.gpu, clear, load, 0.0);
        } else {
            self.brush.clean(&self.gpu);
        }
        self.deposit.discard_brush_reading();
        self.brush_contents = BrushContents { fill: if self.watercolour.is_some() { load } else { 0.0 }, paint: clear.coefficients };
    }

    pub fn brush_contents(&mut self) -> BrushContents {
        self.collect_brush_contents();
        self.brush_contents
    }

    fn collect_brush_contents(&mut self) {
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        if let Some(contents) = self.deposit.brush_contents() {
            self.brush_contents = contents;
        }
        if self.gauge_version != self.contents_version {
            let mut encoder = self.encoder("brush gauge");
            self.request_gauge_reading(&mut encoder);
            self.gpu.queue.submit(Some(encoder.finish()));
            self.deposit.after_submit();
        }
    }

    pub fn clean(&mut self) {
        self.brush.clean(&self.gpu);
        self.deposit.discard_brush_reading();
        self.brush_contents = BrushContents { fill: 0.0, ..self.brush_contents };
    }

    pub fn brush_load(&self) -> f32 {
        read_buffer::<f32>(&self.gpu, &self.brush.sample_load).iter().map(|load| f64::from(*load)).sum::<f64>() as f32
    }

    pub fn brush_sample_absorption(&self) -> Vec<Vec3> {
        read_buffer::<Vec4>(&self.gpu, &self.brush.sample_absorption).iter().map(|v| v.truncate()).collect()
    }

    pub fn brush_pigment(&self) -> Coefficients {
        let loads = read_buffer::<f32>(&self.gpu, &self.brush.sample_load);
        let absorption = read_buffer::<Vec4>(&self.gpu, &self.brush.sample_absorption);
        let scattering = read_buffer::<Vec4>(&self.gpu, &self.brush.sample_scattering);
        let total: f32 = loads.iter().sum::<f32>().max(1e-6);
        let weighted = |values: &[Vec4]| values.iter().zip(&loads).map(|(v, load)| v.truncate() * *load).sum::<Vec3>() / total;
        Coefficients { absorption: weighted(&absorption), scattering: weighted(&scattering) }
    }

    pub fn paint_columns(&mut self) -> Vec<[f32; 8]> {
        let mut encoder = self.encoder("bake");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.gpu.queue.submit(Some(encoder.finish()));
        let store = read_buffer::<i32>(&self.gpu, &self.canvas.paint);
        store.chunks(PAINT_CHANNELS as usize).map(|pixel| std::array::from_fn(|channel| pixel[channel] as f32 / PAINT_SCALE)).collect()
    }

    pub fn paint_heights(&mut self) -> Vec<f32> {
        self.paint_columns().iter().map(|column| column[0]).collect()
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
            if let Some(state) = self.pointer.state_at(self.time) {
                self.touch.update(&state, SIMULATION_DT);
                self.pose = self.pose_for();
                self.pressed = state.pressed;
                if self.touch.engaged() {
                    self.levelling_active_until = self.time + LEVELLING_SETTLE_SECONDS + LEVELLING_SETTLE_SECONDS_PER_MEDIUM * f64::from(self.paint.medium.clamp(0.0, 1.0));
                }
                if state.fresh_press {
                    self.position_at_last_deposit = self.pose.position;
                    self.brush.teleport(&self.gpu, &self.pose);
                    let mut encoder = self.encoder("stroke snapshot");
                    self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
                    self.snapshot(&mut encoder);
                    self.refresh_heights(&mut encoder);
                    self.height_map.encode_tuft_base(&self.gpu, &mut encoder, self.whole_canvas());
                    encoder.copy_buffer_to_buffer(&self.height_map.tuft_base, 0, &self.stroke_base, 0, self.stroke_base.size());
                    if let Some(water_brush) = &self.water_brush {
                        water_brush.start_stroke(&mut encoder, &self.brush);
                    }
                    self.gpu.queue.submit(Some(encoder.finish()));
                }
                self.needs_render |= self.touch.engaged() || self.show_bristles;
            }
            let mut encoder = self
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("substep") });
            let preset = *self.brush.preset();
            let (focus, footprint_radius) = match self.tool {
                Tool::Brush => (self.brush.tuft_centre(), preset.bend_reach() + preset.splayed_radius() + FOOTPRINT_SLACK),
                Tool::Tube => (self.handle_position(), NOZZLE_RADIUS + FOOTPRINT_SLACK),
            };
            if self.window.needs_relocation(focus, footprint_radius) {
                self.window.follow(&self.gpu, &mut encoder, &self.canvas, focus);
            }
            if self.touch.engaged() {
                let touched = WetBounds::around(self.handle_position().round().as_ivec2(), footprint_radius.ceil() as i32 + WET_MARGIN);
                match &mut self.watercolour {
                    Some(watercolour) => {
                        watercolour.wet(touched);
                        self.water_settles_at = self.time + WATER_SETTLE_SECONDS;
                    }
                    None => self.wet_bounds = Some(self.wet_bounds.map_or(touched, |bounds| bounds.include(touched))),
                }
                self.mark_stale(WetBounds::around(self.handle_position().round().as_ivec2(), footprint_radius.ceil() as i32 + DEPOSIT_REACH));
            }
            match self.tool {
                Tool::Brush => {
                    self.brush.encode_step(&self.gpu, &mut encoder, &self.pose, self.touch.pressure, self.canvas.tooth(), canvas);
                    if let Some(water_brush) = self.water_brush.as_ref().filter(|_| self.touch.engaged() && self.substeps.is_multiple_of(SUBSTEPS_PER_DEPOSIT)) {
                        water_brush.encode(&mut encoder, &self.brush);
                    } else if self.water_brush.is_none() && self.touch.engaged() && self.substeps.is_multiple_of(SUBSTEPS_PER_DEPOSIT) {
                        let travel = self.pose.position.truncate().distance(self.position_at_last_deposit.truncate());
                        let stroke = StrokeState { travel, pressure: self.touch.pressure, contact: self.contact(), lifting: !self.pressed };
                        self.deposit.encode(&self.gpu, &mut encoder, &self.brush, stroke);
                        self.contents_version += 1;
                        self.position_at_last_deposit = self.pose.position;
                    }
                }
                Tool::Tube => {
                    if self.pressed && self.watercolour.is_none() {
                        let from = self.position_at_last_deposit.truncate();
                        self.tube.encode_squeeze(&self.gpu, &mut encoder, [from, self.handle_position()], self.touch.pressure, self.paint_on_tool, self.paint.medium.clamp(0.0, 1.0));
                    }
                    self.position_at_last_deposit = self.pose.position;
                }
            }
            if let Some(watercolour) = self.watercolour.as_mut().filter(|water| water.is_wet() && self.substeps.is_multiple_of(SUBSTEPS_PER_WATER_STEP)) {
                let settled = self.time > self.water_settles_at;
                watercolour.encode_step(&self.gpu, &mut encoder, settled);
                if settled {
                    watercolour.settle();
                }
                self.needs_render = true;
            }
            self.substeps += 1;
            let levelling_due = self.levelling_enabled && self.time <= self.levelling_active_until;
            if let Some(wet) = self.wet_bounds.filter(|_| levelling_due && self.substeps.is_multiple_of(SUBSTEPS_PER_LEVELLING)) {
                self.refresh_heights(&mut encoder);
                self.levelling.encode(&self.gpu, &mut encoder, wet);
                self.mark_stale(wet.expand(1));
            }
            self.request_gauge_reading(&mut encoder);
            self.gpu.queue.submit(Some(encoder.finish()));
            self.deposit.after_submit();
            self.collect_brush_contents();
        }
    }

    fn request_gauge_reading(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.gauge_version != self.contents_version && self.deposit.encode_gauge_reading(encoder) {
            self.gauge_version = self.contents_version;
        }
    }

    fn whole_canvas(&self) -> WetBounds {
        WetBounds { min: IVec2::ZERO, max: IVec2::new(self.canvas.size.width as i32, self.canvas.size.height as i32) }
    }

    fn mark_whole_canvas_stale(&mut self) {
        self.mark_stale(self.whole_canvas());
    }

    fn mark_stale(&mut self, region: WetBounds) {
        self.stale_heights = Some(self.stale_heights.map_or(region, |stale| stale.include(region)));
        self.needs_render = true;
    }

    fn refresh_heights(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(region) = self.stale_heights.take() {
            self.height_map.encode_build(&self.gpu, encoder, region);
            self.stale_blur = Some(self.stale_blur.map_or(region, |stale| stale.include(region)));
        }
    }

    fn refresh_blur(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.refresh_heights(encoder);
        if let Some(region) = self.stale_blur.take() {
            self.height_map.encode_blur(&self.gpu, encoder, region);
        }
    }

    fn handle_position(&self) -> Vec2 {
        Vec2::new(self.pose.position.x, self.pose.position.y)
    }

    fn handle_heights(&self) -> (f32, f32) {
        let length = self.brush.preset().length;
        (length * LIFTED_HEIGHT, length * (1.0 - MAX_COMPRESSION * self.touch.pressure))
    }

    fn contact(&self) -> f32 {
        let depth = self.touch.depth();
        if depth >= 1.0 {
            return 1.0;
        }
        let (lifted, pressed) = self.handle_heights();
        let reaching = (lifted - self.brush.preset().length) / (lifted - pressed);
        ((depth - reaching) / (1.0 - reaching).max(1e-3)).clamp(0.0, 1.0)
    }

    fn pose_for(&self) -> BrushPose {
        let (lifted, pressed) = self.handle_heights();
        let handle = handle_direction(self.touch.tilt);
        let spin = Quat::from_rotation_z((self.brush_angle_degrees + self.touch.rotation).to_radians());
        BrushPose {
            position: self.touch.position.extend(0.0) + handle * (lifted + (pressed - lifted) * self.touch.depth()),
            orientation: Quat::from_rotation_arc(Vec3::Z, handle) * spin,
        }
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    pub fn render(&mut self) -> &wgpu::Texture {
        if !self.needs_render {
            return &self.display;
        }
        self.needs_render = false;
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let view = self.display.create_view(&wgpu::TextureViewDescriptor::default());
        self.refresh_blur(&mut encoder);
        self.composite.encode(&mut encoder, &view);
        if self.show_bristles && self.tool == Tool::Brush {
            self.brush_lines.encode(&mut encoder, &view);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        &self.display
    }

    pub fn colour_at(&mut self, x: f32, y: f32) -> [u8; 3] {
        self.render();
        let [r, g, b, _] = read_texel(&self.gpu, &self.display, x.max(0.0) as u32, y.max(0.0) as u32);
        [r, g, b]
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
