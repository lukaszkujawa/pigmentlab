use pigmentlab_core::pigment::by_name;
use pigmentlab_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};
use std::time::Instant;

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

fn scrub(engine: &mut Engine, start: f64, passes: u32) -> f64 {
    engine.pointer(sample(PointerPhase::Down, 600.0, 700.0, 0.8, start));
    let mut t = start;
    for pass in 0..passes {
        t += 0.5;
        let x = if pass % 2 == 0 { 1000.0 } else { 600.0 };
        engine.pointer(sample(PointerPhase::Move, x, 700.0 + (pass % 3) as f32 * 6.0, 0.8, t));
    }
    engine.pointer(sample(PointerPhase::Up, 600.0, 700.0, 0.0, t));
    t
}

fn timed(label: &str, engine: &mut Engine, from: f64, to: f64) {
    engine.paint_mass();
    let started = Instant::now();
    let mut t = from;
    while t < to - 1e-6 {
        t += 4.0 / 240.0;
        engine.advance_exact(t);
        engine.render();
    }
    engine.paint_mass();
    println!("PERF {label}: {:.0} ms per simulated second", started.elapsed().as_secs_f64() * 1000.0 / (to - from));
}

#[test]
#[ignore]
fn report_cost_per_simulated_second() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: 2048, height: 1536 }, 1);
    engine.select_brush("Round medium");
    engine.dip(by_name("cadmium_red").unwrap());
    let end = scrub(&mut engine, 0.0, 2);
    timed("2048 medium fresh+scrub", &mut engine, 0.0, end);
    engine.dip(by_name("cadmium_red").unwrap());
    let end2 = scrub(&mut engine, end + 0.1, 4);
    timed("2048 medium scrub over paint", &mut engine, end + 0.1, end2);
    timed("2048 idle brush up", &mut engine, end2, end2 + 1.0);
    engine.set_levelling_enabled(false);
    timed("2048 idle, levelling off", &mut engine, end2 + 1.0, end2 + 2.0);
    engine.set_levelling_enabled(true);
    engine.select_brush("Round large");
    engine.dip(by_name("french_ultramarine").unwrap());
    let end3 = scrub(&mut engine, end2 + 2.0, 4);
    timed("2048 large scrub over paint", &mut engine, end2 + 2.0, end3);
    let mut water = Engine::new(Gpu::headless(), CanvasSize { width: 2048, height: 1536 }, 1);
    water.set_surface(pigmentlab_core::canvas::Surface::ColdPressed);
    water.select_brush("Round large");
    water.dip(by_name("french_ultramarine").unwrap());
    let wash = scrub(&mut water, 0.0, 4);
    timed("2048 watercolour large wash", &mut water, 0.0, wash);
    timed("2048 watercolour drying", &mut water, wash, wash + 2.0);
    let mut small = Engine::new(Gpu::headless(), CanvasSize { width: 1024, height: 768 }, 1);
    small.select_brush("Round medium");
    small.dip(by_name("cadmium_red").unwrap());
    let e = scrub(&mut small, 0.0, 2);
    timed("1024 medium fresh+scrub", &mut small, 0.0, e);
    timed("1024 idle brush up", &mut small, e, e + 1.0);
}
