use crate::brush::{Brush, BrushAdjustments, BrushPose, BrushPreset, PRESETS, Touch};
use crate::canvas::{Canvas, CanvasSize, Medium};
use crate::gpu::{Gpu, read_rgba8, read_texel};
use crate::history::History;
use crate::input::{Hover, PointerSample, PointerTrack};
use crate::paint::{ActiveWindow, BrushContents, Deposit, Levelling, SMALLEST_WINDOW, Tube, WetBounds};
use crate::pigment::{Coefficients, Paint};
use crate::watercolour::{WaterBrush, WaterTube, Watercolour};
use crate::render::{BrushLines, Composite, DISPLAY_FORMAT, HeightMap, Lighting};
use glam::Vec3;

mod hand;
mod paint;
mod sheet;
mod stepping;
mod tools;

pub const SIMULATION_DT: f32 = 1.0 / 240.0;
const SUBSTEPS_PER_DEPOSIT: u64 = 2;
const DEPOSIT_DT: f32 = SIMULATION_DT * SUBSTEPS_PER_DEPOSIT as f32;
const UNDO_DEPTH: usize = 6;
const LIFTED_HEIGHT: f32 = 1.25;
const SUBSTEPS_PER_WATER_STEP: u64 = 4;
const WATER_SETTLE_SECONDS: f64 = 90.0;
pub const TUBE_TOOL: &str = "Paint tube";
const FOOTPRINT_SLACK: f32 = crate::units::cm(0.15);

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
    paint_on_tool: Paint,
    height_map: HeightMap,
    levelling: Levelling,
    composite: Composite,
    brush_lines: BrushLines,
    display: wgpu::Texture,
    pointer: PointerTrack,
    hover: Option<Hover>,
    pose: BrushPose,
    position_at_last_deposit: Vec3,
    touch: Touch,
    watercolour: Option<Watercolour>,
    water_brush: Option<WaterBrush>,
    water_tube: Option<WaterTube>,
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
    lighting: Lighting,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintSettings {
    pub load: f32,
    pub medium: f32,
    pub dilution: f32,
}

impl Default for PaintSettings {
    fn default() -> Self {
        Self { load: 1.0, medium: 0.0, dilution: 0.0 }
    }
}

impl Engine {
    pub fn new(gpu: Gpu, size: CanvasSize, seed: u64) -> Self {
        let canvas = Canvas::new(&gpu, size, seed);
        let window = ActiveWindow::new(&gpu, &canvas, SMALLEST_WINDOW);
        let stroke_base = gpu.storage_buffer("stroke base heights", canvas.size.pixel_count() * 4);
        let brush = Brush::new(&gpu, BrushPreset::default_round(), seed, &window, &canvas, &stroke_base);
        let height_map = HeightMap::new(&gpu, &canvas, &window);
        let deposit = Deposit::new(&gpu, &brush, &window, &canvas, &stroke_base, DEPOSIT_DT);
        let tube = Tube::new(&gpu, &window, &canvas, &height_map.heights, seed, SIMULATION_DT);
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
            paint_on_tool: Paint::from(Coefficients { absorption: Vec3::ZERO, scattering: Vec3::ZERO }),
            height_map,
            levelling,
            composite,
            brush_lines,
            display,
            pointer: PointerTrack::default(),
            hover: None,
            pose: BrushPose::upright(lifted),
            position_at_last_deposit: lifted,
            touch: Touch::default(),
            watercolour: None,
            water_brush: None,
            water_tube: None,
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
            lighting: Lighting::default(),
        };
        engine.mark_whole_canvas_stale();
        engine
    }

    pub fn canvas_size(&self) -> CanvasSize {
        self.canvas.size
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

    fn encoder(&self, label: &str) -> wgpu::CommandEncoder {
        self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
    }

    pub fn set_lighting(&mut self, lighting: Lighting) {
        self.lighting = lighting;
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

    pub fn pointer(&mut self, sample: PointerSample) {
        self.pointer.push(sample);
    }

    pub fn hover(&mut self, hover: Hover) {
        self.hover = Some(hover);
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
