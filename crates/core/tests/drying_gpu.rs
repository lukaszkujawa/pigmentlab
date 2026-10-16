use simpaint_core::canvas::DRY_SHRINK;
use simpaint_core::pigment::by_name;
use simpaint_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: u32 = 1024;

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

fn thick_red_bar(engine: &mut Engine) {
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 500.0, 150.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 500.0, 600.0, 1.0, 1.0));
    engine.pointer(sample(PointerPhase::Up, 500.0, 600.0, 0.0, 1.0));
    engine.advance_exact(1.1);
}

fn yellow_stroke_across(engine: &mut Engine, y: f32, from: f32, to: f32, start: f64) {
    engine.select_brush("Round large");
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, from, y, 0.8, start));
    engine.pointer(sample(PointerPhase::Move, to, y, 0.8, start + 0.8));
    engine.pointer(sample(PointerPhase::Up, to, y, 0.0, start + 0.8));
    engine.advance_exact(start + 0.9);
}

fn mean_colour(engine: &mut Engine, x0: u32, y0: u32, size: u32) -> [f32; 3] {
    engine.render();
    let image = engine.read_back();
    let mut total = [0.0f32; 3];
    for y in y0..y0 + size {
        for x in x0..x0 + size {
            let pixel = image.get_pixel(x, y);
            for channel in 0..3 {
                total[channel] += pixel[channel] as f32;
            }
        }
    }
    total.map(|t| t / (size * size) as f32)
}

fn wet_mass_of(paint: impl Fn(&mut Engine)) -> f32 {
    let mut twin = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH, height: 768 }, 1);
    paint(&mut twin);
    twin.paint_mass()
}

fn brush_strokes(engine: &mut Engine) {
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 300.0, 300.0, 0.8, 0.0));
    engine.pointer(sample(PointerPhase::Move, 700.0, 300.0, 0.8, 0.6));
    engine.pointer(sample(PointerPhase::Up, 700.0, 300.0, 0.0, 0.6));
    engine.pointer(sample(PointerPhase::Down, 700.0, 400.0, 0.8, 0.7));
    engine.pointer(sample(PointerPhase::Move, 300.0, 400.0, 0.8, 1.3));
    engine.pointer(sample(PointerPhase::Up, 300.0, 400.0, 0.0, 1.3));
    engine.advance_exact(1.4);
}

#[test]
fn drying_straight_after_brush_strokes_keeps_all_the_paint() {
    let wet = wet_mass_of(brush_strokes);
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH, height: 768 }, 1);
    brush_strokes(&mut engine);
    engine.dry();
    let dried: f32 = engine.dry_heights().iter().sum();
    assert!((dried / wet - DRY_SHRINK).abs() < 0.01, "paint under the active window was lost on drying: {:.2} of the wet volume survived", dried / wet);
    assert!(engine.paint_mass() < 1.0);
    assert!(engine.undo());
    assert!((engine.paint_mass() - wet).abs() < wet * 0.001, "undo after drying should restore the whole stroke ({wet} -> {})", engine.paint_mass());
}

#[test]
fn clearing_straight_after_strokes_can_be_undone() {
    let wet = wet_mass_of(brush_strokes);
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH, height: 768 }, 1);
    brush_strokes(&mut engine);
    engine.clear();
    assert_eq!(engine.paint_mass(), 0.0);
    assert!(engine.undo());
    assert!((engine.paint_mass() - wet).abs() < wet * 0.001, "undo after clear should restore the whole stroke ({wet} -> {})", engine.paint_mass());
}

#[test]
fn drying_moves_paint_into_a_shrunken_dry_layer() {
    let wet = wet_mass_of(thick_red_bar);
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH, height: 768 }, 1);
    thick_red_bar(&mut engine);
    engine.dry();
    let dried: f32 = engine.dry_heights().iter().sum();
    assert!(engine.paint_mass() < 1.0, "drying should empty the wet store, left {}", engine.paint_mass());
    assert!((dried / wet - DRY_SHRINK).abs() < 0.01, "dry layer should hold {DRY_SHRINK} of the wet volume, holds {:.3}", dried / wet);
}

#[test]
fn a_stroke_over_dry_paint_shows_on_top_and_leaves_the_dry_paint_in_place() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH, height: 768 }, 1);
    thick_red_bar(&mut engine);
    engine.dry();
    let dry_before = engine.dry_heights();
    yellow_stroke_across(&mut engine, 250.0, 200.0, 800.0, 1.2);
    yellow_stroke_across(&mut engine, 500.0, 800.0, 200.0, 2.2);
    assert_eq!(engine.dry_heights(), dry_before, "wet strokes must not move dried paint");
    let over_red = mean_colour(&mut engine, 492, 242, 16);
    let over_canvas = mean_colour(&mut engine, 292, 242, 16);
    assert!(over_red[1] > 150.0 && over_red[2] < 110.0, "yellow over dry red should read yellow, got {over_red:?}");
    assert!((over_red[1] - over_canvas[1]).abs() < 50.0, "the stroke should look alike on and off the dry paint ({over_red:?} vs {over_canvas:?})");
}

#[test]
fn drying_again_keeps_the_newest_layer_on_top() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH, height: 768 }, 1);
    engine.select_brush("Round large");
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, 200.0, 400.0, 0.9, 0.0));
    engine.pointer(sample(PointerPhase::Move, 800.0, 400.0, 0.9, 0.8));
    engine.pointer(sample(PointerPhase::Up, 800.0, 400.0, 0.0, 0.8));
    engine.advance_exact(0.9);
    engine.dry();
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 500.0, 150.0, 0.9, 1.0));
    engine.pointer(sample(PointerPhase::Move, 500.0, 650.0, 0.9, 1.8));
    engine.pointer(sample(PointerPhase::Up, 500.0, 650.0, 0.0, 1.8));
    engine.advance_exact(1.9);
    let wet_red_over_yellow = mean_colour(&mut engine, 492, 392, 16);
    engine.dry();
    let dried_crossing = mean_colour(&mut engine, 492, 392, 16);
    let dried_red_alone = mean_colour(&mut engine, 492, 242, 16);
    assert!(dried_crossing[0] > 150.0 && dried_crossing[1] < 90.0, "the red on top must stay red after drying, got {dried_crossing:?}");
    for channel in 0..3 {
        assert!((dried_crossing[channel] - dried_red_alone[channel]).abs() < 40.0, "dried red over dried yellow should match dried red over canvas ({dried_crossing:?} vs {dried_red_alone:?})");
        assert!((dried_crossing[channel] - wet_red_over_yellow[channel]).abs() < 60.0, "drying should not shift the colour much ({wet_red_over_yellow:?} -> {dried_crossing:?})");
    }
}

#[test]
fn undo_after_drying_restores_the_wet_paint() {
    let wet = wet_mass_of(thick_red_bar);
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH, height: 768 }, 1);
    thick_red_bar(&mut engine);
    engine.dry();
    assert!(engine.undo());
    assert!((engine.paint_mass() - wet).abs() < wet * 0.001, "undo should bring the wet paint back ({wet} -> {})", engine.paint_mass());
    assert!(engine.dry_heights().iter().all(|h| *h == 0.0), "undo should empty the dry layer again");
}
