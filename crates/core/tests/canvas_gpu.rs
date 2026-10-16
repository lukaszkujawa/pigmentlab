use simpaint_core::pigment::by_name;
use simpaint_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: usize = 1024;

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

fn textured_engine(tooth: f32) -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    engine.set_param("tooth", tooth);
    engine
}

fn mean_paint_where(paint: &[f32], relief: &[f32], region: impl Fn(usize, usize) -> bool, pick: impl Fn(f32) -> bool) -> f32 {
    let mut total = 0.0;
    let mut count = 0usize;
    for y in 0..512 {
        for x in 0..WIDTH {
            let i = y * WIDTH + x;
            if region(x, y) && pick(relief[i]) {
                total += paint[i];
                count += 1;
            }
        }
    }
    total / count.max(1) as f32
}

#[test]
fn a_dry_brush_catches_the_weave_peaks() {
    let mut engine = textured_engine(1.5);
    engine.select_brush("Round large");
    engine.set_param("load", 0.1);
    engine.dip(by_name("ivory_black").unwrap());
    engine.pointer(sample(PointerPhase::Down, 150.0, 256.0, 0.2, 0.0));
    engine.pointer(sample(PointerPhase::Move, 900.0, 256.0, 0.2, 1.0));
    engine.pointer(sample(PointerPhase::Up, 900.0, 256.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    let relief = engine.canvas_relief();
    let paint = engine.paint_heights();
    let in_stroke = |x: usize, y: usize| (300..800).contains(&x) && (236..276).contains(&y);
    let on_peaks = mean_paint_where(&paint, &relief, in_stroke, |r| r > 0.6);
    let in_valleys = mean_paint_where(&paint, &relief, in_stroke, |r| r < -0.3);
    assert!(on_peaks > 0.03, "the dry brush should leave paint on the peaks, got {on_peaks:.3} px");
    assert!(on_peaks > in_valleys * 1.8, "dry paint should sit on the weave peaks ({on_peaks:.3}) more than in the valleys ({in_valleys:.3})");
}

#[test]
fn a_loaded_brush_fills_the_weave() {
    let mut engine = textured_engine(1.5);
    engine.select_brush("Round large");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 150.0, 256.0, 0.9, 0.0));
    engine.pointer(sample(PointerPhase::Move, 900.0, 256.0, 0.9, 1.2));
    engine.pointer(sample(PointerPhase::Up, 900.0, 256.0, 0.0, 1.2));
    engine.advance_exact(1.3);
    let relief = engine.canvas_relief();
    let paint = engine.paint_heights();
    let in_stroke = |x: usize, y: usize| (300..800).contains(&x) && (246..266).contains(&y);
    let on_peaks = mean_paint_where(&paint, &relief, in_stroke, |r| r > 0.6);
    let in_valleys = mean_paint_where(&paint, &relief, in_stroke, |r| r < -0.3);
    assert!(in_valleys > on_peaks + 0.5, "thick paint should fill the valleys deeper than the peaks ({in_valleys:.2} vs {on_peaks:.2} px)");
}

#[test]
fn a_shaper_leaves_paint_in_the_valleys() {
    let mut engine = textured_engine(1.5);
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, 300.0, 256.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 700.0, 256.0, 1.0, 1.0));
    engine.pointer(sample(PointerPhase::Up, 700.0, 256.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    engine.select_brush("Colour shaper");
    engine.set_param("angle", 90.0);
    engine.pointer(sample(PointerPhase::Down, 250.0, 256.0, 0.9, 1.1));
    engine.pointer(sample(PointerPhase::Move, 550.0, 256.0, 0.9, 1.7));
    engine.pointer(sample(PointerPhase::Up, 550.0, 256.0, 0.0, 1.7));
    engine.advance_exact(1.8);
    let relief = engine.canvas_relief();
    let paint = engine.paint_heights();
    let scraped = |x: usize, y: usize| (320..500).contains(&x) && (246..266).contains(&y);
    let on_peaks = mean_paint_where(&paint, &relief, scraped, |r| r > 0.6);
    let in_valleys = mean_paint_where(&paint, &relief, scraped, |r| r < -0.3);
    assert!(in_valleys > on_peaks * 1.5, "the blade should leave paint in the valleys ({in_valleys:.2}) and clear the peaks ({on_peaks:.2})");
}

#[test]
fn a_smooth_canvas_has_no_relief_and_strokes_stay_even() {
    let mut engine = textured_engine(0.0);
    assert!(engine.canvas_relief().iter().all(|r| *r == 0.0));
    engine.select_brush("Round large");
    engine.set_param("load", 0.3);
    engine.dip(by_name("ivory_black").unwrap());
    engine.pointer(sample(PointerPhase::Down, 150.0, 256.0, 0.25, 0.0));
    engine.pointer(sample(PointerPhase::Move, 900.0, 256.0, 0.25, 1.0));
    engine.pointer(sample(PointerPhase::Up, 900.0, 256.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    let paint = engine.paint_heights();
    let covered = (300..800).flat_map(|x| (246..266).map(move |y| y * WIDTH + x)).filter(|&i| paint[i] > 0.05).count();
    assert!(covered > 500 * 20 * 7 / 10, "on a smooth canvas the same dry stroke should cover most of its path, covered {covered} of 10000");
}
