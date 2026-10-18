use glam::Vec2;
use pigmentlab_core::paint::Mark;
use pigmentlab_core::pigment::{Coefficients, by_name};
use pigmentlab_core::{CanvasSize, Engine, Gpu};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;

fn fresh_engine() -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
    engine.set_levelling_enabled(false);
    engine.advance_exact(0.05);
    engine
}

fn ultramarine() -> Coefficients {
    by_name("french_ultramarine").unwrap().coefficients
}

fn laid(marks: &[Mark]) -> Vec<f32> {
    let mut engine = fresh_engine();
    engine.lay(marks);
    engine.advance_exact(0.1);
    engine.paint_heights()
}

fn at(heights: &[f32], x: usize, y: usize) -> f32 {
    heights[y * WIDTH + x]
}

#[test]
fn marks_land_whole_inside_the_active_window_outside_it_and_across_its_edge() {
    let marks = [
        Mark::dot(Vec2::new(120.0, 140.0), 12.0, 900.0, ultramarine(), 0.0),
        Mark::dot(Vec2::new(1800.0, 1300.0), 20.0, 2500.0, ultramarine(), 0.0),
        Mark { from: Vec2::new(40.0, 700.0), to: Vec2::new(2000.0, 760.0), radius: 6.0, volume: 30000.0, paint: ultramarine(), medium: 0.0 },
        Mark::dot(Vec2::new(1000.0, 400.0), 0.4, 0.3, ultramarine(), 0.0),
    ];
    let heights = laid(&marks);
    let expected: f32 = marks.iter().map(|mark| mark.volume).sum();
    let total: f32 = heights.iter().sum();
    assert!((total - expected).abs() < expected * 1e-3, "every mark must land whole ({expected:.1} laid, {total:.1} on the canvas)");
    assert!(at(&heights, 120, 140) > 0.0 && at(&heights, 1800, 1300) > 0.0 && at(&heights, 1000, 400) > 0.0, "paint should land where each mark was dropped");
}

#[test]
fn a_dot_is_a_dome_and_a_line_is_an_even_ridge() {
    let radius = 16.0;
    let dot = Mark::dot(Vec2::new(300.5, 300.5), radius, 1500.0, ultramarine(), 0.0);
    let line = Mark { from: Vec2::new(1000.5, 900.5), to: Vec2::new(1400.5, 900.5), radius: 5.0, volume: 6000.0, paint: ultramarine(), medium: 0.0 };
    let heights = laid(&[dot, line]);
    let peak = at(&heights, 300, 300);
    let expected_peak = 1500.0 / (2.0 / 3.0 * std::f32::consts::PI * radius * radius);
    assert!((peak - expected_peak).abs() < expected_peak * 0.05, "a dot should rise as a dome holding its volume, peak {peak:.2} vs {expected_peak:.2} px");
    assert!((at(&heights, 308, 300) / peak - 0.866).abs() < 0.03, "halfway out the dome should stand at 87% of its peak, got {:.2}", at(&heights, 308, 300) / peak);
    assert_eq!(at(&heights, 317, 300), 0.0, "nothing should land beyond the dot's radius");
    let along: Vec<f32> = (1050..1350).step_by(50).map(|x| at(&heights, x, 900)).collect();
    let (low, high) = along.iter().fold((f32::MAX, 0.0f32), |(low, high), h| (low.min(*h), high.max(*h)));
    assert!(high - low < 1e-3, "a line should lay an even ridge along its length: {along:?}");
}

#[test]
fn overlapping_marks_give_the_same_paint_in_any_order() {
    let red = by_name("cadmium_red").unwrap().coefficients;
    let marks: Vec<Mark> = (0..40)
        .map(|i| {
            let angle = i as f32 * 0.7;
            Mark::dot(Vec2::new(600.0, 600.0) + Vec2::from_angle(angle) * (i as f32 * 1.5), 6.0 + (i % 5) as f32, 200.0 + i as f32 * 10.0, if i % 2 == 0 { red } else { ultramarine() }, 0.3)
        })
        .collect();
    let mut reversed = marks.clone();
    reversed.reverse();
    let mut forward = fresh_engine();
    forward.lay(&marks);
    forward.advance_exact(0.1);
    let mut backward = fresh_engine();
    backward.lay(&reversed);
    backward.advance_exact(0.1);
    assert!(forward.paint_columns() == backward.paint_columns(), "paint must not depend on the order the marks arrive in");
}

#[test]
fn a_mark_carries_its_pigment_and_medium() {
    let medium = 0.6;
    let heights_and_columns = {
        let mut engine = fresh_engine();
        engine.lay(&[Mark::dot(Vec2::new(1700.0, 200.0), 10.0, 600.0, ultramarine(), medium)]);
        engine.advance_exact(0.1);
        engine.paint_columns()
    };
    let column = heights_and_columns[200 * WIDTH + 1700];
    let height = column[0];
    let absorption = glam::Vec3::new(column[1], column[2], column[3]) / height;
    assert!((absorption - ultramarine().absorption).abs().max_element() < 2e-3, "the paint should keep its colour ({absorption} vs {})", ultramarine().absorption);
    assert!((column[7] / height - medium).abs() < 2e-3, "the paint should keep its medium ({:.3})", column[7] / height);
}

#[test]
fn many_marks_beyond_one_batch_all_land() {
    let marks: Vec<Mark> = (0..10_000).map(|i| Mark::dot(Vec2::new(100.0 + (i % 100) as f32 * 18.0, 100.0 + (i / 100) as f32 * 13.0), 3.0, 2.0, ultramarine(), 0.0)).collect();
    let heights = laid(&marks);
    let total: f32 = heights.iter().sum();
    assert!((total - 20_000.0).abs() < 20.0, "all 10000 marks should land over the following steps, {total:.1} of 20000");
}

#[test]
fn runny_marks_level_out_without_losing_paint_inside_the_window_or_out_in_the_store() {
    for origin in [Vec2::new(60.0, 60.0), Vec2::new(1200.0, 900.0)] {
        let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
        engine.advance_exact(0.05);
        let marks: Vec<Mark> = (0..60).map(|i| Mark::dot(origin + Vec2::new((i % 10) as f32 * 25.0, (i / 10) as f32 * 25.0), 6.0, 150.0, ultramarine(), 1.0)).collect();
        engine.lay(&marks);
        engine.advance_exact(0.1);
        let peak = engine.paint_heights().iter().cloned().fold(0.0f32, f32::max);
        engine.advance_exact(4.0);
        let heights = engine.paint_heights();
        let mass: f32 = heights.iter().sum();
        assert!((mass - 9000.0).abs() < 1.0, "levelling runny paint around {origin} must keep every drop, {mass:.1} of 9000");
        assert!(heights.iter().cloned().fold(0.0f32, f32::max) < peak * 0.8, "runny dots should slump while they level");
    }
}
