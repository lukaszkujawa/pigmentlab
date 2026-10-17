const LEVEL_RATE: f32 = 0.3;
const LEVEL_RATE_OILY: f32 = 1.0;
const LEVEL_YIELD_SLOPE: f32 = 0.4;
const LEVEL_YIELD_SLOPE_OILY: f32 = 0.05;
const LEVEL_FULL_MOBILITY_HEIGHT: f32 = 0.03 * PIXELS_PER_CM;
const LEVEL_FULL_MOBILITY_HEIGHT_OILY: f32 = 0.005 * PIXELS_PER_CM;
const SPREAD_MIN_HEIGHT: f32 = 0.08 * PIXELS_PER_CM;
const SPREAD_MIN_HEIGHT_OILY: f32 = 0.01 * PIXELS_PER_CM;
const SURFACE_TENSION_OILY: f32 = 0.1;
const COHESIVE_HEIGHT: f32 = 1.5;

@group(0) @binding(0) var<uniform> window: WindowUniform;
@group(0) @binding(1) var<storage, read_write> volume: array<i32>;
@group(0) @binding(2) var<storage, read_write> store: array<i32>;
@group(0) @binding(3) var<storage, read> heights: array<f32>;
@group(0) @binding(4) var<storage, read_write> columns: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> column_pigment: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> ground: array<f32>;
@group(0) @binding(7) var<uniform> wet: WetBounds;

struct WetBounds {
    origin: vec2<i32>,
    size: vec2<u32>,
};
const MAX_SHARE_PER_NEIGHBOUR: f32 = 0.25;

fn pixel_index(pixel: vec2<i32>) -> u32 {
    return u32(pixel.y) * window.canvas_width + u32(pixel.x);
}

fn height_of(pixel: vec2<i32>) -> f32 {
    if (!on_canvas(pixel)) { return -1.0; }
    return heights[pixel_index(pixel)];
}

fn ground_of(pixel: vec2<i32>) -> f32 {
    return ground[pixel_index(pixel)];
}

fn geometric_mix(stiff: f32, oily: f32, medium: f32) -> f32 {
    return exp(mix(log(stiff), log(oily), medium));
}

fn surface_of(pixel: vec2<i32>, fallback: f32) -> f32 {
    if (!on_canvas(pixel) || heights[pixel_index(pixel)] <= 0.0) { return fallback; }
    return heights[pixel_index(pixel)] + ground_of(pixel);
}

fn steepness(pixel: vec2<i32>) -> f32 {
    let centre = surface_of(pixel, 0.0);
    let dx = surface_of(pixel + vec2<i32>(1, 0), centre) - surface_of(pixel - vec2<i32>(1, 0), centre);
    let dy = surface_of(pixel + vec2<i32>(0, 1), centre) - surface_of(pixel - vec2<i32>(0, 1), centre);
    return 0.5 * length(vec2<f32>(dx, dy));
}

fn flow_between(surface: f32, other_surface: f32, slope: f32, wet: f32, medium: f32, tension: f32) -> f32 {
    let mobility = min(wet / geometric_mix(LEVEL_FULL_MOBILITY_HEIGHT, LEVEL_FULL_MOBILITY_HEIGHT_OILY, medium), 1.0);
    let difference = (surface - other_surface) * mobility;
    let stress = max(slope, abs(surface - other_surface)) * mobility;
    let yielding = max(1.0 - geometric_mix(LEVEL_YIELD_SLOPE, LEVEL_YIELD_SLOPE_OILY, medium) / max(stress, 1e-6), 0.0);
    return difference * min(yielding * mix(LEVEL_RATE, LEVEL_RATE_OILY, medium) + tension, LEVEL_RATE_OILY) * 0.25;
}

fn shared_medium(paint: f32, medium: f32, other_paint: f32, other_medium: f32) -> f32 {
    return (paint * medium + other_paint * other_medium) / max(paint + other_paint, 1e-6);
}

fn pair_flow(paint: f32, surface: f32, other_paint: f32, other_surface: f32, slope: f32, medium: f32) -> f32 {
    let oiliness = clamp(medium, 0.0, 1.0);
    let both_wet = paint > 0.0 && other_paint > 0.0;
    if (!both_wet && max(paint, other_paint) <= geometric_mix(SPREAD_MIN_HEIGHT, SPREAD_MIN_HEIGHT_OILY, oiliness)) { return 0.0; }
    let cohesion = clamp(min(paint, other_paint) / COHESIVE_HEIGHT, 0.0, 1.0);
    let flow = flow_between(surface, other_surface, slope * cohesion, max(paint, other_paint), oiliness, SURFACE_TENSION_OILY * oiliness * cohesion);
    return clamp(flow, -other_paint * MAX_SHARE_PER_NEIGHBOUR, paint * MAX_SHARE_PER_NEIGHBOUR);
}

fn column_slot(local: vec2<u32>) -> u32 {
    return local.y * window.width + local.x;
}

fn neighbour_pigment(local: vec2<i32>) -> array<f32, 7> {
    var pigment: array<f32, 7>;
    if (local.x >= 0 && local.y >= 0 && local.x < i32(window.width) && local.y < i32(window.height)) {
        let slot = column_slot(vec2<u32>(local));
        let p = column_pigment[slot];
        let c = columns[slot];
        pigment = array<f32, 7>(p.x, p.y, p.z, c.y, c.z, c.w, p.w);
        return pigment;
    }
    let cell = store_cell(vec2<u32>(local + window.origin));
    let height = max(f32(store[cell]) / PAINT_SCALE, 1e-6);
    for (var channel = 0u; channel < 7u; channel++) {
        pigment[channel] = f32(store[cell + 1u + channel]) / PAINT_SCALE / height;
    }
    return pigment;
}

fn column_flow(own: vec2<i32>, paint: f32, surface: f32, medium: f32, local: vec2<i32>) -> f32 {
    let pixel = local + window.origin;
    let other = height_of(pixel);
    if (other < 0.0) { return 0.0; }
    let other_medium = neighbour_pigment(local)[6];
    let slope = 0.5 * (steepness(own) + steepness(pixel));
    return pair_flow(paint, surface, other, other + ground_of(pixel), slope, shared_medium(paint, medium, other, other_medium));
}

fn take_from_top(local: vec2<u32>, amount: f32) {
    var remaining = amount;
    for (var step = 0u; step < window.depth; step++) {
        let base = window_cell(vec3<u32>(local, window.depth - 1u - step));
        let density = f32(volume[base]) / PAINT_SCALE;
        if (density <= 0.0) { continue; }
        let taken = min(density, remaining);
        let fraction = taken / density;
        for (var channel = 0u; channel < CHANNELS; channel++) {
            volume[base + channel] = i32(round(f32(volume[base + channel]) * (1.0 - fraction)));
        }
        remaining -= taken;
        if (remaining <= 1e-6) { return; }
    }
}

fn add_on_top(local: vec2<u32>, amount: f32, absorption: vec3<f32>, scattering: vec3<f32>, medium: f32) {
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
        volume[base + 7u] += i32(round(amount * medium * PAINT_SCALE));
        return;
    }
}

@compute @workgroup_size(16, 16)
fn snapshot_columns(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    var height = 0.0;
    var medium_mass = 0.0;
    var top_absorption = vec3<f32>(0.0);
    var top_scattering = vec3<f32>(0.0);
    for (var z = 0u; z < window.depth; z++) {
        let base = window_cell(vec3<u32>(id.xy, z));
        let density = f32(volume[base]) / PAINT_SCALE;
        if (density <= 0.0) { continue; }
        height += density;
        medium_mass += f32(volume[base + 7u]) / PAINT_SCALE;
        top_absorption = vec3<f32>(f32(volume[base + 1u]), f32(volume[base + 2u]), f32(volume[base + 3u])) / PAINT_SCALE / density;
        top_scattering = vec3<f32>(f32(volume[base + 4u]), f32(volume[base + 5u]), f32(volume[base + 6u])) / PAINT_SCALE / density;
    }
    let slot = column_slot(id.xy);
    columns[slot] = vec4<f32>(height, top_scattering);
    column_pigment[slot] = vec4<f32>(top_absorption, medium_mass / max(height, 1e-6));
}

@compute @workgroup_size(16, 16)
fn level_window(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    let local = vec2<i32>(id.xy);
    let pixel = local + window.origin;
    let paint = height_of(pixel);
    if (paint < 0.0) { return; }
    let surface = paint + ground_of(pixel);
    let medium = column_pigment[column_slot(id.xy)].w;
    let offsets = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    var outgoing = 0.0;
    var incoming = 0.0;
    var incoming_absorption = vec3<f32>(0.0);
    var incoming_scattering = vec3<f32>(0.0);
    var incoming_medium = 0.0;
    for (var n = 0u; n < 4u; n++) {
        let flow = column_flow(pixel, paint, surface, medium, local + offsets[n]);
        if (flow > 0.0) {
            outgoing += flow;
        } else if (flow < 0.0) {
            let pigment = neighbour_pigment(local + offsets[n]);
            incoming -= flow;
            incoming_absorption += vec3<f32>(pigment[0], pigment[1], pigment[2]) * -flow;
            incoming_scattering += vec3<f32>(pigment[3], pigment[4], pigment[5]) * -flow;
            incoming_medium += pigment[6] * -flow;
        }
    }
    if (outgoing > 0.0) { take_from_top(id.xy, outgoing); }
    if (incoming > 0.0) { add_on_top(id.xy, incoming, incoming_absorption / incoming, incoming_scattering / incoming, incoming_medium / incoming); }
}

fn store_medium(pixel: vec2<i32>) -> f32 {
    let cell = store_cell(vec2<u32>(pixel));
    return f32(store[cell + 7u]) / max(f32(store[cell]), 1.0);
}

fn store_flow(own: vec2<i32>, paint: f32, surface: f32, medium: f32, neighbour: vec2<i32>) -> f32 {
    if (!on_canvas(neighbour)) { return 0.0; }
    let other = heights[pixel_index(neighbour)];
    let slope = 0.5 * (steepness(own) + steepness(neighbour));
    return pair_flow(paint, surface, other, other + ground_of(neighbour), slope, shared_medium(paint, medium, other, store_medium(neighbour)));
}

fn level_store_pixel(pixel: vec2<i32>) {
    if (!on_canvas(pixel) || window_contains(pixel)) { return; }
    let height = heights[pixel_index(pixel)];
    let surface = height + ground_of(pixel);
    let cell = store_cell(vec2<u32>(pixel));
    let medium = store_medium(pixel);
    let offsets = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    var outgoing = 0.0;
    var incoming = 0.0;
    var incoming_pigment: array<f32, 7>;
    for (var channel = 0u; channel < 7u; channel++) { incoming_pigment[channel] = 0.0; }
    for (var n = 0u; n < 4u; n++) {
        let neighbour = pixel + offsets[n];
        let flow = store_flow(pixel, height, surface, medium, neighbour);
        if (flow > 0.0) {
            outgoing += flow;
        } else if (flow < 0.0) {
            let pigment = neighbour_pigment(neighbour - window.origin);
            incoming -= flow;
            for (var channel = 0u; channel < 7u; channel++) {
                incoming_pigment[channel] += pigment[channel] * -flow;
            }
        }
    }
    let keep = 1.0 - min(outgoing / max(height, 1e-6), 1.0);
    store[cell] = i32(round((height * keep + incoming) * PAINT_SCALE));
    for (var channel = 0u; channel < 7u; channel++) {
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
