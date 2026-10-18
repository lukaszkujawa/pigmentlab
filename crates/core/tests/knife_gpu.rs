use pigmentlab_core::pigment::by_name;
use pigmentlab_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: usize = 1024;
const HEIGHT: usize = 768;
const LANE: f32 = 500.0;
const DROP_Y: f32 = 150.0;
const STROKE_START: f32 = 100.0;
const STROKE_LENGTH: f32 = 600.0;
const DROP_SPREAD_LIMIT: usize = 240;
const LOADED_SPREAD_LIMIT: usize = 320;
const LOADED_STROKE_MINIMUM: usize = 80;
const ANGLES: [f32; 4] = [0.0, 90.0, 180.0, 270.0];
const PAST_TOUCHDOWN: std::ops::Range<usize> = 170..230;

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

fn fresh_engine() -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
    engine.set_levelling_enabled(false);
    engine
}

fn drag_knife(engine: &mut Engine, pressure: f32, angle: f32, start: f64) {
    engine.select_brush("Painting knife");
    engine.set_param("angle", angle);
    engine.pointer(sample(PointerPhase::Down, LANE, STROKE_START, pressure, start));
    for k in 1..=60 {
        let t = start + k as f64 / 30.0;
        engine.pointer(sample(PointerPhase::Move, LANE, STROKE_START + STROKE_LENGTH * k as f32 / 60.0, pressure, t));
        engine.advance_exact(t);
    }
    engine.pointer(sample(PointerPhase::Up, LANE, STROKE_START + STROKE_LENGTH, 0.0, start + 2.0));
    engine.advance_exact(start + 2.5);
}

struct Trace {
    heights: Vec<f32>,
}

impl Trace {
    fn of(engine: &mut Engine) -> Self {
        Self { heights: engine.paint_heights() }
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        self.heights[y * WIDTH + x]
    }

    fn mass_in_rows(&self, rows: std::ops::Range<usize>) -> f32 {
        rows.flat_map(|y| (0..WIDTH).map(move |x| (x, y))).map(|(x, y)| self.at(x, y)).sum()
    }

    fn last_painted_row(&self) -> usize {
        (0..HEIGHT).rev().find(|&y| (0..WIDTH).any(|x| self.at(x, y) > 0.05)).unwrap_or(0)
    }

    fn peak_in_rows(&self, rows: std::ops::Range<usize>) -> f32 {
        rows.flat_map(|y| (0..WIDTH).map(move |x| (x, y))).map(|(x, y)| self.at(x, y)).fold(0.0, f32::max)
    }

    fn centre_line(&self, rows: std::ops::Range<usize>) -> f32 {
        let lane = LANE as usize;
        let samples: Vec<f32> = rows.flat_map(|y| (lane - 4..lane + 4).map(move |x| (x, y))).map(|(x, y)| self.at(x, y)).collect();
        samples.iter().sum::<f32>() / samples.len() as f32
    }
}

struct DropResult {
    drop: f32,
    trace: Trace,
    on_knife: f32,
}

fn knife_through_a_drop(pressure: f32, angle: f32) -> DropResult {
    let mut engine = fresh_engine();
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, LANE, DROP_Y, 0.5, 0.0));
    engine.pointer(sample(PointerPhase::Move, LANE, DROP_Y, 0.5, 0.35));
    engine.pointer(sample(PointerPhase::Up, LANE, DROP_Y, 0.0, 0.35));
    engine.advance_exact(0.5);
    let drop = engine.paint_mass();
    engine.select_brush("Painting knife");
    engine.clean();
    drag_knife(&mut engine, pressure, angle, 0.5);
    let on_knife = engine.brush_load();
    let trace = Trace::of(&mut engine);
    let total = trace.heights.iter().sum::<f32>() + on_knife;
    assert!((total - drop).abs() < drop * 0.01, "the knife must neither create nor destroy paint ({drop:.0} -> {total:.0})");
    println!("pressure {pressure} angle {angle}: drop {drop:.0}, on canvas {:.0}, on knife {on_knife:.0}, reaches {} px past the drop, drop area {:.0}, centre over drop {:.2} px",
        trace.heights.iter().sum::<f32>(), trace.last_painted_row().saturating_sub(DROP_Y as usize), trace.mass_in_rows(100..200), trace.centre_line(140..160));
    DropResult { drop, trace, on_knife }
}

fn assert_stays_close_and_visible(result: &DropResult) {
    let reach = result.trace.last_painted_row().saturating_sub(DROP_Y as usize);
    assert!(reach <= DROP_SPREAD_LIMIT, "a drop must not be dragged further than 3 cm, paint reaches {reach} px past it");
    assert!(result.on_knife < result.drop * 0.2, "the paint must stay visible on the canvas, {:.0} of {:.0} vanished onto the knife", result.on_knife, result.drop);
}

#[test]
fn a_knife_resting_on_a_drop_barely_disturbs_it() {
    for angle in ANGLES {
        let result = knife_through_a_drop(0.0, angle);
        assert_stays_close_and_visible(&result);
        let kept = result.trace.mass_in_rows(100..200) / result.drop;
        assert!(kept > 0.6, "at {angle} degrees with no pressure the drop should mostly stay where it was, kept {:.0}%", kept * 100.0);
    }
}

#[test]
fn a_half_pressed_knife_smears_a_drop_into_a_short_layer() {
    for angle in ANGLES {
        let result = knife_through_a_drop(0.5, angle);
        assert_stays_close_and_visible(&result);
        let smeared = result.trace.mass_in_rows(200..HEIGHT) / result.drop;
        assert!(smeared > 0.2, "at {angle} degrees half pressure should smear the drop out past where it was, smeared {:.0}%", smeared * 100.0);
    }
}

#[test]
fn a_fully_pressed_knife_scrapes_a_drop_and_squeezes_it_to_the_sides() {
    for angle in ANGLES {
        let result = knife_through_a_drop(1.0, angle);
        assert_stays_close_and_visible(&result);
        let centre = result.trace.centre_line(140..160);
        assert!(centre < 0.3, "at {angle} degrees full pressure should scrape the middle of the path down to a stain, left {centre:.2} px");
    }
}

struct LoadedResult {
    dipped: f32,
    trace: Trace,
    on_knife: f32,
}

fn loaded_knife(pressure: f32, angle: f32) -> LoadedResult {
    let mut engine = fresh_engine();
    engine.select_brush("Painting knife");
    engine.dip(by_name("cadmium_yellow").unwrap());
    let dipped = engine.brush_load();
    drag_knife(&mut engine, pressure, angle, 0.0);
    let on_knife = engine.brush_load();
    let trace = Trace::of(&mut engine);
    println!("pressure {pressure} angle {angle}: dipped {dipped:.0}, laid {:.0}, kept {on_knife:.0}, reaches {} px, peak in first 100 px {:.2}",
        trace.heights.iter().sum::<f32>(), trace.last_painted_row().saturating_sub(STROKE_START as usize), trace.peak_in_rows(100..200));
    LoadedResult { dipped, trace, on_knife }
}

fn assert_runs_out_soon(result: &LoadedResult) {
    let reach = result.trace.last_painted_row().saturating_sub(STROKE_START as usize);
    assert!(reach <= LOADED_SPREAD_LIMIT, "a dip should be used up within 4 cm, paint reaches {reach} px");
    assert!(reach >= LOADED_STROKE_MINIMUM, "a full dip should make a stroke, not a dab, paint reaches only {reach} px");
    assert!(result.on_knife < result.dipped * 0.25, "the paint should leave the knife, {:.0} of {:.0} stayed on it", result.on_knife, result.dipped);
}

#[test]
fn a_lightly_held_loaded_knife_lays_a_thick_layer() {
    for angle in ANGLES {
        let result = loaded_knife(0.0, angle);
        assert_runs_out_soon(&result);
        let peak = result.trace.peak_in_rows(100..200);
        assert!(peak > 4.0, "at {angle} degrees a light knife should lay thick paint, peak {peak:.2} px");
    }
}

#[test]
fn a_half_pressed_loaded_knife_lays_a_medium_layer() {
    for angle in ANGLES {
        let result = loaded_knife(0.5, angle);
        assert_runs_out_soon(&result);
        let peak = result.trace.peak_in_rows(100..200);
        assert!(peak > 2.0, "at {angle} degrees half pressure should still lay a solid layer, peak {peak:.2} px");
    }
}

#[test]
fn a_fully_pressed_loaded_knife_lays_a_thin_layer() {
    for angle in ANGLES {
        let result = loaded_knife(1.0, angle);
        assert_runs_out_soon(&result);
        let light = loaded_knife(0.0, angle).trace.centre_line(PAST_TOUCHDOWN);
        let under_blade = result.trace.centre_line(PAST_TOUCHDOWN);
        assert!(under_blade < light * 0.3, "at {angle} degrees past the touchdown full pressure should leave only a thin layer ({under_blade:.2} vs {light:.2} px when held lightly)");
    }
}
