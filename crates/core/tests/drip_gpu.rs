use glam::Vec2;
use pigmentlab_core::drip::Hanging;
use pigmentlab_core::pigment::by_name;
use pigmentlab_core::units::{METRES_PER_PIXEL, cm};
use pigmentlab_core::{CanvasSize, Engine, Gpu, Viewport};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;
const ENAMEL: f32 = 0.35;
const THINNED: f32 = 1.0;

fn centre() -> Vec2 {
    Vec2::new(WIDTH as f32, HEIGHT as f32) * 0.5
}

struct Pour {
    medium: f32,
    string: f32,
    pivot: f32,
    hole: f32,
    swing: Vec2,
    push: Vec2,
    seconds: f64,
}

impl Pour {
    fn still(medium: f32) -> Self {
        Self { medium, string: 100.0, pivot: 115.0, hole: 4.0, swing: Vec2::ZERO, push: Vec2::ZERO, seconds: 3.0 }
    }

    fn swinging(medium: f32) -> Self {
        Self { medium, string: 30.0, pivot: 42.0, hole: 4.0, swing: Vec2::new(cm(8.0), 0.0), push: Vec2::ZERO, seconds: 4.0 }
    }

    fn run(&self) -> (Engine, f64) {
        let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
        engine.set_param("medium", self.medium);
        engine.set_param("string", self.string);
        engine.set_param("pivot", self.pivot);
        engine.set_param("hole", self.hole);
        engine.dip(by_name("cadmium_red").unwrap());
        engine.hang_bottle(Hanging { pivot: centre(), release: centre() + self.swing, push: self.push });
        let filled = engine.drips().bottles()[0].volume;
        engine.advance_exact(self.seconds);
        let released = filled - engine.drips().bottles()[0].volume;
        (engine, released)
    }
}

fn painted(heights: &[f32]) -> impl Iterator<Item = (usize, usize)> + '_ {
    heights.iter().enumerate().filter(|(_, h)| **h > 0.05).map(|(i, _)| (i % WIDTH, i / WIDTH))
}

fn blobs(heights: &[f32]) -> Vec<f32> {
    let mut seen = vec![false; heights.len()];
    let mut volumes = Vec::new();
    for start in 0..heights.len() {
        if seen[start] || heights[start] <= 0.05 {
            continue;
        }
        let mut volume = 0.0;
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            volume += heights[i];
            let (x, y) = (i % WIDTH, i / WIDTH);
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= WIDTH as i32 || ny >= HEIGHT as i32 {
                    continue;
                }
                let n = ny as usize * WIDTH + nx as usize;
                if !seen[n] && heights[n] > 0.05 {
                    seen[n] = true;
                    stack.push(n);
                }
            }
        }
        volumes.push(volume);
    }
    volumes
}

fn largest_share(blobs: &[f32]) -> f32 {
    blobs.iter().copied().fold(0.0, f32::max) / blobs.iter().sum::<f32>()
}

fn band_width(heights: &[f32], x: usize) -> usize {
    (0..HEIGHT).filter(|&y| heights[y * WIDTH + x] > 0.05).count()
}

#[test]
fn every_drop_of_paint_leaving_a_bottle_lands_on_the_canvas_or_is_still_falling() {
    for medium in [ENAMEL, THINNED] {
        let (mut engine, released) = Pour { hole: 2.0, ..Pour::swinging(medium) }.run();
        let on_canvas = f64::from(engine.paint_mass()) * f64::from(METRES_PER_PIXEL).powi(3);
        let falling = f64::from(engine.drips().airborne());
        println!("medium {medium}: released {:.3} cm3, on canvas {:.3}, falling {:.3}", released * 1e6, on_canvas * 1e6, falling * 1e6);
        assert!(released > 1e-7, "the bottle should pour");
        assert!((on_canvas + falling - released).abs() < released * 0.01, "at {medium} medium {:.3} cm3 left the bottle but {:.3} landed or is falling", released * 1e6, (on_canvas + falling) * 1e6);
    }
}

#[test]
fn a_bottle_hanging_still_pools_its_thread_on_one_spot() {
    let (mut engine, released) = Pour::still(ENAMEL).run();
    let heights = engine.paint_heights();
    let reach = painted(&heights).map(|(x, y)| Vec2::new(x as f32, y as f32).distance(centre())).fold(0.0f32, f32::max);
    let thinnest_puddle = cm(0.06) * METRES_PER_PIXEL;
    let widest = (released as f32 / (std::f32::consts::PI * thinnest_puddle)).sqrt() / METRES_PER_PIXEL;
    println!("still bottle pools {:.2} cm3 within {reach:.1} px ({:.2} mm), a puddle no thinner than 0.6 mm reaches {widest:.1} px", released * 1e6, reach * METRES_PER_PIXEL * 1000.0);
    assert!(reach > 4.0 && reach < widest, "a still bottle should pool its thread in one puddle under it, reaches {reach:.1} px of at most {widest:.1} px");
}

#[test]
fn a_fast_swing_lays_a_straight_line_through_the_middle_and_loops_where_it_turns() {
    let (mut engine, _) = Pour { seconds: 2.5, ..Pour::swinging(ENAMEL) }.run();
    let heights = engine.paint_heights();
    let middle = band_width(&heights, centre().x as usize);
    let turning = (cm(4.0) as usize..cm(10.0) as usize).flat_map(|offset| [centre().x as usize - offset, centre().x as usize + offset]).map(|x| band_width(&heights, x)).max().unwrap();
    println!("band width: {middle} px through the middle, {turning} px where it turns");
    assert!(middle > 0 && middle * 2 < turning, "the fast middle of the swing should lay a narrow line ({middle} px) and the slow ends loop widely ({turning} px)");
}

#[test]
fn a_thin_trickle_drips_separate_drops_while_enamel_stays_one_thread() {
    let (mut enamel, _) = Pour::swinging(ENAMEL).run();
    let (mut thinned, _) = Pour { hole: 1.0, ..Pour::swinging(THINNED) }.run();
    let (enamel_blobs, thinned_blobs) = (blobs(&enamel.paint_heights()), blobs(&thinned.paint_heights()));
    println!("enamel keeps {:.3} of its paint in one mark, thinned paint lays {} separate marks", largest_share(&enamel_blobs), thinned_blobs.len());
    assert!(largest_share(&enamel_blobs) > 0.95, "enamel should pour one continuous thread, its largest mark holds {:.3} of the paint", largest_share(&enamel_blobs));
    assert!(thinned_blobs.len() >= 6 && largest_share(&thinned_blobs) < 0.5, "a thin trickle should drip a dotted line, made {} separate marks, the largest holding {:.3} of the paint", thinned_blobs.len(), largest_share(&thinned_blobs));
}

#[test]
fn the_same_swing_paints_the_same_marks() {
    let (mut first, _) = Pour::swinging(THINNED).run();
    let (mut second, _) = Pour::swinging(THINNED).run();
    assert!(first.paint_columns() == second.paint_columns(), "a swing must be reproducible");
}

fn sample(phase: pigmentlab_core::PointerPhase, at: Vec2, time: f64) -> pigmentlab_core::PointerSample {
    pigmentlab_core::PointerSample { phase, x: at.x, y: at.y, pressure: 0.5, tilt: [0.0; 2], rotation: 0.0, time }
}

fn drag(engine: &mut Engine, path: &[Vec2], start: f64, seconds: f64) -> f64 {
    use pigmentlab_core::PointerPhase::{Down, Move, Up};
    engine.pointer(sample(Down, path[0], start));
    let steps = 60;
    for k in 1..=steps {
        let u = k as f32 / steps as f32 * (path.len() - 1) as f32;
        let (i, f) = ((u.floor() as usize).min(path.len() - 2), u - u.floor().min((path.len() - 2) as f32));
        let at = path[i].lerp(path[i + 1], f);
        engine.pointer(sample(Move, at, start + seconds * k as f64 / steps as f64));
    }
    let end = start + seconds;
    engine.pointer(sample(Up, *path.last().unwrap(), end));
    engine.advance_exact(end + 0.01);
    end + 0.01
}

fn bottle_tool() -> Engine {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
    engine.select_brush("Drip bottle");
    engine.set_param("medium", ENAMEL);
    engine.set_param("string", 40.0);
    engine.set_param("pivot", 55.0);
    engine.dip(by_name("ivory_black").unwrap());
    engine
}

fn swing_reach(engine: &Engine, index: usize) -> f32 {
    let pendulum = &engine.drips().bottles()[index].pendulum;
    let rise = pendulum.bob.z - (pendulum.pivot.z - pendulum.length) + 0.5 * pendulum.velocity.length_squared() / pigmentlab_core::units::GRAVITY;
    (2.0 * pendulum.length * rise.max(0.0)).sqrt()
}

fn hole(engine: &Engine, index: usize) -> Vec2 {
    engine.drips().bottles()[index].pendulum.bob.truncate() / METRES_PER_PIXEL
}

fn pivot(engine: &Engine, index: usize) -> Vec2 {
    engine.drips().bottles()[index].pendulum.pivot.truncate() / METRES_PER_PIXEL
}

#[test]
fn pressing_the_canvas_hangs_a_corked_bottle_from_the_hook_into_your_hand() {
    let mut engine = bottle_tool();
    let pulled = centre() + Vec2::new(cm(8.0), 0.0);
    let released = drag(&mut engine, &[pulled, pulled], 0.0, 0.3);
    assert_eq!(engine.drips().bottles().len(), 1, "pressing on bare canvas should hang one bottle");
    assert!(pivot(&engine, 0).distance(centre()) < 1.0, "the bottle should hang from the hook above the middle of the canvas");
    assert!(hole(&engine, 0).distance(pulled) < cm(0.3), "the hand should hold the bottle where it pressed");
    let mut crossed = false;
    for k in 1..=60 {
        engine.advance_exact(released + k as f64 * 0.02);
        crossed |= hole(&engine, 0).x < centre().x - cm(4.0);
    }
    assert!(crossed, "let go 8 cm out, the bottle should swing under the hook to the far side");
    assert_eq!(engine.paint_mass(), 0.0, "a new bottle stays corked until it is told to pour");
}

#[test]
fn a_bottle_pours_only_while_its_flow_is_turned_on() {
    let mut engine = bottle_tool();
    let at = centre() + Vec2::new(cm(6.0), 0.0);
    let time = drag(&mut engine, &[at, at], 0.0, 0.1);
    let id = engine.drips().bottles()[0].id;
    engine.advance_exact(time + 1.0);
    assert_eq!(engine.paint_mass(), 0.0);
    engine.set_pouring(Some(id), true);
    engine.advance_exact(time + 2.0);
    let poured = engine.paint_mass();
    assert!(poured > 0.0, "turning the flow on should start the paint");
    engine.set_pouring(Some(id), false);
    engine.advance_exact(time + 2.5);
    let after = engine.paint_mass();
    engine.advance_exact(time + 4.0);
    assert!((engine.paint_mass() - after).abs() < after * 0.01, "corking the bottle should stop the flow");
}

#[test]
fn a_thrown_bottle_keeps_the_hands_velocity_until_the_string_turns_it_into_an_orbit() {
    for throw in [1.0, 2.0] {
        let mut engine = bottle_tool();
        engine.set_param("throw", throw);
        let start = centre() + Vec2::new(cm(6.0), 0.0);
        let time = drag(&mut engine, &[start, start + Vec2::new(0.0, cm(4.0))], 0.0, 0.2);
        let velocity = engine.drips().bottles()[0].pendulum.velocity.truncate();
        assert!((velocity.y - 0.2 * throw).abs() < 0.06 * throw && velocity.x.abs() < 0.08, "the bottle should leave the hand at {throw} times its speed, at {velocity}");
        let mut previous = hole(&engine, 0) - centre();
        let mut turning = 0.0;
        for k in 1..=150 {
            engine.advance_exact(time + k as f64 * 0.01);
            let now = hole(&engine, 0) - centre();
            turning += previous.perp_dot(now).atan2(previous.dot(now));
            previous = now;
        }
        assert!(turning > 1.5, "the string should swing the thrown bottle round the hook, turned {turning:.2} rad");
    }
}

#[test]
fn dragging_the_hook_carries_its_bottles_along_on_their_strings() {
    let mut engine = bottle_tool();
    let at = centre() + Vec2::new(cm(3.0), 0.0);
    let time = drag(&mut engine, &[at, at], 0.0, 0.1);
    engine.stop_bottles(None);
    let moved = centre() + Vec2::new(cm(6.0), cm(2.0));
    let time = drag(&mut engine, &[centre(), moved], time, 0.5);
    assert_eq!(engine.drips().bottles().len(), 1, "grabbing the hook must not hang another bottle");
    assert!(pivot(&engine, 0).distance(moved) < 1.0, "the bottle's string should now hang from the moved hook");
    let mut closest = f32::MAX;
    for k in 1..=200 {
        engine.advance_exact(time + k as f64 * 0.01);
        closest = closest.min(hole(&engine, 0).distance(moved));
    }
    assert!(closest < cm(2.0), "the bottle should swing over towards the hook's new place, came within {:.1} cm", closest / cm(1.0));
}

#[test]
fn a_thrown_bottle_knocks_another_on_the_same_hook_into_motion() {
    let mut engine = bottle_tool();
    let time = drag(&mut engine, &[centre() + Vec2::new(cm(2.0), 0.0); 2], 0.0, 0.1);
    engine.stop_bottles(None);
    let resting = hole(&engine, 0);
    let time = drag(&mut engine, &[centre() - Vec2::new(cm(9.0), 0.0); 2], time, 0.1);
    let mut moved = 0.0f32;
    for k in 1..=150 {
        engine.advance_exact(time + k as f64 * 0.01);
        moved = moved.max(hole(&engine, 0).distance(resting));
        let gap = hole(&engine, 0).distance(hole(&engine, 1));
        assert!(gap > cm(5.0), "the bottles must not pass through each other, holes {gap:.0} px apart");
    }
    assert!(moved > cm(1.0), "the knocked bottle should swing away, moved {moved:.0} px");
}

#[test]
#[ignore]
fn three_swinging_bottles_fit_a_sixty_hertz_frame() {
    let mut engine = bottle_tool();
    engine.set_param("pour", 1.0);
    let mut time = 0.0;
    for offset in [Vec2::new(-cm(7.0), 0.0), Vec2::new(cm(7.0), 0.0), Vec2::new(0.0, cm(6.0))] {
        let at = centre() + offset;
        time = drag(&mut engine, &[at, at + Vec2::new(cm(3.0), cm(2.0))], time, 0.3);
    }
    let start = engine.time();
    let started = std::time::Instant::now();
    let frames = 600;
    for frame in 1..=frames {
        engine.advance_realtime(start + frame as f64 / 60.0);
        engine.render();
    }
    engine.paint_mass();
    let per_frame = started.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!("{per_frame:.2} ms per 60 Hz frame with three bottles");
    assert!(per_frame < 1000.0 / 60.0, "three swinging bottles should fit a 60 Hz frame, took {per_frame:.2} ms");
}

fn bottles_on_their_own_hooks(count: usize) -> (Engine, f64, Vec<Vec2>) {
    let mut engine = bottle_tool();
    engine.set_param("radius", 2.0);
    let hooks: Vec<Vec2> = (0..count).map(|k| centre() + Vec2::new((k as f32 - 1.0) * cm(9.0), 0.0)).collect();
    let mut time = 0.0;
    for hook in &hooks {
        engine.set_param("hook_x", hook.x);
        engine.set_param("hook_y", hook.y);
        let pulled = *hook + Vec2::new(0.0, cm(3.0));
        time = drag(&mut engine, &[pulled, pulled], time, 0.05);
    }
    engine.set_pouring(None, true);
    engine.advance_exact(time + 1.0);
    (engine, time + 1.0, hooks)
}

#[test]
fn pausing_freezes_the_bottles_and_their_paint_until_resumed() {
    let (mut engine, time, _) = bottles_on_their_own_hooks(2);
    engine.set_drips_paused(true);
    let (held, mass) = (hole(&engine, 0), engine.paint_mass());
    engine.advance_exact(time + 1.0);
    assert_eq!(hole(&engine, 0), held, "a paused bottle must not move");
    assert!((engine.paint_mass() - mass).abs() < mass * 1e-6, "a paused bottle must not pour, {mass} -> {}", engine.paint_mass());
    engine.set_drips_paused(false);
    engine.advance_exact(time + 1.3);
    assert!(hole(&engine, 0).distance(held) > cm(0.5), "resuming should set the bottles going again");
}

#[test]
fn stopping_brings_one_bottle_or_all_of_them_to_rest_under_their_hooks() {
    let (mut engine, time, hooks) = bottles_on_their_own_hooks(3);
    let first = engine.drips().bottles()[0].id;
    engine.stop_bottles(Some(first));
    engine.advance_exact(time + 0.5);
    assert!(hole(&engine, 0).distance(hooks[0]) < cm(0.1), "the stopped bottle should hang still under its hook");
    let reach = swing_reach(&engine, 1);
    assert!(reach > 0.01, "the others should keep going, swinging {:.1} cm", reach * 100.0);
    engine.stop_bottles(None);
    engine.advance_exact(time + 1.0);
    for (k, hook) in hooks.iter().enumerate() {
        assert!(hole(&engine, k).distance(*hook) < cm(0.1), "stop all should rest bottle {k}");
    }
}

#[test]
fn a_selected_bottle_takes_new_settings_while_it_swings_and_can_be_removed() {
    let (mut engine, time, _) = bottles_on_their_own_hooks(2);
    let [first, second] = [0, 1].map(|k| engine.drips().bottles()[k].id);
    assert_eq!(engine.drips().selected(), Some(second), "the bottle hung last should be selected");
    engine.select_bottle(Some(first));
    engine.set_bottle_param(first, "string", 25.0);
    engine.set_bottle_param(first, "radius", 5.0);
    engine.set_bottle_param(first, "hole", 6.0);
    engine.advance_exact(time + 0.2);
    let bottle = engine.drips().bottle(first).unwrap();
    assert!((bottle.pendulum.length - 0.25).abs() < 1e-4 && (bottle.radius - 0.05).abs() < 1e-6 && (bottle.hole_radius - 0.003).abs() < 1e-6, "the settings should apply to the swinging bottle: {:?}", (bottle.pendulum.length, bottle.radius, bottle.hole_radius));
    assert!((bottle.pendulum.bob - bottle.pendulum.pivot).length() <= 0.25 + 1e-4, "the shorter string should pull the bottle up");
    engine.remove_bottle(first);
    assert_eq!(engine.drips().bottles().len(), 1);
    assert_eq!(engine.drips().bottles()[0].id, second);
    assert_eq!(engine.drips().selected(), None, "removing the selected bottle should clear the selection");
}

#[test]
fn a_bottle_held_beyond_the_canvas_is_drawn_on_the_stage_around_it() {
    use pigmentlab_core::PointerPhase::{Down, Move};
    let mut engine = bottle_tool();
    engine.set_show_bristles(true);
    let stage = Viewport { size: glam::UVec2::new(WIDTH as u32, HEIGHT as u32), origin: Vec2::new(WIDTH as f32, HEIGHT as f32) * 0.25, scale: 0.5 };
    engine.set_stage(Some(stage));
    let beyond = Vec2::new(-600.0, centre().y);
    engine.pointer(sample(Down, centre() - Vec2::new(300.0, 0.0), 0.0));
    engine.pointer(sample(Move, beyond, 0.5));
    engine.advance_exact(1.0);
    engine.render();
    let on_stage = engine.read_stage().expect("the stage should be drawn");
    if let Ok(dir) = std::env::var("STAGE_PNG") {
        on_stage.save(format!("{dir}/stage.png")).unwrap();
    }
    let seen = stage.origin + hole(&engine, 0) * stage.scale;
    assert!(seen.x < stage.origin.x, "the bottle should be held off the canvas, at {seen}");
    assert!(on_stage.get_pixel(seen.x as u32, seen.y as u32)[3] > 0, "the bottle off the canvas should be drawn on the stage at {seen}");
    let canvas = engine.read_back();
    assert!(canvas.pixels().all(|pixel| pixel[3] == 255 && pixel[0] > 200), "with a stage the canvas itself carries no overlay");
}

fn most_drops_in_flight(pour: Pour) -> usize {
    let mut engine = Pour { seconds: 0.0, ..pour }.run().0;
    (1..=(pour.seconds * 60.0) as usize).fold(0, |most, frame| {
        engine.advance_exact(frame as f64 / 60.0);
        most.max(engine.drips().droplets().len())
    })
}

#[test]
fn a_runny_stream_falling_far_breaks_into_drops_in_flight_while_enamel_stays_one_thread() {
    let thinned = most_drops_in_flight(Pour { seconds: 0.8, hole: 2.0, pivot: 70.0, ..Pour::swinging(THINNED) });
    let (mut enamel, _) = Pour { seconds: 2.5, pivot: 70.0, ..Pour::swinging(ENAMEL) }.run();
    let enamel_share = largest_share(&blobs(&enamel.paint_heights()));
    println!("at most {thinned} drops of thinned paint in flight; enamel keeps {enamel_share:.3} of its paint in one mark");
    assert!(thinned >= 10, "a runny stream falling 40 cm should break into drops before it lands, at most {thinned} in flight");
    assert!(enamel_share > 0.9, "enamel should land as one thread over the same fall, its largest mark holds {enamel_share:.3} of the paint");
}

#[test]
fn changing_the_paint_refills_only_the_selected_bottle() {
    let mut engine = Engine::new(Gpu::headless(), CanvasSize { width: WIDTH as u32, height: HEIGHT as u32 }, 1);
    engine.set_param("medium", ENAMEL);
    engine.dip(by_name("cadmium_red").unwrap());
    engine.hang_bottle(Hanging { pivot: centre(), release: centre(), push: Vec2::ZERO });
    engine.set_param("medium", THINNED);
    engine.dip(by_name("french_ultramarine").unwrap());
    let bottle = &engine.drips().bottles()[0];
    assert_eq!(bottle.paint, by_name("french_ultramarine").unwrap().coefficients, "the selected bottle should take the new colour");
    assert_eq!((bottle.medium, bottle.fluid), (THINNED, pigmentlab_core::drip::Fluid::of(THINNED)), "the selected bottle should take the new medium");
    engine.select_bottle(None);
    engine.dip(by_name("cadmium_yellow").unwrap());
    assert_eq!(engine.drips().bottles()[0].paint, by_name("french_ultramarine").unwrap().coefficients, "with nothing selected the bottles keep their paint");
}
