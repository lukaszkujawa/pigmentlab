use glam::{Vec2, Vec3, Vec4};
use pigmentlab_core::brush::{Brush, BrushPose, BrushPreset, MAX_COMPRESSION, VERTICES_PER_BRISTLE, clump_count};
use pigmentlab_core::pigment::by_name;
use pigmentlab_core::canvas::Canvas;
use pigmentlab_core::gpu::{Gpu, read_buffer};
use pigmentlab_core::paint::ActiveWindow;
use pigmentlab_core::CanvasSize;
use pigmentlab_core::units::cm;

const CANVAS: [f32; 2] = [512.0, 512.0];
const LANDING_STEPS: usize = 15;

fn run(preset: BrushPreset, poses: impl Iterator<Item = BrushPose>) -> (Gpu, Brush, Vec<Vec4>) {
    run_loaded(preset, 0.0, poses)
}

fn run_loaded(preset: BrushPreset, load: f32, poses: impl Iterator<Item = BrushPose>) -> (Gpu, Brush, Vec<Vec4>) {
    let gpu = Gpu::headless();
    let canvas = Canvas::new(&gpu, CanvasSize { width: 512, height: 512 }, 1);
    let window = ActiveWindow::new(&gpu, &canvas);
    let flat_canvas = gpu.storage_buffer("flat stroke base", 512 * 512 * 4);
    let brush = Brush::new(&gpu, preset, 7, &window, &canvas, &flat_canvas);
    brush.dip(&gpu, by_name("cadmium_red").unwrap().into(), load, 0.0);
    let step = |pose: &BrushPose| {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        brush.encode_step(&gpu, &mut encoder, pose, 1.0, 0.0, CANVAS);
        gpu.queue.submit(Some(encoder.finish()));
    };
    let mut poses = poses.peekable();
    if let Some(first) = poses.peek() {
        let above = BrushPose::upright(first.position.with_z(first.position.z.max(preset.length * 1.25)));
        brush.teleport(&gpu, &above);
        for i in 1..=LANDING_STEPS {
            let height = above.position.z + (first.position.z - above.position.z) * i as f32 / LANDING_STEPS as f32;
            step(&BrushPose::upright(first.position.with_z(height)));
        }
    }
    for pose in poses {
        step(&pose);
    }
    let positions = read_buffer::<Vec4>(&gpu, &brush.positions);
    (gpu, brush, positions)
}

fn canvas_positions(pose: &BrushPose, brush_frame: &[Vec4]) -> Vec<Vec3> {
    brush_frame.iter().map(|p| pose.to_canvas(p.truncate())).collect()
}

#[test]
fn bristles_hold_rest_shape_above_canvas() {
    let preset = BrushPreset::round(32, cm(0.3), cm(2.0));
    let pose = BrushPose::upright(Vec3::new(256.0, 256.0, preset.length * 1.5));
    let (_, brush, positions) = run(preset, std::iter::repeat_n(pose, 120));
    for (vertex, rest) in positions.iter().zip(brush.rest()).skip(1) {
        assert!(vertex.truncate().distance(rest.truncate()) < 0.5, "a hovering bristle should keep its rest shape: {vertex} vs {rest}");
    }
}

#[test]
fn pressing_keeps_bristles_above_canvas_and_inextensible() {
    let preset = BrushPreset::round(64, cm(0.3), cm(2.0));
    let pressed = BrushPose::upright(Vec3::new(256.0, 256.0, preset.length * 0.7));
    let (_, _, positions) = run(preset, std::iter::repeat_n(pressed, 240));
    let canvas = canvas_positions(&pressed, &positions);
    for bristle in canvas.chunks(VERTICES_PER_BRISTLE as usize) {
        let polyline: f32 = bristle[1..].windows(2).map(|pair| pair[0].distance(pair[1])).sum();
        assert!(polyline > preset.length * 0.75 && polyline < preset.length * 1.05, "length {polyline} vs {}", preset.length);
        for vertex in &bristle[1..] {
            assert!(vertex.z >= -0.01, "vertex penetrated canvas: {vertex}");
        }
    }
}

fn contact_points(pose: &BrushPose, positions: &[Vec4]) -> Vec<Vec2> {
    canvas_positions(pose, positions)
        .chunks(VERTICES_PER_BRISTLE as usize)
        .flat_map(|bristle| bristle[2..].to_vec())
        .filter(|p| p.z < 1.0)
        .map(|p| p.truncate())
        .collect()
}

fn extent(points: &[Vec2]) -> Vec2 {
    let (low, high) = points.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    high - low
}

#[test]
fn pressing_spreads_the_footprint() {
    let preset = BrushPreset::round(64, cm(0.3), cm(2.0));
    let pressed = BrushPose::upright(Vec3::new(256.0, 256.0, preset.length * 0.7));
    let (_, _, positions) = run(preset, std::iter::repeat_n(pressed, 240));
    let footprint = extent(&contact_points(&pressed, &positions));
    assert!(footprint.min_element() > preset.width, "a pressed tuft should cover more than its ferrule, got {footprint}");
}

#[test]
fn bristles_follow_a_fast_moving_brush() {
    let preset = BrushPreset::round(32, cm(0.3), cm(2.0));
    let height = preset.length * 1.5;
    let still = BrushPose::upright(Vec3::new(100.0, 256.0, height));
    let (_, _, resting) = run(preset, std::iter::repeat_n(still, 120));
    let poses = (0..120).map(|i| BrushPose::upright(Vec3::new(100.0 + i as f32 * 3.0, 256.0, height)));
    let (_, _, moving) = run(preset, poses);
    for (bristle, rest) in moving.chunks(VERTICES_PER_BRISTLE as usize).zip(resting.chunks(VERTICES_PER_BRISTLE as usize)) {
        let lag = (bristle.last().unwrap() - rest.last().unwrap()).truncate().truncate().length();
        assert!(lag < cm(0.25), "tip lagged {lag}px behind the brush");
    }
}

#[test]
fn dragging_on_canvas_bends_bristles_backwards() {
    let preset = BrushPreset::round(64, cm(0.3), cm(2.0));
    let height = preset.length * 0.7;
    let settle = std::iter::repeat_n(BrushPose::upright(Vec3::new(200.0, 256.0, height)), 60);
    let drag = (0..120).map(|i| BrushPose::upright(Vec3::new(200.0 + i as f32 * 1.5, 256.0, height)));
    let (_, _, positions) = run(preset, settle.chain(drag));
    let mean_tip_x = positions
        .chunks(VERTICES_PER_BRISTLE as usize)
        .map(|b| b.last().unwrap().x)
        .sum::<f32>()
        / 64.0;
    assert!(mean_tip_x < -cm(0.2), "tips should trail behind a dragged brush, mean x = {mean_tip_x}");
}

#[test]
fn static_press_settles_without_jitter() {
    let preset = BrushPreset::round(32, cm(0.3), cm(2.0));
    let pressed = BrushPose::upright(Vec3::new(256.0, 256.0, preset.length * 0.7));
    let (gpu, brush, settled) = run(preset, std::iter::repeat_n(pressed, 120));
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    brush.encode_step(&gpu, &mut encoder, &pressed, 1.0, 0.0, CANVAS);
    gpu.queue.submit(Some(encoder.finish()));
    let after = read_buffer::<Vec4>(&gpu, &brush.positions);
    let max_motion = settled.iter().zip(&after).map(|(a, b)| a.distance(*b)).fold(0.0f32, f32::max);
    assert!(max_motion < 0.05, "settled bristles moved {max_motion}px in one step");
}

fn dragged_contact(preset: BrushPreset) -> Vec<Vec2> {
    let height = preset.length * 0.7;
    let settle = std::iter::repeat_n(BrushPose::upright(Vec3::new(150.0, 256.0, height)), 60);
    let drag = (0..100).map(|i| BrushPose::upright(Vec3::new(150.0 + i as f32 * 2.0, 256.0, height)));
    let last = BrushPose::upright(Vec3::new(150.0 + 99.0 * 2.0, 256.0, height));
    let (_, _, positions) = run(preset, settle.chain(drag));
    contact_points(&last, &positions)
}

#[test]
fn dragged_tuft_fills_its_footprint() {
    let points = dragged_contact(BrushPreset::round(200, cm(0.35), cm(2.2)));
    let low = points.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let high = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let bands = 8;
    let mut filled = vec![false; bands];
    for p in &points {
        filled[(((p.y - low) / (high - low) * bands as f32) as usize).min(bands - 1)] = true;
    }
    assert!(filled.iter().all(|&f| f), "the dragged contact patch has empty lanes across its width: {filled:?}");
}

#[test]
fn dragged_tuft_keeps_the_ferrule_width() {
    let preset = BrushPreset::round(200, cm(0.35), cm(2.2));
    let width = extent(&dragged_contact(preset)).y;
    assert!(width > preset.width * 0.75 && width < preset.splayed_radius() * 2.2, "dragged footprint width {width} vs ferrule {}", preset.width);
}

fn tips_of(pose: &BrushPose, positions: &[Vec4]) -> Vec<Vec2> {
    canvas_positions(pose, positions).chunks(VERTICES_PER_BRISTLE as usize).map(|b| b.last().unwrap().truncate()).collect()
}

#[test]
fn stroke_start_never_throws_the_tuft_backwards() {
    let preset = BrushPreset::round(200, cm(0.35), cm(2.2));
    let height = preset.length * 0.7;
    let landed = BrushPose::upright(Vec3::new(300.0, 256.0, height));
    let (_, _, at_landing) = run(preset, std::iter::once(landed));
    let centroid = |tips: Vec<Vec2>| tips.iter().sum::<Vec2>() / tips.len() as f32;
    let start = centroid(tips_of(&landed, &at_landing));
    for steps in [3usize, 10, 30] {
        let poses = std::iter::once(landed).chain((1..=steps).map(|i| BrushPose::upright(Vec3::new(300.0 - i as f32 * 2.5, 256.0, height))));
        let last = BrushPose::upright(Vec3::new(300.0 - steps as f32 * 2.5, 256.0, height));
        let (_, _, positions) = run(preset, poses);
        let backwards = centroid(tips_of(&last, &positions)).x - start.x;
        assert!(backwards < 2.0, "after {steps} steps leftwards the tips moved {backwards:.1} px to the right");
    }
}

#[test]
fn jittery_hand_does_not_catapult_tips() {
    let preset = BrushPreset::round(200, cm(0.35), cm(2.2));
    let height = preset.length * 0.7;
    let mut poses = vec![BrushPose::upright(Vec3::new(256.0, 256.0, height)); 30];
    let mut rng = 7u32;
    let mut position = Vec2::new(256.0, 256.0);
    for _ in 0..200 {
        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        let angle = (rng >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
        position += Vec2::new(angle.cos(), angle.sin()) * 1.5;
        poses.push(BrushPose::upright(Vec3::new(position.x, position.y, height)));
    }
    let (gpu, brush, before) = run(preset, poses.iter().cloned());
    let last = *poses.last().unwrap();
    let next = BrushPose::upright(last.position + Vec3::new(1.5, 0.0, 0.0));
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    brush.encode_step(&gpu, &mut encoder, &next, 1.0, 0.0, CANVAS);
    gpu.queue.submit(Some(encoder.finish()));
    let after = read_buffer::<Vec4>(&gpu, &brush.positions);
    let jump = tips_of(&last, &before).iter().zip(tips_of(&next, &after)).map(|(a, b)| a.distance(b)).fold(0.0f32, f32::max);
    assert!(jump < 6.0, "a tip jumped {jump:.1} px in one substep of a 1.5 px hand movement");
}

#[test]
fn hovering_tips_follow_the_handle() {
    let preset = BrushPreset::round(120, cm(0.06), cm(1.0));
    let path = |i: usize| BrushPose::upright(Vec3::new(100.0 + i as f32 * 1.5, 256.0, preset.length));
    let (_, _, positions) = run(preset, (0..240).map(path));
    let last = path(239);
    for tip in tips_of(&last, &positions) {
        assert!(tip.distance(last.position.truncate()) <= preset.length, "tip left behind at {tip} while the handle is at {}", last.position.truncate());
    }
}

fn pressed_extent(preset: BrushPreset, pressure: f32) -> Vec2 {
    let pose = BrushPose::upright(Vec3::new(256.0, 256.0, preset.length * (1.0 - MAX_COMPRESSION * pressure)));
    let (_, _, positions) = run(preset, std::iter::repeat_n(pose, 120));
    extent(&contact_points(&pose, &positions))
}

#[test]
fn a_light_touch_paints_with_the_tip_of_a_round_brush() {
    let preset = BrushPreset::round(200, cm(0.35), cm(2.2));
    let light = pressed_extent(preset, 0.05);
    let heavy = pressed_extent(preset, 1.0);
    assert!(light.x < preset.width * 0.5, "a light touch should use the tip only, footprint {light} vs ferrule {}", preset.width);
    assert!(light.x < heavy.x * 0.4, "pressing should widen the footprint far beyond a light touch: {light} vs {heavy}");
}

#[test]
fn a_light_touch_narrows_the_edge_of_a_flat_brush() {
    let preset = BrushPreset::flat(400, cm(1.2), cm(0.3), cm(2.0));
    let light = pressed_extent(preset, 0.05);
    let heavy = pressed_extent(preset, 1.0);
    assert!(light.y < heavy.y * 0.5, "the short side should narrow under a light touch: {light} vs {heavy}");
    assert!(light.x > heavy.x * 0.6, "the long side should stay wide: {light} vs {heavy}");
}

#[test]
fn a_lifted_brush_springs_back_to_its_rest_shape() {
    let preset = BrushPreset::round(120, cm(0.35), cm(2.2));
    let pressed = preset.length * 0.7;
    let lifted = preset.length * 1.25;
    let drag = (0..80).map(|i| BrushPose::upright(Vec3::new(150.0 + i as f32 * 2.0, 256.0, pressed)));
    let lift = (0..24).map(|i| BrushPose::upright(Vec3::new(310.0, 256.0, pressed + (lifted - pressed) * (i as f32 / 12.0).min(1.0))));
    let (_, brush, positions) = run(preset, drag.chain(lift));
    let worst = positions.iter().zip(brush.rest()).skip(1).map(|(p, r)| p.truncate().distance(r.truncate())).fold(0.0f32, f32::max);
    assert!(worst < 3.0, "0.05 s after lifting the hairs should be back in shape, worst vertex is {worst:.1} px off");
}

fn clump_spread(load: f32) -> f32 {
    let preset = BrushPreset::round(200, cm(0.35), cm(2.2));
    let height = preset.length * 0.75;
    let drag = (0..90).map(|i| BrushPose::upright(Vec3::new(150.0 + i as f32 * 2.0, 256.0, height)));
    let last = BrushPose::upright(Vec3::new(150.0 + 89.0 * 2.0, 256.0, height));
    let (_, _, positions) = run_loaded(preset, load, drag);
    let tips = tips_of(&last, &positions);
    let clumps = clump_count(&preset) as usize;
    let mut total = 0.0;
    let mut pairs = 0;
    for (a, tip_a) in tips.iter().enumerate() {
        for (b, tip_b) in tips.iter().enumerate().skip(a + 1) {
            if a % clumps == b % clumps {
                total += tip_a.distance(*tip_b);
                pairs += 1;
            }
        }
    }
    total / pairs as f32
}

#[test]
fn wet_hairs_gather_into_clumps() {
    let dry = clump_spread(0.0);
    let wet = clump_spread(1.0);
    assert!(wet < dry * 0.7, "a loaded brush should pull its hairs into clumps: tips of a clump are {wet:.1} px apart wet vs {dry:.1} px dry");
}
