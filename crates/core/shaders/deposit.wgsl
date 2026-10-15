const MAX_SEGMENT_STEPS: u32 = 16u;
const REFERENCE_SPEED: f32 = 3.0 * PIXELS_PER_CM;
const STATIC_DEPOSIT_FRACTION: f32 = 0.05;
const FILM_MAX_RADIUS: f32 = 0.11 * PIXELS_PER_CM;
const WICKING: f32 = 0.3;
const SCRAPE_FRACTION: f32 = 0.25;
const MAX_OVERLOAD: f32 = 2.0;
const JUMP_DISTANCE: f32 = 0.4 * PIXELS_PER_CM;

struct DepositUniform {
    sample_count: u32,
    samples_per_bristle: u32,
    deposit_rate: f32,
    pickup_rate: f32,
    sample_capacity: f32,
    contact_height: f32,
    dt: f32,
    lift_burst: f32,
};

@group(0) @binding(0) var<uniform> deposit: DepositUniform;
@group(0) @binding(1) var<storage, read> curves: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> previous_curves: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> sample_load: array<f32>;
@group(0) @binding(3) var<storage, read_write> sample_absorption: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> sample_scattering: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> paint: array<atomic<i32>>;
@group(0) @binding(7) var<uniform> window: WindowUniform;
@group(0) @binding(8) var<storage, read> particle_volume: array<i32>;
@group(0) @binding(9) var<storage, read_write> film: array<atomic<i32>>;

fn film_radius(fill: f32) -> f32 {
    return 0.5 + (FILM_MAX_RADIUS - 0.5) * smoothstep(0.2, 1.0, fill);
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

fn joint_density(cell: u32) -> f32 {
    return f32(atomicLoad(&paint[cell]) + particle_volume[cell]) / PAINT_SCALE;
}

fn column_height(local: vec2<u32>) -> f32 {
    var total = 0.0;
    for (var z = 0u; z < window.depth; z++) {
        total += joint_density(window_cell(vec3<u32>(local, z)));
    }
    return total;
}

fn column_top(local: vec2<u32>) -> u32 {
    for (var z = 0u; z < window.depth; z++) {
        if (joint_density(window_cell(vec3<u32>(local, z))) < FULL_CELL) {
            return z;
        }
    }
    return window.depth - 1u;
}

fn pickup(cell: u32, pickup_share: f32, load: ptr<function, f32>, absorption: ptr<function, vec3<f32>>, scattering: ptr<function, vec3<f32>>) {
    pickup_up_to(cell, pickup_share, deposit.sample_capacity, load, absorption, scattering);
}

fn pickup_up_to(cell: u32, pickup_share: f32, capacity: f32, load: ptr<function, f32>, absorption: ptr<function, vec3<f32>>, scattering: ptr<function, vec3<f32>>) {
    let available = f32(atomicLoad(&paint[cell])) / PAINT_SCALE;
    if (available <= 0.0 || *load >= capacity) { return; }
    let wanted = min(min(pickup_share, available), capacity - *load);
    let canvas_absorption = load_vec3(cell, 1u) / available;
    let canvas_scattering = load_vec3(cell, 4u) / available;
    let previous = atomicSub(&paint[cell], fixed(wanted));
    var taken = wanted;
    if (previous < fixed(wanted)) {
        atomicAdd(&paint[cell], fixed(wanted) - max(previous, 0));
        taken = f32(max(previous, 0)) / PAINT_SCALE;
    }
    add_vec3(cell, 1u, -canvas_absorption * taken);
    add_vec3(cell, 4u, -canvas_scattering * taken);
    let total = *load + taken;
    *absorption = (*absorption * *load + canvas_absorption * taken) / max(total, 1e-6);
    *scattering = (*scattering * *load + canvas_scattering * taken) / max(total, 1e-6);
    *load = total;
}

fn give(cell: u32, deposit_share: f32, load: ptr<function, f32>, absorption: vec3<f32>, scattering: vec3<f32>) {
    if (*load <= 0.0) { return; }
    let given = min(deposit_share, *load);
    atomicAdd(&paint[cell], fixed(given));
    add_vec3(cell, 1u, absorption * given);
    add_vec3(cell, 4u, scattering * given);
    *load -= given;
}

fn scrape(local: vec2<u32>, amount: f32, load: ptr<function, f32>, absorption: ptr<function, vec3<f32>>, scattering: ptr<function, vec3<f32>>) {
    var remaining = amount;
    var z = column_top(local);
    loop {
        let cell = window_cell(vec3<u32>(local, z));
        let before = *load;
        let capacity = deposit.sample_capacity;
        let wanted = min(remaining, max(capacity * MAX_OVERLOAD - *load, 0.0));
        if (wanted <= 0.0) { break; }
        let available = f32(atomicLoad(&paint[cell])) / PAINT_SCALE;
        if (available > 0.0) {
            let temporarily_raised = capacity * MAX_OVERLOAD;
            pickup_up_to(cell, wanted, temporarily_raised, load, absorption, scattering);
            remaining -= *load - before;
        }
        if (remaining <= 1e-5 || z == 0u) { break; }
        z -= 1u;
    }
}

fn surface_is_dry(local: vec2<u32>) -> bool {
    let top = column_top(local);
    let top_cell = window_cell(vec3<u32>(local, top));
    var cell = top_cell;
    if (atomicLoad(&paint[top_cell]) <= 0 && top > 0u) {
        cell = window_cell(vec3<u32>(local, top - 1u));
    }
    return atomicLoad(&paint[cell + 7u]) >= i32(PAINT_SCALE);
}

fn exchange(pixel: vec2<i32>, point_xy: vec2<f32>, surface_limit: f32, deposit_share: f32, pickup_share: f32, load: ptr<function, f32>, absorption: ptr<function, vec3<f32>>, scattering: ptr<function, vec3<f32>>) {
    let fill = clamp(*load / deposit.sample_capacity, 0.0, 1.0);
    let local = vec2<u32>(pixel - window.origin);
    let height = column_height(local);
    let wet = !surface_is_dry(local);
    let excess = height - surface_limit;
    if (excess > 0.0) {
        if (wet) { scrape(local, excess * SCRAPE_FRACTION, load, absorption, scattering); }
        return;
    }
    let top = column_top(local);
    let top_cell = window_cell(vec3<u32>(local, top));
    if (wet && f32(atomicLoad(&paint[top_cell])) > 0.0) {
        pickup(top_cell, pickup_share * (1.0 - fill), load, absorption, scattering);
    } else if (wet && top > 0u) {
        pickup(window_cell(vec3<u32>(local, top - 1u)), pickup_share * (1.0 - fill), load, absorption, scattering);
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
    let fill_now = clamp(load / deposit.sample_capacity, 0.0, 1.0);
    let stride = max(u32(round(film_radius(fill_now) * 0.75)), 1u);
    let depositing = (sample % deposit.samples_per_bristle) % stride == 0u;
    let travelled = distance(start.xy, end.xy);
    let speed_factor = travelled / (REFERENCE_SPEED * deposit.dt);
    let lift = deposit.lift_burst * clamp(1.0 - speed_factor, 0.0, 1.0);
    let motion = max(clamp(speed_factor, STATIC_DEPOSIT_FRACTION, 1.0), lift);
    let steps = clamp(u32(ceil(travelled)), 1u, MAX_SEGMENT_STEPS);
    let share = 1.0 / f32(steps);
    for (var i = 1u; i <= steps; i++) {
        let point = mix(start, end, f32(i) / f32(steps));
        let pixel = vec2<i32>(floor(point.xy));
        if (!window_contains(pixel) || pixel.x < 0 || pixel.y < 0 || pixel.x >= i32(window.canvas_width) || pixel.y >= i32(window.canvas_height)) { continue; }
        let local = vec2<u32>(pixel - window.origin);
        let film = mix(0.015 * PIXELS_PER_CM, deposit.contact_height, clamp(load / deposit.sample_capacity, 0.0, 1.0));
        if (point.z > column_height(local) + film) { continue; }
        let deposit_share = select(0.0, deposit.deposit_rate * share * motion * f32(stride), depositing);
        exchange(pixel, point.xy, point.z + film, deposit_share, deposit.pickup_rate * share, &load, &absorption, &scattering);
    }

    sample_load[sample] = load;
    sample_absorption[sample] = vec4<f32>(absorption, 0.0);
    sample_scattering[sample] = vec4<f32>(scattering, 0.0);
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
