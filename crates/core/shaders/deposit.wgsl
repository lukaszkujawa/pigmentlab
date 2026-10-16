const MAX_SEGMENT_STEPS: u32 = 16u;
const REFERENCE_SPEED: f32 = 3.0 * PIXELS_PER_CM;
const FILM_MAX_RADIUS: f32 = 0.11 * PIXELS_PER_CM;
const FILM_MIN_RADIUS: f32 = 0.015 * PIXELS_PER_CM;
const WICKING: f32 = 0.3;
const JUMP_DISTANCE: f32 = 0.4 * PIXELS_PER_CM;
const GROOVE_RADIUS: i32 = 1;
const PUSH_DISTANCE: f32 = 2.5;
const PUSH_PACE: f32 = 1.25;
const MAX_PUSH_REACH: f32 = 16.0;
const MAX_PUSH_TARGETS: u32 = 6u;
const FORWARD_SHARE: f32 = 0.8;
const PUSH_RADIUS: f32 = 3.0;
const LIMIT_ENCODING: i32 = 0x7fffffff;
const MIN_DISPLACING_SPEED: f32 = 48.0;
const DRAG_RADIUS: i32 = 2;
const DRAG_RATE_PER_SECOND: f32 = 24.0;
const WIPE_RATE_PER_SECOND: f32 = 60.0;
const CONTACT_RATE: f32 = 0.3;
const LANDING_SUBSTEPS: f32 = 24.0;
const DISPLACEMENT_SLOTS: u32 = 5u;
const COARSE_CELL: u32 = 16u;

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
    padding_0: f32,
    padding_1: f32,
    padding_2: f32,
};

@group(0) @binding(0) var<uniform> deposit: DepositUniform;
@group(0) @binding(1) var<storage, read> curves: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> previous_curves: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> sample_load: array<f32>;
@group(0) @binding(3) var<storage, read_write> sample_absorption: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> sample_scattering: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> paint: array<atomic<i32>>;
@group(0) @binding(7) var<uniform> window: WindowUniform;
@group(0) @binding(8) var<storage, read> stroke_base: array<f32>;
@group(0) @binding(9) var<storage, read_write> film: array<atomic<i32>>;
@group(0) @binding(10) var<storage, read_write> pickup_demand: array<atomic<i32>>;
@group(0) @binding(11) var<storage, read_write> pickup_grant: array<PickupGrant>;
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
    carries_film: bool,
}

fn segment_of(sample: u32, load: f32) -> Segment {
    let bristle = sample / deposit.samples_per_bristle;
    let t = f32(sample % deposit.samples_per_bristle) / f32(deposit.samples_per_bristle - 1u);
    let end = curve_point(false, bristle, t);
    var start = curve_point(true, bristle, t);
    if (distance(start.xy, end.xy) > JUMP_DISTANCE) { start = end; }
    let travelled = distance(start.xy, end.xy);
    let carries_film = deposit.rigidity < 0.5;
    let fill = clamp(load / deposit.sample_capacity, 0.0, 1.0) * select(0.0, 1.0, carries_film);
    return Segment(start, end, clamp(u32(ceil(travelled)), 1u, MAX_SEGMENT_STEPS), min(travelled / (REFERENCE_SPEED * deposit.dt), 1.0), mix(FILM_MIN_RADIUS, deposit.contact_height, fill), fill, carries_film);
}

fn pickup_wanted(segment: Segment, point: vec3<f32>, pixel: vec2<i32>, state: vec4<f32>, remaining: ptr<function, f32>) -> f32 {
    let height = state.x + state.w;
    if (!segment.carries_film || state.x <= 0.0 || point.z > height + segment.film) { return 0.0; }
    let brush_motion = min(deposit.brush_travel / (REFERENCE_SPEED * deposit.dt), 1.0);
    let buried = clamp(min(state.x, base_height(pixel)) - (point.z - state.w), 0.0, deposit.contact_height);
    let share = (deposit.pickup_rate + buried * WIPE_RATE_PER_SECOND * deposit.dt) / f32(segment.steps) * min(segment.motion, brush_motion);
    let wanted = min(share * (1.0 - segment.fill), *remaining);
    *remaining -= wanted;
    return wanted;
}

fn column_index(local: vec2<u32>) -> u32 {
    return local.y * window.width + local.x;
}

fn film_radius(fill: f32) -> f32 {
    return mix(FILM_MIN_RADIUS, FILM_MAX_RADIUS, smoothstep(0.2, 1.0, fill));
}

fn film_kernel(distance_to_centre: f32, radius: f32) -> f32 {
    let normalised = clamp(distance_to_centre / radius, 0.0, 1.0);
    let falloff = 1.0 - normalised * normalised;
    return falloff * falloff;
}

fn surface_within_reach(pixel: vec2<i32>, reach_floor: f32) -> bool {
    if (!window_contains(pixel)) { return false; }
    let state = column_state[column_index(vec2<u32>(pixel - window.origin))];
    return state.x + state.w >= reach_floor;
}

fn splat_film(centre: vec2<f32>, radius: f32, amount: f32, absorption: vec3<f32>, scattering: vec3<f32>, reach_floor: f32) {
    let reach = i32(ceil(radius));
    let base_pixel = vec2<i32>(floor(centre));
    let fraction = centre - floor(centre) - 0.5;
    var covered = 0.0;
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            if (!surface_within_reach(base_pixel + vec2<i32>(dx, dy), reach_floor)) { continue; }
            covered += film_kernel(length(vec2<f32>(f32(dx), f32(dy)) - fraction), radius);
        }
    }
    if (covered <= 0.0) { return; }
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            let pixel = base_pixel + vec2<i32>(dx, dy);
            if (!surface_within_reach(pixel, reach_floor)) { continue; }
            let weight = film_kernel(length(vec2<f32>(f32(dx), f32(dy)) - fraction), radius);
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

fn load_vec3(cell: u32, offset: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(atomicLoad(&paint[cell + offset])),
        f32(atomicLoad(&paint[cell + offset + 1u])),
        f32(atomicLoad(&paint[cell + offset + 2u])),
    ) / PAINT_SCALE;
}

fn control(previous: bool, index: u32) -> vec3<f32> {
    return select(curves[index].xyz, previous_curves[index].xyz, previous);
}

fn curve_point(previous: bool, bristle: u32, t: f32) -> vec3<f32> {
    let base = bristle * 4u;
    return hermite(control(previous, base), control(previous, base + 1u), control(previous, base + 2u), control(previous, base + 3u), t);
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

fn give(cell: u32, deposit_share: f32, load: ptr<function, f32>, absorption: vec3<f32>, scattering: vec3<f32>) {
    if (*load <= 0.0) { return; }
    let given = min(deposit_share, *load);
    atomicAdd(&paint[cell], fixed(given));
    add_vec3(cell, 1u, absorption * given);
    add_vec3(cell, 4u, scattering * given);
    *load -= given;
}

fn snapshot_height(local: vec2<u32>) -> f32 {
    return column_state[column_index(local)].x;
}

@compute @workgroup_size(16, 16)
fn snapshot_columns(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    var height = 0;
    var top = -1.0;
    for (var z = 0u; z < window.depth; z++) {
        let cell = window_cell(vec3<u32>(id.xy, z));
        let density = atomicLoad(&paint[cell]);
        if (density <= 0) { continue; }
        height += density;
        top = f32(z);
    }
    let column = column_index(id.xy);
    let ground_here = ground_at(vec2<i32>(id.xy) + window.origin);
    column_state[column] = vec4<f32>(f32(height) / PAINT_SCALE, top, 0.0, ground_here);
    atomicMax(&pickup_demand[max_surface_slot()], fixed(f32(height) / PAINT_SCALE + ground_here));
    atomicStore(&pickup_demand[column], 0);
    for (var channel = 0u; channel < CHANNELS; channel++) { atomicStore(&film[column * CHANNELS + channel], 0); }
    for (var slot = 0u; slot < DISPLACEMENT_SLOTS; slot++) { atomicStore(&displacement[column * DISPLACEMENT_SLOTS + slot], 0); }
    atomicStore(&coarse_marks[coarse_index(id.xy)], 0u);
}

fn max_surface_slot() -> u32 {
    return window.width * window.height;
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

fn release(point_xy: vec2<f32>, reach_floor: f32, deposit_share: f32, load: ptr<function, f32>, absorption: vec3<f32>, scattering: vec3<f32>) {
    let fill = clamp(*load / deposit.sample_capacity, 0.0, 1.0);
    let given = min(deposit_share * fill, *load);
    if (given <= 0.0) { return; }
    *load -= given;
    splat_film(point_xy, film_radius(fill), given, absorption, scattering, reach_floor);
}

@compute @workgroup_size(128)
fn step(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x;
    if (sample >= deposit.sample_count) { return; }
    let bristle = sample / deposit.samples_per_bristle;
    let landing = clamp(1.0 - curves[bristle * 4u].w / LANDING_SUBSTEPS, 0.0, 1.0);
    let initial_load = sample_load[sample];
    let segment = segment_of(sample, initial_load);
    let max_surface = f32(atomicLoad(&pickup_demand[max_surface_slot()])) / PAINT_SCALE;
    if (min(segment.start.z, segment.end.z) > max_surface + deposit.contact_height) {
        sample_request[sample] = vec2<f32>(initial_load, 0.0);
        return;
    }
    var load = initial_load;
    let absorption = sample_absorption[sample].xyz;
    let scattering = sample_scattering[sample].xyz;
    var remaining_capacity = deposit.sample_capacity - initial_load;
    var requested = 0.0;
    let carries_film = select(0.0, 1.0, segment.carries_film);
    let stride = max(u32(round(film_radius(segment.fill) * 0.75)), 1u);
    let depositing = (sample % deposit.samples_per_bristle) % stride == 0u;
    let travelled = distance(segment.start.xy, segment.end.xy);
    let brush_moving = deposit.brush_travel >= MIN_DISPLACING_SPEED * deposit.dt;
    let share = 1.0 / f32(segment.steps);
    let direction = select(vec2<f32>(0.0), (segment.end.xy - segment.start.xy) / max(travelled, 1e-6), travelled > 1e-3);
    for (var i = 1u; i <= segment.steps; i++) {
        let point = mix(segment.start, segment.end, f32(i) / f32(segment.steps));
        let pixel = vec2<i32>(floor(point.xy));
        if (!window_contains(pixel) || !on_canvas(pixel)) { continue; }
        let local = vec2<u32>(pixel - window.origin);
        let state = column_state[column_index(local)];
        let wanted = pickup_wanted(segment, point, pixel, state, &remaining_capacity);
        if (wanted > 0.0) {
            atomicAdd(&pickup_demand[column_index(local)], fixed(wanted));
            requested += wanted;
        }
        let film = mix(FILM_MIN_RADIUS, deposit.contact_height, clamp(load / deposit.sample_capacity, 0.0, 1.0) * carries_film);
        let height = state.x + state.w;
        if (point.z > height + film) { continue; }
        if (brush_moving && travelled > 1e-3 && height > point.z) {
            mark_displacement(local, height, point.z, film, direction, DRAG_RATE_PER_SECOND * deposit.dt * segment.motion);
        }
        if (height > point.z + film) { continue; }
        let deposit_share = select(0.0, deposit.deposit_rate * share * max(segment.motion, CONTACT_RATE * landing) * f32(stride), depositing);
        release(point.xy, point.z - film, deposit_share, &load, absorption, scattering);
    }

    sample_load[sample] = load;
    sample_request[sample] = vec2<f32>(initial_load, requested);
}

@compute @workgroup_size(16, 16)
fn grant_pickups(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    let column = column_index(id.xy);
    let demand = f32(atomicLoad(&pickup_demand[column])) / PAINT_SCALE;
    pickup_grant[column] = PickupGrant(vec3<f32>(0.0), 0.0, vec4<f32>(0.0));
    if (demand <= 0.0) { return; }
    let top = topmost_filled_cell(id.xy);
    if (top < 0) { return; }
    let cell = window_cell(vec3<u32>(id.xy, u32(top)));
    let available = f32(atomicLoad(&paint[cell])) / PAINT_SCALE;
    if (available <= 0.0) { return; }
    let granted = min(demand, available);
    let absorption = load_vec3(cell, 1u) / available;
    let scattering = load_vec3(cell, 4u) / available;
    atomicSub(&paint[cell], fixed(granted));
    add_vec3(cell, 1u, -absorption * granted);
    add_vec3(cell, 4u, -scattering * granted);
    pickup_grant[column] = PickupGrant(absorption, granted / demand, vec4<f32>(scattering, 0.0));
}

fn take_from_top(local: vec2<u32>, amount: f32, absorption: ptr<function, vec3<f32>>, scattering: ptr<function, vec3<f32>>) -> f32 {
    var remaining = amount;
    var absorption_mass = vec3<f32>(0.0);
    var scattering_mass = vec3<f32>(0.0);
    for (var step = 0u; step < window.depth; step++) {
        let cell = window_cell(vec3<u32>(local, window.depth - 1u - step));
        let density = f32(atomicLoad(&paint[cell])) / PAINT_SCALE;
        if (density <= 0.0) { continue; }
        let taken = min(density, remaining);
        let fraction = taken / density;
        let absorbed = load_vec3(cell, 1u) * fraction;
        let scattered = load_vec3(cell, 4u) * fraction;
        atomicSub(&paint[cell], fixed(taken));
        add_vec3(cell, 1u, -absorbed);
        add_vec3(cell, 4u, -scattered);
        absorption_mass += absorbed;
        scattering_mass += scattered;
        remaining -= taken;
        if (remaining <= 1e-6) { break; }
    }
    let taken = amount - remaining;
    *absorption = absorption_mass / max(taken, 1e-6);
    *scattering = scattering_mass / max(taken, 1e-6);
    return taken;
}

@compute @workgroup_size(16, 16)
fn grant_displacement(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height || !near_marks(id.xy)) { return; }
    var limit_code = 0;
    var groove_code = 0;
    var level_code = 0;
    var drag_code = 0;
    var push = vec2<f32>(0.0);
    for (var dy = -DRAG_RADIUS; dy <= DRAG_RADIUS; dy++) {
        for (var dx = -DRAG_RADIUS; dx <= DRAG_RADIUS; dx++) {
            let mark = vec2<i32>(id.xy) + vec2<i32>(dx, dy);
            if (mark.x < 0 || mark.y < 0 || mark.x >= i32(window.width) || mark.y >= i32(window.height)) { continue; }
            let slot = column_index(vec2<u32>(mark)) * DISPLACEMENT_SLOTS;
            let code = atomicLoad(&displacement[slot]);
            if (code == 0) { continue; }
            limit_code = max(limit_code, code);
            if (abs(dx) <= GROOVE_RADIUS && abs(dy) <= GROOVE_RADIUS) { groove_code = max(groove_code, code); }
            level_code = max(level_code, atomicLoad(&displacement[slot + 1u]));
            drag_code = max(drag_code, atomicLoad(&displacement[slot + 2u]));
            push += vec2<f32>(f32(atomicLoad(&displacement[slot + 3u])), f32(atomicLoad(&displacement[slot + 4u])));
        }
    }
    if (limit_code == 0) { return; }
    let pixel = vec2<i32>(id.xy) + window.origin;
    let paint_here = column_height(id.xy);
    let height = paint_here + ground_at(pixel);
    let hair_level = f32(LIMIT_ENCODING - level_code) / PAINT_SCALE;
    let drag = f32(drag_code) / PAINT_SCALE;
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
    let moved = take_from_top(id.xy, excess * placeable, &absorption, &scattering);
    for (var t = 0u; t < targets; t++) {
        if (shares[t] <= 0.0) { continue; }
        splat_film(centre + offsets[t], PUSH_RADIUS, moved * shares[t] / placeable, absorption, scattering, -1e9);
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
    var load = sample_load[sample];
    var absorption = sample_absorption[sample].xyz * load;
    var scattering = sample_scattering[sample].xyz * load;
    for (var i = 1u; i <= segment.steps; i++) {
        let point = mix(segment.start, segment.end, f32(i) / f32(segment.steps));
        let pixel = vec2<i32>(floor(point.xy));
        if (!window_contains(pixel) || !on_canvas(pixel)) { continue; }
        let column = column_index(vec2<u32>(pixel - window.origin));
        let wanted = pickup_wanted(segment, point, pixel, column_state[column], &remaining_capacity);
        if (wanted <= 0.0) { continue; }
        let grant = pickup_grant[column];
        let taken = wanted * grant.fraction;
        load += taken;
        absorption += grant.absorption * taken;
        scattering += grant.scattering.xyz * taken;
    }
    sample_load[sample] = load;
    sample_absorption[sample] = vec4<f32>(absorption / max(load, 1e-6), 0.0);
    sample_scattering[sample] = vec4<f32>(scattering / max(load, 1e-6), 0.0);
}

@compute @workgroup_size(64)
fn replenish(@builtin(global_invocation_id) id: vec3<u32>) {
    let bristle = id.x;
    if (bristle * deposit.samples_per_bristle >= deposit.sample_count) { return; }
    let base = bristle * deposit.samples_per_bristle;
    var previous_load = sample_load[base];
    var previous_absorption = sample_absorption[base].xyz;
    var previous_scattering = sample_scattering[base].xyz;
    for (var i = 1u; i < deposit.samples_per_bristle; i++) {
        let here = base + i;
        let load = sample_load[here];
        let transfer = (previous_load - load) * WICKING * 0.5;
        if (transfer > 0.0) {
            let total = load + transfer;
            sample_absorption[here] = vec4<f32>((sample_absorption[here].xyz * load + previous_absorption * transfer) / max(total, 1e-6), 0.0);
            sample_scattering[here] = vec4<f32>((sample_scattering[here].xyz * load + previous_scattering * transfer) / max(total, 1e-6), 0.0);
            sample_load[here] = total;
            sample_load[here - 1u] = previous_load - transfer;
        } else if (transfer < 0.0) {
            let gained = -transfer;
            let total = previous_load + gained;
            sample_absorption[here - 1u] = vec4<f32>((previous_absorption * previous_load + sample_absorption[here].xyz * gained) / max(total, 1e-6), 0.0);
            sample_scattering[here - 1u] = vec4<f32>((previous_scattering * previous_load + sample_scattering[here].xyz * gained) / max(total, 1e-6), 0.0);
            sample_load[here - 1u] = total;
            sample_load[here] = load - gained;
        }
        previous_load = sample_load[here];
        previous_absorption = sample_absorption[here].xyz;
        previous_scattering = sample_scattering[here].xyz;
    }
}

@compute @workgroup_size(16, 16)
fn apply_film(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    if (id.x == 0u && id.y == 0u) { atomicStore(&pickup_demand[max_surface_slot()], 0); }
    let slot = (id.y * window.width + id.x) * CHANNELS;
    let mass = f32(atomicLoad(&film[slot])) / PAINT_SCALE;
    if (mass <= 0.0) { return; }
    let absorption = vec3<f32>(f32(atomicLoad(&film[slot + 1u])), f32(atomicLoad(&film[slot + 2u])), f32(atomicLoad(&film[slot + 3u]))) / PAINT_SCALE / mass;
    let scattering = vec3<f32>(f32(atomicLoad(&film[slot + 4u])), f32(atomicLoad(&film[slot + 5u])), f32(atomicLoad(&film[slot + 6u]))) / PAINT_SCALE / mass;
    let pixel = vec2<i32>(id.xy) + window.origin;
    if (pixel.x < 0 || pixel.y < 0 || pixel.x >= i32(window.canvas_width) || pixel.y >= i32(window.canvas_height)) { return; }
    var remaining = mass;
    give(window_cell(vec3<u32>(id.xy, column_top(id.xy))), mass, &remaining, absorption, scattering);
}
