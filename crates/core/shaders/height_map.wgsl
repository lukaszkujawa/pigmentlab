@group(0) @binding(0) var<uniform> window: WindowUniform;
@group(0) @binding(1) var<storage, read> store: array<i32>;
@group(0) @binding(2) var<storage, read> volume: array<i32>;
@group(0) @binding(3) var<storage, read_write> heights: array<f32>;
@group(0) @binding(4) var<storage, read> ground: array<f32>;
@group(0) @binding(5) var<storage, read_write> surface: array<f32>;
@group(0) @binding(6) var<storage, read_write> smooth_surface: array<f32>;
@group(0) @binding(7) var<storage, read_write> smooth_paint: array<f32>;
@group(0) @binding(8) var<uniform> region: Region;
@group(0) @binding(9) var<storage, read_write> tuft_base: array<f32>;

const TUFT_FLOOR_RADIUS: i32 = 3;

struct Region {
    origin: vec2<i32>,
    size: vec2<u32>,
};

fn region_texel(id: vec2<u32>) -> vec2<i32> {
    if (id.x >= region.size.x || id.y >= region.size.y) { return vec2<i32>(-1); }
    return region.origin + vec2<i32>(id);
}

@compute @workgroup_size(16, 16)
fn build(@builtin(global_invocation_id) id: vec3<u32>) {
    let texel = region_texel(id.xy);
    if (!on_canvas(texel)) { return; }
    var total = 0;
    if (window_contains(texel)) {
        let local = vec2<u32>(texel - window.origin);
        for (var z = 0u; z < window.depth; z++) {
            let cell = window_cell(vec3<u32>(local, z));
            total += volume[cell];
        }
    } else {
        total = store[store_cell(vec2<u32>(texel))];
    }
    let index = u32(texel.y) * window.canvas_width + u32(texel.x);
    let paint = f32(total) / PAINT_SCALE;
    heights[index] = paint;
    surface[index] = paint + ground[index];
}

fn clamped_index(texel: vec2<i32>) -> u32 {
    let clamped = clamp(texel, vec2<i32>(0), vec2<i32>(i32(window.canvas_width) - 1, i32(window.canvas_height) - 1));
    return u32(clamped.y) * window.canvas_width + u32(clamped.x);
}

@compute @workgroup_size(16, 16)
fn blur(@builtin(global_invocation_id) id: vec3<u32>) {
    let texel = region_texel(id.xy);
    if (!on_canvas(texel)) { return; }
    var surface_total = 0.0;
    var paint_total = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let index = clamped_index(texel + vec2<i32>(dx, dy));
            surface_total += surface[index];
            paint_total += heights[index];
        }
    }
    let index = u32(texel.y) * window.canvas_width + u32(texel.x);
    smooth_surface[index] = surface_total / 9.0;
    smooth_paint[index] = paint_total / 9.0;
}

@compute @workgroup_size(16, 16)
fn tuft_floor(@builtin(global_invocation_id) id: vec3<u32>) {
    let texel = region_texel(id.xy);
    if (!on_canvas(texel)) { return; }
    var total = 0.0;
    for (var dy = -TUFT_FLOOR_RADIUS; dy <= TUFT_FLOOR_RADIUS; dy++) {
        for (var dx = -TUFT_FLOOR_RADIUS; dx <= TUFT_FLOOR_RADIUS; dx++) {
            total += heights[clamped_index(texel + vec2<i32>(dx, dy))];
        }
    }
    let taps = f32((2 * TUFT_FLOOR_RADIUS + 1) * (2 * TUFT_FLOOR_RADIUS + 1));
    tuft_base[u32(texel.y) * window.canvas_width + u32(texel.x)] = total / taps;
}
