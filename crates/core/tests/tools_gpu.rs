use pigmentlab_core::pigment::by_name;
use pigmentlab_core::{CanvasSize, Engine, Gpu, Hover, PointerPhase, PointerSample};

const WIDTH: u32 = 1024;

fn sample(phase: PointerPhase, x: f32, y: f32, pressure: f32, time: f64) -> PointerSample {
    PointerSample { phase, x, y, pressure, tilt: [0.0; 2], rotation: 0.0, time }
}

fn fresh_engine() -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH, height: 768 }, 1);
    engine.set_levelling_enabled(false);
    engine.set_param("tooth", 0.0);
    engine
}

fn squeeze_bead(engine: &mut Engine, start: f64, pressure: f32) -> f64 {
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, 300.0, 400.0, pressure, start));
    engine.pointer(sample(PointerPhase::Move, 700.0, 400.0, pressure, start + 1.0));
    engine.pointer(sample(PointerPhase::Up, 700.0, 400.0, 0.0, start + 1.0));
    engine.advance_exact(start + 1.05);
    start + 1.05
}

fn cross_section(heights: &[f32], x: usize) -> Vec<f32> {
    (340..460).map(|y| heights[y * WIDTH as usize + x]).collect()
}

fn row_profile(heights: &[f32], y: usize) -> Vec<f32> {
    (0..WIDTH as usize).map(|x| heights[y * WIDTH as usize + x]).collect()
}

#[test]
fn a_stiff_bead_is_a_semicircle_scaled_by_pressure() {
    let mut engine = fresh_engine();
    squeeze_bead(&mut engine, 0.0, 1.0);
    let heights = engine.paint_heights();
    let section = cross_section(&heights, 500);
    let peak = section.iter().cloned().fold(0.0f32, f32::max);
    let half_width = section.iter().filter(|&&h| h > 0.2).count() as f32 / 2.0;
    assert!(peak > 15.0, "bead should be thick, peak {peak:.1} px");
    assert!((peak - half_width).abs() < 2.0, "a stiff bead stands as tall as it is wide: {peak:.1} px tall, {half_width:.1} px half-width");
    for offset in [0.5f32, 0.8] {
        let expected = peak * (1.0 - offset * offset).sqrt();
        let measured = section[60 + (offset * half_width).round() as usize];
        assert!((measured - expected).abs() < 2.5, "the bead should follow a circle: {measured:.1} px at {offset} of the half-width, expected {expected:.1}");
    }
    let full = engine.paint_mass();

    let mut light = fresh_engine();
    squeeze_bead(&mut light, 0.0, 0.5);
    let half = light.paint_mass();
    assert!((half / full - 0.5).abs() < 0.05, "half pressure should squeeze half the paint ({half:.0} vs {full:.0})");
}

#[test]
fn oily_paint_lies_low_and_wide_while_stiff_paint_holds_its_shape_without_losing_paint() {
    let (oily_peak, oily_footprint, _) = blob(1.0);
    let (stiff_peak, stiff_footprint, stiff_spread) = blob(0.0);
    assert!(oily_peak < stiff_peak * 0.5 && oily_footprint > stiff_footprint, "oily paint should lie lower and wider ({oily_peak:.1} px over {oily_footprint} vs {stiff_peak:.1} px over {stiff_footprint})");
    assert_eq!(stiff_spread, 0, "a stiff blob should hold its shape");
}

fn blob(medium: f32) -> (f32, usize, usize) {
    let mut engine = fresh_engine();
    engine.set_param("medium", medium);
    engine.select_brush("Paint tube");
    engine.dip(by_name("french_ultramarine").unwrap());
    engine.pointer(sample(PointerPhase::Down, 500.0, 400.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Up, 500.0, 400.0, 0.0, 0.6));
    engine.advance_exact(0.65);
    let before = engine.paint_heights();
    let mass_before = engine.paint_mass();
    engine.set_levelling_enabled(true);
    engine.advance_exact(2.5);
    let after = engine.paint_heights();
    let peak = |h: &[f32]| cross_section(h, 500).iter().cloned().fold(0.0f32, f32::max);
    let footprint = |h: &[f32]| cross_section(h, 500).iter().filter(|&&v| v > 0.2).count();
    assert!(peak(&before) > 2.0, "a stationary squeeze should pile up, peak {:.1}", peak(&before));
    assert!(peak(&after) <= peak(&before), "slumping must not raise the blob ({:.1} -> {:.1})", peak(&before), peak(&after));
    let drift = (engine.paint_mass() - mass_before).abs() / mass_before;
    assert!(drift < 0.001, "slumping drifted mass by {:.3}%", drift * 100.0);
    (peak(&after), footprint(&after), footprint(&after).saturating_sub(footprint(&before)))
}

#[test]
fn shaper_scrapes_a_bead_flat_and_pushes_it_ahead() {
    let mut engine = fresh_engine();
    let released = squeeze_bead(&mut engine, 0.0, 1.0);
    let before = row_profile(&engine.paint_heights(), 400);
    let mass_before = engine.paint_mass();
    engine.select_brush("Colour shaper");
    engine.set_param("angle", 90.0);
    engine.pointer(sample(PointerPhase::Down, 250.0, 400.0, 0.9, released));
    engine.pointer(sample(PointerPhase::Move, 550.0, 400.0, 0.9, released + 0.6));
    engine.pointer(sample(PointerPhase::Up, 550.0, 400.0, 0.0, released + 0.6));
    engine.advance_exact(released + 0.7);
    let after = row_profile(&engine.paint_heights(), 400);
    let scraped: f32 = after[320..500].iter().sum::<f32>() / 180.0;
    let untouched: f32 = before[320..500].iter().sum::<f32>() / 180.0;
    assert!(scraped < untouched * 0.35, "shaper should scrape the bead thin ({untouched:.1} -> {scraped:.1} px)");
    let ahead_before: f32 = before[540..620].iter().sum();
    let ahead_after: f32 = after[540..620].iter().sum();
    assert!(ahead_after > ahead_before * 1.3, "scraped paint should pile up ahead of the blade ({ahead_before:.0} -> {ahead_after:.0})");
    let drift = (engine.paint_mass() + engine.brush_load() - mass_before).abs() / mass_before;
    assert!(drift < 0.02, "shaper changed total paint by {:.2}%", drift * 100.0);
}

#[test]
fn shaper_leaves_no_paint_on_bare_canvas() {
    let mut engine = fresh_engine();
    engine.select_brush("Colour shaper");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 200.0, 300.0, 0.9, 0.0));
    engine.pointer(sample(PointerPhase::Move, 600.0, 300.0, 0.9, 0.5));
    engine.pointer(sample(PointerPhase::Up, 600.0, 300.0, 0.0, 0.5));
    engine.advance_exact(0.6);
    let mass = engine.paint_mass();
    assert!(mass < 1.0, "a rigid shaper carries no paint, yet it left {mass:.1}");
}

fn squeeze_ring(engine: &mut Engine, start: f64, cx: f32, cy: f32, radius: f32, pigment: &str) -> f64 {
    engine.select_brush("Paint tube");
    engine.dip(by_name(pigment).unwrap());
    let duration = 2.4;
    for i in 0..=36 {
        let angle = std::f32::consts::TAU * i as f32 / 36.0;
        let phase = if i == 0 { PointerPhase::Down } else { PointerPhase::Move };
        engine.pointer(sample(phase, cx + radius * angle.cos(), cy + radius * angle.sin(), 1.0, start + duration * i as f64 / 36.0));
    }
    engine.pointer(sample(PointerPhase::Up, cx + radius, cy, 0.0, start + duration));
    engine.advance_exact(start + duration + 0.1);
    start + duration + 0.1
}

#[test]
fn tube_drawn_in_a_circle_leaves_an_even_ring_that_closes_over_its_own_start() {
    let mut engine = fresh_engine();
    squeeze_ring(&mut engine, 0.0, 400.0, 380.0, 150.0, "cadmium_yellow");
    let heights = engine.paint_heights();
    let at = |x: f32, y: f32| heights[y.round() as usize * WIDTH as usize + x.round() as usize];
    let crest_at = |degrees: f32| {
        let angle = degrees.to_radians();
        at(400.0 + 150.0 * angle.cos(), 380.0 + 150.0 * angle.sin())
    };
    let crest: Vec<f32> = (30..=330).step_by(5).map(|degrees| crest_at(degrees as f32)).collect();
    let (lowest, highest) = crest.iter().fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
    assert!(lowest > highest * 0.8, "the ring crest should be even away from the seam ({lowest:.1} to {highest:.1} px)");
    assert!(crest_at(0.0) > highest * 1.3, "where the ring closes its end should ride over its start ({:.1} vs {highest:.1} px)", crest_at(0.0));
    let ring_width = (200..300).filter(|&x| at(x as f32, 380.0) > 0.2).count();
    assert!((30..=60).contains(&ring_width), "a ring drawn at hand speed should be about a nozzle diameter wide, got {ring_width} px");
    assert!(at(400.0, 380.0) == 0.0 && at(400.0, 100.0) == 0.0, "inside and outside the ring must stay bare");
}

#[test]
fn shaper_through_a_ring_scrapes_to_a_stain_and_carries_the_paint_on_its_blade() {
    let mut engine = fresh_engine();
    engine.set_param("size", 0.4);
    let released = squeeze_ring(&mut engine, 0.0, 400.0, 380.0, 150.0, "cadmium_yellow");
    let mass_before = engine.paint_mass();
    engine.set_param("size", 1.0);
    engine.select_brush("Colour shaper");
    engine.set_param("angle", 0.0);
    engine.pointer(sample(PointerPhase::Down, 500.0, 150.0, 0.9, released));
    engine.pointer(sample(PointerPhase::Move, 500.0, 620.0, 0.9, released + 1.0));
    engine.pointer(sample(PointerPhase::Up, 500.0, 620.0, 0.0, released + 1.0));
    engine.advance_exact(released + 1.1);
    let heights = engine.paint_heights();
    let at = |x: usize, y: usize| heights[y * WIDTH as usize + x];
    for x in (465..=535).step_by(5) {
        let film = at(x, 560);
        assert!((0.05..=0.3).contains(&film), "a firmly pressed blade should leave only a stain, got {film:.2} px at x {x}");
    }
    assert!(at(450, 560) == 0.0 && at(550, 560) == 0.0, "no paint should appear beyond the blade's edges on bare canvas");
    let carried = engine.brush_load();
    assert!(carried > mass_before * 0.02, "the blade should carry scraped paint away, held {carried:.0} of {mass_before:.0}");
    let drift = (engine.paint_mass() + carried - mass_before).abs() / mass_before;
    assert!(drift < 0.001, "paint on canvas and blade together drifted by {:.3}%", drift * 100.0);
}

#[test]
fn a_brush_dragged_out_of_a_blob_loads_up_and_fades_gradually() {
    let mut engine = fresh_engine();
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 185.0, 250.0, 0.6, 0.0));
    engine.pointer(sample(PointerPhase::Move, 185.0, 250.0, 0.6, 0.5));
    engine.pointer(sample(PointerPhase::Up, 185.0, 250.0, 0.0, 0.5));
    engine.advance_exact(0.6);
    let blob = engine.paint_mass();
    engine.select_brush("Round medium");
    engine.set_param("load", 0.22);
    engine.set_param("medium", 1.0);
    engine.dip(by_name("cadmium_red").unwrap());
    let dipped = engine.brush_load();
    engine.pointer(sample(PointerPhase::Down, 170.0, 250.0, 0.5, 0.6));
    engine.pointer(sample(PointerPhase::Move, 300.0, 250.0, 0.5, 0.9));
    engine.advance_exact(0.9);
    let picked_up = engine.brush_load() - dipped;
    engine.pointer(sample(PointerPhase::Move, 980.0, 250.0, 0.5, 2.4));
    engine.pointer(sample(PointerPhase::Up, 980.0, 250.0, 0.0, 2.4));
    engine.advance_exact(2.5);
    assert!(picked_up > blob * 0.03, "the brush should load itself from the blob, took {:.0}% of it", picked_up / blob * 100.0);
    let heights = engine.paint_heights();
    let centre: Vec<f32> = (260..920).step_by(20).map(|x| (244..256).map(|y| heights[y * WIDTH as usize + x]).sum::<f32>() / 12.0).collect();
    for pair in centre.windows(2) {
        assert!(pair[1] > pair[0] * 0.55, "the trail should fade gradually, not step from {:.1} to {:.1} px within 20 px", pair[0], pair[1]);
    }
    assert!(centre[0] > 1.5 && *centre.last().unwrap() < centre[0] * 0.5, "the trail should start thick and end thin ({:.1} -> {:.1})", centre[0], centre.last().unwrap());
}

#[test]
fn a_clean_brush_smears_stiff_paint_instead_of_wiping_it_away() {
    let mut engine = fresh_engine();
    engine.select_brush("Flat");
    for (k, y) in [300.0, 330.0, 360.0, 390.0].iter().enumerate() {
        engine.dip(by_name("cadmium_red").unwrap());
        let start = k as f64 * 1.5;
        engine.pointer(sample(PointerPhase::Down, 380.0, *y, 0.8, start));
        engine.pointer(sample(PointerPhase::Move, 730.0, *y, 0.8, start + 1.0));
        engine.pointer(sample(PointerPhase::Up, 730.0, *y, 0.0, start + 1.0));
    }
    engine.advance_exact(6.5);
    let band = |heights: &[f32]| (280..410).flat_map(|y| (550..570).map(move |x| (x, y))).map(|(x, y)| heights[y * WIDTH as usize + x]).collect::<Vec<f32>>();
    let before = band(&engine.paint_heights());
    engine.select_brush("Round medium");
    engine.clean();
    engine.pointer(sample(PointerPhase::Down, 560.0, 220.0, 0.7, 7.0));
    engine.pointer(sample(PointerPhase::Move, 560.0, 480.0, 0.7, 8.0));
    engine.pointer(sample(PointerPhase::Up, 560.0, 480.0, 0.0, 8.0));
    engine.advance_exact(8.6);
    let after = band(&engine.paint_heights());
    let left = after.iter().sum::<f32>() / before.iter().sum::<f32>();
    assert!(left > 0.2, "a dry brush should only pick up part of stiff paint, but {:.0}% of it was wiped away", (1.0 - left) * 100.0);
    let bare = before.iter().zip(&after).filter(|(was, now)| **was > 1.0 && **now < 0.05).count();
    assert!(bare < before.len() / 50, "a dry brush should not scrape the canvas bare ({bare} of {} pixels)", before.len());
}

#[test]
fn a_lifted_brush_follows_the_hovering_pointer() {
    let mut engine = fresh_engine();
    engine.select_brush("Round medium");
    engine.pointer(sample(PointerPhase::Down, 300.0, 300.0, 0.5, 0.0));
    engine.pointer(sample(PointerPhase::Move, 400.0, 300.0, 0.5, 0.3));
    engine.pointer(sample(PointerPhase::Up, 400.0, 300.0, 0.0, 0.3));
    engine.advance_exact(0.6);
    engine.hover(Hover { x: 700.0, y: 500.0, tilt: [0.0; 2], rotation: 0.0 });
    engine.advance_exact(1.2);
    let centre = engine.brush().tuft_centre();
    let reach = engine.brush().preset().bend_reach();
    assert!(centre.distance(glam::Vec2::new(700.0, 500.0)) <= reach + 1.0, "the lifted brush should hang over the pointer, but its tuft is at {centre}");
}

#[test]
fn wiping_leaves_a_little_paint_of_the_same_colour() {
    let mut engine = fresh_engine();
    engine.select_brush("Round medium");
    engine.dip(by_name("cadmium_red").unwrap());
    let (full, colour) = (engine.brush_load(), engine.brush_pigment());
    engine.wipe();
    let kept = engine.brush_load() / full;
    assert!((kept - 0.15).abs() < 0.01, "wiping should keep about 15% of the paint, kept {:.0}%", kept * 100.0);
    let after = engine.brush_pigment();
    assert!((after.absorption - colour.absorption).length() < colour.absorption.length() * 0.01, "wiping must not change the colour: {:?} -> {:?}", colour.absorption, after.absorption);
}

#[test]
fn a_tilted_tube_squeezes_under_the_pointer() {
    let mut engine = fresh_engine();
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_red").unwrap());
    let at = |phase, x: f32, time| PointerSample { phase, x, y: 400.0, pressure: 0.8, tilt: [0.0, 0.8], rotation: 0.0, time };
    engine.pointer(at(PointerPhase::Down, 300.0, 0.0));
    engine.pointer(at(PointerPhase::Move, 700.0, 1.0));
    engine.pointer(at(PointerPhase::Up, 700.0, 1.0));
    engine.advance_exact(1.05);
    let heights = engine.paint_heights();
    let section = cross_section(&heights, 500);
    let peak = section.iter().enumerate().fold((0, 0.0f32), |best, (i, h)| if *h > best.1 { (i, *h) } else { best }).0 + 340;
    assert!((peak as i32 - 400).abs() <= 2, "the bead should run under the pointer at y 400, but peaks at y {peak}");
}

fn rope_then_pour(rope_size: f32, rope_seconds: f64, slant: f32) -> Engine {
    let mut engine = fresh_engine();
    engine.select_brush("Paint tube");
    engine.set_param("size", rope_size);
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 600.0 - slant, 100.0, 0.8, 0.0));
    engine.pointer(sample(PointerPhase::Move, 600.0 + slant, 700.0, 0.8, rope_seconds));
    engine.pointer(sample(PointerPhase::Up, 600.0 + slant, 700.0, 0.0, rope_seconds));
    engine.advance_exact(rope_seconds + 0.1);
    engine.set_param("size", 1.0);
    engine.set_param("medium", 1.0);
    engine.dip(by_name("french_ultramarine").unwrap());
    let start = rope_seconds + 0.2;
    engine.pointer(sample(PointerPhase::Down, 520.0, 400.0, 0.8, start));
    engine.pointer(sample(PointerPhase::Up, 520.0, 400.0, 0.0, start + 1.5));
    engine.advance_exact(start + 1.7);
    engine
}

#[test]
fn runny_paint_pools_against_a_rope_instead_of_crossing_it() {
    for (size, seconds, slant) in [(1.0, 4.0, 0.0), (0.6, 3.0, 150.0)] {
        let mut engine = rope_then_pour(size, seconds, slant);
        let heights = engine.paint_heights();
        let rope_x = |y: usize| 600.0 - slant + 2.0 * slant * (y as f32 - 100.0) / 600.0;
        let covered = |range: &dyn Fn(usize) -> std::ops::Range<usize>| (350..450).flat_map(|y| range(y).map(move |x| (x, y))).filter(|(x, y)| heights[y * WIDTH as usize + x] > 0.05).count();
        let near_side = covered(&|y| 400..(rope_x(y) - 45.0) as usize);
        let far_side = covered(&|y| (rope_x(y) + 45.0) as usize..(rope_x(y) + 100.0) as usize);
        assert!(near_side > 8_000, "the pour should fill the near side of a size {size} rope, covered {near_side} px");
        assert!(far_side < 50, "runny paint crossed a size {size} rope onto {far_side} px");
    }
}

#[test]
fn a_rope_laid_across_an_older_rope_rides_over_it() {
    let mut engine = fresh_engine();
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, 300.0, 400.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 700.0, 400.0, 1.0, 1.0));
    engine.pointer(sample(PointerPhase::Up, 700.0, 400.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    let single = engine.paint_heights();
    engine.dip(by_name("french_ultramarine").unwrap());
    engine.pointer(sample(PointerPhase::Down, 500.0, 200.0, 1.0, 1.2));
    engine.pointer(sample(PointerPhase::Move, 500.0, 600.0, 1.0, 2.2));
    engine.pointer(sample(PointerPhase::Up, 500.0, 600.0, 0.0, 2.2));
    engine.advance_exact(2.3);
    let crossed = engine.paint_heights();
    let at = |h: &[f32], x: usize, y: usize| h[y * WIDTH as usize + x];
    let (first, second, stacked) = (at(&single, 500, 400), at(&crossed, 500, 300), at(&crossed, 500, 400));
    assert!(stacked > first + second * 0.8, "the second rope should ride over the first ({first:.1} + {second:.1} -> {stacked:.1} px)");
    let [red, green, blue] = engine.colour_at(500.0, 400.0);
    assert!(blue > red + 40 && blue > green + 40, "the crossing should show the blue rope on top, got {:?}", [red, green, blue]);
}

#[test]
fn a_rope_crossing_its_own_earlier_path_stacks_as_well() {
    let mut engine = fresh_engine();
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, 300.0, 400.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 700.0, 400.0, 1.0, 1.0));
    engine.pointer(sample(PointerPhase::Move, 700.0, 250.0, 1.0, 1.4));
    engine.pointer(sample(PointerPhase::Move, 500.0, 250.0, 1.0, 1.9));
    engine.pointer(sample(PointerPhase::Move, 500.0, 550.0, 1.0, 2.6));
    engine.pointer(sample(PointerPhase::Up, 500.0, 550.0, 0.0, 2.6));
    engine.advance_exact(2.7);
    let heights = engine.paint_heights();
    let at = |x: usize, y: usize| heights[y * WIDTH as usize + x];
    assert!(at(500, 400) > 1.7 * at(400, 400), "the rope should stack where it crosses itself ({:.1} vs {:.1} px alone)", at(500, 400), at(400, 400));
    assert!(at(400, 400) > 15.0 && at(500, 480) > 15.0, "the rope should stay a full rope on either side of the crossing");
}

#[test]
fn thinned_paint_spreads_from_the_nozzle_outward_instead_of_appearing_as_a_faint_disc() {
    let mut engine = fresh_engine();
    engine.set_param("medium", 1.0);
    engine.select_brush("Paint tube");
    engine.dip(by_name("ivory_black").unwrap());
    engine.pointer(sample(PointerPhase::Down, 500.0, 400.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Up, 500.0, 400.0, 0.0, 1.5));
    engine.advance_exact(0.15);
    let early = engine.paint_heights();
    engine.advance_exact(1.6);
    let late = engine.paint_heights();
    let width = |h: &[f32]| row_profile(h, 400).iter().filter(|&&v| v > 0.05).count();
    let centre = |h: &[f32]| h[400 * WIDTH as usize + 500];
    let faint_far_away = early.iter().enumerate().filter(|(i, v)| **v > 0.01 && ((i % WIDTH as usize) as f32 - 500.0).abs() > 60.0).count();
    assert!(centre(&early) > 4.0, "paint under the nozzle should be deep straight away, got {:.2} px", centre(&early));
    assert!(width(&early) < 70, "early paint should stay near the nozzle, got {} px wide", width(&early));
    assert_eq!(faint_far_away, 0, "no faint film should appear far from the nozzle");
    assert!(width(&late) > width(&early) * 2, "the puddle should keep widening as paint arrives ({} -> {} px)", width(&early), width(&late));
}

#[test]
fn a_tap_of_thinned_paint_leaves_a_small_sharp_puddle_and_no_faint_halo() {
    let mut engine = fresh_engine();
    engine.set_param("medium", 1.0);
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 500.0, 400.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 502.0, 401.0, 1.0, 0.05));
    engine.pointer(sample(PointerPhase::Up, 502.0, 401.0, 0.0, 0.12));
    engine.advance_exact(0.3);
    let heights = engine.paint_heights();
    let far = |i: usize| ((i % WIDTH as usize) as f32 - 501.0).hypot((i / WIDTH as usize) as f32 - 400.0) > 40.0;
    let halo = heights.iter().enumerate().filter(|(i, h)| **h > 0.005 && far(*i)).count();
    let peak = heights.iter().cloned().fold(0.0f32, f32::max);
    assert_eq!(halo, 0, "a tap must not leave a faint halo, found {halo} px of film beyond 40 px");
    assert!(peak > 4.0, "the tap should leave a proper puddle, peak {peak:.2} px");
}
