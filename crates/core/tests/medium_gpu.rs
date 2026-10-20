use pigmentlab_core::paint::OVERLOADED_FILL;
use pigmentlab_core::pigment::{Coefficients, by_name};
use pigmentlab_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: usize = 1024;
const HEIGHT: usize = 512;
const LANE: f32 = 256.0;
const SETTLED: f64 = 9.0;
const LEVELS: [f32; 4] = [0.0, 0.25, 0.75, 1.0];

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

const HIDING_NOISE: f32 = 0.03;

fn fresh_engine() -> Engine {
    Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1)
}

fn stroke(engine: &mut Engine, from: (f32, f32), to: (f32, f32), start: f64) -> f64 {
    engine.pointer(sample(PointerPhase::Down, from.0, from.1, 0.7, start));
    engine.pointer(sample(PointerPhase::Move, to.0, to.1, 0.7, start + 1.0));
    engine.pointer(sample(PointerPhase::Up, to.0, to.1, 0.0, start + 1.0));
    engine.advance_exact(start + 1.1);
    start + 1.1
}

fn paint_with(engine: &mut Engine, medium: f32, dilution: f32, pigment: &str) {
    engine.set_param("medium", medium);
    engine.set_param("dilution", dilution);
    engine.dip(by_name(pigment).unwrap());
}

struct Profile {
    across: Vec<f32>,
}

impl Profile {
    fn of(heights: &[f32], columns: std::ops::Range<usize>) -> Self {
        let span = columns.len() as f32;
        Self { across: (0..HEIGHT).map(|y| columns.clone().map(|x| heights[y * WIDTH + x]).sum::<f32>() / span).collect() }
    }

    fn plateau(&self) -> f32 {
        let mut painted: Vec<f32> = self.across.iter().cloned().filter(|h| *h > 0.05).collect();
        painted.sort_by(f32::total_cmp);
        painted[painted.len() * 3 / 4]
    }

    fn edge_width(&self) -> f32 {
        let plateau = self.plateau();
        self.across.iter().filter(|h| **h > 0.1 * plateau && **h < 0.8 * plateau).count() as f32 / 2.0
    }

    fn width(&self) -> usize {
        self.across.iter().filter(|h| **h > 0.1).count()
    }

    fn ridges(&self) -> f32 {
        let core = 0.8 * self.plateau();
        let steps: Vec<f32> = self.across.windows(2).filter(|pair| pair[0] > core && pair[1] > core).map(|pair| (pair[1] - pair[0]).abs()).collect();
        steps.iter().sum::<f32>() / steps.len() as f32 / self.plateau()
    }
}

fn settled_stroke(brush: &str, medium: f32) -> Profile {
    let mut engine = fresh_engine();
    engine.select_brush(brush);
    paint_with(&mut engine, medium, 0.0, "cadmium_red");
    stroke(&mut engine, (150.0, LANE), (850.0, LANE), 0.0);
    engine.advance_exact(SETTLED);
    Profile::of(&engine.paint_heights(), 400..600)
}

#[test]
fn medium_levels_brush_ridges_but_keeps_stroke_edges_until_the_paint_runs() {
    for brush in ["Flat", "Filbert"] {
        let [stiff, light, generous, runny] = LEVELS.map(|medium| settled_stroke(brush, medium));
        println!("{brush}: ridges {:.3} {:.3} {:.3} {:.3}, edge {} {} {} {}, width {} {} {} {}",
            stiff.ridges(), light.ridges(), generous.ridges(), runny.ridges(),
            stiff.edge_width(), light.edge_width(), generous.edge_width(), runny.edge_width(),
            stiff.width(), light.width(), generous.width(), runny.width());
        assert!(light.ridges() < stiff.ridges() * 0.75, "{brush}: a little medium should soften the brush marks ({:.3} -> {:.3})", stiff.ridges(), light.ridges());
        assert!(generous.ridges() < stiff.ridges() * 0.25, "{brush}: plenty of medium should level the brush marks ({:.3} -> {:.3})", stiff.ridges(), generous.ridges());
        for (medium, profile) in [(25, &light), (75, &generous)] {
            assert!(profile.width() <= stiff.width() + 3, "{brush}: at {medium}% medium the stroke should not creep outwards ({} -> {} px)", stiff.width(), profile.width());
            assert!(profile.edge_width() <= stiff.edge_width() + 2.0, "{brush}: at {medium}% medium the edge should stay crisp ({} -> {} px)", stiff.edge_width(), profile.edge_width());
        }
        assert!(runny.width() > stiff.width() + 12, "{brush}: paint that is all medium should run and soften its edges ({} -> {} px)", stiff.width(), runny.width());
    }
}

#[test]
fn medium_leaves_the_colour_and_dilution_leaves_the_amount_of_paint() {
    let mut engine = fresh_engine();
    engine.select_brush("Flat");
    let mut dipped = |medium: f32, dilution: f32| {
        paint_with(&mut engine, medium, dilution, "french_ultramarine");
        (engine.brush_pigment(), engine.brush_load())
    };
    let (neat, neat_load) = dipped(0.0, 0.0);
    let (oily, _) = dipped(1.0, 0.0);
    let (diluted, diluted_load) = dipped(0.0, 1.0);
    assert!((oily.absorption - neat.absorption).abs().max_element() < 1e-4, "medium must not thin the colour ({} -> {})", neat.absorption, oily.absorption);
    assert!(diluted.absorption.max_element() < neat.absorption.max_element() * 0.01, "full dilution should leave only a trace of pigment ({} -> {})", neat.absorption, diluted.absorption);
    assert!((diluted_load - neat_load).abs() < neat_load * 1e-3, "dilution changes the pigment, not how much paint the brush holds ({neat_load:.0} -> {diluted_load:.0})");
}

fn average_colour(image: &image::RgbaImage, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> [f32; 3] {
    let pixels: Vec<[f32; 3]> = ys.flat_map(|y| xs.clone().map(move |x| (x, y))).map(|(x, y)| image.get_pixel(x, y).0).map(|p| [p[0], p[1], p[2]].map(f32::from)).collect();
    [0, 1, 2].map(|channel| pixels.iter().map(|p| p[channel]).sum::<f32>() / pixels.len() as f32)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(&b).map(|(p, q)| (p - q).powi(2)).sum::<f32>().sqrt()
}

#[test]
fn dilution_turns_a_covering_stroke_into_a_glaze_over_dry_paint() {
    let mut engine = fresh_engine();
    engine.select_brush("Flat");
    paint_with(&mut engine, 0.0, 0.0, "french_ultramarine");
    let mut time = stroke(&mut engine, (60.0, 180.0), (964.0, 180.0), 0.0);
    engine.dry();
    let columns: [u32; 4] = [150, 390, 630, 870];
    for (x, dilution) in columns.into_iter().zip(LEVELS) {
        paint_with(&mut engine, 0.5, dilution, "cadmium_yellow");
        time = stroke(&mut engine, (x as f32, 60.0), (x as f32, 440.0), time);
    }
    engine.advance_exact(time + 7.0);
    engine.render();
    let image = engine.read_back();
    let band = average_colour(&image, 260..280, 172..188);
    let hiding: Vec<f32> = columns
        .iter()
        .map(|&x| {
            let alone = average_colour(&image, x - 30..x + 30, 380..400);
            let over_band = average_colour(&image, x - 30..x + 30, 172..188);
            1.0 - distance(over_band, alone) / distance(band, alone).max(1.0)
        })
        .collect();
    println!("hiding power at 0/25/75/100% dilution: {hiding:?}");
    assert!(hiding[0] > 0.8, "undiluted yellow should cover the dry blue, hides {:.0}%", hiding[0] * 100.0);
    for pair in hiding.windows(2) {
        assert!(pair[1] < pair[0] + HIDING_NOISE, "more dilution should never hide noticeably more of the paint underneath: {hiding:?}");
    }
    assert!(hiding[3] < 0.6, "fully diluted yellow should be a glaze the blue shows through, hides {:.0}%", hiding[3] * 100.0);
}

fn crossing(medium: f32, dilution: f32) -> (Engine, Coefficients, Coefficients) {
    let mut engine = fresh_engine();
    engine.select_brush("Flat");
    engine.set_param("load", OVERLOADED_FILL);
    paint_with(&mut engine, medium, dilution, "cadmium_red");
    let red = engine.brush_pigment();
    let time = stroke(&mut engine, (150.0, LANE), (850.0, LANE), 0.0);
    paint_with(&mut engine, medium, dilution, "french_ultramarine");
    let blue = engine.brush_pigment();
    let time = stroke(&mut engine, (500.0, 60.0), (500.0, 450.0), time);
    engine.advance_exact(time + SETTLED);
    (engine, red, blue)
}

#[test]
fn levelling_crossed_strokes_never_concentrates_their_pigment() {
    for (medium, dilution) in [(1.0, 1.0), (1.0, 0.0), (0.75, 1.0)] {
        let (mut engine, red, blue) = crossing(medium, dilution);
        let strongest = red.absorption.max(blue.absorption) * 1.1;
        let concentrated = engine.paint_columns().iter().filter(|column| column[0] > 0.5).filter(|column| (glam::Vec3::new(column[1], column[2], column[3]) / column[0]).cmpgt(strongest).any()).count();
        assert_eq!(concentrated, 0, "at {medium} medium and {dilution} dilution {concentrated} pixels hold more pigment than either paint laid");
    }
}

#[test]
fn crossed_strokes_with_generous_medium_keep_their_edges() {
    let (mut generous, _, _) = crossing(0.75, 0.0);
    let (mut stiff, _, _) = crossing(0.0, 0.0);
    let (generous_heights, stiff_heights) = (generous.paint_heights(), stiff.paint_heights());
    for (place, columns, slack) in [("away from the crossing", 200..300, 4), ("beside the crossing", 440..460, 6)] {
        let (before, after) = (Profile::of(&stiff_heights, columns.clone()), Profile::of(&generous_heights, columns));
        assert!(after.width() <= before.width() + slack, "{place} the first stroke should not spread once crossed ({} -> {} px)", before.width(), after.width());
    }
}

