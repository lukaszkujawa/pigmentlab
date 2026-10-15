use simpaint_core::pigment::by_name;
use simpaint_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

#[test]
fn pushing_a_clean_brush_through_paint_conserves_mass() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: 512, height: 384 }, 3);
    engine.dip(by_name("ivory_black").unwrap());
    engine.pointer(sample(PointerPhase::Down, 200.0, 200.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Up, 200.0, 200.0, 1.0, 0.5));
    engine.advance_exact(0.6);
    let blob = engine.paint_mass();
    assert!(blob > 50.0, "stab should leave a substantial blob, got {blob}");

    engine.clean();
    engine.pointer(sample(PointerPhase::Down, 140.0, 200.0, 0.5, 0.6));
    engine.pointer(sample(PointerPhase::Move, 280.0, 200.0, 0.5, 1.0));
    engine.pointer(sample(PointerPhase::Up, 280.0, 200.0, 0.5, 1.0));
    engine.advance_exact(1.2);
    let pushed = engine.paint_mass() + engine.brush_load();
    let drift = (pushed - blob).abs() / blob;
    assert!(drift < 0.05, "mass drifted by {:.1}% ({blob} -> {pushed})", drift * 100.0);
}

fn centroid_x(engine: &Engine, width: u32) -> f32 {
    let heights = engine.paint_heights();
    let mut weighted = 0.0;
    let mut total = 0.0;
    for (index, height) in heights.iter().enumerate() {
        weighted += (index as u32 % width) as f32 * height;
        total += height;
    }
    weighted / total.max(1e-6)
}

#[test]
fn paint_moves_when_pushed() {
    let width = 512;
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width, height: 384 }, 3);
    engine.dip(by_name("ivory_black").unwrap());
    engine.pointer(sample(PointerPhase::Down, 200.0, 200.0, 1.0, 0.0));
    for pass in 0..6 {
        let x = if pass % 2 == 0 { 215.0 } else { 185.0 };
        engine.pointer(sample(PointerPhase::Move, x, 200.0, 1.0, 0.1 + pass as f64 * 0.15));
    }
    engine.pointer(sample(PointerPhase::Up, 200.0, 200.0, 1.0, 1.0));
    engine.advance_exact(1.1);
    let before = centroid_x(&engine, width);

    engine.clean();
    engine.pointer(sample(PointerPhase::Down, 140.0, 200.0, 0.5, 1.1));
    engine.pointer(sample(PointerPhase::Move, 280.0, 200.0, 0.5, 1.5));
    engine.pointer(sample(PointerPhase::Up, 280.0, 200.0, 0.5, 1.5));
    engine.advance_exact(1.7);
    let after = centroid_x(&engine, width);

    assert!((before - 200.0).abs() < 5.0, "blob should be centred where it was stabbed, got {before}");
    assert!(after > before + 4.0, "paint centroid should move with the push ({before} -> {after})");
}

#[test]
fn undo_restores_the_previous_stroke_state() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: 512, height: 384 }, 3);
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 100.0, 100.0, 0.6, 0.0));
    engine.pointer(sample(PointerPhase::Move, 300.0, 100.0, 0.6, 0.4));
    engine.pointer(sample(PointerPhase::Up, 300.0, 100.0, 0.6, 0.4));
    engine.advance_exact(0.5);
    let first = engine.paint_mass();
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 100.0, 250.0, 0.6, 0.5));
    engine.pointer(sample(PointerPhase::Move, 300.0, 250.0, 0.6, 0.9));
    engine.pointer(sample(PointerPhase::Up, 300.0, 250.0, 0.6, 0.9));
    engine.advance_exact(1.0);
    assert!(engine.paint_mass() > first * 1.5);
    assert!(engine.undo());
    assert!((engine.paint_mass() - first).abs() < first * 0.01);
    assert!(engine.undo());
    assert_eq!(engine.paint_mass(), 0.0);
    assert!(!engine.undo());
    engine.clear();
    assert_eq!(engine.paint_mass(), 0.0);
}

#[test]
fn dried_paint_is_not_pushed() {
    let width = 512;
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width, height: 384 }, 3);
    engine.dip(by_name("ivory_black").unwrap());
    engine.pointer(sample(PointerPhase::Down, 200.0, 200.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Up, 200.0, 200.0, 1.0, 0.5));
    engine.advance_exact(0.6);
    engine.dry();
    let before = centroid_x(&engine, width);
    engine.clean();
    engine.pointer(sample(PointerPhase::Down, 140.0, 200.0, 0.5, 1.1));
    engine.pointer(sample(PointerPhase::Move, 280.0, 200.0, 0.5, 1.5));
    engine.pointer(sample(PointerPhase::Up, 280.0, 200.0, 0.5, 1.5));
    engine.advance_exact(1.7);
    let after = centroid_x(&engine, width);
    assert!((after - before).abs() < 2.0, "dry paint should stay put ({before} -> {after})");
}
