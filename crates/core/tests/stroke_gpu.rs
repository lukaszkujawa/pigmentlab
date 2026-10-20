use pigmentlab_core::brush::{BrushAdjustments, SAMPLES_PER_BRISTLE};
use pigmentlab_core::paint::OVERLOADED_FILL;
use pigmentlab_core::pigment::by_name;
use pigmentlab_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

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
    engine.configure_brush("Round small", BrushAdjustments { scale: 0.4, bristle_count: Some(600), ..Default::default() });
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

#[test]
fn a_stroke_ends_in_a_narrowing_tail() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Round medium");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 200.0, 256.0, 0.8, 0.0));
    engine.pointer(sample(PointerPhase::Move, 600.0, 256.0, 0.8, 1.0));
    engine.pointer(sample(PointerPhase::Up, 600.0, 256.0, 0.0, 1.0));
    engine.advance_exact(1.3);
    let heights = engine.paint_heights();
    let width_at = |x: usize| (156..356).filter(|y| heights[y * WIDTH + x] > 0.05).count();
    let body = width_at(400);
    let tail: Vec<usize> = (560..680).step_by(20).map(width_at).collect();
    assert!(tail.iter().any(|&w| w > 0 && w < body / 2), "the stroke should end in a narrowing tail, body {body} px, tail {tail:?}");
    assert!(tail.windows(2).all(|pair| pair[1] <= pair[0] + 2), "the tail should narrow towards its end: {tail:?}");
}

#[test]
fn releasing_a_still_brush_leaves_the_paint_alone() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Round large");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 200.0, 256.0, 0.9, 0.0));
    engine.pointer(sample(PointerPhase::Move, 600.0, 256.0, 0.9, 0.8));
    engine.pointer(sample(PointerPhase::Move, 600.0, 256.0, 0.9, 1.2));
    engine.pointer(sample(PointerPhase::Up, 600.0, 256.0, 0.0, 1.2));
    engine.advance_exact(1.19);
    let held = engine.paint_heights();
    engine.advance_exact(1.6);
    let released = engine.paint_heights();
    let total: f32 = held.iter().sum();
    let changed: f32 = held.iter().zip(&released).map(|(a, b)| (a - b).abs()).sum();
    assert!(changed < total * 0.005, "lifting a still brush reshaped {:.1}% of the paint", changed / total * 100.0);
}

fn strokes_until_dry(load: f32, strokes: usize) -> (Vec<f32>, Vec<f32>, f32) {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: (strokes * 80) as u32 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Round medium");
    engine.set_param("load", load);
    engine.dip(by_name("cadmium_red").unwrap());
    let full = engine.brush_load();
    let (mut peaks, mut coats) = (Vec::new(), Vec::new());
    for row in 0..strokes {
        let y = 40 + row * 80;
        let start = row as f64 * 1.4;
        engine.pointer(sample(PointerPhase::Down, 80.0, y as f32, 0.6, start));
        engine.pointer(sample(PointerPhase::Move, 940.0, y as f32, 0.6, start + 1.2));
        engine.pointer(sample(PointerPhase::Up, 940.0, y as f32, 0.0, start + 1.2));
        engine.advance_exact(start + 1.4);
        let heights = engine.paint_heights();
        peaks.push((y - 20..y + 20).map(|row| heights[row * WIDTH + 500]).fold(0.0f32, f32::max));
        let mut painted: Vec<f32> = (y - 40..y + 40).flat_map(|row| (300..700).map(move |x| row * WIDTH + x)).map(|i| heights[i]).filter(|h| *h > 0.05).collect();
        painted.sort_by(f32::total_cmp);
        coats.push(painted.get(painted.len() / 2).copied().unwrap_or(0.0));
    }
    (peaks, coats, engine.brush_load() / full)
}

#[test]
fn a_loaded_brush_runs_out_over_a_few_long_strokes() {
    let (peaks, _, remaining) = strokes_until_dry(1.0, 10);
    assert!(peaks[0] > 2.0, "the first stroke should lay down thick paint, peak {:.2} px", peaks[0]);
    assert!(peaks[9] < peaks[0] * 0.25, "the brush should be nearly empty after ten strokes: peaks {peaks:?}");
    assert!(remaining < 0.25, "the brush still holds {:.0}% of its paint", remaining * 100.0);
}

#[test]
fn an_overloaded_brush_lays_impasto_that_thins_to_the_fitting_coat() {
    let (_, fitting, _) = strokes_until_dry(1.0, 2);
    let (_, overloaded, _) = strokes_until_dry(OVERLOADED_FILL, 6);
    assert!(fitting[0] < 2.5, "a fitting load should lay a thin coat, got {:.2} px", fitting[0]);
    assert!(overloaded[0] > fitting[0] * 2.0, "an overloaded brush should lay impasto: {:.2} px vs a fitting {:.2} px", overloaded[0], fitting[0]);
    assert!(overloaded.windows(2).all(|pair| pair[1] < pair[0] * 1.1), "the overloaded coat should only thin stroke by stroke: {overloaded:?}");
    assert!((overloaded[5] - fitting[0]).abs() < fitting[0] * 0.5, "once the excess is spent the coat should settle near the fitting one: {overloaded:?} vs {:.2}", fitting[0]);
}

fn settled_contents(engine: &mut Engine) -> pigmentlab_core::paint::BrushContents {
    for _ in 0..2 {
        engine.brush_contents();
        engine.brush_load();
    }
    engine.brush_contents()
}

#[test]
fn the_brush_gauge_follows_paint_leaving_and_entering_the_brush() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, 300.0, 400.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 900.0, 400.0, 1.0, 1.0));
    engine.pointer(sample(PointerPhase::Up, 900.0, 400.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    engine.select_brush("Round medium");
    assert_eq!(engine.brush_contents().fill, 0.0, "a freshly selected brush is clean");
    engine.set_param("load", 0.6);
    engine.dip(by_name("french_ultramarine").unwrap());
    let dipped = engine.brush_contents();
    assert!((dipped.fill - 0.6).abs() < 1e-6, "dipping shows the chosen load at once, got {}", dipped.fill);
    engine.pointer(sample(PointerPhase::Down, 100.0, 150.0, 0.7, 1.2));
    engine.pointer(sample(PointerPhase::Move, 900.0, 150.0, 0.7, 2.2));
    engine.pointer(sample(PointerPhase::Up, 900.0, 150.0, 0.0, 2.2));
    engine.advance_exact(2.4);
    let painted = settled_contents(&mut engine);
    assert!(painted.fill < dipped.fill - 0.05, "painting should drain the gauge: {} -> {}", dipped.fill, painted.fill);
    let preset = *engine.brush().preset();
    let exact = engine.brush_load() / (preset.sample_capacity * (preset.bristle_count * SAMPLES_PER_BRISTLE) as f32);
    assert!((painted.fill - exact).abs() < 0.01, "the gauge should match the brush's real load: {} vs {exact}", painted.fill);
    engine.set_param("load", 0.1);
    engine.dip(by_name("french_ultramarine").unwrap());
    let light = engine.brush_contents();
    engine.pointer(sample(PointerPhase::Down, 250.0, 400.0, 0.9, 2.5));
    engine.pointer(sample(PointerPhase::Move, 950.0, 400.0, 0.9, 3.5));
    engine.pointer(sample(PointerPhase::Up, 950.0, 400.0, 0.0, 3.5));
    engine.advance_exact(3.7);
    let wiped = settled_contents(&mut engine);
    assert!(wiped.fill > light.fill, "dragging a lightly loaded brush through a thick bead should load it up: {} -> {}", light.fill, wiped.fill);
    assert!(wiped.paint.absorption.z > dipped.paint.absorption.z * 1.1, "the brush should take on yellow: blue absorption {} -> {}", dipped.paint.absorption.z, wiped.paint.absorption.z);
    engine.clean();
    assert_eq!(engine.brush_contents().fill, 0.0, "cleaning empties the gauge");
}

#[test]
fn the_eyedropper_reads_the_colour_on_the_canvas() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.select_brush("Paint tube");
    engine.dip(pigmentlab_core::pigment::Coefficients::from_srgb([40, 90, 200]));
    engine.pointer(sample(PointerPhase::Down, 300.0, 256.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 700.0, 256.0, 1.0, 1.0));
    engine.pointer(sample(PointerPhase::Up, 700.0, 256.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    let picked = engine.colour_at(500.0, 256.0);
    let canvas = engine.colour_at(500.0, 60.0);
    assert!(picked[2] > picked[0] + 60 && picked[2] > picked[1] + 40, "the bead should read blue, got {picked:?}");
    assert!(canvas.iter().all(|c| *c > 180), "bare canvas should read pale, got {canvas:?}");
}

#[test]
fn a_steeply_tilted_pen_still_paints_under_the_pointer() {
    for tilt in [-1.0, 1.0] {
        let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: 512, height: 512 }, 1);
        engine.set_levelling_enabled(false);
        engine.select_brush("Round medium");
        engine.dip(by_name("cadmium_red").unwrap());
        let at = |phase, pressure, time| PointerSample { phase, x: 256.0, y: 256.0, pressure, tilt: [0.0, tilt], rotation: 0.0, time };
        engine.pointer(at(PointerPhase::Down, 0.002, 0.0));
        for i in 1..=20 {
            engine.pointer(at(PointerPhase::Move, 0.8 * i as f32 / 20.0, i as f64 * 0.015));
        }
        engine.advance_exact(0.6);
        let heights = engine.paint_heights();
        let (mut mass, mut moment) = (0.0, 0.0);
        for y in 0..512 {
            for x in 0..512 {
                mass += heights[y * 512 + x];
                moment += heights[y * 512 + x] * y as f32;
            }
        }
        let offset = moment / mass - 256.0;
        assert!(heights[256 * 512 + 256] > 0.0, "tilt {tilt}: the dab should cover the pointer");
        assert!(offset.abs() < 25.0, "tilt {tilt}: the dab centre drifted {offset:.0} px from the pointer");
    }
}

fn flat_stroke_width(tilt: [f32; 2]) -> usize {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: 1024, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Flat");
    engine.dip(by_name("cadmium_red").unwrap());
    let at = |phase, x, pressure, time| PointerSample { phase, x, y: 256.0, pressure, tilt, rotation: 0.0, time };
    engine.pointer(at(PointerPhase::Down, 300.0, 0.5, 0.0));
    engine.pointer(at(PointerPhase::Move, 700.0, 0.5, 1.0));
    engine.pointer(at(PointerPhase::Up, 700.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    let heights = engine.paint_heights();
    (56..456).filter(|y| heights[y * 1024 + 500] > 0.05).count()
}

#[test]
fn a_flat_brush_turns_with_the_pen_tilt() {
    let broadside = flat_stroke_width([0.5, 0.0]);
    let edge_on = flat_stroke_width([0.0, 0.5]);
    let (wide, narrow) = (broadside.max(edge_on), broadside.min(edge_on));
    assert!(wide > narrow * 2, "turning the pen should turn the flat: {broadside} px vs {edge_on} px");
}
