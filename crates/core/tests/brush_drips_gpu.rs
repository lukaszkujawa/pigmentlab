use glam::Vec2;
use pigmentlab_core::pigment::by_name;
use pigmentlab_core::units::METRES_PER_PIXEL;
use pigmentlab_core::{CanvasSize, Engine, Gpu, Hover};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1024;
const RUNNY: f32 = 1.0;

fn loaded_brush(medium: f32, load: f32) -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
    engine.select_brush("Round large");
    engine.set_param("medium", medium);
    engine.set_param("load", load);
    engine.dip(by_name("cadmium_red").unwrap());
    engine
}

fn hold_over(engine: &mut Engine, at: Vec2, until: f64) {
    engine.hover(Hover { x: at.x, y: at.y, tilt: [0.0; 2], rotation: 0.0 });
    engine.advance_exact(until);
}

fn painted(heights: &[f32]) -> Vec<Vec2> {
    heights.iter().enumerate().filter(|(_, h)| **h > 0.05).map(|(i, _)| Vec2::new((i % WIDTH) as f32, (i / WIDTH) as f32)).collect()
}

fn accounted(engine: &mut Engine) -> f32 {
    engine.paint_mass() + engine.brush_load() + engine.drips().airborne() / METRES_PER_PIXEL.powi(3)
}

#[test]
fn a_brush_soaked_in_runny_paint_held_still_drips_under_its_tip() {
    let under = Vec2::new(1024.0, 512.0);
    let mut engine = loaded_brush(RUNNY, 1.0);
    hold_over(&mut engine, under, 4.0);
    let heights = engine.paint_heights();
    let spots = painted(&heights);
    let farthest = spots.iter().map(|spot| spot.distance(under)).fold(0.0f32, f32::max);
    println!("{} painted pixels, reaching {farthest:.0} px from under the brush", spots.len());
    assert!(spots.len() > 50, "a soaked brush of runny paint should drip on the canvas, painted {} pixels", spots.len());
    assert!(farthest < 120.0, "drips should land under the still brush, reached {farthest:.0} px away");
}

#[test]
fn a_normally_loaded_brush_or_one_of_stiff_paint_does_not_drip() {
    for (medium, load) in [(RUNNY, 0.5), (0.0, 1.0)] {
        let mut engine = loaded_brush(medium, load);
        hold_over(&mut engine, Vec2::new(1024.0, 512.0), 4.0);
        assert_eq!(engine.paint_mass(), 0.0, "medium {medium} at load {load} should keep its paint in the brush");
        assert!(engine.drips().airborne() == 0.0, "medium {medium} at load {load} should not be dripping");
    }
}

#[test]
fn paint_dripping_from_a_brush_is_kept_between_brush_air_and_canvas() {
    let mut engine = loaded_brush(RUNNY, 1.0);
    let before = engine.brush_load();
    hold_over(&mut engine, Vec2::new(1024.0, 512.0), 3.0);
    let after = accounted(&mut engine);
    println!("brush held {before:.0}, now brush, air and canvas hold {after:.0}, on canvas {:.0}", engine.paint_mass());
    assert!(engine.paint_mass() > 0.0, "the brush should have dripped");
    assert!((after - before).abs() < before * 0.005, "paint must be kept: {before:.0} before, {after:.0} after");
}

#[test]
fn a_dripping_brush_carried_over_the_canvas_trails_drips_along_its_path() {
    let mut engine = loaded_brush(RUNNY, 1.0);
    let (from, to) = (Vec2::new(300.0, 512.0), Vec2::new(1700.0, 512.0));
    hold_over(&mut engine, from, 1.0);
    let steps = 240;
    for k in 1..=steps {
        let t = k as f32 / steps as f32;
        hold_over(&mut engine, from.lerp(to, t), 1.0 + 3.0 * f64::from(t));
    }
    engine.advance_exact(5.0);
    let heights = engine.paint_heights();
    let spots = painted(&heights);
    let (left, right) = spots.iter().fold((f32::MAX, 0.0f32), |(left, right), spot| (left.min(spot.x), right.max(spot.x)));
    let off_path = spots.iter().map(|spot| (spot.y - from.y).abs()).fold(0.0f32, f32::max);
    println!("drips span x {left:.0}..{right:.0}, at most {off_path:.0} px off the path");
    assert!(right - left > 0.7 * (to.x - from.x), "drips should trail along the carried brush, spanning {:.0} px", right - left);
    assert!(off_path < 120.0, "drips should stay near the path, {off_path:.0} px off");
}
