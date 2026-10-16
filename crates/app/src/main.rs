slint::include_modules!();

mod tablet;

use simpaint_core::brush::BrushAdjustments;
use simpaint_core::engine::{PaintSettings, tool_names};
use simpaint_core::scenario::Recorder;
use simpaint_core::pigment::{PALETTE, linear_to_srgb};
use simpaint_core::{CanvasSize, Engine, Gpu, Lighting, PointerPhase, PointerSample};
use tablet::Tablet;
use slint::wgpu_30::{WGPUConfiguration, WGPUSettings};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

const CANVAS: CanvasSize = CanvasSize { width: 2048, height: 1536 };
const SEED: u64 = 1;

#[derive(Default)]
struct Requests {
    pointer: Vec<PointerSample>,
    dip: Option<usize>,
    brush: Option<(usize, BrushAdjustments)>,
    paint: Option<PaintSettings>,
    clear: bool,
    undo: bool,
    save: bool,
    dry: bool,
}

fn main() -> Result<(), slint::PlatformError> {
    let mut wgpu_settings = WGPUSettings::default();
    wgpu_settings.device_required_limits = simpaint_core::gpu::required_limits();
    slint::BackendSelector::new()
        .require_wgpu_30(WGPUConfiguration::Automatic(wgpu_settings))
        .select()?;

    let app = AppWindow::new()?;
    let scale = app.window().scale_factor().max(1.0);
    app.set_canvas_width(CANVAS.width as f32 / scale);
    app.set_canvas_height(CANVAS.height as f32 / scale);
    app.set_canvas_scale(scale);
    app.set_swatches(swatches());
    app.set_brush_names(slint::ModelRc::new(slint::VecModel::from(
        tool_names().into_iter().map(slint::SharedString::from).collect::<Vec<_>>(),
    )));

    let requests: Rc<RefCell<Requests>> = Rc::default();
    let started = Instant::now();
    let tablet = Rc::new(Tablet::install());

    let app_weak = app.as_weak();
    let shared = requests.clone();
    let pen = tablet.clone();
    app.on_pointer(move |kind, x, y| {
        let app = app_weak.unwrap();
        let pen = pen.state();
        let (pressure, tilt, rotation) =
            if pen.present { (pen.pressure, pen.tilt, pen.rotation) } else { (app.get_pressure(), [0.0; 2], 0.0) };
        let scale = app.get_canvas_scale();
        shared.borrow_mut().pointer.push(PointerSample {
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
    app.on_dip(move |index| {
        app_weak.unwrap().set_active_swatch(index);
        shared.borrow_mut().dip = Some(index as usize);
    });
    let shared = requests.clone();
    let app_weak = app.as_weak();
    app.on_brush_changed(move || {
        let app = app_weak.unwrap();
        shared.borrow_mut().brush = Some((
            app.get_brush_index().max(0) as usize,
            BrushAdjustments {
                scale: app.get_brush_scale(),
                bristle_count: Some(app.get_brush_bristles().round() as u32),
                stiffness: app.get_brush_stiffness(),
            },
        ));
    });
    let shared = requests.clone();
    let app_weak = app.as_weak();
    app.on_paint_changed(move || {
        let app = app_weak.unwrap();
        shared.borrow_mut().paint = Some(PaintSettings { load: app.get_paint_load(), medium: app.get_paint_medium() });
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
    let log_frames = std::env::var_os("SIMPAINT_LOG_FRAMES").is_some();
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
                    update_recorder(&mut recorder, &app, now, &requests.borrow());
                    apply_requests(engine, &app, &mut requests.borrow_mut());
                    engine.set_brush_angle(app.get_brush_angle());
                    engine.set_show_bristles(app.get_show_bristles());
                    engine.set_levelling_enabled(app.get_levelling_enabled());
                    engine.set_lighting(Lighting {
                        azimuth_degrees: app.get_light_azimuth(),
                        elevation_degrees: app.get_light_elevation(),
                        gloss: app.get_gloss(),
                        ..Lighting::default()
                    });
                    let pen = tablet.state();
                    app.set_pen_present(pen.present);
                    app.set_pen_status(if pen.present {
                        format!("Pen {:.0}%  tilt {:+.2} {:+.2}", pen.pressure * 100.0, pen.tilt[0], pen.tilt[1]).into()
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
        if let Some(index) = requests.dip {
            active.dip(now, PALETTE[index].name);
        }
        if let Some((index, adjustments)) = requests.brush {
            active.tool(now, tool_name(index));
            active.param(now, "size", adjustments.scale);
            active.param(now, "bristles", adjustments.bristle_count.unwrap_or(0) as f32);
            active.param(now, "stiffness", adjustments.stiffness);
        }
        if let Some(paint) = requests.paint {
            active.param(now, "load", paint.load);
            active.param(now, "medium", paint.medium);
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
    if let Some((index, adjustments)) = requests.brush.take() {
        engine.select_brush(tool_name(index));
        engine.set_brush_adjustments(adjustments);
    }
    if let Some(paint) = requests.paint.take() {
        engine.set_paint(paint);
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

fn tool_name(index: usize) -> &'static str {
    let names = tool_names();
    names[index.min(names.len() - 1)]
}
