struct TubeUniform {
    absorption: vec4<f32>,
    scattering: vec4<f32>,
    path_start: vec2<f32>,
    path_end: vec2<f32>,
    corner: vec2<i32>,
    strand: vec2<f32>,
    mound: vec2<f32>,
    strand_core: f32,
    strand_drape: f32,
    mound_core: f32,
    mound_drape: f32,
    arc: f32,
    memory: f32,
    squeezed: f32,
    backlog_limit: f32,
    padding: vec2<f32>,
};

struct Ledger {
    budget: atomic<u32>,
    missing: atomic<u32>,
};

@group(0) @binding(0) var<uniform> tube: TubeUniform;
@group(0) @binding(1) var<uniform> window: WindowUniform;
@group(0) @binding(2) var<storage, read_write> volume: array<i32>;
@group(0) @binding(3) var<storage, read> ground: array<f32>;
@group(0) @binding(4) var<storage, read> heights: array<f32>;
@group(0) @binding(5) var<storage, read> store: array<i32>;
@group(0) @binding(6) var<storage, read_write> marks: array<vec2<f32>>;
@group(0) @binding(7) var<storage, read_write> planned: array<vec2<f32>>;
@group(0) @binding(8) var<storage, read> relief: array<f32>;
@group(0) @binding(9) var<storage, read_write> ledger: Ledger;

const RESTING_REACH: i32 = 2;
const NO_TARGET: f32 = -1e30;
const LEDGER_SCALE: f32 = 16.0;

var<workgroup> anchor_floors: vec2<f32>;

fn canvas_index(pixel: vec2<i32>) -> u32 {
    let clamped = clamp(pixel, vec2<i32>(0), vec2<i32>(i32(window.canvas_width) - 1, i32(window.canvas_height) - 1));
    return u32(clamped.y) * window.canvas_width + u32(clamped.x);
}

fn column_height(local: vec2<u32>) -> f32 {
    var height = 0.0;
    for (var z = 0u; z < window.depth; z++) {
        height += f32(volume[window_cell(vec3<u32>(local, z))]) / PAINT_SCALE;
    }
    return height;
}

fn window_slot(pixel: vec2<i32>) -> u32 {
    let local = vec2<u32>(pixel - window.origin);
    return local.y * window.width + local.x;
}

fn live_height(pixel: vec2<i32>) -> f32 {
    if (window_contains(pixel)) { return column_height(vec2<u32>(pixel - window.origin)); }
    return f32(store[store_cell(vec2<u32>(pixel))]) / PAINT_SCALE;
}

fn laid_by_this_pass(index: u32) -> bool {
    return tube.arc - marks[index].x < tube.memory;
}

fn bed(index: u32) -> f32 {
    return ground[index] - relief[index];
}

fn resting_surface(pixel: vec2<i32>) -> f32 {
    let index = canvas_index(pixel);
    if (laid_by_this_pass(index)) { return marks[index].y; }
    return bed(index) + live_height(pixel);
}

fn floor_under(pixel: vec2<i32>) -> f32 {
    var total = 0.0;
    for (var dy = -RESTING_REACH; dy <= RESTING_REACH; dy++) {
        for (var dx = -RESTING_REACH; dx <= RESTING_REACH; dx++) {
            total += resting_surface(pixel + vec2<i32>(dx, dy));
        }
    }
    return total / f32((2 * RESTING_REACH + 1) * (2 * RESTING_REACH + 1));
}

fn lowest_around(centre: vec2<f32>, radius: f32) -> f32 {
    let origin = vec2<i32>(floor(centre));
    var lowest = floor_under(origin);
    for (var direction = 0; direction < 8; direction++) {
        let angle = f32(direction) * 0.7853982;
        let step = vec2<f32>(cos(angle), sin(angle)) * 2.0;
        for (var reach = 2.0; reach <= radius; reach += 2.0) {
            let pixel = vec2<i32>(floor(centre + step * (reach * 0.5)));
            if (on_canvas(pixel)) { lowest = min(lowest, resting_surface(pixel)); }
        }
    }
    return lowest;
}

fn nearest_on_path(p: vec2<f32>) -> vec2<f32> {
    let along = tube.path_end - tube.path_start;
    let span = dot(along, along);
    let t = select(0.0, clamp(dot(p - tube.path_start, along) / span, 0.0, 1.0), span > 1e-6);
    return tube.path_start + along * t;
}

fn walled_off(source: vec2<f32>, p: vec2<f32>, cap: f32) -> bool {
    let steps = i32(ceil(distance(source, p)));
    for (var i = 1; i < steps; i++) {
        let sample = vec2<i32>(floor(mix(source, p, f32(i) / f32(steps))));
        if (!on_canvas(sample)) { continue; }
        let index = canvas_index(sample);
        if (!laid_by_this_pass(index) && ground[index] + heights[index] > cap) { return true; }
    }
    return false;
}

fn reachable_level(source: vec2<f32>, p: vec2<f32>, core: f32, drape: f32, fresh: bool, floor_here: f32, profile: f32, free_surface: f32) -> f32 {
    if (profile <= 0.0) { return NO_TARGET; }
    if (distance(source, p) < core) { return max(min(floor_here + profile, free_surface), floor_here + profile * drape); }
    if (fresh && walled_off(source, p, free_surface)) { return NO_TARGET; }
    return min(floor_here + profile, free_surface);
}

fn missing_under(level: f32, pixel: vec2<i32>) -> f32 {
    return level - ground[canvas_index(pixel)] - column_height(vec2<u32>(pixel - window.origin));
}

fn cap_profile(offset: f32, half_width: f32, height: f32) -> f32 {
    if (offset >= half_width) { return 0.0; }
    let normalised = offset / half_width;
    return height * sqrt(1.0 - normalised * normalised);
}

fn make_room_on_top(local: vec2<u32>) {
    let bottom = window_cell(vec3<u32>(local, 0u));
    let above_bottom = window_cell(vec3<u32>(local, 1u));
    for (var channel = 0u; channel < CHANNELS; channel++) {
        volume[bottom + channel] += volume[above_bottom + channel];
    }
    for (var z = 1u; z + 1u < window.depth; z++) {
        let cell = window_cell(vec3<u32>(local, z));
        let above = window_cell(vec3<u32>(local, z + 1u));
        for (var channel = 0u; channel < CHANNELS; channel++) {
            volume[cell + channel] = volume[above + channel];
        }
    }
    let top = window_cell(vec3<u32>(local, window.depth - 1u));
    for (var channel = 0u; channel < CHANNELS; channel++) {
        volume[top + channel] = 0;
    }
}

fn free_cell(local: vec2<u32>) -> u32 {
    for (var z = 0u; z < window.depth; z++) {
        let cell = window_cell(vec3<u32>(local, z));
        if (f32(volume[cell]) / PAINT_SCALE < FULL_CELL) { return cell; }
    }
    make_room_on_top(local);
    return window_cell(vec3<u32>(local, window.depth - 1u));
}

fn add_on_top(local: vec2<u32>, amount: f32) {
    let cell = free_cell(local);
    let amounts = array<f32, 8>(amount, amount * tube.absorption.x, amount * tube.absorption.y, amount * tube.absorption.z, amount * tube.scattering.x, amount * tube.scattering.y, amount * tube.scattering.z, amount * tube.absorption.w);
    for (var channel = 0u; channel < CHANNELS; channel++) {
        volume[cell + channel] += i32(round(amounts[channel] * PAINT_SCALE));
    }
}

@compute @workgroup_size(1)
fn offer() {
    let budget = atomicLoad(&ledger.budget);
    let spent = min(budget, atomicLoad(&ledger.missing));
    atomicStore(&ledger.budget, min(budget - spent + u32(tube.squeezed * LEDGER_SCALE), u32(tube.backlog_limit * LEDGER_SCALE)));
    atomicStore(&ledger.missing, 0u);
}

@compute @workgroup_size(16, 16)
fn plan(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local_index: u32) {
    if (local_index == 0u) {
        anchor_floors = vec2<f32>(lowest_around(tube.path_end, tube.strand.x), lowest_around(tube.path_end, tube.mound.x));
    }
    workgroupBarrier();
    let pixel = tube.corner + vec2<i32>(id.xy);
    if (!on_canvas(pixel) || !window_contains(pixel)) { return; }
    let slot = window_slot(pixel);
    let p = vec2<f32>(pixel) + 0.5;
    let path_point = nearest_on_path(p);
    let strand = cap_profile(distance(p, path_point), tube.strand.x, tube.strand.y);
    let mound = cap_profile(distance(p, tube.path_end), tube.mound.x, tube.mound.y);
    if (strand <= 0.0 && mound <= 0.0) {
        planned[slot].y = NO_TARGET;
        return;
    }
    let floor_here = floor_under(pixel);
    let fresh = !laid_by_this_pass(canvas_index(pixel));
    let strand_level = reachable_level(path_point, p, tube.strand_core, tube.strand_drape, fresh, floor_here, strand, anchor_floors.x + tube.strand.y);
    let mound_level = reachable_level(tube.path_end, p, tube.mound_core, tube.mound_drape, fresh, floor_here, mound, anchor_floors.y + tube.mound.y);
    let level = max(strand_level, mound_level);
    planned[slot] = vec2<f32>(resting_surface(pixel), level);
    if (level > NO_TARGET) { atomicAdd(&ledger.missing, u32(max(missing_under(level, pixel), 0.0) * LEDGER_SCALE)); }
}

@compute @workgroup_size(16, 16)
fn fill(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = tube.corner + vec2<i32>(id.xy);
    if (!on_canvas(pixel) || !window_contains(pixel)) { return; }
    let plan = planned[window_slot(pixel)];
    if (plan.y <= NO_TARGET) { return; }
    let wanted = f32(atomicLoad(&ledger.missing));
    if (wanted <= 0.0) { return; }
    let share = min(f32(atomicLoad(&ledger.budget)) / wanted, 1.0);
    let missing = missing_under(plan.y, pixel) * share;
    if (missing <= 0.0) { return; }
    marks[canvas_index(pixel)] = vec2<f32>(tube.arc, plan.x);
    add_on_top(vec2<u32>(pixel - window.origin), missing);
}
