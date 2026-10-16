use glam::{Vec2, Vec3, Vec4};
use simpaint_core::brush::{Brush, BrushPose, BrushPreset, VERTICES_PER_BRISTLE};
use simpaint_core::canvas::Canvas;
use simpaint_core::gpu::{Gpu, read_buffer};
use simpaint_core::paint::ActiveWindow;
use simpaint_core::CanvasSize;
use simpaint_core::units::cm;

const CANVAS: [f32; 2] = [512.0, 512.0];

fn run(preset: BrushPreset, poses: impl Iterator<Item = BrushPose>) -> (Gpu, Brush, Vec<Vec4>) {
    let gpu = Gpu::headless();
    let canvas = Canvas::new(&gpu, CanvasSize { width: 512, height: 512 }, 1);
    let window = ActiveWindow::new(&gpu, &canvas);
    let flat_canvas = gpu.storage_buffer("flat stroke base", 512 * 512 * 4);
    let brush = Brush::new(&gpu, preset, 7, &window, &flat_canvas);
    let mut teleported = false;
    for pose in poses {
        if !teleported {
            brush.teleport(&gpu, &pose);
            teleported = true;
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        brush.encode_step(&gpu, &mut encoder, &pose, 1.0, 0.0, CANVAS);
        gpu.queue.submit(Some(encoder.finish()));
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
    let (_, _, positions) = run(preset, std::iter::repeat_n(pose, 120));
    for bristle in positions.chunks(VERTICES_PER_BRISTLE as usize) {
        let tip = bristle.last().unwrap();
        assert!(tip.z < -preset.length * 0.95, "tip should hang straight down, got {tip}");
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

#[test]
fn pressing_spreads_the_footprint() {
    let preset = BrushPreset::round(64, cm(0.3), cm(2.0));
    let pressed = BrushPose::upright(Vec3::new(256.0, 256.0, preset.length * 0.7));
    let (_, _, positions) = run(preset, std::iter::repeat_n(pressed, 240));
    let canvas = canvas_positions(&pressed, &positions);
    let spread = canvas
        .chunks(VERTICES_PER_BRISTLE as usize)
        .map(|b| (b.last().unwrap().truncate() - pressed.position.truncate()).length())
        .fold(0.0f32, f32::max);
    assert!(spread > cm(0.3) * 1.3, "tips should splay beyond the ferrule, got {spread}");
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

fn dragged_tips(preset: BrushPreset) -> (BrushPose, Vec<Vec2>) {
    let height = preset.length * 0.7;
    let settle = std::iter::repeat_n(BrushPose::upright(Vec3::new(150.0, 256.0, height)), 60);
    let drag = (0..100).map(|i| BrushPose::upright(Vec3::new(150.0 + i as f32 * 2.0, 256.0, height)));
    let last = BrushPose::upright(Vec3::new(150.0 + 99.0 * 2.0, 256.0, height));
    let (_, _, positions) = run(preset, settle.chain(drag));
    let tips = canvas_positions(&last, &positions)
        .chunks(VERTICES_PER_BRISTLE as usize)
        .map(|b| b.last().unwrap().truncate())
        .collect();
    (last, tips)
}

#[test]
fn dragged_tuft_fills_its_footprint() {
    let (_, tips) = dragged_tips(BrushPreset::round(200, cm(0.35), cm(2.2)));
    let centroid = tips.iter().sum::<Vec2>() / tips.len() as f32;
    let radii: Vec<f32> = tips.iter().map(|t| t.distance(centroid)).collect();
    let outer = radii.iter().cloned().fold(0.0f32, f32::max);
    let nearest = radii.iter().cloned().fold(f32::MAX, f32::min);
    let inner = radii.iter().filter(|&&r| r < outer * 0.5).count() as f32 / tips.len() as f32;
    assert!(nearest < outer * 0.1, "no tip within {:.0}% of the outer radius from the centre, the footprint is a ring", nearest / outer * 100.0);
    assert!(inner > 0.15, "only {:.0}% of tips lie in the inner half of the footprint", inner * 100.0);
}

#[test]
fn dragged_tuft_keeps_the_ferrule_width() {
    let preset = BrushPreset::round(200, cm(0.35), cm(2.2));
    let (_, tips) = dragged_tips(preset);
    let (low, high) = tips.iter().fold((f32::MAX, f32::MIN), |(lo, hi), t| (lo.min(t.y), hi.max(t.y)));
    let width = high - low;
    assert!(width > preset.width * 0.9 && width < preset.splayed_radius() * 2.2, "dragged footprint width {width} vs ferrule {}", preset.width);
}

fn tips_of(pose: &BrushPose, positions: &[Vec4]) -> Vec<Vec2> {
    canvas_positions(pose, positions).chunks(VERTICES_PER_BRISTLE as usize).map(|b| b.last().unwrap().truncate()).collect()
}

#[test]
fn stroke_start_never_throws_tips_backwards() {
    let preset = BrushPreset::round(200, cm(0.35), cm(2.2));
    let height = preset.length * 0.7;
    let landed = BrushPose::upright(Vec3::new(300.0, 256.0, height));
    let (_, _, at_landing) = run(preset, std::iter::once(landed));
    let start = tips_of(&landed, &at_landing);
    for steps in [3usize, 10, 30] {
        let poses = std::iter::once(landed).chain((1..=steps).map(|i| BrushPose::upright(Vec3::new(300.0 - i as f32 * 2.5, 256.0, height))));
        let last = BrushPose::upright(Vec3::new(300.0 - steps as f32 * 2.5, 256.0, height));
        let (_, _, positions) = run(preset, poses);
        let backwards = tips_of(&last, &positions).iter().zip(&start).map(|(now, then)| now.x - then.x).fold(0.0f32, f32::max);
        assert!(backwards < 2.0, "after {steps} steps leftwards a tip moved {backwards:.1} px to the right");
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
