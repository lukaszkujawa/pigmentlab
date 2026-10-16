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
