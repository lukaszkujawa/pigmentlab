use glam::Vec2;
use pigmentlab_core::pigment::by_name;
use pigmentlab_core::units::METRES_PER_PIXEL;
use pigmentlab_core::{CanvasSize, Engine, Gpu, PointerPhase, PointerSample};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;
const START: Vec2 = Vec2::new(300.0, 768.0);
const ARROW: f32 = 400.0;
const STIFF: f32 = 0.2;
const RUNNY: f32 = 1.0;
const TRACE: f32 = 0.01;

fn splatter_tool(medium: f32, load: f32) -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
    engine.select_brush("Splatter");
    engine.set_param("medium", medium);
    engine.set_param("load", load);
    engine.dip(by_name("cadmium_red").unwrap());
    engine
}

fn thrown(medium: f32, load: f32, force: f32) -> Engine {
    let mut engine = splatter_tool(medium, load);
    engine.set_param("splatter_force", force);
    engine.splatter(START, START + Vec2::new(ARROW, 0.0));
    engine.advance_exact(1.5);
    engine
}

fn painted(engine: &mut Engine) -> Vec<Vec2> {
    engine.paint_heights().iter().enumerate().filter(|(_, height)| **height > 0.05).map(|(i, _)| Vec2::new((i % WIDTH) as f32, (i / WIDTH) as f32)).collect()
}

fn separate_marks(engine: &mut Engine) -> usize {
    let heights = engine.paint_heights();
    let mut seen = vec![false; heights.len()];
    let mut marks = 0;
    for start in 0..heights.len() {
        if seen[start] || heights[start] <= TRACE {
            continue;
        }
        marks += 1;
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % WIDTH) as i32, (i / WIDTH) as i32);
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < WIDTH as i32 && ny < HEIGHT as i32 {
                    let n = ny as usize * WIDTH + nx as usize;
                    if !seen[n] && heights[n] > TRACE {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
    }
    marks
}

#[test]
fn every_drop_of_a_throw_lands_on_the_canvas_is_settling_into_it_or_is_still_in_the_air() {
    let mut engine = splatter_tool(0.5, 1.0);
    engine.set_param("splatter_force", 5.0);
    engine.splatter(START, START + Vec2::new(ARROW, 0.0));
    let thrown = engine.drips().airborne() / METRES_PER_PIXEL.powi(3);
    for seconds in [0.1, 0.4, 1.5] {
        engine.advance_exact(seconds);
        let accounted = engine.paint_mass() + engine.film().unsettled() + engine.drips().airborne() / METRES_PER_PIXEL.powi(3);
        assert!((accounted - thrown).abs() < thrown * 0.01, "after {seconds} s {thrown:.0} thrown but {accounted:.0} found");
    }
    assert_eq!(engine.drips().airborne(), 0.0, "a throw should have landed after a second and a half");
}

#[test]
fn however_hard_the_throw_the_paint_lands_along_the_arrow() {
    for force in [1.0, 4.0, 9.0] {
        let mut engine = thrown(0.4, 1.0, force);
        let spots = painted(&mut engine);
        let inside = spots.iter().filter(|at| (-40.0..ARROW + 120.0).contains(&(at.x - START.x))).count();
        let reach = spots.iter().map(|at| at.x - START.x).fold(0.0, f32::max);
        println!("{force} m/s: {inside} of {} painted pixels within the arrow, reaching {reach:.0} px", spots.len());
        assert!(inside as f32 > 0.97 * spots.len() as f32, "at {force} m/s the paint should land along the arrow, {inside} of {} pixels did", spots.len());
        assert!(reach > 0.5 * ARROW, "at {force} m/s the paint should spread towards the arrow tip, reached {reach:.0} px");
    }
}

#[test]
fn a_harder_throw_into_the_same_arrow_spreads_wider_and_shatters_runny_paint() {
    let (soft, hard) = (painted(&mut thrown(0.4, 1.0, 1.0)).len(), painted(&mut thrown(0.4, 1.0, 9.0)).len());
    let (gentle, smashed) = (separate_marks(&mut thrown(0.7, 1.0, 1.0)), separate_marks(&mut thrown(0.7, 1.0, 9.0)));
    println!("a gentle throw covers {soft} px, a hard one {hard} px; runny paint lands as {gentle} and {smashed} marks");
    assert!(hard as f32 > 1.3 * soft as f32, "throwing harder should spread the paint wider, {soft} vs {hard} px");
    assert!(smashed > 2 * gentle, "throwing runny paint harder should shatter it into more pieces, {gentle} vs {smashed} marks");
}

#[test]
fn runny_paint_splashes_wider_and_thinner_than_stiff_paint() {
    let (mut stiff, mut runny) = (thrown(STIFF, 1.0, 4.0), thrown(RUNNY, 1.0, 4.0));
    let cover = |engine: &mut Engine| painted(engine).len() as f32 / engine.paint_mass();
    let (stiff_cover, runny_cover) = (cover(&mut stiff), cover(&mut runny));
    let (stiff_marks, runny_marks) = (separate_marks(&mut stiff), separate_marks(&mut runny));
    println!("stiff paint covers {stiff_cover:.3} px per unit in {stiff_marks} marks, runny paint {runny_cover:.3} in {runny_marks}");
    assert!(runny_cover > 1.5 * stiff_cover, "runny paint should splash out thinner over more canvas, {stiff_cover:.3} vs {runny_cover:.3}");
    assert!(runny_marks >= stiff_marks, "runny paint should land in at least as many drops, {stiff_marks} vs {runny_marks}");
}

#[test]
fn the_load_sets_how_much_paint_is_thrown() {
    let (light, full) = (thrown(0.5, 0.25, 3.0).paint_mass(), thrown(0.5, 1.0, 3.0).paint_mass());
    assert!((full / light - 4.0).abs() < 0.1, "a full load should throw four times a quarter load, {light:.0} vs {full:.0}");
}

#[test]
fn the_same_throw_splatters_the_same_way() {
    assert!(thrown(0.6, 1.0, 6.0).paint_columns() == thrown(0.6, 1.0, 6.0).paint_columns(), "a throw must be reproducible");
}

fn sample(phase: PointerPhase, at: Vec2, time: f64) -> PointerSample {
    PointerSample { phase, x: at.x, y: at.y, pressure: 0.5, tilt: [0.0; 2], rotation: 0.0, time }
}

#[test]
fn dragging_shows_the_arrow_and_releasing_throws_along_it() {
    let mut engine = splatter_tool(0.5, 1.0);
    let tip = START + Vec2::new(150.0, 0.0);
    engine.pointer(sample(PointerPhase::Down, START, 0.0));
    engine.pointer(sample(PointerPhase::Move, tip, 0.1));
    engine.advance_exact(0.1);
    engine.render();
    let frame = engine.read_back();
    let middle = START.lerp(tip, 0.5);
    let pixel = frame.get_pixel(middle.x as u32, middle.y as u32);
    assert!(pixel[0] > 200 && pixel[2] < 120, "the arrow should be drawn while aiming, saw {pixel:?}");
    assert_eq!(engine.paint_mass(), 0.0, "nothing is thrown before release");
    engine.pointer(sample(PointerPhase::Up, tip, 0.2));
    engine.advance_exact(1.7);
    assert!(engine.paint_mass() > 0.0, "releasing should throw the paint");
}
