use pigmentlab_core::pigment::by_name;
use pigmentlab_core::{CanvasSize, Engine, Gpu, Lighting, PointerPhase, PointerSample};

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 512;

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

fn engine() -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH, height: HEIGHT }, 1);
    engine.set_levelling_enabled(false);
    engine
}

fn stroke(engine: &mut Engine, points: &[(f32, f32)], pressure: f32, start: f64, step: f64) -> f64 {
    let mut time = start;
    engine.pointer(sample(PointerPhase::Down, points[0].0, points[0].1, pressure, time));
    for point in &points[1..] {
        time += step;
        engine.pointer(sample(PointerPhase::Move, point.0, point.1, pressure, time));
    }
    let last = points[points.len() - 1];
    engine.pointer(sample(PointerPhase::Up, last.0, last.1, 0.0, time));
    engine.advance_exact(time + 0.1);
    time + 0.2
}

fn squeeze_slab(engine: &mut Engine, pigment: &str, centre: (f32, f32), start: f64) -> f64 {
    engine.select_brush("Paint tube");
    engine.dip(by_name(pigment).unwrap());
    let points: Vec<(f32, f32)> = (0..=16)
        .map(|i| {
            let angle = i as f32 / 16.0 * std::f32::consts::TAU;
            (centre.0 + 10.0 * angle.cos(), centre.1 + 10.0 * angle.sin())
        })
        .collect();
    stroke(engine, &points, 0.3, start, 0.025)
}

fn scrub(engine: &mut Engine, pigment: &str, load: f32, span: (f32, f32), y: f32, passes: usize, start: f64) -> f64 {
    engine.select_brush("Round large");
    engine.set_param("load", load);
    engine.dip(by_name(pigment).unwrap());
    let mut points = vec![(span.0, y)];
    for pass in 0..passes {
        points.push((if pass % 2 == 0 { span.1 } else { span.0 }, y));
    }
    stroke(engine, &points, 0.8, start, 0.6)
}

fn colour_at(engine: &mut Engine, x: u32, y: u32) -> [f32; 3] {
    engine.render();
    let image = engine.read_back();
    let mut sum = [0.0f32; 3];
    for dy in 0..5 {
        for dx in 0..5 {
            let pixel = image.get_pixel(x + dx - 2, y + dy - 2);
            for channel in 0..3 {
                sum[channel] += pixel[channel] as f32 / 25.0;
            }
        }
    }
    sum
}

fn luminance(srgb: [f32; 3]) -> f32 {
    let linear = |v: f32| {
        let c = v / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(srgb[0]) + 0.7152 * linear(srgb[1]) + 0.0722 * linear(srgb[2])
}

#[test]
fn thick_white_squeezed_over_red_hides_it() {
    let mut engine = engine();
    let time = squeeze_slab(&mut engine, "cadmium_red", (300.0, 256.0), 0.0);
    let red = colour_at(&mut engine, 300, 256);
    assert!(red[0] > 150.0 && red[1] < 100.0, "expected a red slab, got {red:?}");
    squeeze_slab(&mut engine, "titanium_white", (300.0, 256.0), time);
    let covered = colour_at(&mut engine, 300, 256);
    assert!(covered.iter().all(|c| *c > 180.0), "white over red should read white, got {covered:?}");
    assert!((covered[0] - covered[2]).abs() < 25.0, "white over red should be neutral, got {covered:?}");
}

#[test]
fn baked_columns_render_like_window_columns() {
    let mut engine = engine();
    let time = squeeze_slab(&mut engine, "cadmium_red", (300.0, 256.0), 0.0);
    let time = squeeze_slab(&mut engine, "titanium_white", (300.0, 256.0), time);
    let in_window = colour_at(&mut engine, 300, 256);
    let edge = colour_at(&mut engine, 309, 256);
    engine.select_brush("Round small");
    engine.dip(by_name("cadmium_yellow").unwrap());
    stroke(&mut engine, &[(900.0, 400.0), (950.0, 420.0)], 0.5, time, 0.3);
    let baked = colour_at(&mut engine, 300, 256);
    let baked_edge = colour_at(&mut engine, 309, 256);
    for (before, after) in in_window.iter().zip(&baked).chain(edge.iter().zip(&baked_edge)) {
        assert!((before - after).abs() <= 6.0, "colour must not change when the window leaves: {in_window:?}/{edge:?} -> {baked:?}/{baked_edge:?}");
    }
}

#[test]
fn scrubbing_white_into_red_makes_pink() {
    let mut engine = engine();
    let time = squeeze_slab(&mut engine, "cadmium_red", (330.0, 256.0), 0.0);
    scrub(&mut engine, "titanium_white", 0.3, (230.0, 430.0), 256.0, 4, time);
    let mixed = colour_at(&mut engine, 330, 256);
    assert!(mixed[0] > 170.0, "mix should stay bright, got {mixed:?}");
    assert!(mixed[1] > 90.0 && mixed[1] < 235.0, "mix should be pink, neither red nor white, got {mixed:?}");
    assert!(mixed[0] - mixed[1] > 12.0, "mix should keep a red cast, got {mixed:?}");
    assert!((mixed[1] - mixed[2]).abs() < 40.0, "mix must not turn grey or yellow, got {mixed:?}");
}

#[test]
fn a_thin_red_glaze_over_white_reads_pink() {
    let mut engine = engine();
    let time = squeeze_slab(&mut engine, "titanium_white", (300.0, 256.0), 0.0);
    engine.select_brush("Round medium");
    engine.set_param("load", 0.12);
    engine.dip(by_name("cadmium_red").unwrap());
    stroke(&mut engine, &[(240.0, 256.0), (360.0, 256.0)], 0.5, time, 0.6);
    let glazed = colour_at(&mut engine, 300, 256);
    assert!(glazed[0] > glazed[1] + 25.0, "glaze should tint towards red, got {glazed:?}");
    assert!(glazed[1] > 100.0, "a thin glaze must stay translucent over white, got {glazed:?}");
}

#[test]
fn shading_never_darkens_paint_below_the_floor() {
    let mut engine = engine();
    let time = squeeze_slab(&mut engine, "cadmium_red", (330.0, 256.0), 0.0);
    let time = squeeze_slab(&mut engine, "titanium_white", (200.0, 265.0), time);
    scrub(&mut engine, "titanium_white", 0.0, (190.0, 340.0), 265.0, 2, time);
    engine.render();
    let lit = engine.read_back();
    engine.set_lighting(Lighting { ambient: 1.0, gloss: 0.0, shadow_strength: 0.0, ..Lighting::default() });
    engine.render();
    let albedo = engine.read_back();
    let mut darkest = f32::MAX;
    for (lit_pixel, flat_pixel) in lit.pixels().zip(albedo.pixels()) {
        let flat = luminance([flat_pixel[0] as f32, flat_pixel[1] as f32, flat_pixel[2] as f32]);
        if flat < 0.15 { continue; }
        let shaded = luminance([lit_pixel[0] as f32, lit_pixel[1] as f32, lit_pixel[2] as f32]);
        darkest = darkest.min(shaded / flat);
    }
    assert!(darkest >= 0.33, "shading darkened paint to {darkest:.2} of its albedo");
}
