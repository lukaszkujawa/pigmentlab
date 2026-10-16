const LEVEL_RATE: f32 = 0.3;
const LEVEL_YIELD_SLOPE: f32 = 0.4;
const LEVEL_MIN_HEIGHT: f32 = 0.03 * PIXELS_PER_CM;
const SPREAD_MIN_HEIGHT: f32 = 0.08 * PIXELS_PER_CM;

@group(0) @binding(0) var<uniform> window: WindowUniform;
@group(0) @binding(1) var<storage, read_write> volume: array<i32>;
@group(0) @binding(2) var<storage, read_write> store: array<i32>;
@group(0) @binding(3) var<storage, read> heights: array<f32>;
@group(0) @binding(4) var<storage, read_write> columns: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> column_pigment: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> relief: array<f32>;
@group(0) @binding(7) var<uniform> wet: WetBounds;

struct WetBounds {
    origin: vec2<i32>,
    size: vec2<u32>,
};
const MAX_SHARE_PER_NEIGHBOUR: f32 = 0.25;

fn on_canvas(pixel: vec2<i32>) -> bool {
    return pixel.x >= 0 && pixel.y >= 0 && pixel.x < i32(window.canvas_width) && pixel.y < i32(window.canvas_height);
}

fn pixel_index(pixel: vec2<i32>) -> u32 {
    return u32(pixel.y) * window.canvas_width + u32(pixel.x);
}

fn height_of(pixel: vec2<i32>) -> f32 {
    if (!on_canvas(pixel)) { return -1.0; }
    return heights[pixel_index(pixel)];
}

fn relief_of(pixel: vec2<i32>) -> f32 {
    return relief[pixel_index(pixel)];
}

fn store_is_dry(pixel: vec2<i32>) -> bool {
    return store[store_cell(vec2<u32>(pixel)) + 7u] >= i32(PAINT_SCALE);
}

fn flow_between(surface: f32, other_surface: f32, wet: f32) -> f32 {
    let flowing = max(wet - LEVEL_MIN_HEIGHT, 0.0);
    let difference = surface - other_surface;
    let beyond_yield = sign(difference) * max(abs(difference) - LEVEL_YIELD_SLOPE, 0.0);
    return beyond_yield * LEVEL_RATE * 0.25 * min(flowing / LEVEL_MIN_HEIGHT, 1.0);
}

fn pair_flow(paint: f32, surface: f32, other_paint: f32, other_surface: f32) -> f32 {
    let both_wet = paint > 0.0 && other_paint > 0.0;
    if (!both_wet && max(paint, other_paint) <= SPREAD_MIN_HEIGHT) { return 0.0; }
    let flow = flow_between(surface, other_surface, max(paint, other_paint));
    return clamp(flow, -other_paint * MAX_SHARE_PER_NEIGHBOUR, paint * MAX_SHARE_PER_NEIGHBOUR);
}

fn column_slot(local: vec2<u32>) -> u32 {
    return local.y * window.width + local.x;
}

fn neighbour_dry(local: vec2<i32>) -> bool {
    if (local.x >= 0 && local.y >= 0 && local.x < i32(window.width) && local.y < i32(window.height)) {
        return columns[column_slot(vec2<u32>(local))].y > 0.0;
    }
    return store_is_dry(local + window.origin);
}

fn neighbour_pigment(local: vec2<i32>) -> array<f32, 6> {
    var pigment: array<f32, 6>;
    if (local.x >= 0 && local.y >= 0 && local.x < i32(window.width) && local.y < i32(window.height)) {
        let slot = column_slot(vec2<u32>(local));
        let p = column_pigment[slot];
        let c = columns[slot];
        pigment = array<f32, 6>(p.x, p.y, p.z, c.z, p.w, c.w);
        return pigment;
    }
    let cell = store_cell(vec2<u32>(local + window.origin));
    let height = max(f32(store[cell]) / PAINT_SCALE, 1e-6);
    for (var channel = 0u; channel < 6u; channel++) {
        pigment[channel] = f32(store[cell + 1u + channel]) / PAINT_SCALE / height;
    }
    return pigment;
}

fn column_flow(paint: f32, surface: f32, local: vec2<i32>) -> f32 {
    let pixel = local + window.origin;
    let other = height_of(pixel);
    if (other < 0.0 || neighbour_dry(local)) { return 0.0; }
    return pair_flow(paint, surface, other, other + relief_of(pixel));
}

fn take_from_top(local: vec2<u32>, amount: f32) {
    var remaining = amount;
    for (var step = 0u; step < window.depth; step++) {
        let base = window_cell(vec3<u32>(local, window.depth - 1u - step));
        let density = f32(volume[base]) / PAINT_SCALE;
        if (density <= 0.0) { continue; }
        let taken = min(density, remaining);
        let fraction = taken / density;
        for (var channel = 0u; channel < 7u; channel++) {
            volume[base + channel] = i32(round(f32(volume[base + channel]) * (1.0 - fraction)));
        }
        remaining -= taken;
        if (remaining <= 1e-6) { return; }
    }
}

fn add_on_top(local: vec2<u32>, amount: f32, absorption: vec3<f32>, scattering: vec3<f32>) {
    for (var z = 0u; z < window.depth; z++) {
        let base = window_cell(vec3<u32>(local, z));
        let density = f32(volume[base]) / PAINT_SCALE;
        if (density >= FULL_CELL && z + 1u < window.depth) { continue; }
        volume[base] += i32(round(amount * PAINT_SCALE));
        volume[base + 1u] += i32(round(amount * absorption.x * PAINT_SCALE));
        volume[base + 2u] += i32(round(amount * absorption.y * PAINT_SCALE));
        volume[base + 3u] += i32(round(amount * absorption.z * PAINT_SCALE));
        volume[base + 4u] += i32(round(amount * scattering.x * PAINT_SCALE));
        volume[base + 5u] += i32(round(amount * scattering.y * PAINT_SCALE));
        volume[base + 6u] += i32(round(amount * scattering.z * PAINT_SCALE));
        return;
    }
}

@compute @workgroup_size(16, 16)
fn snapshot_columns(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    var height = 0.0;
    var top_absorption = vec3<f32>(0.0);
    var top_scattering = vec3<f32>(0.0);
    var dry = false;
    for (var z = 0u; z < window.depth; z++) {
        let base = window_cell(vec3<u32>(id.xy, z));
        let density = f32(volume[base]) / PAINT_SCALE;
        if (density <= 0.0) { continue; }
        height += density;
        top_absorption = vec3<f32>(f32(volume[base + 1u]), f32(volume[base + 2u]), f32(volume[base + 3u])) / PAINT_SCALE / density;
        top_scattering = vec3<f32>(f32(volume[base + 4u]), f32(volume[base + 5u]), f32(volume[base + 6u])) / PAINT_SCALE / density;
        dry = dry || volume[base + 7u] >= i32(PAINT_SCALE);
    }
    let slot = column_slot(id.xy);
    columns[slot] = vec4<f32>(height, select(0.0, 1.0, dry), top_scattering.x, top_scattering.z);
    column_pigment[slot] = vec4<f32>(top_absorption, top_scattering.y);
}

@compute @workgroup_size(16, 16)
fn level_window(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    let local = vec2<i32>(id.xy);
    let own = columns[column_slot(id.xy)];
    let pixel = local + window.origin;
    let paint = height_of(pixel);
    if (own.y > 0.0 || paint < 0.0) { return; }
    let surface = paint + relief_of(pixel);
    let offsets = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    var outgoing = 0.0;
    var incoming = 0.0;
    var incoming_absorption = vec3<f32>(0.0);
    var incoming_scattering = vec3<f32>(0.0);
    for (var n = 0u; n < 4u; n++) {
        let flow = column_flow(paint, surface, local + offsets[n]);
        if (flow > 0.0) {
            outgoing += flow;
        } else if (flow < 0.0) {
            let pigment = neighbour_pigment(local + offsets[n]);
            incoming -= flow;
            incoming_absorption += vec3<f32>(pigment[0], pigment[1], pigment[2]) * -flow;
            incoming_scattering += vec3<f32>(pigment[3], pigment[4], pigment[5]) * -flow;
        }
    }
    if (outgoing > 0.0) { take_from_top(id.xy, outgoing); }
    if (incoming > 0.0) { add_on_top(id.xy, incoming, incoming_absorption / incoming, incoming_scattering / incoming); }
}

fn store_flow(paint: f32, surface: f32, neighbour: vec2<i32>) -> f32 {
    if (!on_canvas(neighbour) || store_is_dry(neighbour)) { return 0.0; }
    let other = heights[pixel_index(neighbour)];
    return pair_flow(paint, surface, other, other + relief_of(neighbour));
}

fn level_store_pixel(pixel: vec2<i32>) {
    if (!on_canvas(pixel) || window_contains(pixel)) { return; }
    let height = heights[pixel_index(pixel)];
    if (store_is_dry(pixel)) { return; }
    let surface = height + relief_of(pixel);
    let cell = store_cell(vec2<u32>(pixel));
    let offsets = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    var outgoing = 0.0;
    var incoming = 0.0;
    var incoming_pigment: array<f32, 6>;
    for (var channel = 0u; channel < 6u; channel++) { incoming_pigment[channel] = 0.0; }
    for (var n = 0u; n < 4u; n++) {
        let neighbour = pixel + offsets[n];
        let flow = store_flow(height, surface, neighbour);
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

fn wet_pixel(id: vec2<u32>, parity: u32) -> vec2<i32> {
    let local = vec2<i32>(i32(id.x) * 2 + i32((id.y + parity) & 1u), i32(id.y));
    if (local.x >= i32(wet.size.x) || local.y >= i32(wet.size.y)) { return vec2<i32>(-1); }
    return wet.origin + local;
}

@compute @workgroup_size(16, 16)
fn level_store_even(@builtin(global_invocation_id) id: vec3<u32>) {
    level_store_pixel(wet_pixel(id.xy, 0u));
}

@compute @workgroup_size(16, 16)
fn level_store_odd(@builtin(global_invocation_id) id: vec3<u32>) {
    level_store_pixel(wet_pixel(id.xy, 1u));
}

@compute @workgroup_size(16, 16)
fn dry_store(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = vec2<i32>(id.xy);
    if (!on_canvas(pixel)) { return; }
    let cell = store_cell(vec2<u32>(pixel));
    if (store[cell] <= 0) { return; }
    store[cell + 7u] = i32(PAINT_SCALE);
}

@compute @workgroup_size(64)
fn dry_volume(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width * window.height * window.depth) { return; }
    let base = id.x * CHANNELS;
    if (volume[base] <= 0) { return; }
    volume[base + 7u] = i32(PAINT_SCALE);
}
