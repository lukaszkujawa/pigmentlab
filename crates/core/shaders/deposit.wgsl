const MAX_SEGMENT_STEPS: u32 = 16u;
const REFERENCE_SPEED: f32 = 3.0 * PIXELS_PER_CM;
const FILM_MIN_RADIUS: f32 = 0.015 * PIXELS_PER_CM;
const DRY_HAIR_RADIUS: f32 = 0.3;
const LOADED_HAIR_RADIUS: f32 = 0.9;
const PIXEL_SPREAD_SQUARED: f32 = 0.5;
const BLADE_SPLAT_RADIUS: f32 = 2.5;
override BLADE: bool = false;
override STEEL: bool = false;
const SQUEEZE_OUTWARD: f32 = 3.0;
const SQUEEZED_LAYER: f32 = 4.0;
const WEAVE_PEAK_REACH: i32 = 3;
const VALLEY_SOFTNESS: f32 = 0.5;
const WICKING_PER_SECOND: f32 = 6.0;
const RETAINED_FILL: f32 = 0.05;
const JUMP_DISTANCE: f32 = 0.4 * PIXELS_PER_CM;
const GROOVE_RADIUS: i32 = 0;
const BLADE_GROOVE_RADIUS: i32 = 1;
const PUSH_DISTANCE: f32 = 1.5;
const PUSH_PACE: f32 = 1.25;
const MAX_PUSH_REACH: f32 = 16.0;
const MAX_PUSH_TARGETS: u32 = 6u;
const FORWARD_SHARE: f32 = 0.8;
const PUSH_RADIUS: f32 = 2.0;
const LIMIT_ENCODING: i32 = 0x7fffffff;
const MIN_DISPLACING_SPEED: f32 = 48.0;
const DRAG_RADIUS: i32 = 2;
const DRAG_RATE_PER_SECOND: f32 = 24.0;
const WIPE_RATE_PER_SECOND: f32 = 60.0;
const STIFF_PAINT_ABSORBENCY: f32 = 0.4;
const SOAK_RATE_PER_SECOND: f32 = 30.0;
const SOAK_IMMERSION: f32 = 5.0;
const PICKUP_FILM: f32 = 0.15;
const BRUSH_CONTENTS_SCALE: f32 = 16.0;
const SATURATED_BLEED: f32 = 0.5;
const STIR_RATE_PER_SECOND: f32 = 30.0;
const DISPLACEMENT_SLOTS: u32 = 5u;
const COARSE_CELL: u32 = 16u;
const BRUSH_CONTENTS_CHANNELS: u32 = 8u;

struct DepositUniform {
    sample_count: u32,
    samples_per_bristle: u32,
    deposit_rate: f32,
    pickup_rate: f32,
    sample_capacity: f32,
    contact_height: f32,
    dt: f32,
    brush_travel: f32,
    rigidity: f32,
    wet_floor_keep: f32,
    contact: f32,
    lifting: u32,
    film_height: f32,
    region_x: u32,
    region_y: u32,
    blade_flattening: f32,
};

@group(0) @binding(0) var<uniform> deposit: DepositUniform;
@group(0) @binding(1) var<storage, read> chain: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> previous_chain: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> sample_load: array<f32>;
@group(0) @binding(3) var<storage, read_write> sample_absorption: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> sample_scattering: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> paint: array<atomic<i32>>;
@group(0) @binding(7) var<uniform> window: WindowUniform;
@group(0) @binding(8) var<storage, read> stroke_base: array<f32>;
@group(0) @binding(9) var<storage, read_write> film: array<atomic<i32>>;
@group(0) @binding(10) var<storage, read_write> pickup_demand: array<atomic<i32>>;
@group(0) @binding(11) var<storage, read_write> pickup_grant: array<PickupGrant>;
@group(0) @binding(12) var<storage, read_write> stir_depth: array<atomic<i32>>;
@group(0) @binding(13) var<storage, read_write> sample_request: array<vec2<f32>>;
@group(0) @binding(14) var<storage, read_write> displacement: array<atomic<i32>>;
@group(0) @binding(15) var<storage, read_write> column_state: array<vec4<f32>>;
@group(0) @binding(16) var<storage, read_write> coarse_marks: array<atomic<u32>>;
@group(0) @binding(17) var<storage, read> ground: array<f32>;

struct PickupGrant {
    absorption: vec3<f32>,
    fraction: f32,
    scattering: vec4<f32>,
};

struct Segment {
    start: vec3<f32>,
    end: vec3<f32>,
    steps: u32,
    motion: f32,
    film: f32,
    fill: f32,
}

fn segment_of(sample: u32, load: f32) -> Segment {
    let bristle = sample / deposit.samples_per_bristle;
    let t = f32(sample % deposit.samples_per_bristle) / f32(deposit.samples_per_bristle - 1u);
    let end = curve_point(false, bristle, t);
    var start = curve_point(true, bristle, t);
    if (distance(start.xy, end.xy) > JUMP_DISTANCE) { start = end; }
    let travelled = distance(start.xy, end.xy);
    let fill = clamp(load / deposit.sample_capacity, 0.0, 1.0) * select(1.0, 0.0, deposit.rigidity >= 0.5);
    return Segment(start, end, clamp(u32(ceil(travelled)), 1u, MAX_SEGMENT_STEPS), min(travelled / (REFERENCE_SPEED * deposit.dt), 1.0), film_reach(load), fill);
}

fn film_reach(load: f32) -> f32 {
    if (deposit.rigidity >= 0.5) { return deposit.film_height; }
    return mix(FILM_MIN_RADIUS, deposit.film_height, clamp(load / deposit.sample_capacity, 0.0, 1.0));
}

fn exchange_portion(segment: Segment) -> f32 {
    return (1.0 - segment.fill) + segment.fill * SATURATED_BLEED;
}

fn returned_portion(segment: Segment) -> f32 {
    return segment.fill * SATURATED_BLEED / max(exchange_portion(segment), 1e-6);
}

fn buried_depth(point: vec3<f32>, pixel: vec2<i32>, state: vec4<f32>) -> f32 {
    let paint_below_blade = select(min(state.x, base_height(pixel)), state.x, BLADE);
    return clamp(paint_below_blade - (point.z - state.w), 0.0, deposit.contact_height);
}

fn pickup_wanted(segment: Segment, point: vec3<f32>, pixel: vec2<i32>, state: vec4<f32>, remaining: ptr<function, f32>) -> f32 {
    let height = state.x + state.w;
    if (state.x <= 0.0 || point.z > height + segment.film) { return 0.0; }
    let brush_motion = min(deposit.brush_travel / (REFERENCE_SPEED * deposit.dt), 1.0);
    let buried = buried_depth(point, pixel, state);
    let share = (deposit.pickup_rate + buried * WIPE_RATE_PER_SECOND * deposit.dt) / f32(segment.steps) * min(segment.motion, brush_motion);
    let wanted = min(share * exchange_portion(segment), *remaining);
    *remaining -= wanted * (1.0 - returned_portion(segment));
    return wanted;
}

fn column_index(local: vec2<u32>) -> u32 {
    return local.y * window.width + local.x;
}

fn film_radius(fill: f32) -> f32 {
    if (BLADE) { return BLADE_SPLAT_RADIUS; }
    let hair = mix(DRY_HAIR_RADIUS, LOADED_HAIR_RADIUS, smoothstep(0.2, 1.0, fill));
    return sqrt(hair * hair + PIXEL_SPREAD_SQUARED);
}

fn film_kernel(distance_to_centre: f32, radius: f32) -> f32 {
    let normalised = clamp(distance_to_centre / radius, 0.0, 1.0);
    let falloff = 1.0 - normalised * normalised;
    return falloff * falloff;
}

fn surface_reach(pixel: vec2<i32>, reach_floor: f32) -> f32 {
    if (!window_contains(pixel)) { return 0.0; }
    let state = column_state[column_index(vec2<u32>(pixel - window.origin))];
    return smoothstep(reach_floor - VALLEY_SOFTNESS, reach_floor, state.x + state.w);
}

fn splat_film(centre: vec2<f32>, radius: f32, amount: f32, absorption: vec3<f32>, scattering: vec3<f32>, medium: f32, reach_floor: f32) {
    let reach = i32(ceil(radius));
    let base_pixel = vec2<i32>(floor(centre));
    let fraction = centre - floor(centre) - 0.5;
    var covered = 0.0;
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            covered += film_kernel(length(vec2<f32>(f32(dx), f32(dy)) - fraction), radius) * surface_reach(base_pixel + vec2<i32>(dx, dy), reach_floor);
        }
    }
    if (covered <= 0.0) { return; }
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            let pixel = base_pixel + vec2<i32>(dx, dy);
            let weight = film_kernel(length(vec2<f32>(f32(dx), f32(dy)) - fraction), radius) * surface_reach(pixel, reach_floor);
            if (weight <= 0.0) { continue; }
            let share = amount * weight / covered;
            let local = vec2<u32>(pixel - window.origin);
            let slot = (local.y * window.width + local.x) * CHANNELS;
            atomicAdd(&film[slot], fixed(share));
            atomicAdd(&film[slot + 1u], fixed(share * absorption.x));
            atomicAdd(&film[slot + 2u], fixed(share * absorption.y));
            atomicAdd(&film[slot + 3u], fixed(share * absorption.z));
            atomicAdd(&film[slot + 4u], fixed(share * scattering.x));
            atomicAdd(&film[slot + 5u], fixed(share * scattering.y));
            atomicAdd(&film[slot + 6u], fixed(share * scattering.z));
            atomicAdd(&film[slot + 7u], fixed(share * medium));
        }
    }
}

fn fixed(value: f32) -> i32 {
    return i32(round(value * PAINT_SCALE));
}

fn add_vec3(cell: u32, offset: u32, value: vec3<f32>) {
    atomicAdd(&paint[cell + offset], fixed(value.x));
    atomicAdd(&paint[cell + offset + 1u], fixed(value.y));
    atomicAdd(&paint[cell + offset + 2u], fixed(value.z));
}

fn store_vec3(cell: u32, offset: u32, value: vec3<f32>) {
    atomicStore(&paint[cell + offset], fixed(value.x));
    atomicStore(&paint[cell + offset + 1u], fixed(value.y));
    atomicStore(&paint[cell + offset + 2u], fixed(value.z));
}

fn load_vec3(cell: u32, offset: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(atomicLoad(&paint[cell + offset])),
        f32(atomicLoad(&paint[cell + offset + 1u])),
        f32(atomicLoad(&paint[cell + offset + 2u])),
    ) / PAINT_SCALE;
}

fn column_height(local: vec2<u32>) -> f32 {
    var total = 0;
    for (var z = 0u; z < window.depth; z++) {
        total += atomicLoad(&paint[window_cell(vec3<u32>(local, z))]);
    }
    return f32(total) / PAINT_SCALE;
}

fn column_top(local: vec2<u32>) -> u32 {
    return min(u32(floor(column_height(local))), window.depth - 1u);
}

fn give(cell: u32, deposit_share: f32, load: ptr<function, f32>, absorption: vec3<f32>, scattering: vec3<f32>, medium: f32) {
    if (*load <= 0.0) { return; }
    let given = min(deposit_share, *load);
    atomicAdd(&paint[cell], fixed(given));
    add_vec3(cell, 1u, absorption * given);
    add_vec3(cell, 4u, scattering * given);
    atomicAdd(&paint[cell + 7u], fixed(medium * given));
    *load -= given;
}

fn snapshot_height(local: vec2<u32>) -> f32 {
    return column_state[column_index(local)].x;
}

@compute @workgroup_size(16, 16)
fn snapshot_columns(@builtin(global_invocation_id) id: vec3<u32>) {
    let local = id.xy + vec2<u32>(deposit.region_x, deposit.region_y);
    if (local.x >= window.width || local.y >= window.height) { return; }
    var height = 0;
    var medium = 0;
    var top = -1.0;
    for (var z = 0u; z < window.depth; z++) {
        let cell = window_cell(vec3<u32>(local, z));
        let density = atomicLoad(&paint[cell]);
        if (density <= 0) { continue; }
        height += density;
        medium += atomicLoad(&paint[cell + 7u]);
        top = f32(z);
    }
    let column = column_index(local);
    let ground_here = ground_at(vec2<i32>(local) + window.origin);
    let medium_fraction = select(0.0, clamp(f32(medium) / f32(height), 0.0, 1.0), height > 0);
    column_state[column] = vec4<f32>(f32(height) / PAINT_SCALE, top, medium_fraction, ground_here);
    atomicMax(&pickup_demand[max_surface_slot()], fixed(f32(height) / PAINT_SCALE + ground_here));
    atomicStore(&pickup_demand[column], 0);
    atomicStore(&pickup_demand[hair_floor_slot(column)], 0);
    atomicStore(&pickup_demand[net_demand_slot(column)], 0);
    atomicStore(&stir_depth[column], 0);
    for (var channel = 0u; channel < CHANNELS; channel++) { atomicStore(&film[column * CHANNELS + channel], 0); }
    for (var slot = 0u; slot < DISPLACEMENT_SLOTS; slot++) { atomicStore(&displacement[column * DISPLACEMENT_SLOTS + slot], 0); }
    atomicStore(&coarse_marks[coarse_index(local)], 0u);
    if (id.x < 8u && id.y == 0u) { atomicStore(&pickup_demand[brush_contents_slot() + id.x], 0); }
}

fn max_surface_slot() -> u32 {
    return window.width * window.height;
}

fn hair_floor_slot(column: u32) -> u32 {
    return window.width * window.height + 1u + column;
}

fn topmost_filled_cell(local: vec2<u32>) -> i32 {
    for (var step = 0u; step < window.depth; step++) {
        let z = window.depth - 1u - step;
        if (atomicLoad(&paint[window_cell(vec3<u32>(local, z))]) > 0) { return i32(z); }
    }
    return -1;
}

fn ground_at(pixel: vec2<i32>) -> f32 {
    return ground[u32(pixel.y) * window.canvas_width + u32(pixel.x)];
}

fn base_height(pixel: vec2<i32>) -> f32 {
    return stroke_base[u32(pixel.y) * window.canvas_width + u32(pixel.x)];
}

fn squeeze_outward(sample: u32, point: vec2<f32>, heading: vec2<f32>) -> vec2<f32> {
    let base = sample / deposit.samples_per_bristle * BRISTLE_VERTICES;
    let blade = chain[base];
    let along_blade = normalize(chain[base + BRISTLE_VERTICES - 1u].xy - chain[base + 1u].xy + vec2<f32>(1e-6, 0.0));
    let across_blade = vec2<f32>(-along_blade.y, along_blade.x);
    let side = vec2<f32>(-heading.y, heading.x);
    let reach = blade.z * abs(dot(along_blade, side)) + blade.w * abs(dot(across_blade, side));
    return side * clamp(dot(point - blade.xy, side) / max(reach, 1.0), -1.0, 1.0);
}

fn above_floor(point: vec3<f32>, pixel: vec2<i32>) -> vec3<f32> {
    let support = select(ground_at(pixel), weave_peak(pixel), STEEL);
    return vec3<f32>(point.xy, max(point.z, support + base_height(pixel) * deposit.wet_floor_keep));
}

fn weave_peak(pixel: vec2<i32>) -> f32 {
    var peak = ground_at(pixel);
    for (var dy = -WEAVE_PEAK_REACH; dy <= WEAVE_PEAK_REACH; dy += WEAVE_PEAK_REACH) {
        for (var dx = -WEAVE_PEAK_REACH; dx <= WEAVE_PEAK_REACH; dx += WEAVE_PEAK_REACH) {
            let neighbour = pixel + vec2<i32>(dx, dy);
            if (on_canvas(neighbour)) { peak = max(peak, ground_at(neighbour)); }
        }
    }
    return peak;
}

fn base_surface(pixel: vec2<i32>) -> f32 {
    return base_height(pixel) + ground_at(pixel);
}

fn groove_amount(height: f32, hair_level: f32, limit: f32, base: f32, drag: f32) -> f32 {
    let above_film = max(height - limit, 0.0);
    let dragged = max(min(min(height, limit), base) - hair_level, 0.0);
    return above_film + dragged * drag;
}

fn ridge_amount(height: f32, limit: f32, drag: f32) -> f32 {
    return max(height - limit, 0.0) * drag;
}

fn coarse_index(local: vec2<u32>) -> u32 {
    return (local.y / COARSE_CELL) * ((window.width + COARSE_CELL - 1u) / COARSE_CELL) + local.x / COARSE_CELL;
}

fn near_marks(local: vec2<u32>) -> bool {
    let columns = i32((window.width + COARSE_CELL - 1u) / COARSE_CELL);
    let rows = i32((window.height + COARSE_CELL - 1u) / COARSE_CELL);
    let block = vec2<i32>(local / COARSE_CELL);
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let b = block + vec2<i32>(dx, dy);
            if (b.x < 0 || b.y < 0 || b.x >= columns || b.y >= rows) { continue; }
            if (atomicLoad(&coarse_marks[u32(b.y * columns + b.x)]) > 0u) { return true; }
        }
    }
    return false;
}

fn mark_displacement(local: vec2<u32>, height: f32, hair_level: f32, film: f32, direction: vec2<f32>, drag: f32) {
    let weight = max(height - hair_level, 0.0);
    let slot = column_index(local) * DISPLACEMENT_SLOTS;
    atomicMax(&displacement[slot], LIMIT_ENCODING - fixed(hair_level + film));
    atomicMax(&displacement[slot + 1u], LIMIT_ENCODING - fixed(hair_level));
    atomicMax(&displacement[slot + 2u], fixed(drag));
    atomicAdd(&displacement[slot + 3u], fixed(direction.x * weight));
    atomicAdd(&displacement[slot + 4u], fixed(direction.y * weight));
    atomicMax(&coarse_marks[coarse_index(local)], 1u);
}

fn releasable(fill: f32) -> f32 {
    return clamp((fill - RETAINED_FILL) / (1.0 - RETAINED_FILL), 0.0, 1.0);
}

fn release(point_xy: vec2<f32>, reach_floor: f32, deposit_share: f32, load: ptr<function, f32>, absorption: vec3<f32>, scattering: vec3<f32>, medium: f32) {
    let fill = clamp(*load / deposit.sample_capacity, 0.0, 1.0);
    let given = min(deposit_share * select(pow(releasable(fill), 1.5), sqrt(fill), BLADE), *load);
    if (given <= 0.0) { return; }
    *load -= given;
    splat_film(point_xy, film_radius(fill), given, absorption, scattering, medium, reach_floor);
}

@compute @workgroup_size(128)
fn step(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x;
    if (sample >= deposit.sample_count) { return; }
    let initial_load = sample_load[sample];
    let segment = segment_of(sample, initial_load);
    let max_surface = f32(atomicLoad(&pickup_demand[max_surface_slot()])) / PAINT_SCALE;
    if (min(segment.start.z, segment.end.z) > max_surface + deposit.contact_height) {
        sample_request[sample] = vec2<f32>(initial_load, 0.0);
        return;
    }
    var load = initial_load;
    let absorption = sample_absorption[sample].xyz;
    let medium = sample_absorption[sample].w;
    let scattering = sample_scattering[sample].xyz;
    var remaining_capacity = deposit.sample_capacity - initial_load;
    var requested = 0.0;
    let travelled = distance(segment.start.xy, segment.end.xy);
    let brush_moving = deposit.brush_travel >= MIN_DISPLACING_SPEED * deposit.dt;
    let share = 1.0 / f32(segment.steps);
    let brush_motion = min(deposit.brush_travel / (REFERENCE_SPEED * deposit.dt), 1.0);
    let heading = select(vec2<f32>(0.0), (segment.end.xy - segment.start.xy) / max(travelled, 1e-6), travelled > 1e-3);
    let direction = select(heading, normalize(heading + SQUEEZE_OUTWARD * deposit.blade_flattening * squeeze_outward(sample, segment.end.xy, heading)), STEEL && travelled > 1e-3);
    for (var i = 1u; i <= segment.steps; i++) {
        let pixel = vec2<i32>(floor(mix(segment.start.xy, segment.end.xy, f32(i) / f32(segment.steps))));
        if (!window_contains(pixel) || !on_canvas(pixel)) { continue; }
        let point = above_floor(mix(segment.start, segment.end, f32(i) / f32(segment.steps)), pixel);
        let local = vec2<u32>(pixel - window.origin);
        let state = column_state[column_index(local)];
        let wanted = pickup_wanted(segment, point, pixel, state, &remaining_capacity);
        if (wanted > 0.0) {
            atomicAdd(&pickup_demand[column_index(local)], fixed(wanted));
            atomicAdd(&pickup_demand[net_demand_slot(column_index(local))], fixed(wanted * (1.0 - returned_portion(segment))));
            atomicMax(&pickup_demand[hair_floor_slot(column_index(local))], LIMIT_ENCODING - fixed(point.z));
            requested += wanted;
        }
        if (state.x > 0.0) {
            let stirring = buried_depth(point, pixel, state) * min(segment.motion, brush_motion);
            if (stirring > 0.0) { atomicMax(&stir_depth[column_index(local)], fixed(stirring)); }
        }
        let film = film_reach(load);
        let height = state.x + state.w;
        if (point.z > height + film) { continue; }
        if (brush_moving && travelled > 1e-3 && height > point.z && !(BLADE && deposit.lifting != 0u)) {
            mark_displacement(local, height, point.z, film, direction, DRAG_RATE_PER_SECOND * deposit.dt * segment.motion);
        }
        if (height > point.z + select(film, max(film, SQUEEZED_LAYER), STEEL)) { continue; }
        let lift_carry = select(1.0, select(brush_motion, 0.0, BLADE), deposit.lifting != 0u);
        let transfer = segment.motion;
        let deposit_share = deposit.deposit_rate * share * transfer * deposit.contact * lift_carry;
        release(point.xy, point.z - film, deposit_share, &load, absorption, scattering, medium);
    }

    sample_load[sample] = load;
    sample_request[sample] = vec2<f32>(initial_load, requested);
}

fn stir_column(local: vec2<u32>, depth: f32) {
    if (depth <= 0.0) { return; }
    var covered = 0.0;
    var absorption = vec3<f32>(0.0);
    var scattering = vec3<f32>(0.0);
    var medium = 0.0;
    var bottom = window.depth;
    for (var step = 0u; step < window.depth && covered < depth; step++) {
        let z = window.depth - 1u - step;
        let cell = window_cell(vec3<u32>(local, z));
        let density = f32(atomicLoad(&paint[cell])) / PAINT_SCALE;
        if (density <= 0.0) { continue; }
        covered += density;
        absorption += load_vec3(cell, 1u);
        scattering += load_vec3(cell, 4u);
        medium += f32(atomicLoad(&paint[cell + 7u])) / PAINT_SCALE;
        bottom = z;
    }
    if (covered <= 0.0) { return; }
    let rate = min(STIR_RATE_PER_SECOND * deposit.dt, 1.0);
    for (var z = bottom; z < window.depth; z++) {
        let cell = window_cell(vec3<u32>(local, z));
        let density = f32(atomicLoad(&paint[cell])) / PAINT_SCALE;
        if (density <= 0.0) { continue; }
        let share = density / covered;
        store_vec3(cell, 1u, mix(load_vec3(cell, 1u), absorption * share, rate));
        store_vec3(cell, 4u, mix(load_vec3(cell, 4u), scattering * share, rate));
        atomicStore(&paint[cell + 7u], fixed(mix(f32(atomicLoad(&paint[cell + 7u])) / PAINT_SCALE, medium * share, rate)));
    }
}

@compute @workgroup_size(16, 16)
fn grant_pickups(@builtin(global_invocation_id) id: vec3<u32>) {
    let local = id.xy + vec2<u32>(deposit.region_x, deposit.region_y);
    if (local.x >= window.width || local.y >= window.height) { return; }
    let column = column_index(local);
    stir_column(local, f32(atomicLoad(&stir_depth[column])) / PAINT_SCALE);
    let demand = f32(atomicLoad(&pickup_demand[column])) / PAINT_SCALE;
    pickup_grant[column] = PickupGrant(vec3<f32>(0.0), 0.0, vec4<f32>(0.0));
    if (demand <= 0.0) { return; }
    let top = topmost_filled_cell(local);
    if (top < 0) { return; }
    let cell = window_cell(vec3<u32>(local, u32(top)));
    let available = f32(atomicLoad(&paint[cell])) / PAINT_SCALE;
    if (available <= 0.0) { return; }
    let state = column_state[column];
    let lowest_hair = f32(LIMIT_ENCODING - atomicLoad(&pickup_demand[hair_floor_slot(column)])) / PAINT_SCALE;
    let above_hairs = max(state.x + state.w - lowest_hair, 0.0) + PICKUP_FILM;
    let immersion = above_hairs / SOAK_IMMERSION;
    let soak = 1.0 - exp(-SOAK_RATE_PER_SECOND * mix(STIFF_PAINT_ABSORBENCY, 1.0, state.z) * immersion * deposit.dt);
    let net_share = f32(atomicLoad(&pickup_demand[net_demand_slot(column)])) / PAINT_SCALE / demand;
    let granted = min(min(demand, available), min(above_hairs, above_hairs * soak / max(net_share, 1e-6)));
    if (granted <= 0.0) { return; }
    let absorption = load_vec3(cell, 1u) / available;
    let scattering = load_vec3(cell, 4u) / available;
    let medium = f32(atomicLoad(&paint[cell + 7u])) / PAINT_SCALE / available;
    atomicSub(&paint[cell], fixed(granted));
    add_vec3(cell, 1u, -absorption * granted);
    add_vec3(cell, 4u, -scattering * granted);
    atomicSub(&paint[cell + 7u], fixed(medium * granted));
    pickup_grant[column] = PickupGrant(absorption, granted / demand, vec4<f32>(scattering, medium));
}

fn take_from_top(local: vec2<u32>, amount: f32, absorption: ptr<function, vec3<f32>>, scattering: ptr<function, vec3<f32>>, medium: ptr<function, f32>) -> f32 {
    var remaining = amount;
    var absorption_mass = vec3<f32>(0.0);
    var scattering_mass = vec3<f32>(0.0);
    var medium_mass = 0.0;
    for (var step = 0u; step < window.depth; step++) {
        let cell = window_cell(vec3<u32>(local, window.depth - 1u - step));
        let density = f32(atomicLoad(&paint[cell])) / PAINT_SCALE;
        if (density <= 0.0) { continue; }
        let taken = min(density, remaining);
        let fraction = taken / density;
        let absorbed = load_vec3(cell, 1u) * fraction;
        let scattered = load_vec3(cell, 4u) * fraction;
        let oiled = f32(atomicLoad(&paint[cell + 7u])) / PAINT_SCALE * fraction;
        atomicSub(&paint[cell], fixed(taken));
        add_vec3(cell, 1u, -absorbed);
        add_vec3(cell, 4u, -scattered);
        atomicSub(&paint[cell + 7u], fixed(oiled));
        absorption_mass += absorbed;
        scattering_mass += scattered;
        medium_mass += oiled;
        remaining -= taken;
        if (remaining <= 1e-6) { break; }
    }
    let taken = amount - remaining;
    *absorption = absorption_mass / max(taken, 1e-6);
    *scattering = scattering_mass / max(taken, 1e-6);
    *medium = medium_mass / max(taken, 1e-6);
    return taken;
}

@compute @workgroup_size(16, 16)
fn grant_displacement(@builtin(global_invocation_id) id: vec3<u32>) {
    let local = id.xy + vec2<u32>(deposit.region_x, deposit.region_y);
    if (local.x >= window.width || local.y >= window.height) { return; }
    if (!near_marks(local)) { return; }
    var limit_code = 0;
    var groove_code = 0;
    var level_code = 0;
    var drag_code = 0;
    var push = vec2<f32>(0.0);
    let groove_radius = select(GROOVE_RADIUS, BLADE_GROOVE_RADIUS, BLADE);
    for (var dy = -DRAG_RADIUS; dy <= DRAG_RADIUS; dy++) {
        for (var dx = -DRAG_RADIUS; dx <= DRAG_RADIUS; dx++) {
            let mark = vec2<i32>(local) + vec2<i32>(dx, dy);
            if (mark.x < 0 || mark.y < 0 || mark.x >= i32(window.width) || mark.y >= i32(window.height)) { continue; }
            let slot = column_index(vec2<u32>(mark)) * DISPLACEMENT_SLOTS;
            let code = atomicLoad(&displacement[slot]);
            if (code == 0) { continue; }
            limit_code = max(limit_code, code);
            if (abs(dx) <= groove_radius && abs(dy) <= groove_radius) { groove_code = max(groove_code, code); }
            level_code = max(level_code, atomicLoad(&displacement[slot + 1u]));
            drag_code = max(drag_code, atomicLoad(&displacement[slot + 2u]));
            push += vec2<f32>(f32(atomicLoad(&displacement[slot + 3u])), f32(atomicLoad(&displacement[slot + 4u])));
        }
    }
    if (limit_code == 0) { return; }
    let pixel = vec2<i32>(local) + window.origin;
    let paint_here = column_height(local);
    let height = paint_here + ground_at(pixel);
    let hair_level = f32(LIMIT_ENCODING - level_code) / PAINT_SCALE;
    let drag = f32(drag_code) / PAINT_SCALE * (1.0 - MEDIUM_THINNING * column_state[column_index(local)].z);
    var excess = ridge_amount(height, f32(LIMIT_ENCODING - limit_code) / PAINT_SCALE, drag);
    if (groove_code != 0) {
        excess = groove_amount(height, hair_level, f32(LIMIT_ENCODING - groove_code) / PAINT_SCALE, base_surface(pixel), drag);
    }
    excess = min(excess, paint_here);
    if (excess <= 1e-4 || length(push) <= 1e-3) { return; }
    let forward = normalize(push);
    let side = vec2<f32>(-forward.y, forward.x);
    let nearest = min(max(deposit.brush_travel + PUSH_RADIUS * deposit.rigidity, PUSH_DISTANCE), MAX_PUSH_REACH);
    let reach = clamp(deposit.brush_travel * PUSH_PACE, nearest, MAX_PUSH_REACH);
    let hops = max(u32(ceil((reach - nearest) / PUSH_RADIUS)) + 1u, 1u);
    var offsets: array<vec2<f32>, MAX_PUSH_TARGETS>;
    var shares: array<f32, MAX_PUSH_TARGETS>;
    offsets[0] = side * PUSH_DISTANCE;
    offsets[1] = -side * PUSH_DISTANCE;
    shares[0] = 0.5 * (1.0 - FORWARD_SHARE);
    shares[1] = shares[0];
    for (var hop = 0u; hop < hops; hop++) {
        let along = mix(nearest, reach, f32(hop) / f32(max(hops - 1u, 1u)));
        offsets[2u + hop] = forward * along;
        shares[2u + hop] = FORWARD_SHARE / f32(hops);
    }
    let targets = 2u + hops;
    let centre = vec2<f32>(pixel) + 0.5;
    var placeable = 0.0;
    for (var t = 0u; t < targets; t++) {
        let landing = vec2<i32>(floor(centre + offsets[t]));
        if (!window_contains(landing) || !on_canvas(landing)) { shares[t] = 0.0; }
        placeable += shares[t];
    }
    if (placeable <= 0.0) { return; }
    var absorption = vec3<f32>(0.0);
    var scattering = vec3<f32>(0.0);
    var medium = 0.0;
    let moved = take_from_top(local, excess * placeable, &absorption, &scattering, &medium);
    for (var t = 0u; t < targets; t++) {
        if (shares[t] <= 0.0) { continue; }
        splat_film(centre + offsets[t], PUSH_RADIUS, moved * shares[t] / placeable, absorption, scattering, medium, -1e9);
    }
}


@compute @workgroup_size(128)
fn apply_pickups(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x;
    if (sample >= deposit.sample_count) { return; }
    let request = sample_request[sample];
    if (request.y <= 0.0) { return; }
    let segment = segment_of(sample, request.x);
    var remaining_capacity = deposit.sample_capacity - request.x;
    let own_absorption = sample_absorption[sample].xyz;
    let own_scattering = sample_scattering[sample].xyz;
    let own_medium = sample_absorption[sample].w;
    var load = sample_load[sample];
    var absorption = own_absorption * load;
    var scattering = own_scattering * load;
    var medium = own_medium * load;
    let returned_share = returned_portion(segment);
    for (var i = 1u; i <= segment.steps; i++) {
        let pixel = vec2<i32>(floor(mix(segment.start.xy, segment.end.xy, f32(i) / f32(segment.steps))));
        if (!window_contains(pixel) || !on_canvas(pixel)) { continue; }
        let point = above_floor(mix(segment.start, segment.end, f32(i) / f32(segment.steps)), pixel);
        let column = column_index(vec2<u32>(pixel - window.origin));
        let wanted = pickup_wanted(segment, point, pixel, column_state[column], &remaining_capacity);
        if (wanted <= 0.0) { continue; }
        let grant = pickup_grant[column];
        let taken = wanted * grant.fraction;
        let returned = taken * returned_share;
        let kept = taken - returned;
        if (returned > 0.0) {
            let top = window_cell(vec3<u32>(vec2<u32>(pixel - window.origin), u32(max(column_state[column].y, 0.0))));
            atomicAdd(&paint[top], fixed(returned));
            add_vec3(top, 1u, own_absorption * returned);
            add_vec3(top, 4u, own_scattering * returned);
            atomicAdd(&paint[top + 7u], fixed(own_medium * returned));
        }
        load += kept;
        absorption += grant.absorption * taken - own_absorption * returned;
        scattering += grant.scattering.xyz * taken - own_scattering * returned;
        medium += grant.scattering.w * taken - own_medium * returned;
    }
    sample_load[sample] = load;
    sample_absorption[sample] = vec4<f32>(absorption, medium) / max(load, 1e-6);
    sample_scattering[sample] = vec4<f32>(scattering / max(load, 1e-6), 0.0);
}

fn brush_contents_slot() -> u32 {
    return 2u * window.width * window.height + 1u;
}

fn net_demand_slot(column: u32) -> u32 {
    return brush_contents_slot() + BRUSH_CONTENTS_CHANNELS + column;
}

fn report_brush_contents(load: f32, absorption: vec3<f32>, scattering: vec3<f32>) {
    let slot = brush_contents_slot();
    let values = array<f32, 8>(load, absorption.x, absorption.y, absorption.z, scattering.x, scattering.y, scattering.z, load);
    for (var channel = 0u; channel < 8u; channel++) {
        atomicAdd(&pickup_demand[slot + channel], i32(round(values[channel] * BRUSH_CONTENTS_SCALE)));
    }
}

@compute @workgroup_size(64)
fn replenish(@builtin(global_invocation_id) id: vec3<u32>) {
    let bristle = id.x;
    if (bristle * deposit.samples_per_bristle >= deposit.sample_count) { return; }
    let base = bristle * deposit.samples_per_bristle;
    var total = 0.0;
    var absorption_mass = vec3<f32>(0.0);
    var scattering_mass = vec3<f32>(0.0);
    for (var i = 0u; i < deposit.samples_per_bristle; i++) {
        let load = sample_load[base + i];
        total += load;
        absorption_mass += sample_absorption[base + i].xyz * load;
        scattering_mass += sample_scattering[base + i].xyz * load;
    }
    report_brush_contents(total, absorption_mass, scattering_mass);
    if (BLADE) { return; }
    let mean = total / f32(deposit.samples_per_bristle);
    let rate = min(WICKING_PER_SECOND * deposit.dt, 1.0);
    var given = 0.0;
    var given_absorption = vec4<f32>(0.0);
    var given_scattering = vec4<f32>(0.0);
    for (var i = 0u; i < deposit.samples_per_bristle; i++) {
        let surplus = (sample_load[base + i] - mean) * rate;
        if (surplus <= 0.0) { continue; }
        given += surplus;
        given_absorption += sample_absorption[base + i] * surplus;
        given_scattering += sample_scattering[base + i] * surplus;
    }
    if (given <= 0.0) { return; }
    given_absorption /= given;
    given_scattering /= given;
    for (var i = 0u; i < deposit.samples_per_bristle; i++) {
        let here = base + i;
        let load = sample_load[here];
        let received = (mean - load) * rate;
        if (received <= 0.0) {
            sample_load[here] = load + received;
            continue;
        }
        let total_here = load + received;
        sample_absorption[here] = (sample_absorption[here] * load + given_absorption * received) / total_here;
        sample_scattering[here] = (sample_scattering[here] * load + given_scattering * received) / total_here;
        sample_load[here] = total_here;
    }
}

@compute @workgroup_size(16, 16)
fn clear_marks(@builtin(global_invocation_id) id: vec3<u32>) {
    let local = id.xy + vec2<u32>(deposit.region_x, deposit.region_y);
    if (local.x >= window.width || local.y >= window.height) { return; }
    let column = column_index(local);
    for (var slot = 0u; slot < DISPLACEMENT_SLOTS; slot++) { atomicStore(&displacement[column * DISPLACEMENT_SLOTS + slot], 0); }
    atomicStore(&coarse_marks[coarse_index(local)], 0u);
}

@compute @workgroup_size(16, 16)
fn apply_film(@builtin(global_invocation_id) id: vec3<u32>) {
    let local = id.xy + vec2<u32>(deposit.region_x, deposit.region_y);
    if (local.x >= window.width || local.y >= window.height) { return; }
    if (id.x == 0u && id.y == 0u) { atomicStore(&pickup_demand[max_surface_slot()], 0); }
    let slot = (local.y * window.width + local.x) * CHANNELS;
    let mass = f32(atomicLoad(&film[slot])) / PAINT_SCALE;
    if (mass <= 0.0) { return; }
    let absorption = vec3<f32>(f32(atomicLoad(&film[slot + 1u])), f32(atomicLoad(&film[slot + 2u])), f32(atomicLoad(&film[slot + 3u]))) / PAINT_SCALE / mass;
    let scattering = vec3<f32>(f32(atomicLoad(&film[slot + 4u])), f32(atomicLoad(&film[slot + 5u])), f32(atomicLoad(&film[slot + 6u]))) / PAINT_SCALE / mass;
    let medium = f32(atomicLoad(&film[slot + 7u])) / PAINT_SCALE / mass;
    let pixel = vec2<i32>(local) + window.origin;
    if (!on_canvas(pixel)) { return; }
    var remaining = mass;
    give(window_cell(vec3<u32>(local, film_cell(local))), mass, &remaining, absorption, scattering, medium);
}

fn film_cell(local: vec2<u32>) -> u32 {
    let top = window.depth - 1u;
    if (f32(atomicLoad(&paint[window_cell(vec3<u32>(local, top))])) / PAINT_SCALE < FULL_CELL) { return column_top(local); }
    let bottom = window_cell(vec3<u32>(local, 0u));
    let above_bottom = window_cell(vec3<u32>(local, 1u));
    for (var channel = 0u; channel < CHANNELS; channel++) {
        atomicAdd(&paint[bottom + channel], atomicLoad(&paint[above_bottom + channel]));
    }
    for (var z = 1u; z < top; z++) {
        let cell = window_cell(vec3<u32>(local, z));
        let above = window_cell(vec3<u32>(local, z + 1u));
        for (var channel = 0u; channel < CHANNELS; channel++) {
            atomicStore(&paint[cell + channel], atomicLoad(&paint[above + channel]));
        }
    }
    let top_cell = window_cell(vec3<u32>(local, top));
    for (var channel = 0u; channel < CHANNELS; channel++) {
        atomicStore(&paint[top_cell + channel], 0);
    }
    return top;
}
