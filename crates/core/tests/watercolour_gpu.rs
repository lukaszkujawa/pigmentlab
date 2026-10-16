use glam::Vec4;
use simpaint_core::canvas::{Medium, Surface};
use simpaint_core::gpu::read_buffer;
use simpaint_core::pigment::by_name;
use simpaint_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: usize = 640;
const HEIGHT: usize = 320;
const MAX_GRID_LOCKING: f32 = 0.35;
const OUTLINE_LEVEL: f32 = 0.2;
const INTERIOR_DEPTH: usize = 5;

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
    let floating = read_buffer::<[Vec4; 4]>(engine.gpu(), &water.suspended);
    let settled = read_buffer::<[Vec4; 4]>(engine.gpu(), &water.deposited);
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
    engine.dip_water();
    let end = stroke(&mut engine, (300.0, 120.0), (320.0, 210.0), 0.9, end, 0.4);
    let early = pigment_mass(&engine);
    engine.advance_exact(end + 3.0);
    let later = pigment_mass(&engine);
    assert!(early > 0.0, "the stroke should carry pigment onto the paper");
    assert!((later - early).abs() < early * 0.005, "pigment changed while water flowed through it: {early} -> {later}");
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
    assert!(wet_paper > dry_paper * 5 / 4, "pigment should bloom on wet paper: {dry_paper} px dry vs {wet_paper} px wet");
}

#[test]
fn a_thirsty_brush_lifts_wet_paint_and_it_stays_lifted_as_the_wash_dries() {
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
        engine.advance_exact(t + 90.0);
        engine.dry();
        darkness(&engine, &blank)
    };
    let (plain, lifted) = (wash(false), wash(true));
    let band = |values: &[f32]| (150..160).flat_map(|y| (250..390).map(move |x| (x, y))).map(|(x, y)| values[y * WIDTH + x]).sum::<f32>();
    assert!(band(&lifted) < band(&plain) * 0.4, "paint should stay lifted after drying: {} vs {}", band(&lifted), band(&plain));
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

fn edge_profile(values: &[f32], threshold: f32, columns: std::ops::Range<usize>, from_top: bool) -> Vec<f32> {
    columns
        .map(|x| {
            let column: Vec<f32> = if from_top { (0..HEIGHT).map(|y| values[y * WIDTH + x]).collect() } else { (0..HEIGHT).rev().map(|y| values[y * WIDTH + x]).collect() };
            let first = column.iter().position(|v| *v >= threshold).unwrap_or(0);
            let inside = (first + INTERIOR_DEPTH).min(HEIGHT - 1);
            let reference = column[inside].max(1e-6);
            let covered: f32 = column[..inside].iter().map(|v| (v / reference).clamp(0.0, 1.0)).sum();
            inside as f32 - covered
        })
        .collect()
}

fn grid_locking(edge: &[f32]) -> f32 {
    let (sum_cos, sum_sin) = edge.iter().fold((0.0f32, 0.0f32), |(c, s), y| {
        let phase = std::f32::consts::PI * y;
        (c + phase.cos(), s + phase.sin())
    });
    (sum_cos * sum_cos + sum_sin * sum_sin).sqrt() / edge.len() as f32
}

fn assert_smooth_outline(values: &[f32], columns: std::ops::Range<usize>, what: &str) {
    let peak = values.iter().cloned().fold(0.0f32, f32::max);
    for from_top in [true, false] {
        let edge = edge_profile(values, peak * OUTLINE_LEVEL, columns.clone(), from_top);
        let locking = grid_locking(&edge);
        let side = if from_top { "top" } else { "bottom" };
        assert!(locking < MAX_GRID_LOCKING, "the {side} edge of {what} locks onto the 2 px simulation grid ({locking:.2}, smooth edges stay under {MAX_GRID_LOCKING}): it is a staircase");
    }
}

fn red_stroke_on_flat_paper(to: (f32, f32)) -> (Engine, Vec<Vec4>, f64) {
    let mut engine = paper(Surface::HotPressed);
    engine.set_param("tooth", 0.0);
    let blank = engine.dry_layer();
    engine.select_brush("Round medium");
    engine.dip(by_name("cadmium_red").unwrap());
    let end = stroke(&mut engine, (100.0, 150.0), to, 0.5, 0.0, 0.8);
    (engine, blank, end)
}

#[test]
fn a_dried_wash_has_a_smooth_edge_not_a_cell_staircase() {
    let (mut engine, blank, end) = red_stroke_on_flat_paper((540.0, 190.0));
    engine.advance_exact(end + 4.0);
    engine.dry();
    assert_smooth_outline(&darkness(&engine, &blank), 180..460, "a dried stroke");
}

#[test]
fn a_steep_dried_wash_has_a_smooth_edge_not_a_cell_staircase() {
    let (mut engine, blank, end) = red_stroke_on_flat_paper((420.0, 260.0));
    engine.advance_exact(end + 4.0);
    engine.dry();
    assert_smooth_outline(&darkness(&engine, &blank), 170..360, "a steep dried stroke");
}

#[test]
fn a_wet_wash_renders_a_smooth_edge_not_a_cell_staircase() {
    let (mut engine, _, end) = red_stroke_on_flat_paper((540.0, 190.0));
    engine.advance_exact(end + 0.5);
    let blank = {
        let mut clean = paper(Surface::HotPressed);
        clean.set_param("tooth", 0.0);
        clean.render();
        clean.read_back()
    };
    engine.render();
    let frame = engine.read_back();
    let shade: Vec<f32> = frame.pixels().zip(blank.pixels()).map(|(p, b)| (b.0[1] as f32 - p.0[1] as f32).max(0.0)).collect();
    assert_smooth_outline(&shade, 180..460, "a wet stroke");
}

fn settled_contents(engine: &mut Engine) -> simpaint_core::paint::BrushContents {
    for _ in 0..3 {
        engine.brush_contents();
        engine.brush_load();
    }
    engine.brush_contents()
}

#[test]
fn the_brush_gauge_drains_while_painting_watercolour() {
    let mut engine = paper(Surface::ColdPressed);
    engine.dip(by_name("french_ultramarine").unwrap());
    let dipped = engine.brush_contents();
    let end = stroke(&mut engine, (60.0, 160.0), (580.0, 170.0), 0.7, 0.0, 1.0);
    engine.advance_exact(end + 0.2);
    let painted = settled_contents(&mut engine);
    assert!(painted.fill < dipped.fill - 0.05, "painting should drain the gauge: {} -> {}", dipped.fill, painted.fill);
    let blue = painted.display_srgb();
    assert!(blue[2] > blue[0] + 40, "the gauge should show the blue still on the brush, got {blue:?}");
}

#[test]
fn a_brush_of_clear_water_shows_as_water_not_black() {
    let mut engine = paper(Surface::ColdPressed);
    engine.dip_water();
    let water = engine.brush_contents();
    assert!(water.fill > 0.5, "dipping in water should fill the brush");
    assert!(water.display_srgb().iter().all(|c| *c > 200), "clear water should look clear, got {:?}", water.display_srgb());
}

fn dried_ultramarine_band(tool: &str) -> Vec<f32> {
    let mut engine = paper(Surface::ColdPressed);
    let blank = engine.dry_layer();
    engine.select_brush(tool);
    engine.dip(by_name("french_ultramarine").unwrap());
    let end = stroke(&mut engine, (100.0, 160.0), (540.0, 160.0), 0.6, 0.0, 1.0);
    engine.advance_exact(end + 4.0);
    engine.dry();
    let darkness = darkness(&engine, &blank);
    (156..164).flat_map(|y| (200..440).map(move |x| y * WIDTH + x)).map(|i| darkness[i]).collect()
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

fn relative_variation(values: &[f32]) -> f32 {
    let average = mean(values);
    (values.iter().map(|v| (v - average).powi(2)).sum::<f32>() / values.len() as f32).sqrt() / average
}

#[test]
fn tube_paste_on_paper_dries_far_darker_than_a_brushed_wash() {
    let brushed = dried_ultramarine_band("Round medium");
    let paste = dried_ultramarine_band("Paint tube");
    assert!(mean(&paste) > mean(&brushed) * 1.5, "paste should dry darker: brushed {} vs paste {}", mean(&brushed), mean(&paste));
}

#[test]
fn thick_paste_fills_the_paper_tooth_instead_of_granulating() {
    let brushed = relative_variation(&dried_ultramarine_band("Round medium"));
    let paste = relative_variation(&dried_ultramarine_band("Paint tube"));
    assert!(paste < brushed * 0.6, "paste should granulate less than a wash: brushed {brushed} vs paste {paste}");
}

fn scrub_loaded_wash(engine: &mut Engine) -> f64 {
    engine.set_param("load", 1.0);
    engine.dip(by_name("quinacridone_rose").unwrap());
    let path = [(40.0, 120.0), (280.0, 125.0), (40.0, 150.0), (280.0, 155.0)];
    let leg = 0.27;
    engine.pointer(sample(PointerPhase::Down, path[0].0, path[0].1, 0.7, 0.0));
    for (i, point) in path.iter().enumerate().skip(1) {
        engine.pointer(sample(PointerPhase::Move, point.0, point.1, 0.7, i as f64 * leg));
    }
    let end = (path.len() - 1) as f64 * leg;
    engine.pointer(sample(PointerPhase::Up, path[3].0, path[3].1, 0.0, end));
    end
}

#[test]
fn standing_water_levels_out_instead_of_sloshing_into_streaks() {
    let mut engine = paper(Surface::ColdPressed);
    let end = scrub_loaded_wash(&mut engine);
    let cells_wide = WIDTH / 2;
    for settle in [0.05, 0.2, 0.4] {
        engine.advance_exact(end + settle);
        let water: Vec<Vec4> = read_buffer(engine.gpu(), &engine.watercolour().unwrap().water);
        let depths: Vec<f32> = (62..76).flat_map(|y| (30..120).map(move |x| y * cells_wide + x)).map(|i| water[i].x).collect();
        let holes = depths.iter().filter(|depth| **depth < 0.01).count();
        let variation = relative_variation(&depths);
        assert_eq!(holes, 0, "{settle} s after the stroke the wash has {holes} dry holes");
        assert!(variation < 0.35, "{settle} s after the stroke water depth still varies by {variation}");
    }
}

#[test]
fn water_dropped_into_a_damp_wash_dries_into_a_backrun_with_a_dark_rim() {
    let mut engine = paper(Surface::ColdPressed);
    let blank = engine.dry_layer();
    engine.set_param("load", 1.0);
    engine.dip(by_name("quinacridone_rose").unwrap());
    let mut t = 0.0;
    for y in [100.0, 140.0, 180.0, 220.0] {
        t = stroke(&mut engine, (120.0, y), (520.0, y), 0.7, t, 0.6);
    }
    engine.advance_exact(t + 42.0);
    engine.select_brush("Round medium");
    engine.dip_water();
    let centre = (307usize, 155usize);
    let end = stroke(&mut engine, (300.0, 150.0), (315.0, 160.0), 0.6, t + 42.0, 0.3);
    engine.advance_exact(end + 60.0);
    engine.dry();
    let dark = darkness(&engine, &blank);
    let ring = |radius: f32| {
        let samples: Vec<f32> = (0..72)
            .map(|step| step as f32 * std::f32::consts::TAU / 72.0)
            .map(|angle| (centre.0 as f32 + radius * angle.cos(), centre.1 as f32 + radius * angle.sin()))
            .map(|(x, y)| dark[y as usize * WIDTH + x as usize])
            .collect();
        mean(&samples)
    };
    let inside = ring(15.0);
    let rim = (40..=70).map(|radius| ring(radius as f32)).fold(0.0f32, f32::max);
    let surrounding = ring(95.0);
    assert!(inside < surrounding * 0.8, "the bloom should be paler than the wash: {inside} vs {surrounding}");
    assert!(rim > surrounding * 1.2, "the bloom should dry with a dark rim: {rim} vs {surrounding}");
}

fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let (mean_a, mean_b) = (mean(a), mean(b));
    let covariance: f32 = a.iter().zip(b).map(|(x, y)| (x - mean_a) * (y - mean_b)).sum();
    let spread = |values: &[f32], centre: f32| values.iter().map(|v| (v - centre).powi(2)).sum::<f32>().sqrt();
    covariance / (spread(a, mean_a) * spread(b, mean_b))
}

#[test]
fn mixed_pigments_separate_with_the_granulating_one_settling_into_the_paper_valleys() {
    let mut engine = paper(Surface::Rough);
    let blank = engine.dry_layer();
    let relief = engine.canvas_relief();
    engine.set_param("load", 1.0);
    let mut t = 0.0;
    for pigment in ["burnt_umber", "phthalo_green"] {
        engine.dip(by_name(pigment).unwrap());
        for y in [140.0, 160.0, 180.0] {
            t = stroke(&mut engine, (120.0, y), (520.0, y), 0.7, t, 0.5);
        }
    }
    engine.advance_exact(t + 90.0);
    engine.dry();
    let dried = engine.dry_layer();
    let pixels: Vec<usize> = (150..172).flat_map(|y| (200..440).map(move |x| y * WIDTH + x)).collect();
    let green_share: Vec<f32> = pixels.iter().map(|&i| {
        let red = blank[i].y - dried[i].y;
        let blue = blank[i].w - dried[i].w;
        red / (red + blue).max(1e-6)
    }).collect();
    let heights: Vec<f32> = pixels.iter().map(|&i| relief[i]).collect();
    let separation = correlation(&heights, &green_share);
    assert!(separation > 0.3, "fine phthalo should sit on the peaks and granulating umber in the valleys, correlation {separation}");
}
