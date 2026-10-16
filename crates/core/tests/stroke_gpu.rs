use simpaint_core::brush::BrushAdjustments;
use simpaint_core::pigment::by_name;
use simpaint_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: usize = 1024;

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

fn blob_around(engine: &mut Engine, centre: (usize, usize)) -> (f32, f32, f32) {
    let heights = engine.paint_heights();
    let (mut mass, mut x_moment, mut y_moment) = (0.0f32, 0.0f32, 0.0f32);
    for y in centre.1 - 40..centre.1 + 40 {
        for x in centre.0 - 40..centre.0 + 40 {
            let height = heights[y * WIDTH + x];
            mass += height;
            x_moment += height * x as f32;
            y_moment += height * y as f32;
        }
    }
    (mass, x_moment / mass.max(1e-6), y_moment / mass.max(1e-6))
}

#[test]
fn paint_at_the_stroke_start_stays_put_once_the_brush_has_left() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    engine.configure_brush("Round small", BrushAdjustments { scale: 0.4, bristle_count: Some(600), stiffness: 0.5 });
    engine.dip(by_name("cadmium_red").unwrap());
    let start = (500.0f32, 256.0f32);
    engine.pointer(sample(PointerPhase::Down, start.0, start.1, 0.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, start.0 + 1.0, start.1, 0.0, 0.5));
    for i in 1..=80 {
        let angle = i as f32 / 80.0 * std::f32::consts::TAU * 1.5;
        let radius = 60.0 + 60.0 * i as f32 / 80.0;
        engine.pointer(sample(PointerPhase::Move, start.0 + radius * angle.cos(), start.1 + radius * angle.sin(), 0.0, 0.5 + 2.0 * i as f64 / 80.0));
    }
    engine.pointer(sample(PointerPhase::Up, start.0 + 120.0, start.1, 0.0, 2.5));
    engine.advance_exact(0.9);
    let (mass_before, x_before, y_before) = blob_around(&mut engine, (start.0 as usize, start.1 as usize));
    assert!(mass_before > 0.0, "the stroke start should hold paint");
    engine.advance_exact(2.6);
    let (mass_after, x_after, y_after) = blob_around(&mut engine, (start.0 as usize, start.1 as usize));
    let change = (mass_after - mass_before).abs() / mass_before;
    assert!(change < 0.03, "paint at the start changed by {:.0}% after the brush left ({mass_before:.0} -> {mass_after:.0})", change * 100.0);
    let drift = ((x_after - x_before).powi(2) + (y_after - y_before).powi(2)).sqrt();
    assert!(drift < 2.0, "the start blob moved {drift:.1} px after the brush left");
}

fn stroke_width_at(pressure: f32) -> usize {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Round medium");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 300.0, 256.0, pressure, 0.0));
    engine.pointer(sample(PointerPhase::Move, 700.0, 256.0, pressure, 1.0));
    engine.pointer(sample(PointerPhase::Up, 700.0, 256.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    let heights = engine.paint_heights();
    (156..356).filter(|y| heights[y * WIDTH + 500] > 0.05).count()
}

#[test]
fn a_light_stroke_is_narrower_than_a_heavy_one() {
    let light = stroke_width_at(0.05);
    let heavy = stroke_width_at(1.0);
    assert!(light > 0, "a light stroke must still leave paint");
    assert!(light < heavy * 6 / 10, "light stroke {light} px wide vs heavy {heavy} px");
}

fn ridge_roughness(engine: &mut Engine) -> f32 {
    let heights = engine.paint_heights();
    let mut roughness = 0.0;
    for y in 226..286 {
        for x in 350..650 {
            let i = y * WIDTH + x;
            roughness += (heights[i + WIDTH] - heights[i - WIDTH]).abs();
        }
    }
    roughness
}

fn paint_moved(before: &[f32], after: &[f32]) -> f32 {
    before.iter().zip(after).map(|(b, a)| (a - b).abs()).sum()
}

fn pass_over_a_heavy_stroke(pressure: f32) -> (f32, f32, f32) {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Round large");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 300.0, 256.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 700.0, 256.0, 1.0, 1.0));
    engine.pointer(sample(PointerPhase::Up, 700.0, 256.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    let before = ridge_roughness(&mut engine);
    let heights_before = engine.paint_heights();
    engine.set_param("load", 0.0);
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 300.0, 256.0, pressure, 1.2));
    engine.pointer(sample(PointerPhase::Move, 700.0, 256.0, pressure, 2.2));
    engine.pointer(sample(PointerPhase::Up, 700.0, 256.0, 0.0, 2.2));
    engine.advance_exact(2.3);
    let after = ridge_roughness(&mut engine);
    let moved = paint_moved(&heights_before, &engine.paint_heights());
    (before, after, moved)
}

#[test]
fn a_light_pass_smooths_ridges_while_a_heavy_pass_reshapes_the_paint() {
    let (before, after_light, moved_light) = pass_over_a_heavy_stroke(0.05);
    let (_, _, moved_heavy) = pass_over_a_heavy_stroke(1.0);
    assert!(after_light < before, "a light pass should plane the ridges down: {before:.0} -> {after_light:.0}");
    assert!(moved_heavy > moved_light * 2.0, "a heavy pass should move far more paint than a light one: {moved_heavy:.0} vs {moved_light:.0}");
}
