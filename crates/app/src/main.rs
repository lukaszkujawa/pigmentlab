slint::include_modules!();

mod tablet;

use simpaint_core::brush::BrushPreset;
use simpaint_core::scenario::Recorder;
use simpaint_core::pigment::{PALETTE, linear_to_srgb};
use simpaint_core::units::cm;
use simpaint_core::{CanvasSize, Engine, Gpu, Lighting, PointerPhase, PointerSample};
use tablet::Tablet;
use slint::wgpu_30::{WGPUConfiguration, WGPUSettings, wgpu};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

const CANVAS: CanvasSize = CanvasSize { width: 1024, height: 768 };
const SEED: u64 = 1;
const REFERENCE_RADIUS_CM: f32 = 0.35;
const REFERENCE_BRISTLES: f32 = 200.0;

#[derive(Default)]
struct Requests {
    pointer: Vec<PointerSample>,
    dip: Option<usize>,
    brush_radius_cm: Option<f32>,
    clear: bool,
    undo: bool,
    save: bool,
    dry: bool,
}

fn main() -> Result<(), slint::PlatformError> {
    let mut wgpu_settings = WGPUSettings::default();
    wgpu_settings.device_required_limits = wgpu::Limits::default();
    slint::BackendSelector::new()
        .require_wgpu_30(WGPUConfiguration::Automatic(wgpu_settings))
        .select()?;

    let app = AppWindow::new()?;
    app.set_canvas_width(CANVAS.width as f32);
    app.set_canvas_height(CANVAS.height as f32);
    app.set_swatches(swatches());

    let requests: Rc<RefCell<Requests>> = Rc::default();
    let started = Instant::now();
    let tablet = Rc::new(Tablet::install());

    let app_weak = app.as_weak();
    let shared = requests.clone();
    let pen = tablet.clone();
    app.on_pointer(move |kind, x, y| {
        let app = app_weak.unwrap();
        let pen = pen.state();
        let (pressure, tilt) = if pen.present { (pen.pressure, pen.tilt) } else { (app.get_pressure(), [0.0; 2]) };
        shared.borrow_mut().pointer.push(PointerSample {
            phase: phase(kind),
            x,
            y,
            pressure,
            tilt,
            time: started.elapsed().as_secs_f64(),
        });
    });
    let shared = requests.clone();
    app.on_dip(move |index| shared.borrow_mut().dip = Some(index as usize));
    let shared = requests.clone();
    let app_weak = app.as_weak();
    app.on_brush_changed(move || shared.borrow_mut().brush_radius_cm = Some(app_weak.unwrap().get_brush_radius_cm()));
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
    app.window()
        .set_rendering_notifier(move |state, graphics_api| match state {
            slint::RenderingState::RenderingSetup => {
                if let slint::GraphicsAPI::WGPU30 { device, queue, .. } = graphics_api {
                    let created = Engine::new(Gpu::shared(device, queue), CANVAS, SEED);
                    created.dip(PALETTE[2]);
                    engine = Some(created);
                }
            }
            slint::RenderingState::BeforeRendering => {
                if let (Some(engine), Some(app)) = (engine.as_mut(), app_weak.upgrade()) {
                    let now = started.elapsed().as_secs_f64();
                    update_recorder(&mut recorder, &app, now, &requests.borrow());
                    apply_requests(engine, &app, &mut requests.borrow_mut());
                    engine.set_show_bristles(app.get_show_bristles());
                    engine.set_fluid_enabled(app.get_fluid_enabled());
                    engine.set_lighting(Lighting {
                        azimuth_degrees: app.get_light_azimuth(),
                        elevation_degrees: app.get_light_elevation(),
                        gloss: app.get_gloss(),
                        ..Lighting::default()
                    });
                    let pen = tablet.state();
                    app.set_pen_status(if pen.present {
                        format!("Pen {:.0}% tilt {:+.2} {:+.2}", pen.pressure * 100.0, pen.tilt[0], pen.tilt[1]).into()
                    } else {
                        "Pen: none (slider pressure)".into()
                    });
                    engine.advance_realtime(started.elapsed().as_secs_f64());
                    let texture = engine.render().clone();
                    app.set_canvas_texture(slint::Image::try_from(texture).unwrap());
                    let now = Instant::now();
                    app.set_frame_ms((now - last_frame).as_secs_f32() * 1000.0);
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
        if let Some(index) = requests.dip {
            active.dip(now, PALETTE[index].name);
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

fn apply_requests(engine: &mut Engine, app: &AppWindow, requests: &mut Requests) {
    if let Some(index) = requests.dip.take() {
        engine.dip(PALETTE[index]);
    }
    if let Some(radius_cm) = requests.brush_radius_cm.take() {
        engine.set_brush(preset_for_radius(radius_cm));
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
}

fn preset_for_radius(radius_cm: f32) -> BrushPreset {
    let scale = radius_cm / REFERENCE_RADIUS_CM;
    let bristles = (REFERENCE_BRISTLES * scale * scale).round().clamp(40.0, 600.0) as u32;
    BrushPreset::round(bristles, cm(radius_cm), cm(1.6 + radius_cm * 1.8))
}

fn stamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn save_path() -> std::path::PathBuf {
    std::path::PathBuf::from(format!("simpaint-{}.png", stamp()))
}

fn swatches() -> slint::ModelRc<slint::Color> {
    let colors: Vec<slint::Color> = PALETTE
        .iter()
        .map(|pigment| {
            let reflectance = pigment.coefficients.opaque_reflectance();
            let channel = |v: f32| (linear_to_srgb(v) * 255.0).round() as u8;
            slint::Color::from_rgb_u8(channel(reflectance.x), channel(reflectance.y), channel(reflectance.z))
        })
        .collect();
    slint::ModelRc::new(slint::VecModel::from(colors))
}

fn phase(kind: i32) -> PointerPhase {
    match kind {
        0 => PointerPhase::Down,
        2 => PointerPhase::Up,
        _ => PointerPhase::Move,
    }
}
