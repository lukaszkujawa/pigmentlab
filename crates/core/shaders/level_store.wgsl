const LEVEL_RATE: f32 = 0.5;
const LEVEL_YIELD_SLOPE: f32 = 0.0;
const LEVEL_MIN_HEIGHT: f32 = 0.03 * PIXELS_PER_CM;

@group(0) @binding(0) var<uniform> window: WindowUniform;
@group(0) @binding(1) var<storage, read_write> store: array<i32>;
@group(0) @binding(2) var<storage, read> heights: array<f32>;

fn pixel_index(pixel: vec2<i32>) -> u32 {
    return u32(pixel.y) * window.canvas_width + u32(pixel.x);
}

fn on_canvas(pixel: vec2<i32>) -> bool {
    return pixel.x >= 0 && pixel.y >= 0 && pixel.x < i32(window.canvas_width) && pixel.y < i32(window.canvas_height);
}

fn is_dry(pixel: vec2<i32>) -> bool {
    return store[store_cell(vec2<u32>(pixel)) + 7u] >= i32(PAINT_SCALE);
}

fn flow_to(height: f32, neighbour: vec2<i32>) -> f32 {
    if (!on_canvas(neighbour) || is_dry(neighbour)) { return 0.0; }
    let other = heights[pixel_index(neighbour)];
    if (other <= 0.0) { return 0.0; }
    let flowing = max(max(height, other) - LEVEL_MIN_HEIGHT, 0.0);
    let difference = height - other;
    let beyond_yield = sign(difference) * max(abs(difference) - LEVEL_YIELD_SLOPE, 0.0);
    return beyond_yield * LEVEL_RATE * 0.25 * min(flowing / LEVEL_MIN_HEIGHT, 1.0);
}

fn level_pixel(pixel: vec2<i32>) {
    if (!on_canvas(pixel) || window_contains(pixel)) { return; }
    let height = heights[pixel_index(pixel)];
    if (height <= 0.0 || is_dry(pixel)) { return; }
    let cell = store_cell(vec2<u32>(pixel));
    let offsets = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    var outgoing = 0.0;
    var incoming = 0.0;
    var incoming_pigment: array<f32, 6>;
    for (var channel = 0u; channel < 6u; channel++) { incoming_pigment[channel] = 0.0; }
    for (var n = 0u; n < 4u; n++) {
        let neighbour = pixel + offsets[n];
        let flow = flow_to(height, neighbour);
        if (flow > 0.0) {
            outgoing += flow;
        } else if (flow < 0.0) {
            let other_cell = store_cell(vec2<u32>(neighbour));
            let other_height = max(f32(store[other_cell]) / PAINT_SCALE, 1e-6);
            incoming -= flow;
            for (var channel = 0u; channel < 6u; channel++) {
                incoming_pigment[channel] += f32(store[other_cell + 1u + channel]) / PAINT_SCALE / other_height * -flow;
            }
        }
    }
    let keep = 1.0 - min(outgoing / max(height, 1e-6), 1.0);
    store[cell] = i32(round((height * keep + incoming) * PAINT_SCALE));
    for (var channel = 0u; channel < 6u; channel++) {
        let own = f32(store[cell + 1u + channel]);
        store[cell + 1u + channel] = i32(round(own * keep + incoming_pigment[channel] * PAINT_SCALE));
    }
}

@compute @workgroup_size(16, 16)
fn level_even(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = vec2<i32>(i32(id.x) * 2 + i32(id.y & 1u), i32(id.y));
    level_pixel(pixel);
}

@compute @workgroup_size(16, 16)
fn level_odd(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = vec2<i32>(i32(id.x) * 2 + 1 - i32(id.y & 1u), i32(id.y));
    level_pixel(pixel);
}

@compute @workgroup_size(16, 16)
fn dry_all(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = vec2<i32>(id.xy);
    if (!on_canvas(pixel)) { return; }
    let cell = store_cell(vec2<u32>(pixel));
    if (store[cell] <= 0) { return; }
    store[cell + 7u] = i32(PAINT_SCALE);
}
