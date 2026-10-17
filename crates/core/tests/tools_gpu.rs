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
fn tube_lays_a_rounded_bead_scaled_by_pressure() {
    let mut engine = fresh_engine();
    squeeze_bead(&mut engine, 0.0, 1.0);
    let heights = engine.paint_heights();
    let section = cross_section(&heights, 500);
    let peak = section.iter().cloned().fold(0.0f32, f32::max);
    let centre = section[60];
    let shoulder = section[60 + 14];
    let edge = section[60 + 24];
    assert!(peak > 4.0, "bead should be thick, peak {peak:.1} px");
    assert!(centre > shoulder && shoulder > edge && edge < 0.1, "bead should be rounded: {centre:.1} {shoulder:.1} {edge:.1}");
    let full = engine.paint_mass();

    let mut light = fresh_engine();
    squeeze_bead(&mut light, 0.0, 0.5);
    let half = light.paint_mass();
    assert!((half / full - 0.5).abs() < 0.05, "half pressure should squeeze half the paint ({half:.0} vs {full:.0})");
}

#[test]
fn squeezed_blob_spreads_at_its_foot_without_losing_paint() {
    let mut engine = fresh_engine();
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
    assert!(peak(&before) > 20.0, "a stationary squeeze should pile up, peak {:.1}", peak(&before));
    assert!(peak(&after) <= peak(&before), "slumping must not raise the blob ({:.1} -> {:.1})", peak(&before), peak(&after));
    assert!(footprint(&after) >= footprint(&before) + 2, "the steep foot should spread onto bare canvas ({} -> {})", footprint(&before), footprint(&after));
    let drift = (engine.paint_mass() - mass_before).abs() / mass_before;
    assert!(drift < 0.001, "slumping drifted mass by {:.3}%", drift * 100.0);
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
fn tube_drawn_in_a_circle_leaves_an_even_ring() {
    let mut engine = fresh_engine();
    squeeze_ring(&mut engine, 0.0, 400.0, 380.0, 150.0, "cadmium_yellow");
    let heights = engine.paint_heights();
    let at = |x: f32, y: f32| heights[y.round() as usize * WIDTH as usize + x.round() as usize];
    let crest: Vec<f32> = (0..72).map(|i| {
        let angle = std::f32::consts::TAU * i as f32 / 72.0;
        at(400.0 + 150.0 * angle.cos(), 380.0 + 150.0 * angle.sin())
    }).collect();
    let (lowest, highest) = crest.iter().fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
    assert!(lowest > highest * 0.7, "the ring crest should be even all the way round ({lowest:.1} to {highest:.1} px)");
    let ring_width = (200..300).filter(|&x| at(x as f32, 380.0) > 0.2).count();
    assert!((34..=46).contains(&ring_width), "ring should be about one nozzle diameter wide, got {ring_width} px");
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
    engine.pointer(sample(PointerPhase::Down, 150.0, 250.0, 0.15, 0.0));
    engine.pointer(sample(PointerPhase::Move, 220.0, 250.0, 0.15, 0.5));
    engine.pointer(sample(PointerPhase::Up, 220.0, 250.0, 0.0, 0.5));
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
