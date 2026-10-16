use simpaint_core::pigment::by_name;
use simpaint_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: usize = 1024;

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

fn yellow_bar(engine: &mut Engine, medium: f32) -> f64 {
    engine.select_brush("Paint tube");
    engine.set_param("medium", medium);
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, 250.0, 256.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 750.0, 256.0, 1.0, 1.0));
    engine.pointer(sample(PointerPhase::Up, 750.0, 256.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    1.1
}

#[test]
fn a_full_brush_picks_up_colour_without_gaining_paint() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    let released = yellow_bar(&mut engine, 0.0);
    let canvas_before = engine.paint_mass();
    engine.select_brush("Round medium");
    engine.set_param("load", 1.0);
    engine.dip(by_name("cadmium_red").unwrap());
    let load_before = engine.brush_load();
    let colour_before = engine.brush_pigment();
    engine.pointer(sample(PointerPhase::Down, 150.0, 256.0, 0.7, released));
    engine.pointer(sample(PointerPhase::Move, 850.0, 256.0, 0.7, released + 1.0));
    engine.pointer(sample(PointerPhase::Up, 850.0, 256.0, 0.0, released + 1.0));
    engine.advance_exact(released + 1.1);
    let colour_after = engine.brush_pigment();
    let load_after = engine.brush_load();
    assert!(load_after <= load_before * 1.001, "a saturated brush must not gain paint ({load_before:.0} -> {load_after:.0})");
    assert!(colour_after.absorption.z > colour_before.absorption.z * 1.003, "the brush as a whole should shift towards yellow (blue absorption {:.3} -> {:.3})", colour_before.absorption.z, colour_after.absorption.z);
    let yellow = by_name("cadmium_yellow").unwrap().coefficients.absorption.z;
    let halfway = 0.5 * (colour_before.absorption.z + yellow);
    let samples = engine.brush_sample_absorption();
    let mostly_yellow = samples.iter().filter(|a| a.z > halfway).count();
    let tinted = samples.iter().filter(|a| a.z > colour_before.absorption.z * 1.07).count();
    assert!(mostly_yellow > 50 && tinted > 500, "the hairs that touched the bar should have taken on yellow: {mostly_yellow} mostly yellow, {tinted} tinted, of {}", samples.len());
    let total_before = canvas_before + load_before;
    let total_after = engine.paint_mass() + load_after;
    assert!((total_after - total_before).abs() < total_before * 0.01, "colour exchange must conserve paint ({total_before:.0} -> {total_after:.0})");
}

fn settled_stroke_roughness(medium: f32) -> f32 {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    engine.select_brush("Round large");
    engine.set_param("medium", medium);
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 150.0, 256.0, 0.9, 0.0));
    engine.pointer(sample(PointerPhase::Move, 850.0, 256.0, 0.9, 0.8));
    engine.pointer(sample(PointerPhase::Up, 850.0, 256.0, 0.0, 0.8));
    engine.advance_exact(3.0);
    let heights = engine.paint_heights();
    let mut roughness = 0.0;
    for y in 226..286 {
        for x in 300..700 {
            let i = y * WIDTH + x;
            roughness += (heights[i + WIDTH] - heights[i - WIDTH]).abs();
        }
    }
    roughness
}

fn bead_profile(medium: f32) -> (f32, usize) {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: 512 }, 1);
    let released = yellow_bar(&mut engine, medium);
    engine.advance_exact(released + 6.5);
    let heights = engine.paint_heights();
    let section: Vec<f32> = (196..316).map(|y| heights[y * WIDTH + 500]).collect();
    let peak = section.iter().cloned().fold(0.0f32, f32::max);
    let footprint = section.iter().filter(|h| **h > 0.3).count();
    (peak, footprint)
}

#[test]
fn an_oily_bead_slumps_while_a_stiff_bead_holds_its_shape() {
    let (stiff_peak, stiff_footprint) = bead_profile(0.0);
    let (oily_peak, oily_footprint) = bead_profile(1.0);
    assert!(oily_peak < stiff_peak * 0.92, "oily paint should slump ({stiff_peak:.2} stiff vs {oily_peak:.2} oily px)");
    assert!(oily_footprint > stiff_footprint + 6, "oily paint should spread ({stiff_footprint} vs {oily_footprint} px wide)");
}

#[test]
fn oily_paint_loses_its_brush_marks_while_stiff_paint_keeps_them() {
    let stiff = settled_stroke_roughness(0.0);
    let oily = settled_stroke_roughness(1.0);
    assert!(oily < stiff * 0.6, "oily paint should smooth its ridges ({stiff:.0} stiff vs {oily:.0} oily)");
}
