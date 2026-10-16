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

fn centroid_x(engine: &mut Engine, width: u32) -> f32 {
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
    let before = centroid_x(&mut engine, width);

    engine.clean();
    engine.pointer(sample(PointerPhase::Down, 140.0, 200.0, 0.5, 1.1));
    engine.pointer(sample(PointerPhase::Move, 400.0, 200.0, 0.5, 1.5));
    engine.pointer(sample(PointerPhase::Up, 400.0, 200.0, 0.5, 1.5));
    engine.advance_exact(1.7);
    let after = centroid_x(&mut engine, width);

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
    let wet = engine.paint_mass();
    engine.dry();
    let before = engine.dry_heights();
    engine.clean();
    engine.pointer(sample(PointerPhase::Down, 140.0, 200.0, 0.5, 1.1));
    engine.pointer(sample(PointerPhase::Move, 400.0, 200.0, 0.5, 1.5));
    engine.pointer(sample(PointerPhase::Up, 400.0, 200.0, 0.5, 1.5));
    engine.advance_exact(1.7);
    let after = engine.dry_heights();
    assert!(before.iter().sum::<f32>() > wet * 0.7, "drying should have kept the blob ({wet} wet -> {} dry)", before.iter().sum::<f32>());
    assert_eq!(before, after, "dry paint must not move under a brush");
    assert!(engine.paint_mass() < 1.0, "a clean brush over dry paint should leave almost no wet paint, got {}", engine.paint_mass());
}

fn stroke_region(heights: &[f32], width: usize) -> impl Iterator<Item = usize> + '_ {
    (380..420usize).flat_map(move |y| (470..630usize).map(move |x| y * width + x)).filter(move |&i| heights[i] > 0.5)
}

fn gradient_energies(heights: &[f32], width: usize) -> (f32, f32) {
    let mut along = 0.0;
    let mut across = 0.0;
    for i in stroke_region(heights, width) {
        along += (heights[i + 1] - heights[i - 1]).powi(2);
        across += (heights[i + width] - heights[i - width]).powi(2);
    }
    (along, across)
}

fn scrub_passes(engine: &mut Engine, from: f32, to: f32, passes: u32, start: f64) -> f64 {
    let mut t = start;
    for pass in 0..passes {
        t += 0.5;
        let x = if pass % 2 == 0 { to } else { from };
        engine.pointer(sample(PointerPhase::Move, x, 400.0 + (pass % 3) as f32 * 6.0, 0.8, t));
    }
    t
}

#[test]
fn brushing_over_wet_paint_leaves_grooves_along_the_stroke() {
    let width = 1024;
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width, height: 768 }, 1);
    engine.select_brush("Round medium");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 400.0, 400.0, 0.8, 0.0));
    let released = scrub_passes(&mut engine, 400.0, 700.0, 3, 0.0);
    engine.pointer(sample(PointerPhase::Up, 700.0, 400.0, 0.0, released));
    engine.advance_exact(released + 0.1);
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 700.0, 404.0, 0.8, released + 0.1));
    let second = scrub_passes(&mut engine, 700.0, 400.0, 2, released + 0.1);
    engine.pointer(sample(PointerPhase::Up, 400.0, 400.0, 0.0, second));
    engine.advance_exact(second + 0.1);
    let heights = engine.paint_heights();
    let (along, across) = gradient_energies(&heights, width as usize);
    let ratio = across / along.max(1e-6);
    assert!(ratio > 3.0, "marks should run along the stroke, across/along gradient ratio {ratio:.2}");
}

#[test]
fn resting_brush_leaves_wet_paint_alone() {
    let width = 1024;
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width, height: 768 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Round medium");
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 400.0, 400.0, 0.8, 0.0));
    let released = scrub_passes(&mut engine, 400.0, 700.0, 2, 0.0);
    engine.pointer(sample(PointerPhase::Up, 400.0, 400.0, 0.0, released));
    engine.advance_exact(released + 0.1);
    engine.clean();
    engine.pointer(sample(PointerPhase::Down, 550.0, 400.0, 0.7, released + 0.1));
    engine.advance_exact(released + 0.2);
    let before = engine.paint_heights();
    engine.advance_exact(released + 1.2);
    let after = engine.paint_heights();
    let largest_change = before.iter().zip(&after).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    let mass_before: f32 = before.iter().sum();
    let mass_after: f32 = after.iter().sum();
    assert!(largest_change < 0.05, "a resting brush changed a column by {largest_change:.3} px");
    assert!((mass_after - mass_before).abs() < mass_before * 0.001);
}

fn centreline_profile(heights: &[f32], width: usize) -> Vec<f32> {
    (0..width).map(|x| (380..400).map(|y| heights[y * width + x]).sum::<f32>() / 20.0).collect()
}

#[test]
fn dragging_through_a_slab_smears_it_outward() {
    let width = 1024;
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width, height: 768 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Round medium");
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, 420.0, 380.0, 0.8, 0.0));
    engine.pointer(sample(PointerPhase::Move, 620.0, 380.0, 0.8, 0.5));
    engine.pointer(sample(PointerPhase::Move, 420.0, 390.0, 0.8, 1.0));
    engine.pointer(sample(PointerPhase::Move, 620.0, 400.0, 0.8, 1.5));
    engine.pointer(sample(PointerPhase::Up, 620.0, 400.0, 0.0, 1.5));
    engine.advance_exact(1.6);
    let slab = centreline_profile(&engine.paint_heights(), width as usize);
    engine.clean();
    engine.pointer(sample(PointerPhase::Down, 250.0, 390.0, 0.8, 1.6));
    engine.pointer(sample(PointerPhase::Move, 950.0, 390.0, 0.8, 2.6));
    engine.pointer(sample(PointerPhase::Up, 950.0, 390.0, 0.0, 2.6));
    engine.advance_exact(2.7);
    let smeared = centreline_profile(&engine.paint_heights(), width as usize);
    let slab_end = (0..width as usize).rev().find(|&x| slab[x] > 0.5).unwrap();
    let total: f32 = smeared.iter().sum();
    let beyond: f32 = smeared[slab_end + 1..].iter().sum();
    let steepest = smeared.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    let steepest_before = slab.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(beyond > total * 0.15, "only {:.1}% of the paint was carried past the slab edge", beyond / total * 100.0);
    assert!(steepest < steepest_before * 0.5, "the slab edge is still a cliff: {steepest_before:.2} -> {steepest:.2} px per px");
}

#[test]
fn a_held_brush_stamps_once_and_then_stops_adding_paint() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: 512, height: 384 }, 3);
    engine.set_levelling_enabled(false);
    engine.dip(by_name("cadmium_red").unwrap());
    engine.pointer(sample(PointerPhase::Down, 250.0, 200.0, 0.8, 0.0));
    engine.pointer(sample(PointerPhase::Up, 250.0, 200.0, 0.0, 1.5));
    engine.advance_exact(0.25);
    let stamped = engine.paint_mass();
    engine.advance_exact(1.4);
    let held = engine.paint_mass();
    assert!(stamped > 20.0, "landing should leave a dab, got {stamped:.1}");
    assert!(held < stamped * 1.1, "holding the brush still kept adding paint ({stamped:.0} -> {held:.0})");
}

#[test]
fn selecting_a_preset_keeps_its_own_bristle_count() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: 512, height: 384 }, 1);
    assert!(engine.configure_brush("Rigger", simpaint_core::brush::BrushAdjustments::default()));
    assert_eq!(engine.brush().preset().bristle_count, 40);
    assert!(engine.configure_brush("Round large", simpaint_core::brush::BrushAdjustments::default()));
    assert_eq!(engine.brush().preset().bristle_count, 450);
}

#[test]
fn pickup_is_spread_along_a_fast_stroke() {
    let width = 1024usize;
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: width as u32, height: 512 }, 1);
    engine.set_levelling_enabled(false);
    engine.select_brush("Paint tube");
    engine.dip(by_name("cadmium_yellow").unwrap());
    engine.pointer(sample(PointerPhase::Down, 200.0, 256.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 800.0, 256.0, 1.0, 1.0));
    engine.pointer(sample(PointerPhase::Up, 800.0, 256.0, 0.0, 1.0));
    engine.advance_exact(1.1);
    engine.select_brush("Round medium");
    engine.clean();
    engine.pointer(sample(PointerPhase::Down, 150.0, 256.0, 0.6, 1.1));
    engine.pointer(sample(PointerPhase::Move, 850.0, 256.0, 0.6, 1.3));
    engine.pointer(sample(PointerPhase::Up, 850.0, 256.0, 0.0, 1.3));
    engine.advance_exact(1.4);
    let heights = engine.paint_heights();
    let centre: Vec<f32> = (300..700).map(|x| heights[256 * width + x]).collect();
    let mut sorted = centre.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[sorted.len() / 2];
    let lowest = sorted[0];
    assert!(lowest > median * 0.5, "a fast pass dug a hole: lowest column {lowest:.2} px against a median of {median:.2} px");
}

#[test]
fn levelling_keeps_pigment_consistent_across_the_window_edge() {
    let width = 1600usize;
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: width as u32, height: 512 }, 1);
    engine.select_brush("Paint tube");
    engine.dip(by_name("french_ultramarine").unwrap());
    engine.pointer(sample(PointerPhase::Down, 100.0, 256.0, 1.0, 0.0));
    engine.pointer(sample(PointerPhase::Move, 1500.0, 256.0, 1.0, 2.0));
    engine.pointer(sample(PointerPhase::Up, 1500.0, 256.0, 0.0, 2.0));
    engine.advance_exact(4.5);
    let expected = by_name("french_ultramarine").unwrap().coefficients.absorption.x;
    let columns = engine.paint_columns();
    let mut checked = 0;
    for (index, column) in columns.iter().enumerate() {
        if column[0] < 0.1 {
            continue;
        }
        let absorption = column[1] / column[0];
        assert!((absorption - expected).abs() < expected * 0.25, "pixel {} {} has absorption {absorption:.3}, expected about {expected:.3}: pigment went missing while levelling", index % width, index / width);
        checked += 1;
    }
    assert!(checked > 20_000, "the bar should cover plenty of pixels, checked {checked}");
}
