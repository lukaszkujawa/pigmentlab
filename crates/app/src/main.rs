slint::include_modules!();

mod dock_icon;
mod tablet;

use pigmentlab_core::brush::{BrushAdjustments, PRESETS};
use pigmentlab_core::engine::{PaintSettings, tool_names};
use pigmentlab_core::scenario::Recorder;
use pigmentlab_core::canvas::{Medium, Surface};
use pigmentlab_core::pigment::{Coefficients, PALETTE};
use pigmentlab_core::{CanvasSize, Engine, Gpu, Lighting, PointerPhase, PointerSample};
use tablet::{PenState, Tablet};
use slint::wgpu_30::{WGPUConfiguration, WGPUSettings};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

const CANVAS: CanvasSize = CanvasSize { width: 2048, height: 1536 };
const SEED: u64 = 1;

const MAX_CUSTOM_COLOURS: usize = 12;
const HOVER: i32 = 3;

#[derive(Clone, Copy, Debug)]
enum Dip {
    Pigment(usize),
    Colour([u8; 3]),
    Water,
}

#[derive(Default)]
struct Requests {
    pointer: Vec<PointerSample>,
    hover: Option<(f32, f32)>,
    last_pointer: Option<PointerSample>,
    dip: Option<Dip>,
    clean: bool,
    pick: Option<(f32, f32)>,
    surface: Option<Surface>,
    brush: Option<(usize, BrushAdjustments)>,
    paint: Option<PaintSettings>,
    tooth: Option<f32>,
    clear: bool,
    undo: bool,
    save: bool,
    dry: bool,
}

impl Requests {
    fn push_pointer(&mut self, sample: PointerSample) {
        self.pointer.push(sample);
        self.last_pointer = Some(sample);
    }

    fn follow_held_pen(&mut self, pen: PenState, firmness: f32, time: f64) {
        let Some(last) = self.last_pointer.filter(|last| pen.present && last.phase != PointerPhase::Up) else { return };
        let pressure = pen.brush_pressure(firmness);
        if (last.pressure, last.tilt, last.rotation) != (pressure, pen.tilt, pen.rotation) {
            self.push_pointer(PointerSample { phase: PointerPhase::Move, pressure, tilt: pen.tilt, rotation: pen.rotation, time, ..last });
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let mut wgpu_settings = WGPUSettings::default();
    wgpu_settings.device_required_limits = pigmentlab_core::gpu::required_limits();
    slint::BackendSelector::new()
        .require_wgpu_30(WGPUConfiguration::Automatic(wgpu_settings))
        .select()?;

    let app = AppWindow::new()?;
    dock_icon::install();
    let scale = app.window().scale_factor().max(1.0);
    app.set_canvas_width(CANVAS.width as f32 / scale);
    app.set_canvas_height(CANVAS.height as f32 / scale);
    app.set_canvas_scale(scale);
    let custom: Rc<RefCell<Vec<[u8; 3]>>> = Rc::default();
    app.set_swatches(swatches(&custom.borrow()));
    app.set_surface_names(slint::ModelRc::new(slint::VecModel::from(
        Surface::ALL.iter().map(|surface| slint::SharedString::from(surface.name())).collect::<Vec<_>>(),
    )));
    app.set_brush_names(slint::ModelRc::new(slint::VecModel::from(
        tool_names().into_iter().map(slint::SharedString::from).collect::<Vec<_>>(),
    )));

    let [r, g, b] = PALETTE[2].coefficients.srgb();
    app.invoke_show_in_picker(slint::Color::from_rgb_u8(r, g, b));
    app.set_picker_hex(format!("#{r:02x}{g:02x}{b:02x}").into());

    let requests: Rc<RefCell<Requests>> = Rc::default();
    let started = Instant::now();
    let tablet = Rc::new(Tablet::install());

    let app_weak = app.as_weak();
    let shared = requests.clone();
    let pen = tablet.clone();
    app.on_pointer(move |kind, x, y| {
        let app = app_weak.unwrap();
        let scale = app.get_canvas_scale();
        if kind == HOVER {
            shared.borrow_mut().hover = Some((x * scale, y * scale));
            return;
        }
        let pen = pen.state();
        let (pressure, tilt, rotation) = if pen.present {
            (pen.brush_pressure(app.get_pen_firmness()), pen.tilt, pen.rotation)
        } else {
            (app.get_pressure(), [0.0; 2], 0.0)
        };
        shared.borrow_mut().push_pointer(PointerSample {
            phase: phase(kind),
            x: x * scale,
            y: y * scale,
            pressure,
            tilt,
            rotation,
            time: started.elapsed().as_secs_f64(),
        });
    });
    let shared = requests.clone();
    let app_weak = app.as_weak();
    let colours = custom.clone();
    app.on_dip(move |index| {
        app_weak.unwrap().set_active_swatch(index);
        let index = index.max(0) as usize;
        shared.borrow_mut().dip = Some(match index.checked_sub(PALETTE.len()) {
            Some(custom_index) => Dip::Colour(colours.borrow()[custom_index]),
            None => Dip::Pigment(index),
        });
    });
    let shared = requests.clone();
    app.on_clean(move || shared.borrow_mut().clean = true);
    let shared = requests.clone();
    app.on_dip_water(move || shared.borrow_mut().dip = Some(Dip::Water));
    let shared = requests.clone();
    let app_weak = app.as_weak();
    let colours = custom.clone();
    app.on_surface_selected(move |index| {
        let app = app_weak.unwrap();
        let surface = Surface::ALL[index.clamp(0, Surface::ALL.len() as i32 - 1) as usize];
        app.set_watercolour(surface.medium() == Medium::Watercolour);
        let mut requests = shared.borrow_mut();
        requests.surface = Some(surface);
        let active = app.get_active_swatch();
        requests.dip = Some(match active {
            index if index < 0 => Dip::Water,
            index => match (index as usize).checked_sub(PALETTE.len()) {
                Some(custom_index) => Dip::Colour(colours.borrow()[custom_index]),
                None => Dip::Pigment(index as usize),
            },
        });
    });
    let shared = requests.clone();
    let app_weak = app.as_weak();
    let colours = custom.clone();
    app.on_use_colour(move |colour| {
        let rgb = [colour.red(), colour.green(), colour.blue()];
        let app = app_weak.unwrap();
        select_custom_colour(&app, &mut colours.borrow_mut(), rgb);
        shared.borrow_mut().dip = Some(Dip::Colour(rgb));
    });
    let app_weak = app.as_weak();
    app.on_picker_changed(move |colour| {
        app_weak.unwrap().set_picker_hex(format!("#{:02x}{:02x}{:02x}", colour.red(), colour.green(), colour.blue()).into());
    });
    let app_weak = app.as_weak();
    app.on_hex_entered(move |text| {
        let app = app_weak.unwrap();
        match parse_hex(&text) {
            Some([r, g, b]) => app.invoke_show_in_picker(slint::Color::from_rgb_u8(r, g, b)),
            None => app.set_status(format!("not a colour: {text}").into()),
        }
    });
    let shared = requests.clone();
    let app_weak = app.as_weak();
    app.on_pick_colour(move |x, y| {
        let scale = app_weak.unwrap().get_canvas_scale();
        shared.borrow_mut().pick = Some((x * scale, y * scale));
    });
    let shared = requests.clone();
    let app_weak = app.as_weak();
    app.on_brush_selected(move |index| {
        let app = app_weak.unwrap();
        if let Some(named) = PRESETS.get(index.max(0) as usize) {
            app.set_brush_bristles(named.preset.adjusted(adjustments_of(&app, None)).bristle_count as f32);
        }
        shared.borrow_mut().brush = Some((index.max(0) as usize, adjustments_of(&app, None)));
    });
    let shared = requests.clone();
    let app_weak = app.as_weak();
    app.on_brush_adjusted(move || {
        let app = app_weak.unwrap();
        let count = app.get_brush_bristles().round() as u32;
        shared.borrow_mut().brush = Some((app.get_brush_index().max(0) as usize, adjustments_of(&app, Some(count))));
    });
    let shared = requests.clone();
    let app_weak = app.as_weak();
    app.on_paint_changed(move || {
        let app = app_weak.unwrap();
        shared.borrow_mut().paint = Some(PaintSettings { load: app.get_paint_load(), medium: app.get_paint_medium() });
    });
    let shared = requests.clone();
    let app_weak = app.as_weak();
    app.on_canvas_changed(move || {
        shared.borrow_mut().tooth = Some(app_weak.unwrap().get_canvas_tooth());
    });
    let shared = requests.clone();
    app.on_clear(move || shared.borrow_mut().clear = true);
    let shared = requests.clone();
    app.on_undo(move || shared.borrow_mut().undo = true);
    let shared = requests.clone();
    app.on_save(move || shared.borrow_mut().save = true);
    let shared = requests.clone();
    app.on_dry(move || shared.borrow_mut().dry = true);

    let app_weak = app.as_weak();
    let mut engine: Option<Engine> = None;
    let mut last_frame = Instant::now();
    let mut recorder: Option<Recorder> = None;
    let log_frames = std::env::var_os("PIGMENTLAB_LOG_FRAMES").is_some();
    app.window()
        .set_rendering_notifier(move |state, graphics_api| match state {
            slint::RenderingState::RenderingSetup => {
                if let slint::GraphicsAPI::WGPU30 { device, queue, .. } = graphics_api {
                    let mut created = Engine::new(Gpu::shared(device, queue), CANVAS, SEED);
                    created.dip(PALETTE[2]);
                    engine = Some(created);
                }
            }
            slint::RenderingState::BeforeRendering => {
                if let (Some(engine), Some(app)) = (engine.as_mut(), app_weak.upgrade()) {
                    let now = started.elapsed().as_secs_f64();
                    let pen = tablet.state();
                    requests.borrow_mut().follow_held_pen(pen, app.get_pen_firmness(), now);
                    update_recorder(&mut recorder, &app, now, &requests.borrow());
                    apply_requests(engine, &app, &mut requests.borrow_mut(), &mut custom.borrow_mut());
                    show_brush_contents(engine, &app);
                    engine.set_brush_angle(app.get_brush_angle());
                    engine.set_show_bristles(app.get_show_bristles());
                    engine.set_levelling_enabled(app.get_levelling_enabled());
                    engine.set_lighting(Lighting {
                        azimuth_degrees: app.get_light_azimuth(),
                        elevation_degrees: app.get_light_elevation(),
                        gloss: app.get_gloss(),
                        ..Lighting::default()
                    });
                    if pen.present {
                        app.set_pen_seen(true);
                    }
                    app.set_pen_status(if pen.present {
                        let brush = pen.brush_pressure(app.get_pen_firmness());
                        format!("Pen {:.0}% → brush {:.0}%  tilt {:+.2} {:+.2}", pen.pressure * 100.0, brush * 100.0, pen.tilt[0], pen.tilt[1]).into()
                    } else {
                        "Mouse".into()
                    });
                    engine.advance_realtime(started.elapsed().as_secs_f64());
                    let texture = engine.render().clone();
                    app.set_canvas_texture(slint::Image::try_from(texture).unwrap());
                    let now = Instant::now();
                    let frame_ms = (now - last_frame).as_secs_f32() * 1000.0;
                    app.set_frame_ms(frame_ms);
                    if log_frames {
                        eprintln!("frame {frame_ms:.1} ms");
                    }
                    last_frame = now;
                    app.window().request_redraw();
                }
            }
            slint::RenderingState::RenderingTeardown => drop(engine.take()),
            _ => {}
        })
        .expect("cannot register rendering notifier");

    app.run()
}

fn update_recorder(recorder: &mut Option<Recorder>, app: &AppWindow, now: f64, requests: &Requests) {
    if app.get_recording() {
        let active = recorder.get_or_insert_with(|| Recorder::start(CANVAS, SEED, now));
        if let Some(surface) = requests.surface {
            active.surface(now, surface);
        }
        match requests.dip {
            Some(Dip::Pigment(index)) => active.dip(now, PALETTE[index].name),
            Some(Dip::Colour(colour)) => active.dip_colour(now, colour),
            Some(Dip::Water) => active.dip_water(now),
            None => {}
        }
        if requests.clean {
            active.clean(now);
        }
        if let Some((index, adjustments)) = requests.brush {
            active.tool(now, tool_name(index));
            active.param(now, "size", adjustments.scale);
            if let Some(count) = adjustments.bristle_count {
                active.param(now, "bristles", count as f32);
            }
            active.param(now, "stiffness", adjustments.stiffness);
        }
        if let Some(paint) = requests.paint {
            active.param(now, "load", paint.load);
            active.param(now, "medium", paint.medium);
        }
        if let Some(tooth) = requests.tooth {
            active.param(now, "tooth", tooth);
        }
        if requests.clear {
            active.clear(now);
        }
        if requests.undo {
            active.undo(now);
        }
        if requests.dry {
            active.dry(now);
        }
        for sample in &requests.pointer {
            active.pointer(*sample);
        }
    } else if let Some(finished) = recorder.take() {
        let path = std::path::PathBuf::from(format!("scenarios/recorded-{}.json", stamp()));
        match finished.save(now, &path) {
            Ok(()) => app.set_status(format!("recorded {}", path.display()).into()),
            Err(error) => app.set_status(format!("recording failed: {error}").into()),
        }
    }
}

fn apply_requests(engine: &mut Engine, app: &AppWindow, requests: &mut Requests, custom: &mut Vec<[u8; 3]>) {
    if let Some((x, y)) = requests.pick.take() {
        let colour = engine.colour_at(x, y);
        select_custom_colour(app, custom, colour);
        app.invoke_show_in_picker(slint::Color::from_rgb_u8(colour[0], colour[1], colour[2]));
        requests.dip = Some(Dip::Colour(colour));
    }
    if let Some(surface) = requests.surface.take() {
        engine.set_surface(surface);
    }
    match requests.dip.take() {
        Some(Dip::Pigment(index)) => engine.dip(PALETTE[index]),
        Some(Dip::Colour(colour)) => engine.dip(Coefficients::from_srgb(colour)),
        Some(Dip::Water) => engine.dip_water(),
        None => {}
    }
    if std::mem::take(&mut requests.clean) {
        engine.clean();
    }
    if let Some((index, adjustments)) = requests.brush.take() {
        engine.configure_brush(tool_name(index), adjustments);
    }
    if let Some(paint) = requests.paint.take() {
        engine.set_paint(paint);
    }
    if let Some(tooth) = requests.tooth.take() {
        engine.set_param("tooth", tooth);
    }
    if std::mem::take(&mut requests.clear) {
        engine.clear();
    }
    if std::mem::take(&mut requests.dry) {
        engine.dry();
    }
    if std::mem::take(&mut requests.undo) {
        let restored = engine.undo();
        app.set_status(if restored { "undone".into() } else { "nothing to undo".into() });
    }
    if std::mem::take(&mut requests.save) {
        let path = save_path();
        match engine.save_png(&path) {
            Ok(()) => app.set_status(format!("saved {}", path.display()).into()),
            Err(error) => app.set_status(format!("save failed: {error}").into()),
        }
    }
    for sample in requests.pointer.drain(..) {
        engine.pointer(sample);
    }
    if let Some((x, y)) = requests.hover.take() {
        engine.hover(x, y);
    }
}

fn stamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn save_path() -> std::path::PathBuf {
    std::path::PathBuf::from(format!("pigmentlab-{}.png", stamp()))
}

fn swatches(custom: &[[u8; 3]]) -> slint::ModelRc<slint::Color> {
    let colours: Vec<slint::Color> = PALETTE
        .iter()
        .map(|pigment| pigment.coefficients.srgb())
        .chain(custom.iter().copied())
        .map(|[r, g, b]| slint::Color::from_rgb_u8(r, g, b))
        .collect();
    slint::ModelRc::new(slint::VecModel::from(colours))
}

fn select_custom_colour(app: &AppWindow, custom: &mut Vec<[u8; 3]>, colour: [u8; 3]) {
    let position = custom.iter().position(|existing| *existing == colour).unwrap_or_else(|| {
        if custom.len() == MAX_CUSTOM_COLOURS {
            custom.remove(0);
        }
        custom.push(colour);
        custom.len() - 1
    });
    app.set_swatches(swatches(custom));
    app.set_active_swatch((PALETTE.len() + position) as i32);
}

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| u8::from_str_radix(digits.get(range)?, 16).ok();
    Some([channel(0..2)?, channel(2..4)?, channel(4..6)?])
}

fn show_brush_contents(engine: &mut Engine, app: &AppWindow) {
    let contents = engine.brush_contents();
    app.set_brush_fill(contents.fill);
    if contents.fill > 0.0 {
        let [r, g, b] = contents.display_srgb();
        app.set_brush_paint(slint::Color::from_rgb_u8(r, g, b));
    }
}

fn phase(kind: i32) -> PointerPhase {
    match kind {
        0 => PointerPhase::Down,
        2 => PointerPhase::Up,
        _ => PointerPhase::Move,
    }
}

fn tool_name(index: usize) -> &'static str {
    let names = tool_names();
    names[index.min(names.len() - 1)]
}

fn adjustments_of(app: &AppWindow, bristle_count: Option<u32>) -> BrushAdjustments {
    BrushAdjustments { scale: app.get_brush_scale(), bristle_count, stiffness: app.get_brush_stiffness() }
}

#[cfg(test)]
mod tests {
    use super::parse_hex;

    #[test]
    fn hex_colours_parse_with_or_without_hash() {
        assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex(" 0a0B0c "), Some([10, 11, 12]));
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(parse_hex("#gg0000"), None);
    }
}
