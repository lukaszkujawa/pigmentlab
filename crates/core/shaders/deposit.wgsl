const MAX_SEGMENT_STEPS: u32 = 16u;
const REFERENCE_SPEED: f32 = 3.0 * PIXELS_PER_CM;
const FILM_MAX_RADIUS: f32 = 0.11 * PIXELS_PER_CM;
const FILM_MIN_RADIUS: f32 = 0.015 * PIXELS_PER_CM;
const WICKING: f32 = 0.3;
const JUMP_DISTANCE: f32 = 0.4 * PIXELS_PER_CM;
const GROOVE_RADIUS: i32 = 1;
const PUSH_DISTANCE: f32 = 2.5;
const FORWARD_SHARE: f32 = 0.5;
const PUSH_RADIUS: f32 = 3.0;
const LIMIT_ENCODING: i32 = 0x7fffffff;
const MIN_DISPLACING_TRAVEL: f32 = 0.2;

struct DepositUniform {
    sample_count: u32,
    samples_per_bristle: u32,
    deposit_rate: f32,
    pickup_rate: f32,
    sample_capacity: f32,
    contact_height: f32,
    dt: f32,
    brush_travel: f32,
};

@group(0) @binding(0) var<uniform> deposit: DepositUniform;
@group(0) @binding(1) var<storage, read> curves: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> previous_curves: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> sample_load: array<f32>;
@group(0) @binding(3) var<storage, read_write> sample_absorption: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> sample_scattering: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> paint: array<atomic<i32>>;
@group(0) @binding(7) var<uniform> window: WindowUniform;
@group(0) @binding(9) var<storage, read_write> film: array<atomic<i32>>;
@group(0) @binding(10) var<storage, read_write> pickup_demand: array<atomic<i32>>;
@group(0) @binding(11) var<storage, read_write> pickup_grant: array<vec4<f32>>;
@group(0) @binding(12) var<storage, read_write> pickup_grant_scattering: array<vec4<f32>>;
@group(0) @binding(13) var<storage, read_write> sample_request: array<vec2<f32>>;
@group(0) @binding(14) var<storage, read_write> displacement: array<atomic<i32>>;

const NO_COLUMN: f32 = -1.0;

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

fn splat_film(centre: vec2<f32>, radius: f32, amount: f32, absorption: vec3<f32>, scattering: vec3<f32>) {
    let reach = i32(ceil(radius));
    let base_pixel = vec2<i32>(floor(centre));
    let fraction = centre - floor(centre) - 0.5;
    var covered = 0.0;
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            covered += film_kernel(length(vec2<f32>(f32(dx), f32(dy)) - fraction), radius);
        }
    }
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            let weight = film_kernel(length(vec2<f32>(f32(dx), f32(dy)) - fraction), radius);
            if (weight <= 0.0) { continue; }
            let share = amount * weight / max(covered, 1e-6);
            let pixel = base_pixel + vec2<i32>(dx, dy);
            if (!window_contains(pixel)) { continue; }
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

fn surface_is_dry(local: vec2<u32>) -> bool {
    let height = column_height(local);
    if (height <= 0.0) { return false; }
    let top = min(u32(ceil(height)) - 1u, window.depth - 1u);
    return atomicLoad(&paint[window_cell(vec3<u32>(local, top)) + 7u]) >= i32(PAINT_SCALE);
}

fn on_canvas(pixel: vec2<i32>) -> bool {
    return pixel.x >= 0 && pixel.y >= 0 && pixel.x < i32(window.canvas_width) && pixel.y < i32(window.canvas_height);
}

fn request_displacement(pixel: vec2<i32>, limit: f32, direction: vec2<f32>) {
    for (var dy = -GROOVE_RADIUS; dy <= GROOVE_RADIUS; dy++) {
        for (var dx = -GROOVE_RADIUS; dx <= GROOVE_RADIUS; dx++) {
            let neighbour = pixel + vec2<i32>(dx, dy);
            if (!window_contains(neighbour) || !on_canvas(neighbour)) { continue; }
            let local = vec2<u32>(neighbour - window.origin);
            let excess = column_height(local) - limit;
            if (excess <= 0.0 || surface_is_dry(local)) { continue; }
            let slot = column_index(local) * 4u;
            atomicAdd(&displacement[slot], fixed(excess));
            atomicMax(&displacement[slot + 1u], LIMIT_ENCODING - fixed(limit));
            atomicAdd(&displacement[slot + 2u], fixed(direction.x * excess));
            atomicAdd(&displacement[slot + 3u], fixed(direction.y * excess));
        }
    }
}

fn exchange(pixel: vec2<i32>, point_xy: vec2<f32>, height: f32, deposit_share: f32, pickup_share: f32, load: ptr<function, f32>, absorption: ptr<function, vec3<f32>>, scattering: ptr<function, vec3<f32>>, request: ptr<function, vec2<f32>>) {
    let fill = clamp(*load / deposit.sample_capacity, 0.0, 1.0);
    let local = vec2<u32>(pixel - window.origin);
    let wet = !surface_is_dry(local);
    if (wet && height > 0.0 && *load < deposit.sample_capacity) {
        let wanted = min(pickup_share * (1.0 - fill), deposit.sample_capacity - *load);
        if (wanted > 0.0) {
            *request = vec2<f32>(f32(column_index(local)), (*request).y + wanted);
        }
    }
    let given = min(deposit_share * fill, *load);
    if (given <= 0.0) { return; }
    *load -= given;
    splat_film(point_xy, film_radius(fill), given, *absorption, *scattering);
}

@compute @workgroup_size(128)
fn step(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x;
    if (sample >= deposit.sample_count) { return; }
    let bristle = sample / deposit.samples_per_bristle;
    let t = f32(sample % deposit.samples_per_bristle) / f32(deposit.samples_per_bristle - 1u);
    let end = curve_point(false, bristle, t);
    var start = curve_point(true, bristle, t);
    if (distance(start.xy, end.xy) > JUMP_DISTANCE) { start = end; }

    var load = sample_load[sample];
    var absorption = sample_absorption[sample].xyz;
    var scattering = sample_scattering[sample].xyz;
    var request = vec2<f32>(NO_COLUMN, 0.0);
    let fill_now = clamp(load / deposit.sample_capacity, 0.0, 1.0);
    let stride = max(u32(round(film_radius(fill_now) * 0.75)), 1u);
    let depositing = (sample % deposit.samples_per_bristle) % stride == 0u;
    let travelled = distance(start.xy, end.xy);
    let motion = min(travelled / (REFERENCE_SPEED * deposit.dt), 1.0);
    let brush_motion = min(deposit.brush_travel / (REFERENCE_SPEED * deposit.dt), 1.0);
    let brush_moving = deposit.brush_travel >= MIN_DISPLACING_TRAVEL;
    let steps = clamp(u32(ceil(travelled)), 1u, MAX_SEGMENT_STEPS);
    let share = 1.0 / f32(steps);
    let direction = select(vec2<f32>(0.0), (end.xy - start.xy) / max(travelled, 1e-6), travelled > 1e-3);
    for (var i = 1u; i <= steps; i++) {
        let point = mix(start, end, f32(i) / f32(steps));
        let pixel = vec2<i32>(floor(point.xy));
        if (!window_contains(pixel) || !on_canvas(pixel)) { continue; }
        let local = vec2<u32>(pixel - window.origin);
        let film = mix(0.015 * PIXELS_PER_CM, deposit.contact_height, clamp(load / deposit.sample_capacity, 0.0, 1.0));
        let height = column_height(local);
        if (point.z > height + film) { continue; }
        let surface_limit = point.z + film;
        if (height > surface_limit) {
            if (brush_moving && travelled > 1e-3) { request_displacement(pixel, surface_limit, direction); }
            continue;
        }
        let deposit_share = select(0.0, deposit.deposit_rate * share * motion * f32(stride), depositing);
        exchange(pixel, point.xy, height, deposit_share, deposit.pickup_rate * share * min(motion, brush_motion), &load, &absorption, &scattering, &request);
    }

    sample_load[sample] = load;
    sample_absorption[sample] = vec4<f32>(absorption, 0.0);
    sample_scattering[sample] = vec4<f32>(scattering, 0.0);
    sample_request[sample] = request;
    if (request.x >= 0.0 && request.y > 0.0) {
        atomicAdd(&pickup_demand[u32(request.x)], fixed(request.y));
    }
}

@compute @workgroup_size(16, 16)
fn grant_pickups(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    let column = column_index(id.xy);
    let demand = f32(atomicLoad(&pickup_demand[column])) / PAINT_SCALE;
    pickup_grant[column] = vec4<f32>(0.0);
    pickup_grant_scattering[column] = vec4<f32>(0.0);
    if (demand <= 0.0) { return; }
    let height = column_height(id.xy);
    if (height <= 0.0) { return; }
    let top = min(u32(ceil(height)) - 1u, window.depth - 1u);
    let cell = window_cell(vec3<u32>(id.xy, top));
    let available = f32(atomicLoad(&paint[cell])) / PAINT_SCALE;
    if (available <= 0.0) { return; }
    let granted = min(demand, available);
    let absorption = load_vec3(cell, 1u) / available;
    let scattering = load_vec3(cell, 4u) / available;
    atomicSub(&paint[cell], fixed(granted));
    add_vec3(cell, 1u, -absorption * granted);
    add_vec3(cell, 4u, -scattering * granted);
    pickup_grant[column] = vec4<f32>(absorption, granted / demand);
    pickup_grant_scattering[column] = vec4<f32>(scattering, 0.0);
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
    if (id.x >= window.width || id.y >= window.height) { return; }
    let slot = column_index(id.xy) * 4u;
    let demand = f32(atomicLoad(&displacement[slot])) / PAINT_SCALE;
    if (demand <= 0.0) { return; }
    let limit = f32(LIMIT_ENCODING - atomicLoad(&displacement[slot + 1u])) / PAINT_SCALE;
    let excess = min(column_height(id.xy) - limit, demand);
    if (excess <= 0.0) { return; }
    let push = vec2<f32>(f32(atomicLoad(&displacement[slot + 2u])), f32(atomicLoad(&displacement[slot + 3u])));
    if (length(push) <= 1e-3) { return; }
    let forward = normalize(push);
    let side = vec2<f32>(-forward.y, forward.x);
    let offsets = array<vec2<f32>, 3>(forward, side, -side);
    var shares = array<f32, 3>(FORWARD_SHARE, 0.5 * (1.0 - FORWARD_SHARE), 0.5 * (1.0 - FORWARD_SHARE));
    let centre = vec2<f32>(vec2<i32>(id.xy) + window.origin) + 0.5;
    var placeable = 0.0;
    for (var t = 0u; t < 3u; t++) {
        let landing = vec2<i32>(floor(centre + offsets[t] * PUSH_DISTANCE));
        if (!window_contains(landing) || !on_canvas(landing)) { shares[t] = 0.0; }
        placeable += shares[t];
    }
    if (placeable <= 0.0) { return; }
    var absorption = vec3<f32>(0.0);
    var scattering = vec3<f32>(0.0);
    let moved = take_from_top(id.xy, excess * placeable, &absorption, &scattering);
    for (var t = 0u; t < 3u; t++) {
        if (shares[t] <= 0.0) { continue; }
        splat_film(centre + offsets[t] * PUSH_DISTANCE, PUSH_RADIUS, moved * shares[t] / placeable, absorption, scattering);
    }
}

@compute @workgroup_size(128)
fn apply_pickups(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x;
    if (sample >= deposit.sample_count) { return; }
    let request = sample_request[sample];
    if (request.x < 0.0 || request.y <= 0.0) { return; }
    let column = u32(request.x);
    let grant = pickup_grant[column];
    let taken = request.y * grant.w;
    if (taken <= 0.0) { return; }
    let load = sample_load[sample];
    let total = load + taken;
    let scattering = pickup_grant_scattering[column].xyz;
    sample_absorption[sample] = vec4<f32>((sample_absorption[sample].xyz * load + grant.xyz * taken) / max(total, 1e-6), 0.0);
    sample_scattering[sample] = vec4<f32>((sample_scattering[sample].xyz * load + scattering * taken) / max(total, 1e-6), 0.0);
    sample_load[sample] = total;
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
