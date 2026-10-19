use super::*;
use crate::paint::Profile;
use crate::pigment::Coefficients;
use glam::{Vec2, Vec3};

const SIZE: u32 = 256;
const TICK: f32 = 1.0 / 60.0;

fn film() -> Film {
    Film::new(SIZE, SIZE, vec![0.0; (SIZE * SIZE) as usize])
}

fn drop_at(centre: Vec2, radius: f32, volume: f32, medium: f32) -> Mark {
    Mark { profile: Profile { flatness: 0.0, ..Profile::ROUND }, ..Mark::dot(centre, radius, volume, Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium) }
}

fn run(film: &mut Film, seconds: f32) -> Vec<[i32; 8]> {
    let mut canvas = vec![[0; 8]; (SIZE * SIZE) as usize];
    for _ in 0..(seconds / TICK).round() as usize {
        for change in film.settle(TICK, usize::MAX) {
            for channel in 0..8 {
                canvas[change.pixel as usize][channel] += change.delta[channel];
            }
        }
    }
    canvas
}

fn add(canvas: &mut [[i32; 8]], more: &[[i32; 8]]) {
    for (cell, extra) in canvas.iter_mut().zip(more) {
        for channel in 0..8 {
            cell[channel] += extra[channel];
        }
    }
}

fn heights(canvas: &[[i32; 8]]) -> Vec<f32> {
    canvas.iter().map(|cell| cell[0] as f32 / PAINT_SCALE).collect()
}

fn footprint(heights: &[f32]) -> usize {
    heights.iter().filter(|height| **height > 0.01).count()
}

fn peaks(heights: &[f32]) -> usize {
    let size = SIZE as i32;
    (1..size - 1)
        .flat_map(|y| (1..size - 1).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let at = |dx: i32, dy: i32| heights[((y + dy) * size + x + dx) as usize];
            at(0, 0) > 0.05 && [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)].iter().all(|&(dx, dy)| at(0, 0) > at(dx, dy))
        })
        .count()
}

#[test]
fn the_film_hands_the_canvas_every_drop_of_paint_it_holds() {
    let mut film = film();
    film.lay(&[drop_at(Vec2::new(100.0, 100.0), 8.0, 400.0, 1.0), drop_at(Vec2::new(160.0, 120.0), 5.0, 120.0, 0.2)]);
    let laid: f32 = heights(&run(&mut film, WET_SECONDS + 1.0)).iter().sum();
    assert!((laid - 520.0).abs() < 0.5, "the canvas should receive all 520 px^3 of paint, got {laid}");
}

#[test]
fn two_overlapping_drops_of_runny_paint_merge_into_one_pool() {
    let mut film = film();
    film.lay(&[drop_at(Vec2::new(110.5, 128.5), 10.0, 600.0, 1.0), drop_at(Vec2::new(130.5, 128.5), 10.0, 600.0, 1.0)]);
    let canvas = heights(&run(&mut film, 2.0));
    assert_eq!(peaks(&canvas), 1, "the two domes should level into one pool");
}

#[test]
fn stiff_paint_holds_the_shape_it_landed_in() {
    let mut film = film();
    let marks = [drop_at(Vec2::new(110.5, 128.5), 10.0, 600.0, 0.0), drop_at(Vec2::new(130.5, 128.5), 10.0, 600.0, 0.0)];
    film.lay(&marks);
    let canvas = heights(&run(&mut film, 2.0));
    assert_eq!(peaks(&canvas), 2, "a stiff paste keeps both domes");
}

#[test]
fn a_tall_runny_pool_spreads_until_its_height_matches_the_wetting_angle() {
    let mut film = film();
    film.lay(&[drop_at(Vec2::new(128.5, 128.5), 10.0, 8000.0, 1.0)]);
    let mut canvas = run(&mut film, TICK);
    let landed = footprint(&heights(&canvas));
    add(&mut canvas, &run(&mut film, 3.0));
    let canvas = heights(&canvas);
    let tallest = canvas.iter().copied().fold(0.0, f32::max);
    let limit = CAPILLARY_HEIGHT * 1.6;
    assert!(footprint(&canvas) > 3 * landed, "the pool should spread well past where it landed: {landed} -> {} px", footprint(&canvas));
    assert!(tallest < limit, "a wide pool flattens to about l_c tan(theta), tallest {tallest:.2} px against {limit:.2}");
}

#[test]
fn enamel_stays_close_to_where_it_landed_while_runny_paint_creeps_out() {
    let spread = |medium: f32| {
        let mut film = film();
        film.lay(&[drop_at(Vec2::new(128.5, 128.5), 10.0, 3000.0, medium)]);
        let mut canvas = run(&mut film, TICK);
        let landed = footprint(&heights(&canvas));
        add(&mut canvas, &run(&mut film, 2.0));
        footprint(&heights(&canvas)) as f32 / landed as f32
    };
    let (enamel, runny) = (spread(0.35), spread(1.0));
    assert!(enamel < 1.6, "enamel's contact line is held by its viscosity, spread {enamel:.2}x");
    assert!(runny > 1.5 * enamel, "runny paint creeps out much further, {runny:.2}x against {enamel:.2}x");
}

const CAPILLARY_HEIGHT: f32 = 16.2 * crate::drip::fluid::WETTING_SLOPE;

#[test]
fn paint_landing_on_paint_that_has_set_still_levels_over_the_weave() {
    let weave: Vec<f32> = (0..SIZE * SIZE).map(|index| if (index % SIZE / 3 + index / SIZE / 3) % 2 == 0 { 0.5 } else { -0.5 }).collect();
    let mut film = Film::new(SIZE, SIZE, weave.clone());
    let pool = || drop_at(Vec2::new(128.5, 128.5), 24.0, 12000.0, 1.0);
    film.lay(&[pool()]);
    let mut canvas = run(&mut film, WET_SECONDS + 1.0);
    film.lay(&[pool()]);
    add(&mut canvas, &run(&mut film, 2.0));
    let heights = heights(&canvas);
    let surface = |x: u32, y: u32| heights[(y * SIZE + x) as usize] + weave[(y * SIZE + x) as usize];
    let roughness: Vec<f32> = (118..138).flat_map(|y| (118..138).map(move |x| (x, y))).map(|(x, y)| surface(x, y) - (0..25).map(|k| surface(x + k % 5 - 2, y + k / 5 - 2)).sum::<f32>() / 25.0).collect();
    let spread = (roughness.iter().map(|bump| bump * bump).sum::<f32>() / roughness.len() as f32).sqrt();
    assert!(spread < 0.15, "the second coat should level its surface over the first, rms {spread:.2} px against a 0.5 px weave");
}

