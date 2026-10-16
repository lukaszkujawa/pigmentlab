const GRAVITY: f32 = 0.25;
const WAVE_LIMIT: f32 = 0.35;
const PAPER_SLOPE: f32 = 0.03;
const VISCOSITY: f32 = 0.1;
const DRAG: f32 = 0.03;
const THIN_DRAG: f32 = 0.6;
const THIN_WATER: f32 = 0.25;
const MAX_SPEED: f32 = 0.45;
const IMPULSE_GAIN: f32 = 0.5;
const ADSORPTION_RATE: f32 = 4.0;
const DESORPTION_RATE: f32 = 1.5;
const FILM_WATER: f32 = 0.05;
const FILM_DEPOSITION: f32 = 6.0;
const ABSORPTION_RATE: f32 = 0.6;
const EVAPORATION: f32 = 0.012;
const EDGE_EVAPORATION: f32 = 6.0;
const PAPER_DRYING: f32 = 0.02;
const CAPILLARY_RATE: f32 = 6.0;
const CAPILLARY_SOURCE: f32 = 0.3;
const CAPILLARY_RECEIVER: f32 = 0.05;
const CAPILLARY_WETTING: f32 = 0.35;
const DRY_PAPER: f32 = 0.02;
const SIZED_ABSORBENCY: f32 = 0.3;
const UNSIZED_STAINING: f32 = 40.0;
const UNSIZED_WICKING: f32 = 3.0;
const MAX_SCRUB: f32 = 0.2;
const PIGMENT_DIFFUSION: f32 = 12.0;
const ACCUMULATOR_SCALE: f32 = 65536.0;
const ACCUMULATORS: u32 = 13u;
const MASK_RADIUS: i32 = 3;
const GRANULATION_CONTRAST: f32 = 2.5;
const GRANULATION_FLOOR: f32 = 0.3;
const EDGE_SHARPNESS: f32 = 6.0;

struct WaterUniform {
    cells: vec2<u32>,
    canvas: vec2<u32>,
    region_origin: vec2<i32>,
    region_size: vec2<u32>,
    dt: f32,
    absorbency: f32,
    granulation: f32,
    tooth: f32,
    force_dry: u32,
    scrub: f32,
    padding_0: f32,
    padding_1: f32,
};

struct Pigment {
    absorption: vec4<f32>,
    scattering: vec4<f32>,
    behaviour: vec4<f32>,
};

@group(0) @binding(0) var<uniform> sim: WaterUniform;
@group(0) @binding(1) var<storage, read> paper: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> water: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> water_next: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> velocity_u: array<f32>;
@group(0) @binding(5) var<storage, read_write> velocity_v: array<f32>;
@group(0) @binding(6) var<storage, read_write> next_u: array<f32>;
@group(0) @binding(7) var<storage, read_write> next_v: array<f32>;
@group(0) @binding(8) var<storage, read_write> suspended: array<Pigment>;
@group(0) @binding(9) var<storage, read_write> suspended_next: array<Pigment>;
@group(0) @binding(10) var<storage, read_write> deposited: array<Pigment>;
@group(0) @binding(11) var<storage, read_write> impulse: array<vec4<f32>>;
@group(0) @binding(12) var<storage, read_write> accumulators: array<atomic<i32>>;
@group(0) @binding(13) var<storage, read_write> dry: array<vec4<f32>>;
@group(0) @binding(14) var<storage, read> relief: array<f32>;
@group(0) @binding(15) var<storage, read_write> coverage: array<vec2<f32>>;
@group(0) @binding(16) var<storage, read_write> drying: array<Pigment>;

fn no_pigment() -> Pigment {
    return Pigment(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
}

fn scaled(p: Pigment, factor: f32) -> Pigment {
    return Pigment(p.absorption * factor, p.scattering * factor, p.behaviour * factor);
}

fn added(a: Pigment, b: Pigment) -> Pigment {
    return Pigment(a.absorption + b.absorption, a.scattering + b.scattering, a.behaviour + b.behaviour);
}

fn clamped(p: Pigment) -> Pigment {
    return Pigment(max(p.absorption, vec4<f32>(0.0)), max(p.scattering, vec4<f32>(0.0)), max(p.behaviour, vec4<f32>(0.0)));
}

fn mass(p: Pigment) -> f32 {
    return p.absorption.w;
}

fn region_cell(id: vec2<u32>) -> vec2<i32> {
    if (id.x >= sim.region_size.x || id.y >= sim.region_size.y) { return vec2<i32>(-1); }
    return sim.region_origin + vec2<i32>(id);
}

fn in_region(cell: vec2<i32>) -> bool {
    let local = cell - sim.region_origin;
    return local.x >= 0 && local.y >= 0 && u32(local.x) < sim.region_size.x && u32(local.y) < sim.region_size.y;
}

fn on_grid(cell: vec2<i32>) -> bool {
    return cell.x >= 0 && cell.y >= 0 && u32(cell.x) < sim.cells.x && u32(cell.y) < sim.cells.y;
}

fn cell_index(cell: vec2<i32>) -> u32 {
    return u32(cell.y) * sim.cells.x + u32(cell.x);
}

fn u_index(face: vec2<i32>) -> u32 {
    return u32(face.y) * (sim.cells.x + 1u) + u32(face.x);
}

fn v_index(face: vec2<i32>) -> u32 {
    return u32(face.y) * sim.cells.x + u32(face.x);
}

fn wet(cell: vec2<i32>) -> bool {
    return on_grid(cell) && in_region(cell) && water[cell_index(cell)].z > 0.5;
}

fn u_at(face: vec2<i32>) -> f32 {
    if (face.y < 0 || u32(face.y) >= sim.cells.y || face.x < 0 || u32(face.x) > sim.cells.x) { return 0.0; }
    return velocity_u[u_index(face)];
}

fn v_at(face: vec2<i32>) -> f32 {
    if (face.x < 0 || u32(face.x) >= sim.cells.x || face.y < 0 || u32(face.y) > sim.cells.y) { return 0.0; }
    return velocity_v[v_index(face)];
}

fn pressure_push(from_cell: vec2<i32>, to_cell: vec2<i32>, depth: f32) -> f32 {
    let a = cell_index(from_cell);
    let b = cell_index(to_cell);
    let gravity = min(GRAVITY, WAVE_LIMIT * WAVE_LIMIT / max(depth, 1e-3));
    return (water[a].x - water[b].x) * gravity + (paper[a].x - paper[b].x) * PAPER_SLOPE;
}

fn accumulator(cell: vec2<i32>, channel: u32) -> f32 {
    return f32(atomicExchange(&accumulators[cell_index(cell) * ACCUMULATORS + channel], 0)) / ACCUMULATOR_SCALE;
}

@compute @workgroup_size(16, 16)
fn apply_brush(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell = region_cell(id.xy);
    if (!on_grid(cell)) { return; }
    let index = cell_index(cell);
    let added_water = accumulator(cell, 0u);
    let pigment = Pigment(
        vec4<f32>(accumulator(cell, 1u), accumulator(cell, 2u), accumulator(cell, 3u), accumulator(cell, 4u)),
        vec4<f32>(accumulator(cell, 5u), accumulator(cell, 6u), accumulator(cell, 7u), accumulator(cell, 8u)),
        vec4<f32>(accumulator(cell, 9u), accumulator(cell, 10u), 0.0, 0.0),
    );
    let push = vec2<f32>(accumulator(cell, 11u), accumulator(cell, 12u));
    var state = water[index];
    if (added_water != 0.0 || mass(pigment) != 0.0) {
        state.x = max(state.x + added_water, 0.0);
        if (added_water > 0.0) { state.z = 1.0; }
        suspended[index] = clamped(added(suspended[index], pigment));
    }
    let settled = deposited[index];
    let staining = max(settled.behaviour.y / max(mass(settled), 1e-9), 0.1) * (1.0 + unsized_share() * UNSIZED_STAINING);
    let scrub = min(length(push) * sim.scrub / staining, MAX_SCRUB);
    if (scrub > 0.0 && state.z > 0.5) {
        let lifted = scaled(settled, scrub);
        deposited[index] = added(deposited[index], scaled(lifted, -1.0));
        suspended[index] = added(suspended[index], lifted);
    }
    water[index] = state;
    impulse[index] = vec4<f32>(push * IMPULSE_GAIN / max(state.x, THIN_WATER), 0.0, 0.0);
}

fn unsized_share() -> f32 {
    return clamp((sim.absorbency - SIZED_ABSORBENCY) / (1.0 - SIZED_ABSORBENCY), 0.0, 1.0);
}

fn face_drag(depth: f32) -> f32 {
    return DRAG + THIN_DRAG * max(1.0 - depth / THIN_WATER, 0.0);
}

fn limited(speed: f32) -> f32 {
    return clamp(speed, -MAX_SPEED, MAX_SPEED);
}

@compute @workgroup_size(16, 16)
fn update_u(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x > sim.region_size.x || id.y >= sim.region_size.y) { return; }
    let face = sim.region_origin + vec2<i32>(id.xy);
    if (face.y < 0 || u32(face.y) >= sim.cells.y || face.x < 0 || u32(face.x) > sim.cells.x) { return; }
    let left = face - vec2<i32>(1, 0);
    if (!wet(left) || !wet(face)) {
        next_u[u_index(face)] = 0.0;
        return;
    }
    let u = u_at(face);
    let centre_left = 0.5 * (u_at(face - vec2<i32>(1, 0)) + u);
    let centre_right = 0.5 * (u + u_at(face + vec2<i32>(1, 0)));
    let corner_up = 0.5 * (u + u_at(face - vec2<i32>(0, 1))) * 0.5 * (v_at(left) + v_at(face));
    let corner_down = 0.5 * (u + u_at(face + vec2<i32>(0, 1))) * 0.5 * (v_at(left + vec2<i32>(0, 1)) + v_at(face + vec2<i32>(0, 1)));
    let advection = centre_left * centre_left - centre_right * centre_right + corner_up - corner_down;
    let diffusion = u_at(face + vec2<i32>(1, 0)) + u_at(face - vec2<i32>(1, 0)) + u_at(face + vec2<i32>(0, 1)) + u_at(face - vec2<i32>(0, 1)) - 4.0 * u;
    let depth = 0.5 * (water[cell_index(left)].x + water[cell_index(face)].x);
    let push = 0.5 * (impulse[cell_index(left)].x + impulse[cell_index(face)].x);
    let accelerated = u + (advection + VISCOSITY * diffusion + pressure_push(left, face, depth) - face_drag(depth) * u) + push;
    next_u[u_index(face)] = limited(accelerated);
}

@compute @workgroup_size(16, 16)
fn update_v(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= sim.region_size.x || id.y > sim.region_size.y) { return; }
    let face = sim.region_origin + vec2<i32>(id.xy);
    if (face.x < 0 || u32(face.x) >= sim.cells.x || face.y < 0 || u32(face.y) > sim.cells.y) { return; }
    let up = face - vec2<i32>(0, 1);
    if (!wet(up) || !wet(face)) {
        next_v[v_index(face)] = 0.0;
        return;
    }
    let v = v_at(face);
    let centre_up = 0.5 * (v_at(face - vec2<i32>(0, 1)) + v);
    let centre_down = 0.5 * (v + v_at(face + vec2<i32>(0, 1)));
    let corner_left = 0.5 * (v + v_at(face - vec2<i32>(1, 0))) * 0.5 * (u_at(up) + u_at(face));
    let corner_right = 0.5 * (v + v_at(face + vec2<i32>(1, 0))) * 0.5 * (u_at(up + vec2<i32>(1, 0)) + u_at(face + vec2<i32>(1, 0)));
    let advection = centre_up * centre_up - centre_down * centre_down + corner_left - corner_right;
    let diffusion = v_at(face + vec2<i32>(1, 0)) + v_at(face - vec2<i32>(1, 0)) + v_at(face + vec2<i32>(0, 1)) + v_at(face - vec2<i32>(0, 1)) - 4.0 * v;
    let depth = 0.5 * (water[cell_index(up)].x + water[cell_index(face)].x);
    let push = 0.5 * (impulse[cell_index(up)].y + impulse[cell_index(face)].y);
    let accelerated = v + (advection + VISCOSITY * diffusion + pressure_push(up, face, depth) - face_drag(depth) * v) + push;
    next_v[v_index(face)] = limited(accelerated);
}

fn outflow(cell: vec2<i32>) -> f32 {
    return max(u_at(cell + vec2<i32>(1, 0)), 0.0) + max(-u_at(cell), 0.0) + max(v_at(cell + vec2<i32>(0, 1)), 0.0) + max(-v_at(cell), 0.0);
}

@compute @workgroup_size(16, 16)
fn limit_outflow(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell = region_cell(id.xy);
    if (!on_grid(cell)) { return; }
    let index = cell_index(cell);
    impulse[index].w = 1.0 / max(outflow(cell), 1.0);
}

fn leaving(cell: vec2<i32>, speed: f32) -> f32 {
    return speed * impulse[cell_index(cell)].w;
}

@compute @workgroup_size(16, 16)
fn advect(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell = region_cell(id.xy);
    if (!on_grid(cell)) { return; }
    let index = cell_index(cell);
    let state = water[index];
    let own_share = 1.0 - outflow(cell) * impulse[index].w;
    var depth = state.x * own_share;
    var pigment = scaled(suspended[index], own_share);
    let neighbours = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    for (var n = 0u; n < 4u; n++) {
        let other = cell + neighbours[n];
        if (!wet(other)) { continue; }
        var speed = 0.0;
        switch n {
            case 0u: { speed = max(u_at(cell), 0.0); }
            case 1u: { speed = max(-u_at(other), 0.0); }
            case 2u: { speed = max(v_at(cell), 0.0); }
            default: { speed = max(-v_at(other), 0.0); }
        }
        if (speed <= 0.0) { continue; }
        let share = leaving(other, speed);
        let other_index = cell_index(other);
        depth += water[other_index].x * share;
        pigment = added(pigment, scaled(suspended[other_index], share));
    }
    let mixing = min(PIGMENT_DIFFUSION * sim.dt, 0.2);
    let own_staying = scaled(suspended[index], own_share);
    for (var n = 0u; n < 4u; n++) {
        let other = cell + neighbours[n];
        if (!wet(other) || state.z < 0.5) { continue; }
        let other_index = cell_index(other);
        let other_depth = water[other_index].x;
        let total_depth = state.x + other_depth;
        if (total_depth <= 1e-6) { continue; }
        let other_staying = scaled(suspended[other_index], 1.0 - outflow(other) * impulse[other_index].w);
        pigment = added(pigment, added(scaled(other_staying, mixing * state.x / total_depth), scaled(own_staying, -mixing * other_depth / total_depth)));
    }
    let fibres = unsized_share();
    if (fibres > 0.0) {
        let capacity = paper[index].y;
        let own = suspended[index];
        let own_concentration = 1.0 / max(state.x + state.y, 1e-6);
        for (var n = 0u; n < 4u; n++) {
            let other = cell + neighbours[n];
            if (!on_grid(other) || !in_region(other)) { continue; }
            let other_index = cell_index(other);
            let other_state = water[other_index];
            let other_capacity = paper[other_index].y;
            let incoming = capillary_flow(other_state, other_capacity, state, capacity) * fibres / max(other_state.x + other_state.y, 1e-6);
            let outgoing = capillary_flow(state, capacity, other_state, other_capacity) * fibres * own_concentration;
            pigment = added(pigment, added(scaled(suspended[other_index], min(incoming, 0.25)), scaled(own, -min(outgoing, 0.25))));
        }
    }
    water_next[index] = vec4<f32>(depth, state.yzw);
    suspended_next[index] = clamped(pigment);
}

@compute @workgroup_size(16, 16)
fn transfer(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell = region_cell(id.xy);
    if (!on_grid(cell)) { return; }
    let index = cell_index(cell);
    let state = water_next[index];
    var floating = suspended_next[index];
    var settled = deposited[index];
    if (state.z > 0.5) {
        let height = paper[index].x;
        let floating_mass = max(mass(floating), 1e-9);
        let settled_mass = max(mass(settled), 1e-9);
        let density = floating.behaviour.x / floating_mass;
        let granulation = floating.scattering.w / floating_mass * sim.granulation;
        let settled_density = settled.behaviour.x / settled_mass;
        let staining = max(settled.behaviour.y / settled_mass, 0.1);
        let settled_granulation = settled.scattering.w / settled_mass * sim.granulation;
        let film = clamp(1.0 - state.x / FILM_WATER, 0.0, 1.0);
        let soak = 1.0 + unsized_share() * UNSIZED_STAINING;
        let down = clamp((max(1.0 - height * granulation, 0.0) * density * ADSORPTION_RATE * soak + film * FILM_DEPOSITION) * sim.dt, 0.0, 1.0);
        let up = clamp(max(1.0 + (height - 1.0) * settled_granulation, 0.0) * settled_density / (staining * soak) * DESORPTION_RATE * sim.dt * (1.0 - film), 0.0, 1.0);
        let landing = scaled(floating, down);
        let rising = scaled(settled, up);
        floating = added(added(floating, scaled(landing, -1.0)), rising);
        settled = added(added(settled, scaled(rising, -1.0)), landing);
    }
    suspended[index] = floating;
    deposited[index] = settled;
}

fn capillary_flow(source: vec4<f32>, source_capacity: f32, receiver: vec4<f32>, receiver_capacity: f32) -> f32 {
    let receiving = CAPILLARY_RECEIVER * max(1.0 - 2.0 * unsized_share(), 0.0) * receiver_capacity;
    if (source.y <= CAPILLARY_SOURCE * source_capacity || source.y <= receiver.y || receiver.y < receiving || (receiving > 0.0 && receiver.y == receiving)) { return 0.0; }
    return max(min(source.y - receiver.y, receiver_capacity - receiver.y), 0.0) * 0.25 * min(CAPILLARY_RATE * (1.0 + unsized_share() * UNSIZED_WICKING) * sim.dt, 1.0);
}

@compute @workgroup_size(16, 16)
fn capillary(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell = region_cell(id.xy);
    if (!on_grid(cell)) { return; }
    let index = cell_index(cell);
    var state = water_next[index];
    let capacity = paper[index].y;
    let neighbours = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    var saturation = state.y;
    for (var n = 0u; n < 4u; n++) {
        let other = cell + neighbours[n];
        if (!on_grid(other) || !in_region(other)) { continue; }
        let other_index = cell_index(other);
        let other_state = water_next[other_index];
        let other_capacity = paper[other_index].y;
        saturation += capillary_flow(other_state, other_capacity, state, capacity) - capillary_flow(state, capacity, other_state, other_capacity);
    }
    var depth = state.x;
    if (state.z > 0.5) {
        let soaked = min(min(sim.absorbency * ABSORPTION_RATE * sim.dt, max(capacity - saturation, 0.0)), depth);
        saturation += soaked;
        depth -= soaked;
        depth = max(depth - EVAPORATION * sim.dt * (1.0 + EDGE_EVAPORATION * (1.0 - state.w)), 0.0);
    }
    saturation = max(saturation - PAPER_DRYING * sim.dt * select(1.0, 0.25, depth > 0.0), 0.0);
    var mask = state.z;
    if (saturation > CAPILLARY_WETTING * capacity) { mask = 1.0; }
    var covered = 0.0;
    for (var dy = -MASK_RADIUS; dy <= MASK_RADIUS; dy++) {
        for (var dx = -MASK_RADIUS; dx <= MASK_RADIUS; dx++) {
            let other = cell + vec2<i32>(dx, dy);
            if (on_grid(other) && water_next[cell_index(other)].z > 0.5) { covered += 1.0; }
        }
    }
    let taps = f32((2 * MASK_RADIUS + 1) * (2 * MASK_RADIUS + 1));
    water[index] = vec4<f32>(depth, saturation, mask, covered / taps);
    var blurred = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let other = cell + vec2<i32>(dx, dy);
            if (on_grid(other)) { blurred += coverage[cell_index(other)].x; }
        }
    }
    coverage[index].y = blurred / 9.0;
}

fn blurred_coverage(cell: vec2<i32>) -> f32 {
    return coverage[cell_index(clamp(cell, vec2<i32>(0), vec2<i32>(sim.cells) - 1))].y;
}

fn wash_contour(pixel: vec2<u32>) -> f32 {
    let position = (vec2<f32>(pixel) + 0.5) / 2.0 - 0.5;
    let base = vec2<i32>(floor(position));
    let f = position - floor(position);
    let top = mix(blurred_coverage(base), blurred_coverage(base + vec2<i32>(1, 0)), f.x);
    let bottom = mix(blurred_coverage(base + vec2<i32>(0, 1)), blurred_coverage(base + vec2<i32>(1, 1)), f.x);
    return mix(top, bottom, f.y);
}

fn paper_height(pixel: vec2<u32>) -> f32 {
    return relief[pixel.y * sim.canvas.x + pixel.x] / max(sim.tooth, 1e-6) * 0.5 + 0.5;
}

fn footprint_weight(pixel: vec2<i32>, cell: vec2<i32>, granulation: f32, mean_height: f32) -> f32 {
    if (pixel.x < 0 || pixel.y < 0 || u32(pixel.x) >= sim.canvas.x || u32(pixel.y) >= sim.canvas.y) { return 0.0; }
    let offset = abs(vec2<f32>(pixel) + 0.5 - (vec2<f32>(cell) * 2.0 + 1.0));
    let tent = max(1.0 - offset.x / 2.0, 0.0) * max(1.0 - offset.y / 2.0, 0.0);
    let granular = max(1.0 + granulation * (mean_height - paper_height(vec2<u32>(pixel))) * GRANULATION_CONTRAST, GRANULATION_FLOOR);
    return tent * granular * pow(wash_contour(vec2<u32>(pixel)), EDGE_SHARPNESS);
}

@compute @workgroup_size(16, 16)
fn dry_cells(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell = region_cell(id.xy);
    if (!on_grid(cell)) { return; }
    let index = cell_index(cell);
    drying[index] = no_pigment();
    let state = water[index];
    if (state.z < 0.5) { return; }
    coverage[index].x = 1.0;
    let neighbours = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    var refilling = false;
    for (var n = 0u; n < 4u; n++) {
        let other = cell + neighbours[n];
        refilling = refilling || (on_grid(other) && water[cell_index(other)].x > 0.0);
    }
    let dried = state.x <= 0.0 && state.y < DRY_PAPER * paper[index].y && !refilling;
    if (!dried && sim.force_dry == 0u) { return; }
    let pigment = added(suspended[index], deposited[index]);
    let total = mass(pigment);
    if (total > 1e-7) {
        let granulation = pigment.scattering.w / total * sim.granulation;
        var norm = 0.0;
        for (var dy = -1; dy <= 2; dy++) {
            for (var dx = -1; dx <= 2; dx++) {
                norm += footprint_weight(cell * 2 + vec2<i32>(dx, dy), cell, granulation, paper[index].x);
            }
        }
        let share = select(total / norm, 0.0, norm <= 1e-9);
        drying[index] = Pigment(vec4<f32>(pigment.absorption.xyz / total, share), vec4<f32>(pigment.scattering.xyz / total, granulation), vec4<f32>(paper[index].x, select(0.0, total, norm <= 1e-9), 0.0, 0.0));
    }
    suspended[index] = no_pigment();
    deposited[index] = no_pigment();
    water[index] = vec4<f32>(0.0, select(state.y, 0.0, sim.force_dry != 0u), 0.0, state.w);
}

@compute @workgroup_size(16, 16)
fn bake_pixels(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= sim.region_size.x * 2u || id.y >= sim.region_size.y * 2u) { return; }
    let pixel = sim.region_origin * 2 + vec2<i32>(id.xy);
    if (pixel.x < 0 || pixel.y < 0 || u32(pixel.x) >= sim.canvas.x || u32(pixel.y) >= sim.canvas.y) { return; }
    let nearest = vec2<i32>(floor((vec2<f32>(pixel) + 0.5) / 2.0 - 0.5));
    var mass_here = 0.0;
    var absorption = vec3<f32>(0.0);
    var scattering = vec3<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let cell = nearest + vec2<i32>(k & 1, k >> 1);
        if (!on_grid(cell) || !in_region(cell)) { continue; }
        let source = drying[cell_index(cell)];
        var share = 0.0;
        if (source.absorption.w > 0.0) {
            share = source.absorption.w * footprint_weight(pixel, cell, source.scattering.w, source.behaviour.x);
        } else if (source.behaviour.y > 0.0 && all(pixel / 2 == cell)) {
            share = source.behaviour.y / 4.0;
        }
        if (share <= 0.0) { continue; }
        mass_here += share;
        absorption += source.absorption.xyz * share;
        scattering += source.scattering.xyz * share;
    }
    if (mass_here <= 0.0) { return; }
    let slot = u32(pixel.y) * sim.canvas.x + u32(pixel.x);
    let below = dry[slot];
    dry[slot] = vec4<f32>(below.x, layer_reflectance(absorption / mass_here, scattering / mass_here, mass_here * OPTICAL_THICKNESS_PER_PIXEL, below.yzw));
}
