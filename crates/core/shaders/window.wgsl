const PAINT_SCALE: f32 = 65536.0;
const CHANNELS: u32 = 8u;
const FULL_CELL: f32 = 0.999;

struct WindowUniform {
    origin: vec2<i32>,
    canvas_width: u32,
    canvas_height: u32,
    width: u32,
    height: u32,
    depth: u32,
    padding: u32,
};

fn window_cell(local: vec3<u32>) -> u32 {
    return ((local.z * window.height + local.y) * window.width + local.x) * CHANNELS;
}

fn store_cell(pixel: vec2<u32>) -> u32 {
    return (pixel.y * window.canvas_width + pixel.x) * CHANNELS;
}

fn window_contains(pixel: vec2<i32>) -> bool {
    let local = pixel - window.origin;
    return local.x >= 0 && local.y >= 0 && local.x < i32(window.width) && local.y < i32(window.height);
}
