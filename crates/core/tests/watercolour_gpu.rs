use glam::Vec4;
use simpaint_core::canvas::{Medium, Surface};
use simpaint_core::gpu::read_buffer;
use simpaint_core::pigment::by_name;
use simpaint_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: usize = 640;
const HEIGHT: usize = 320;

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

fn paper(surface: Surface) -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
    engine.set_surface(surface);
    engine.select_brush("Round large");
    engine
}

fn stroke(engine: &mut Engine, from: (f32, f32), to: (f32, f32), pressure: f32, start: f64, duration: f64) -> f64 {
    engine.pointer(sample(PointerPhase::Down, from.0, from.1, pressure, start));
    engine.pointer(sample(PointerPhase::Move, to.0, to.1, pressure, start + duration));
    engine.pointer(sample(PointerPhase::Up, to.0, to.1, 0.0, start + duration));
    engine.advance_exact(start + duration + 0.2);
    start + duration + 0.2
}

fn darkness(engine: &Engine, blank: &[Vec4]) -> Vec<f32> {
    engine.dry_layer().iter().zip(blank).map(|(pixel, paper)| ((paper.y + paper.z + paper.w) - (pixel.y + pixel.z + pixel.w)) / 3.0).collect()
}

fn pigment_mass(engine: &Engine) -> f32 {
    let water = engine.watercolour().unwrap();
    let floating = read_buffer::<[Vec4; 3]>(engine.gpu(), &water.suspended);
    let settled = read_buffer::<[Vec4; 3]>(engine.gpu(), &water.deposited);
    floating.iter().chain(&settled).map(|p| p[0].w).sum()
}

fn row_profile(values: &[f32], x: usize) -> Vec<f32> {
    (0..HEIGHT).map(|y| values[y * WIDTH + x]).collect()
}

#[test]
fn papers_switch_the_medium_and_linen_switches_back() {
    let mut engine = paper(Surface::ColdPressed);
    assert_eq!(engine.medium(), Medium::Watercolour);
    assert!(engine.watercolour().is_some());
    engine.set_surface(Surface::Linen);
    assert_eq!(engine.medium(), Medium::Oil);
    assert!(engine.watercolour().is_none());
}

#[test]
fn flowing_water_conserves_pigment() {
    let mut engine = paper(Surface::ColdPressed);
    engine.dip(by_name("french_ultramarine").unwrap());
    let end = stroke(&mut engine, (100.0, 160.0), (540.0, 170.0), 0.6, 0.0, 0.8);
    let early = pigment_mass(&engine);
    engine.advance_exact(end + 3.0);
    let later = pigment_mass(&engine);
    assert!(early > 0.0, "the stroke should carry pigment onto the paper");
    assert!((later - early).abs() < early * 0.005, "pigment changed while flowing: {early} -> {later}");
}

#[test]
fn a_wet_on_dry_wash_dries_with_darker_edges() {
    let mut engine = paper(Surface::HotPressed);
    let blank = engine.dry_layer();
    engine.dip(by_name("quinacridone_rose").unwrap());
    let end = stroke(&mut engine, (100.0, 160.0), (540.0, 160.0), 0.7, 0.0, 0.8);
    engine.advance_exact(end + 80.0);
    let dark = darkness(&engine, &blank);
    let mut edge_ratio = Vec::new();
    for x in [250usize, 320, 390] {
        let profile = row_profile(&dark, x);
        let painted: Vec<usize> = (0..HEIGHT).filter(|&y| profile[y] > 0.02).collect();
        let (top, bottom) = (*painted.first().unwrap(), *painted.last().unwrap());
        let rim = (top..top + 4).chain(bottom - 3..=bottom).map(|y| profile[y]).fold(0.0f32, f32::max);
        let middle = (top + bottom) / 2;
        let interior = (middle - 4..=middle + 4).map(|y| profile[y]).sum::<f32>() / 9.0;
        edge_ratio.push(rim / interior);
    }
    let mean = edge_ratio.iter().sum::<f32>() / edge_ratio.len() as f32;
    assert!(mean > 1.3, "dried edges should be darker than the interior, ratios {edge_ratio:?}");
}

fn footprint(engine: &mut Engine, prewet: bool) -> usize {
    let blank = engine.dry_layer();
    let mut start = 0.0;
    if prewet {
        engine.dip_water();
        for y in [130.0, 160.0, 190.0] {
            start = stroke(engine, (60.0, y), (580.0, y), 0.9, start, 0.6);
        }
    }
    engine.set_param("load", 0.6);
    engine.dip(by_name("quinacridone_rose").unwrap());
    let end = stroke(engine, (220.0, 160.0), (420.0, 160.0), 0.3, start, 0.4);
    engine.advance_exact(end + 4.0);
    engine.dry();
    darkness(engine, &blank).iter().filter(|d| **d > 0.02).count()
}

#[test]
fn wet_in_wet_spreads_far_wider_than_wet_on_dry() {
    let dry_paper = footprint(&mut paper(Surface::ColdPressed), false);
    let wet_paper = footprint(&mut paper(Surface::ColdPressed), true);
    assert!(dry_paper > 0, "the stroke should leave paint");
    assert!(wet_paper > dry_paper * 3 / 2, "pigment should bloom on wet paper: {dry_paper} px dry vs {wet_paper} px wet");
}

#[test]
fn a_thirsty_brush_lifts_wet_paint() {
    let wash = |lift: bool| {
        let mut engine = paper(Surface::ColdPressed);
        let blank = engine.dry_layer();
        engine.dip(by_name("french_ultramarine").unwrap());
        let mut t = 0.0;
        for y in [140.0, 170.0] {
            t = stroke(&mut engine, (100.0, y), (540.0, y), 0.7, t, 0.6);
        }
        if lift {
            engine.set_param("load", 0.05);
            engine.dip_water();
            t = stroke(&mut engine, (150.0, 155.0), (490.0, 155.0), 0.6, t, 0.5);
        }
        engine.advance_exact(t + 1.0);
        engine.dry();
        darkness(&engine, &blank)
    };
    let (plain, lifted) = (wash(false), wash(true));
    let band = |values: &[f32]| (150..160).flat_map(|y| (250..390).map(move |x| (x, y))).map(|(x, y)| values[y * WIDTH + x]).sum::<f32>();
    assert!(band(&lifted) < band(&plain) * 0.75, "lifting should pull paint out: {} vs {}", band(&lifted), band(&plain));
}

#[test]
fn a_glaze_mixes_optically_over_a_dried_wash_without_lifting_it() {
    let mut engine = paper(Surface::HotPressed);
    engine.dip(by_name("cadmium_yellow").unwrap());
    let mut t = 0.0;
    for y in [140.0, 170.0] {
        t = stroke(&mut engine, (100.0, y), (540.0, y), 0.7, t, 0.6);
    }
    engine.dry();
    let yellow = engine.dry_layer();
    engine.dip_water();
    t = stroke(&mut engine, (100.0, 155.0), (540.0, 155.0), 0.7, t, 0.6);
    engine.dry();
    let rewetted = engine.dry_layer();
    let probe = 155 * WIDTH + 320;
    assert!((rewetted[probe] - yellow[probe]).length() < 0.02, "clear water must not lift dried paint: {} -> {}", yellow[probe], rewetted[probe]);
    engine.dip(by_name("french_ultramarine").unwrap());
    stroke(&mut engine, (100.0, 155.0), (540.0, 155.0), 0.7, t, 0.6);
    engine.dry();
    let glazed = engine.dry_layer()[probe];
    assert!(glazed.z > glazed.y.max(glazed.w) * 0.9 && glazed.z > glazed.w + 0.05, "blue over yellow should read green, got {glazed}");
}

#[test]
fn granulating_pigment_settles_into_paper_hollows() {
    let mut engine = paper(Surface::Rough);
    let blank = engine.dry_layer();
    engine.dip(by_name("french_ultramarine").unwrap());
    let mut t = 0.0;
    for y in [130.0, 160.0, 190.0] {
        t = stroke(&mut engine, (80.0, y), (560.0, y), 0.8, t, 0.6);
    }
    engine.advance_exact(t + 4.0);
    engine.dry();
    let dark = darkness(&engine, &blank);
    let relief = engine.canvas_relief();
    let (mut hollows, mut peaks) = ((0.0, 0), (0.0, 0));
    for y in 140..180 {
        for x in 150..490 {
            let i = y * WIDTH + x;
            if relief[i] < -0.3 * relief.iter().cloned().fold(0.0f32, f32::max) {
                hollows = (hollows.0 + dark[i], hollows.1 + 1);
            } else if relief[i] > 0.3 * relief.iter().cloned().fold(0.0f32, f32::max) {
                peaks = (peaks.0 + dark[i], peaks.1 + 1);
            }
        }
    }
    let (hollow_mean, peak_mean) = (hollows.0 / hollows.1 as f32, peaks.0 / peaks.1 as f32);
    assert!(hollow_mean > peak_mean * 1.1, "ultramarine should sit in the hollows: hollows {hollow_mean:.3} vs peaks {peak_mean:.3}");
}

fn spread_of_a_stroke(surface: Surface) -> usize {
    let mut engine = paper(surface);
    let blank = engine.dry_layer();
    engine.dip(by_name("quinacridone_rose").unwrap());
    let end = stroke(&mut engine, (200.0, 160.0), (440.0, 160.0), 0.6, 0.0, 0.5);
    engine.advance_exact(end + 6.0);
    engine.dry();
    darkness(&engine, &blank).iter().filter(|d| **d > 0.02).count()
}

#[test]
fn unsized_rice_paper_feathers_beyond_the_brush() {
    let sized = spread_of_a_stroke(Surface::ColdPressed);
    let rice = spread_of_a_stroke(Surface::Rice);
    assert!(rice > sized * 6 / 5, "rice paper should wick paint past the stroke: {sized} px sized vs {rice} px rice");
}
