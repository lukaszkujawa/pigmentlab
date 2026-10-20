use crate::brush::{Brush, BrushAdjustments, BrushPose, BrushPreset, PRESETS, Touch};
use crate::canvas::{Canvas, CanvasSize, Medium};
use crate::drip::{Drips, Rig};
use crate::film::{Film, FilmDeltas, FilmWorker};
use crate::gpu::{Gpu, read_rgba8, read_texel};
use crate::history::History;
use crate::input::{Hover, PointerSample, PointerTrack};
use crate::paint::{ActiveWindow, BrushContents, Deposit, Levelling, Mark, Marks, SMALLEST_WINDOW, Tube, WetBounds, WetRegions};
use crate::pigment::{Coefficients, Paint};
use crate::watercolour::{WaterBrush, WaterTube, Watercolour};
use crate::render::{BrushLines, Composite, DripOverlay, Frame, Guides, HeightMap, Lighting, Stage, Target, ViewFrames, Viewport, create_presented_texture, display_view};
use glam::Vec3;

mod drip;
mod hand;
mod paint;
mod sheet;
mod stepping;
mod tools;

pub const SIMULATION_DT: f32 = 1.0 / 240.0;
const SUBSTEPS_PER_DEPOSIT: u64 = 2;
const DEPOSIT_TRAVEL: f32 = 2.0;
const SUBSTEPS_PER_FILM_STEP: u64 = 8;
const DEPOSIT_DT: f32 = SIMULATION_DT * SUBSTEPS_PER_DEPOSIT as f32;
const UNDO_DEPTH: usize = 6;
const LIFTED_HEIGHT: f32 = 1.25;
const SUBSTEPS_PER_WATER_STEP: u64 = 4;
const WATER_SETTLE_SECONDS: f64 = 90.0;
pub const DRIP_TOOL: &str = "Drip bottle";
pub const SPLATTER_TOOL: &str = "Splatter";
const DEFAULT_SPLATTER_HEIGHT: f32 = 30.0;
const DEFAULT_SPLATTER_FORCE: f32 = 3.0;
pub const TUBE_TOOL: &str = "Paint tube";
const FOOTPRINT_SLACK: f32 = crate::units::cm(0.15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Brush,
    Tube,
    Drip,
    Splatter,
}

pub fn tool_names() -> Vec<&'static str> {
    PRESETS.iter().map(|named| named.name).chain([TUBE_TOOL, DRIP_TOOL, SPLATTER_TOOL]).collect()
}

pub struct Engine {
    gpu: Gpu,
    canvas: Canvas,
    brush: Brush,
    window: ActiveWindow,
    stroke_base: wgpu::Buffer,
    deposit: Deposit,
    tube: Tube,
    marks: Marks,
    pending_marks: Vec<Mark>,
    drips: Drips,
    film: FilmWorker,
    realtime: bool,
    film_deltas: FilmDeltas,
    drip_overlay: DripOverlay,
    stage: Stage,
    stage_view: Option<Viewport>,
    rig: Rig,
    holding_hook: bool,
    splatter_height: f32,
    splatter_force: f32,
    aim: Option<[glam::Vec2; 2]>,
    drained: f32,
    lifted_load: Option<f32>,
    tool: Tool,
    paint_on_tool: Paint,
    height_map: HeightMap,
    levelling: Levelling,
    composite: Composite,
    brush_lines: BrushLines,
    frames: ViewFrames,
    display: wgpu::Texture,
    display_stale: bool,
    view_stale: bool,
    pointer: PointerTrack,
    hover: Option<Hover>,
    pose: BrushPose,
    position_at_last_deposit: Vec3,
    substep_at_last_deposit: u64,
    touch: Touch,
    watercolour: Option<Watercolour>,
    water_brush: Option<WaterBrush>,
    water_tube: Option<WaterTube>,
    water_settles_at: f64,
    brush_contents: BrushContents,
    contents_version: u64,
    gauge_version: u64,
    pressed: bool,
    show_bristles: bool,
    time: f64,
    substeps: u64,
    levelling_enabled: bool,
    wet: WetRegions,
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
        let height_map = HeightMap::new(&gpu, &canvas, &window, &stroke_base);
        let deposit = Deposit::new(&gpu, &brush, &window, &canvas, &stroke_base);
        let tube = Tube::new(&gpu, &window, &canvas, &height_map.heights, seed, SIMULATION_DT);
        let marks = Marks::new(&gpu, &window, &canvas);
        let film_deltas = FilmDeltas::new(&gpu, &window, &canvas);
        let film = FilmWorker::spawn(Film::new(size.width, size.height, canvas.relief_heights()));
        let levelling = Levelling::new(&gpu, &canvas, &window, &height_map);
        let frames = ViewFrames::new(&gpu, glam::UVec2::new(size.width, size.height));
        let composite = Composite::new(&gpu, &canvas, &window, &height_map, &frames);
        let brush_lines = BrushLines::new(&gpu, &brush, &frames);
        let display = create_presented_texture(
            &gpu,
            "display",
            size.extent(),
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let history = History::new(&gpu, canvas.paint.size(), UNDO_DEPTH);
        let dry_history = History::new(&gpu, canvas.dry.size(), UNDO_DEPTH);
        let lifted = Vec3::new(size.width as f32 * 0.5, size.height as f32 * 0.5, brush.preset().length * LIFTED_HEIGHT);
        let drip_overlay = DripOverlay::new(&gpu);
        let stage = Stage::new(&gpu);
        let mut engine = Self {
            gpu,
            canvas,
            brush,
            window,
            stroke_base,
            deposit,
            tube,
            marks,
            pending_marks: Vec::new(),
            drips: Drips::new(seed),
            film,
            realtime: false,
            film_deltas,
            drip_overlay,
            stage,
            stage_view: None,
            rig: Rig { hook: glam::Vec2::new(size.width as f32, size.height as f32) * 0.5, ..Rig::default() },
            holding_hook: false,
            splatter_height: crate::units::cm(DEFAULT_SPLATTER_HEIGHT),
            splatter_force: DEFAULT_SPLATTER_FORCE,
            aim: None,
            drained: 0.0,
            lifted_load: None,
            tool: Tool::Brush,
            paint_on_tool: Paint::from(Coefficients { absorption: Vec3::ZERO, scattering: Vec3::ZERO }),
            height_map,
            levelling,
            composite,
            brush_lines,
            frames,
            display,
            display_stale: true,
            view_stale: false,
            pointer: PointerTrack::default(),
            hover: None,
            pose: BrushPose::upright(lifted),
            position_at_last_deposit: lifted,
            substep_at_last_deposit: 0,
            touch: Touch::default(),
            watercolour: None,
            water_brush: None,
            water_tube: None,
            water_settles_at: 0.0,
            brush_contents: BrushContents::empty(),
            contents_version: 0,
            gauge_version: 0,
            pressed: false,
            show_bristles: false,
            time: 0.0,
            substeps: 0,
            levelling_enabled: true,
            wet: WetRegions::default(),
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

    pub fn render(&mut self) {
        if !self.needs_render && !self.view_stale {
            return;
        }
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        if self.needs_render {
            self.refresh_blur(&mut encoder);
            self.composite.encode_surface(&mut encoder);
            self.display_stale = true;
        }
        self.needs_render = false;
        self.view_stale = false;
        let splattering = self.tool == Tool::Splatter;
        let guides = Guides { hook: (self.show_bristles && self.tool == Tool::Drip).then_some(self.rig.hook), aim: self.aim.filter(|_| splattering), reach: self.splatter_reach().filter(|_| splattering) };
        let drips = (self.show_bristles || guides.aim.is_some()).then_some(&self.drips);
        let bristles = self.show_bristles && self.tool == Tool::Brush;
        match self.stage_view {
            Some(stage) => {
                let layer = self.stage.layer(&self.gpu, stage);
                self.composite.encode(&mut encoder, layer, Frame::Stage);
                if bristles {
                    self.brush_lines.encode(&mut encoder, layer, Frame::Stage);
                }
                self.drip_overlay.encode(&self.gpu, &mut encoder, &Target { texture: layer, view: stage, premultiplied: true }, drips, guides);
                self.stage.encode_output(&mut encoder);
            }
            None => {
                let view = display_view(&self.display);
                self.composite.encode(&mut encoder, &view, Frame::Canvas);
                if bristles {
                    self.brush_lines.encode(&mut encoder, &view, Frame::Canvas);
                }
                if let Some(drips) = drips {
                    let canvas = Viewport::canvas(glam::UVec2::new(self.canvas.size.width, self.canvas.size.height));
                    self.drip_overlay.encode(&self.gpu, &mut encoder, &Target { texture: &view, view: canvas, premultiplied: false }, Some(drips), guides);
                }
                self.display_stale = false;
            }
        }
        self.gpu.queue.submit(Some(encoder.finish()));
    }

    fn refresh_display(&mut self) {
        self.render();
        if !self.display_stale {
            return;
        }
        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("display") });
        self.composite.encode(&mut encoder, &display_view(&self.display), Frame::Canvas);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.display_stale = false;
    }

    pub fn set_stage(&mut self, view: Option<Viewport>) {
        if self.stage_view != view {
            self.stage_view = view;
            if let Some(stage) = view {
                self.frames.set_stage(&self.gpu, stage);
            }
            self.view_stale = true;
        }
    }

    pub fn stage(&self) -> Option<&wgpu::Texture> {
        self.stage_view.and(self.stage.output())
    }

    pub fn read_stage(&self) -> Option<image::RgbaImage> {
        self.stage().map(|texture| read_rgba8(&self.gpu, texture))
    }

    pub fn colour_at(&mut self, x: f32, y: f32) -> [u8; 3] {
        self.refresh_display();
        let [r, g, b, _] = read_texel(&self.gpu, &self.display, x.max(0.0) as u32, y.max(0.0) as u32);
        [r, g, b]
    }

    pub fn read_back(&mut self) -> image::RgbaImage {
        self.refresh_display();
        read_rgba8(&self.gpu, &self.display)
    }
}
