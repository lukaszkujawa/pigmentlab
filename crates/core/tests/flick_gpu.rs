use pigmentlab_core::pigment::by_name;
use pigmentlab_core::units::METRES_PER_PIXEL;
use pigmentlab_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1024;
const LANE: f32 = 512.0;
const STROKE_END: f32 = 800.0;

fn sample(phase: PointerPhase, x: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y: LANE, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

struct Flick {
    engine: Engine,
    before: f32,
}

fn stroke_ending_at(medium: f32, final_speed: f32) -> Flick {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
    engine.select_brush("Round");
    engine.set_param("medium", medium);
    engine.dip(by_name("cadmium_red").unwrap());
    let before = engine.brush_load();
    engine.pointer(sample(PointerPhase::Down, 300.0, 0.6, 0.0));
    engine.pointer(sample(PointerPhase::Move, 650.0, 0.6, 0.7));
    let whip = (STROKE_END - 650.0) / (final_speed / METRES_PER_PIXEL);
    let mut time = 0.7;
    for k in 1..=10 {
        time = 0.7 + f64::from(whip) * k as f64 / 10.0;
        engine.pointer(sample(PointerPhase::Move, 650.0 + (STROKE_END - 650.0) * k as f32 / 10.0, 0.6, time));
    }
    engine.pointer(sample(PointerPhase::Up, STROKE_END, 0.0, time + 1.0 / 240.0));
    engine.advance_exact(time + 1.0);
    Flick { engine, before }
}

fn paint_beyond(engine: &mut Engine, x: usize) -> f32 {
    let heights = engine.paint_heights();
    (0..HEIGHT).flat_map(|y| (x..WIDTH).map(move |x| y * WIDTH + x)).map(|i| heights[i]).sum()
}

#[test]
fn a_hard_flick_throws_runny_paint_well_past_the_end_of_the_stroke() {
    let mut flick = stroke_ending_at(1.0, 1.5);
    let thrown = paint_beyond(&mut flick.engine, STROKE_END as usize + 120);
    println!("thrown past the stroke: {thrown:.0} of {:.0}", flick.before);
    assert!(thrown > flick.before * 0.01, "a hard flick should spatter paint ahead of the stroke, {thrown:.0} landed beyond it");
}

#[test]
fn a_flick_keeps_every_drop_of_paint() {
    let mut flick = stroke_ending_at(1.0, 1.5);
    let total = flick.engine.paint_mass() + flick.engine.brush_load() + flick.engine.drips().droplets().iter().map(|d| d.volume() / METRES_PER_PIXEL.powi(3)).sum::<f32>();
    assert!((total - flick.before).abs() < flick.before * 0.01, "paint must be kept: {:.0} before, {total:.0} after", flick.before);
}

#[test]
fn a_gentle_lift_or_stiff_paint_throws_nothing() {
    let mut gentle = stroke_ending_at(1.0, 0.2);
    let mut paste = stroke_ending_at(0.0, 1.5);
    for (what, flick) in [("a gentle lift", &mut gentle), ("stiff paste", &mut paste)] {
        let thrown = paint_beyond(&mut flick.engine, STROKE_END as usize + 120);
        assert!(thrown < flick.before * 0.0005, "{what} should not spatter, {thrown:.1} landed ahead of the stroke");
    }
}
