use glam::{Vec3, Vec4};
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
    let brush = Brush::new(&gpu, preset, 7, &window);
    let mut teleported = false;
    for pose in poses {
        if !teleported {
            brush.teleport(&gpu, &pose);
            teleported = true;
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        brush.encode_step(&gpu, &mut encoder, &pose, CANVAS);
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
    brush.encode_step(&gpu, &mut encoder, &pressed, CANVAS);
    gpu.queue.submit(Some(encoder.finish()));
    let after = read_buffer::<Vec4>(&gpu, &brush.positions);
    let max_motion = settled.iter().zip(&after).map(|(a, b)| a.distance(*b)).fold(0.0f32, f32::max);
    assert!(max_motion < 0.05, "settled bristles moved {max_motion}px in one step");
}
