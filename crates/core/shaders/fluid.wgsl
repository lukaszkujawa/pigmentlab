const LIQUID_THRESHOLD: f32 = 0.05;
const ADHERED_LAYERS: i32 = 2;
const ADHESION_CELLS: i32 = 2;
const JUMP_DISTANCE: f32 = 0.4 * PIXELS_PER_CM;
const WORKGROUP: u32 = 64u;

fn linear_index(id: vec3<u32>, groups: vec3<u32>, workgroup_size: u32) -> u32 {
    return id.x + id.y * groups.x * workgroup_size;
}

struct FluidUniform {
    dt: f32,
    damping: f32,
    viscosity: f32,
    dryness_rate: f32,
    sample_count: u32,
    samples_per_bristle: u32,
    advection_dt: f32,
    gravity: f32,
    yield_speed: f32,
    padding_0: f32,
    padding_1: f32,
    padding_2: f32,
};

@group(0) @binding(0) var<uniform> fluid: FluidUniform;
@group(0) @binding(1) var<uniform> window: WindowUniform;
@group(0) @binding(2) var<storage, read_write> volume: array<i32>;
@group(0) @binding(3) var<storage, read_write> volume_next: array<i32>;
@group(0) @binding(4) var<storage, read_write> velocity: array<f32>;
@group(0) @binding(5) var<storage, read_write> velocity_next: array<f32>;
@group(0) @binding(6) var<storage, read_write> pressure: array<f32>;
@group(0) @binding(7) var<storage, read_write> pressure_next: array<f32>;
@group(0) @binding(8) var<storage, read_write> divergence: array<f32>;
@group(0) @binding(9) var<storage, read_write> bristle: array<atomic<i32>>;
@group(0) @binding(11) var<storage, read> curves: array<vec4<f32>>;
@group(0) @binding(12) var<storage, read> previous_curves: array<vec4<f32>>;

fn cell_index(c: vec3<u32>) -> u32 {
    return (c.z * window.height + c.y) * window.width + c.x;
}

fn cell_count() -> u32 {
    return window.width * window.height * window.depth;
}

fn cell_from_index(index: u32) -> vec3<u32> {
    let x = index % window.width;
    let y = (index / window.width) % window.height;
    let z = index / (window.width * window.height);
    return vec3<u32>(x, y, z);
}

fn u_index(i: u32, j: u32, k: u32) -> u32 {
    return (k * window.height + j) * (window.width + 1u) + i;
}

fn v_index(i: u32, j: u32, k: u32) -> u32 {
    return (window.width + 1u) * window.height * window.depth + (k * (window.height + 1u) + j) * window.width + i;
}

fn w_index(i: u32, j: u32, k: u32) -> u32 {
    return (window.width + 1u) * window.height * window.depth + window.width * (window.height + 1u) * window.depth + (k * window.height + j) * window.width + i;
}

fn inside(c: vec3<i32>) -> bool {
    return c.x >= 0 && c.y >= 0 && c.z >= 0 && c.x < i32(window.width) && c.y < i32(window.height) && c.z < i32(window.depth);
}

fn density_at(c: vec3<i32>) -> f32 {
    if (!inside(c)) { return 0.0; }
    return f32(volume[cell_index(vec3<u32>(c)) * CHANNELS]) / PAINT_SCALE;
}

fn is_dry(c: vec3<u32>) -> bool {
    return f32(volume[cell_index(c) * CHANNELS + 7u]) / PAINT_SCALE >= 1.0;
}

fn bristle_density(c: vec3<u32>) -> f32 {
    return f32(atomicLoad(&bristle[cell_index(c) * 4u])) / PAINT_SCALE;
}

fn bristle_velocity(c: vec3<u32>) -> vec3<f32> {
    let base = cell_index(c) * 4u;
    let density = f32(atomicLoad(&bristle[base]));
    if (density <= 0.0) { return vec3<f32>(0.0); }
    return vec3<f32>(f32(atomicLoad(&bristle[base + 1u])), f32(atomicLoad(&bristle[base + 2u])), f32(atomicLoad(&bristle[base + 3u]))) / density;
}

fn on_canvas(c: vec3<i32>) -> bool {
    let pixel = c.xy + window.origin;
    return pixel.x >= 0 && pixel.y >= 0 && pixel.x < i32(window.canvas_width) && pixel.y < i32(window.canvas_height);
}

fn is_solid(c: vec3<i32>) -> bool {
    if (!inside(c)) { return true; }
    if (!on_canvas(c)) { return true; }
    if (c.z < ADHERED_LAYERS) { return true; }
    let cell = vec3<u32>(c);
    return bristle_density(cell) > 0.0 || is_dry(cell);
}

fn joint_density_at(c: vec3<i32>) -> f32 {
    if (!inside(c)) { return 0.0; }
    let cell = cell_index(vec3<u32>(c));
    return (f32(volume[cell * CHANNELS]) + f32(atomicLoad(&particle_volume[cell * CHANNELS]))) / PAINT_SCALE;
}

fn is_liquid(c: vec3<i32>) -> bool {
    return inside(c) && joint_density_at(c) > LIQUID_THRESHOLD && !is_solid(c);
}

fn sample_u(p: vec3<f32>) -> f32 {
    let q = clamp(p - vec3<f32>(0.0, 0.5, 0.5), vec3<f32>(0.0), vec3<f32>(f32(window.width), f32(window.height - 1u), f32(window.depth - 1u)) - 1e-3);
    let base = vec3<u32>(floor(q));
    let f = q - floor(q);
    var result = 0.0;
    for (var corner = 0u; corner < 8u; corner++) {
        let offset = vec3<u32>(corner & 1u, (corner >> 1u) & 1u, (corner >> 2u) & 1u);
        let weight = mix(1.0 - f, f, vec3<f32>(offset));
        let i = min(base.x + offset.x, window.width);
        let j = min(base.y + offset.y, window.height - 1u);
        let k = min(base.z + offset.z, window.depth - 1u);
        result += velocity[u_index(i, j, k)] * weight.x * weight.y * weight.z;
    }
    return result;
}

fn sample_v(p: vec3<f32>) -> f32 {
    let q = clamp(p - vec3<f32>(0.5, 0.0, 0.5), vec3<f32>(0.0), vec3<f32>(f32(window.width - 1u), f32(window.height), f32(window.depth - 1u)) - 1e-3);
    let base = vec3<u32>(floor(q));
    let f = q - floor(q);
    var result = 0.0;
    for (var corner = 0u; corner < 8u; corner++) {
        let offset = vec3<u32>(corner & 1u, (corner >> 1u) & 1u, (corner >> 2u) & 1u);
        let weight = mix(1.0 - f, f, vec3<f32>(offset));
        let i = min(base.x + offset.x, window.width - 1u);
        let j = min(base.y + offset.y, window.height);
        let k = min(base.z + offset.z, window.depth - 1u);
        result += velocity[v_index(i, j, k)] * weight.x * weight.y * weight.z;
    }
    return result;
}

fn sample_w(p: vec3<f32>) -> f32 {
    let q = clamp(p - vec3<f32>(0.5, 0.5, 0.0), vec3<f32>(0.0), vec3<f32>(f32(window.width - 1u), f32(window.height - 1u), f32(window.depth)) - 1e-3);
    let base = vec3<u32>(floor(q));
    let f = q - floor(q);
    var result = 0.0;
    for (var corner = 0u; corner < 8u; corner++) {
        let offset = vec3<u32>(corner & 1u, (corner >> 1u) & 1u, (corner >> 2u) & 1u);
        let weight = mix(1.0 - f, f, vec3<f32>(offset));
        let i = min(base.x + offset.x, window.width - 1u);
        let j = min(base.y + offset.y, window.height - 1u);
        let k = min(base.z + offset.z, window.depth);
        result += velocity[w_index(i, j, k)] * weight.x * weight.y * weight.z;
    }
    return result;
}

fn sample_velocity(p: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(sample_u(p), sample_v(p), sample_w(p));
}

fn curve_point(previous: bool, bristle_index: u32, t: f32) -> vec3<f32> {
    let base = bristle_index * 4u;
    if (previous) {
        return hermite(previous_curves[base].xyz, previous_curves[base + 1u].xyz, previous_curves[base + 2u].xyz, previous_curves[base + 3u].xyz, t);
    }
    return hermite(curves[base].xyz, curves[base + 1u].xyz, curves[base + 2u].xyz, curves[base + 3u].xyz, t);
}

@compute @workgroup_size(128)
fn grow_region(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let sample = linear_index(id, groups, 128u);
    if (sample >= fluid.sample_count) { return; }
    let bristle_index = sample / fluid.samples_per_bristle;
    let t = f32(sample % fluid.samples_per_bristle) / f32(fluid.samples_per_bristle - 1u);
    let base = bristle_index * 4u;
    let point = hermite(curves[base].xyz, curves[base + 1u].xyz, curves[base + 2u].xyz, curves[base + 3u].xyz, t);
    let local = vec3<i32>(floor(vec3<f32>(point.xy - vec2<f32>(window.origin), point.z)));
    if (!inside(vec3<i32>(local.xy, 0))) { return; }
    atomicMin(&region[0], local.x);
    atomicMin(&region[1], local.y);
    atomicMax(&region[3], local.x);
    atomicMax(&region[4], local.y);
}

const BASE_SCALE: f32 = 256.0;

@compute @workgroup_size(1)
fn finalize_bounds() {
    let low = vec2<i32>(atomicLoad(&region[0]), atomicLoad(&region[1]));
    let high = vec2<i32>(atomicLoad(&region[3]), atomicLoad(&region[4]));
    let engaged = low.x <= high.x && low.y <= high.y;
    let margin = vec2<i32>(REGION_MARGIN);
    let clamped_low = max(low - margin, vec2<i32>(0));
    let clamped_high = min(high + margin, vec2<i32>(i32(window.width) - 1, i32(window.height) - 1));
    let size = select(vec2<i32>(0), clamped_high - clamped_low + 1, engaged);
    atomicStore(&region[0], clamped_low.x);
    atomicStore(&region[1], clamped_low.y);
    atomicStore(&region[2], 0);
    atomicStore(&region[3], size.x);
    atomicStore(&region[4], size.y);
    atomicStore(&region[5], 0);
    atomicStore(&region[6], 0);
    let column_groups = (u32(size.x * size.y) + 63u) / 64u;
    indirect[6] = min(column_groups, 65535u);
    indirect[7] = (column_groups + 65534u) / 65535u;
    indirect[8] = 1u;
}

@compute @workgroup_size(64)
fn scan_region(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 64u);
    let size = region_size();
    if (index >= size.x * size.y) { return; }
    let column = vec2<i32>(region_min().xy) + vec2<i32>(i32(index % size.x), i32(index / size.x));
    let pixel = column + window.origin;
    if (pixel.x < 0 || pixel.y < 0 || pixel.x >= i32(window.canvas_width) || pixel.y >= i32(window.canvas_height)) { return; }
    let base = stroke_base[u32(pixel.y) * window.canvas_width + u32(pixel.x)];
    atomicMax(&region[6], i32(base * BASE_SCALE));
}

@compute @workgroup_size(1)
fn finalize_depth() {
    let tallest = f32(atomicLoad(&region[6])) / BASE_SCALE;
    let engaged = tallest > f32(ADHERED_LAYERS) && atomicLoad(&region[3]) > 0;
    let depth = select(0, clamp(i32(ceil(tallest)) + 2, 1, i32(window.depth)), engaged);
    let size = vec3<i32>(atomicLoad(&region[3]), atomicLoad(&region[4]), depth);
    atomicStore(&region[5], depth);
    atomicStore(&region[6], select(0, 1, engaged));
    let cells = u32(size.x * size.y * size.z);
    let slots = u32((size.x + 1) * (size.y + 1) * (size.z + 1)) * u32(engaged);
    let cell_groups = (cells + 63u) / 64u;
    let slot_groups = (slots + 63u) / 64u;
    indirect[0] = min(cell_groups, 65535u);
    indirect[1] = (cell_groups + 65534u) / 65535u;
    indirect[2] = 1u;
    indirect[3] = min(slot_groups, 65535u);
    indirect[4] = (slot_groups + 65534u) / 65535u;
    indirect[5] = 1u;
    let column_groups = (u32(size.x * size.y) + 63u) / 64u * u32(engaged);
    indirect[6] = min(column_groups, 65535u);
    indirect[7] = (column_groups + 65534u) / 65535u;
    indirect[8] = 1u;
}

@compute @workgroup_size(128)
fn rasterize_bristles(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let sample = linear_index(id, groups, 128u);
    if (sample >= fluid.sample_count || !region_active()) { return; }
    let bristle_index = sample / fluid.samples_per_bristle;
    let t = f32(sample % fluid.samples_per_bristle) / f32(fluid.samples_per_bristle - 1u);
    let point = above_canvas(curve_point(false, bristle_index, t));
    let previous = above_canvas(curve_point(true, bristle_index, t));
    var motion = (point - previous) / fluid.dt;
    if (distance(point, previous) > JUMP_DISTANCE) { motion = vec3<f32>(0.0); }
    let local = vec3<i32>(floor(vec3<f32>(point.xy - vec2<f32>(window.origin), point.z)));
    for (var dz = -ADHESION_CELLS; dz <= ADHESION_CELLS; dz++) {
        for (var dy = -ADHESION_CELLS; dy <= ADHESION_CELLS; dy++) {
            for (var dx = -ADHESION_CELLS; dx <= ADHESION_CELLS; dx++) {
                let cell = local + vec3<i32>(dx, dy, dz);
                if (!inside(cell)) { continue; }
                let base = cell_index(vec3<u32>(cell)) * 4u;
                atomicAdd(&bristle[base], i32(PAINT_SCALE));
                atomicAdd(&bristle[base + 1u], i32(round(motion.x * PAINT_SCALE)));
                atomicAdd(&bristle[base + 2u], i32(round(motion.y * PAINT_SCALE)));
                atomicAdd(&bristle[base + 3u], i32(round(motion.z * PAINT_SCALE)));
            }
        }
    }
}

fn source_channels(from_next: bool, c: vec3<u32>) -> array<f32, 7> {
    let base = cell_index(c) * CHANNELS;
    var values: array<f32, 7>;
    for (var channel = 0u; channel < 7u; channel++) {
        values[channel] = f32(select(volume[base + channel], volume_next[base + channel], from_next));
    }
    return values;
}

fn cell_faces(c: vec3<u32>) -> array<f32, 6> {
    return array<f32, 6>(
        velocity[u_index(c.x, c.y, c.z)], velocity[u_index(c.x + 1u, c.y, c.z)],
        velocity[v_index(c.x, c.y, c.z)], velocity[v_index(c.x, c.y + 1u, c.z)],
        velocity[w_index(c.x, c.y, c.z)], velocity[w_index(c.x, c.y, c.z + 1u)],
    );
}

fn outflow_scale(c: vec3<u32>) -> f32 {
    let faces = cell_faces(c);
    var outflow = 0.0;
    for (var face = 0u; face < 6u; face++) {
        let lower = (face & 1u) == 0u;
        let outgoing = select(faces[face] > 0.0, faces[face] < 0.0, lower);
        if (outgoing) { outflow += abs(faces[face]) * fluid.advection_dt; }
    }
    return min(1.0, 1.0 / max(outflow, 1e-6));
}

fn advect_cell(index: u32, from_next: bool) {
    if (index >= region_cell_count()) { return; }
    let c = region_cell(index);
    let dt = fluid.advection_dt;
    let faces = cell_faces(c);
    let own = source_channels(from_next, c);
    let own_scale = outflow_scale(c);
    var result = own;
    var outflow = 0.0;
    for (var face = 0u; face < 6u; face++) {
        let speed = faces[face];
        let lower = (face & 1u) == 0u;
        let incoming = select((speed < 0.0), (speed > 0.0), lower);
        let fraction = abs(speed) * dt;
        if (fraction <= 0.0) { continue; }
        if (incoming) {
            let axis = face / 2u;
            var neighbour = vec3<i32>(c);
            neighbour[axis] += select(1, -1, lower);
            if (!inside(neighbour)) { continue; }
            let values = source_channels(from_next, vec3<u32>(neighbour));
            let share = fraction * outflow_scale(vec3<u32>(neighbour));
            for (var channel = 0u; channel < 7u; channel++) {
                result[channel] += values[channel] * share;
            }
        } else {
            outflow += fraction * own_scale;
        }
    }
    for (var channel = 0u; channel < 7u; channel++) {
        result[channel] -= own[channel] * min(outflow, 1.0);
    }
    let base = cell_index(c) * CHANNELS;
    for (var channel = 0u; channel < 7u; channel++) {
        let value = i32(round(max(result[channel], 0.0)));
        if (from_next) { volume[base + channel] = value; } else { volume_next[base + channel] = value; }
    }
    if (!from_next) { volume_next[base + 7u] = volume[base + 7u]; }
}

@compute @workgroup_size(64)
fn advect_forward(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    advect_cell(linear_index(id, groups, 64u), false);
}

@compute @workgroup_size(64)
fn advect_backward(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    advect_cell(linear_index(id, groups, 64u), true);
}

fn face_velocity(solid_left: bool, solid_right: bool, left: vec3<i32>, right: vec3<i32>, component: u32, current: f32) -> f32 {
    if (!inside(left) || !inside(right) || !on_canvas(left) || !on_canvas(right)) { return 0.0; }
    if (is_dry(vec3<u32>(left)) || is_dry(vec3<u32>(right))) { return 0.0; }
    if (left.z < ADHERED_LAYERS || right.z < ADHERED_LAYERS) { return 0.0; }
    if (!below_stroke_base(left) || !below_stroke_base(right)) { return 0.0; }
    let liquid_left = joint_density_at(left) > LIQUID_THRESHOLD;
    let liquid_right = joint_density_at(right) > LIQUID_THRESHOLD;
    if (!liquid_left && !liquid_right) { return 0.0; }
    if (solid_left || solid_right) {
        if (!liquid_left || !liquid_right) { return 0.0; }
        var prescribed = vec3<f32>(0.0);
        if (solid_left && inside(left) && bristle_density(vec3<u32>(left)) > 0.0) {
            prescribed = bristle_velocity(vec3<u32>(left));
        } else if (solid_right && inside(right) && bristle_density(vec3<u32>(right)) > 0.0) {
            prescribed = bristle_velocity(vec3<u32>(right));
        }
        prescribed.z = min(prescribed.z, 0.0);
        return prescribed[component];
    }
    let settle = select(0.0, fluid.gravity * fluid.dt, component == 2u);
    let damped = current * fluid.damping;
    return select(damped, 0.0, abs(damped) < fluid.yield_speed) - settle;
}

@compute @workgroup_size(64)
fn apply_boundaries(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 64u);
    if (index >= region_face_slots()) { return; }
    let c = region_face_slot(index);
    let i = u32(c.x);
    let j = u32(c.y);
    let k = u32(c.z);
    if (j < window.height && k < window.depth) {
        let left = c - vec3<i32>(1, 0, 0);
        let at = u_index(i, j, k);
        velocity[at] = face_velocity(is_solid(left), is_solid(c), left, c, 0u, velocity[at]);
    }
    if (i < window.width && k < window.depth) {
        let left = c - vec3<i32>(0, 1, 0);
        let at = v_index(i, j, k);
        velocity[at] = face_velocity(is_solid(left), is_solid(c), left, c, 1u, velocity[at]);
    }
    if (i < window.width && j < window.height) {
        let left = c - vec3<i32>(0, 0, 1);
        let at = w_index(i, j, k);
        velocity[at] = face_velocity(is_solid(left), is_solid(c), left, c, 2u, velocity[at]);
    }
}

fn diffuse_face(at: u32, position: vec3<i32>, extent: vec3<u32>, stride: vec3<u32>, source_is_next: bool) {
    let coefficient = fluid.viscosity * fluid.dt;
    let offsets = array<vec3<i32>, 6>(vec3<i32>(-1, 0, 0), vec3<i32>(1, 0, 0), vec3<i32>(0, -1, 0), vec3<i32>(0, 1, 0), vec3<i32>(0, 0, -1), vec3<i32>(0, 0, 1));
    let origin_value = select(velocity[at], velocity_next[at], source_is_next);
    var neighbours = 0.0;
    var count = 0.0;
    for (var n = 0u; n < 6u; n++) {
        let neighbour_position = position + offsets[n];
        if (neighbour_position.x < 0 || neighbour_position.y < 0 || neighbour_position.z < 0 || neighbour_position.x >= i32(extent.x) || neighbour_position.y >= i32(extent.y) || neighbour_position.z >= i32(extent.z)) { continue; }
        let neighbour = at + u32(offsets[n].x) * stride.x + u32(offsets[n].y) * stride.y + u32(offsets[n].z) * stride.z;
        neighbours += select(velocity[neighbour], velocity_next[neighbour], source_is_next);
        count += 1.0;
    }
    let result = (origin_value + coefficient * neighbours) / (1.0 + coefficient * count);
    if (source_is_next) { velocity[at] = result; } else { velocity_next[at] = result; }
}

fn diffuse_slot(index: u32, source_is_next: bool) {
    if (index >= region_face_slots()) { return; }
    let c = region_face_slot(index);
    let i = u32(c.x);
    let j = u32(c.y);
    let k = u32(c.z);
    if (j < window.height && k < window.depth) {
        let extent = vec3<u32>(window.width + 1u, window.height, window.depth);
        diffuse_face(u_index(i, j, k), c, extent, vec3<u32>(1u, extent.x, extent.x * extent.y), source_is_next);
    }
    if (i < window.width && k < window.depth) {
        let extent = vec3<u32>(window.width, window.height + 1u, window.depth);
        diffuse_face(v_index(i, j, k), c, extent, vec3<u32>(1u, extent.x, extent.x * extent.y), source_is_next);
    }
    if (i < window.width && j < window.height) {
        let extent = vec3<u32>(window.width, window.height, window.depth + 1u);
        diffuse_face(w_index(i, j, k), c, extent, vec3<u32>(1u, extent.x, extent.x * extent.y), source_is_next);
    }
}

@compute @workgroup_size(64)
fn diffuse_forward(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    diffuse_slot(linear_index(id, groups, 64u), false);
}

@compute @workgroup_size(64)
fn diffuse_backward(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    diffuse_slot(linear_index(id, groups, 64u), true);
}

@compute @workgroup_size(64)
fn compute_divergence(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let slot = linear_index(id, groups, 64u);
    if (slot >= region_cell_count()) { return; }
    let c = region_cell(slot);
    let index = cell_index(c);
    if (!is_liquid(vec3<i32>(c))) {
        divergence[index] = 0.0;
        pressure[index] = 0.0;
        return;
    }
    divergence[index] = velocity[u_index(c.x + 1u, c.y, c.z)] - velocity[u_index(c.x, c.y, c.z)]
        + velocity[v_index(c.x, c.y + 1u, c.z)] - velocity[v_index(c.x, c.y, c.z)]
        + velocity[w_index(c.x, c.y, c.z + 1u)] - velocity[w_index(c.x, c.y, c.z)];
}

fn jacobi_step(slot: u32, source_is_next: bool) {
    if (slot >= region_cell_count()) { return; }
    let c = vec3<i32>(region_cell(slot));
    let index = cell_index(vec3<u32>(c));
    if (!is_liquid(c)) {
        if (source_is_next) { pressure[index] = 0.0; } else { pressure_next[index] = 0.0; }
        return;
    }
    let own = select(pressure[index], pressure_next[index], source_is_next);
    let offsets = array<vec3<i32>, 6>(vec3<i32>(-1, 0, 0), vec3<i32>(1, 0, 0), vec3<i32>(0, -1, 0), vec3<i32>(0, 1, 0), vec3<i32>(0, 0, -1), vec3<i32>(0, 0, 1));
    var sum = 0.0;
    for (var n = 0u; n < 6u; n++) {
        let neighbour = c + offsets[n];
        if (is_solid(neighbour)) {
            sum += own;
        } else if (is_liquid(neighbour)) {
            let ni = cell_index(vec3<u32>(neighbour));
            sum += select(pressure[ni], pressure_next[ni], source_is_next);
        }
    }
    let result = (sum - divergence[index]) / 6.0;
    if (source_is_next) { pressure[index] = result; } else { pressure_next[index] = result; }
}

@compute @workgroup_size(64)
fn jacobi_forward(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    jacobi_step(linear_index(id, groups, 64u), false);
}

@compute @workgroup_size(64)
fn jacobi_backward(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    jacobi_step(linear_index(id, groups, 64u), true);
}

fn pressure_at(c: vec3<i32>) -> f32 {
    if (!is_liquid(c)) { return 0.0; }
    return pressure[cell_index(vec3<u32>(c))];
}

@compute @workgroup_size(64)
fn apply_gradient(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 64u);
    if (index >= region_face_slots()) { return; }
    let c = region_face_slot(index);
    let i = u32(c.x);
    let j = u32(c.y);
    let k = u32(c.z);
    let axes = array<vec3<i32>, 3>(vec3<i32>(1, 0, 0), vec3<i32>(0, 1, 0), vec3<i32>(0, 0, 1));
    for (var axis = 0u; axis < 3u; axis++) {
        let in_range = select(select(i < window.width && j < window.height, i < window.width && k < window.depth, axis == 1u), j < window.height && k < window.depth, axis == 0u);
        if (!in_range) { continue; }
        let left = c - axes[axis];
        if (is_solid(left) || is_solid(c)) { continue; }
        if (!is_liquid(left) && !is_liquid(c)) { continue; }
        var at: u32;
        if (axis == 0u) { at = u_index(i, j, k); } else if (axis == 1u) { at = v_index(i, j, k); } else { at = w_index(i, j, k); }
        velocity[at] -= pressure_at(c) - pressure_at(left);
    }
}

@compute @workgroup_size(64)
fn dry(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let slot = linear_index(id, groups, 64u);
    if (slot >= region_cell_count()) { return; }
    let base = cell_index(region_cell(slot)) * CHANNELS;
    if (volume[base] <= 0) { return; }
    volume[base + 7u] = min(volume[base + 7u] + i32(round(fluid.dryness_rate * fluid.dt * PAINT_SCALE)), i32(PAINT_SCALE));
}

@compute @workgroup_size(64)
fn dry_all(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 64u);
    if (index >= cell_count()) { return; }
    let base = index * CHANNELS;
    if (volume[base] <= 0) { return; }
    volume[base + 7u] = i32(PAINT_SCALE);
}

const PARTICLES_PER_AXIS: u32 = 2u;
const COARSE_CELL: u32 = 16u;
const PIC_WEIGHT: f32 = 0.8;
const CANVAS_FRICTION_RANGE: f32 = 0.2 * PIXELS_PER_CM;
const CANVAS_FRICTION: f32 = 0.5;
const RETURN_SPEED: f32 = 0.5 * PIXELS_PER_CM;

struct Particle {
    position: vec4<f32>,
    velocity: vec4<f32>,
    absorption: vec4<f32>,
    scattering: vec4<f32>,
};

@group(0) @binding(13) var<storage, read_write> particles_in: array<Particle>;
@group(0) @binding(14) var<storage, read_write> particles_out: array<Particle>;
@group(0) @binding(15) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(16) var<storage, read_write> particle_volume: array<atomic<i32>>;
@group(0) @binding(17) var<storage, read_write> particle_velocity: array<atomic<i32>>;
@group(0) @binding(18) var<storage, read_write> coarse_flags: array<atomic<u32>>;
@group(0) @binding(19) var<storage, read_write> velocity_before: array<f32>;
@group(0) @binding(20) var<storage, read_write> volume_atomic: array<atomic<i32>>;
@group(0) @binding(21) var<storage, read_write> stroke_base: array<f32>;
@group(0) @binding(22) var<storage, read_write> region: array<atomic<i32>>;
@group(0) @binding(23) var<storage, read_write> indirect: array<u32>;
@group(0) @binding(24) var<storage, read_write> columns: array<vec4<f32>>;
@group(0) @binding(25) var<storage, read_write> column_pigment: array<vec4<f32>>;

const LEVEL_RATE: f32 = 0.5;
const LEVEL_YIELD_SLOPE: f32 = 0.25;
const LEVEL_MIN_HEIGHT: f32 = 0.03 * PIXELS_PER_CM;

const REGION_MARGIN: i32 = 10;

fn region_min() -> vec3<i32> {
    return vec3<i32>(atomicLoad(&region[0]), atomicLoad(&region[1]), atomicLoad(&region[2]));
}

fn region_size() -> vec3<u32> {
    return vec3<u32>(u32(atomicLoad(&region[3])), u32(atomicLoad(&region[4])), u32(atomicLoad(&region[5])));
}

fn region_active() -> bool {
    return atomicLoad(&region[6]) > 0;
}

fn region_cell_count() -> u32 {
    let size = region_size();
    return size.x * size.y * size.z;
}

fn region_cell(index: u32) -> vec3<u32> {
    let size = region_size();
    let local = vec3<u32>(index % size.x, (index / size.x) % size.y, index / (size.x * size.y));
    return vec3<u32>(region_min()) + local;
}

fn region_face_slots() -> u32 {
    let size = region_size() + 1u;
    return size.x * size.y * size.z;
}

fn region_face_slot(index: u32) -> vec3<i32> {
    let size = region_size() + 1u;
    let local = vec3<u32>(index % size.x, (index / size.x) % size.y, index / (size.x * size.y));
    return region_min() + vec3<i32>(local);
}

fn below_stroke_base(c: vec3<i32>) -> bool {
    let pixel = c.xy + window.origin;
    return f32(c.z) + 0.5 < stroke_base[u32(pixel.y) * window.canvas_width + u32(pixel.x)];
}

fn coarse_width() -> u32 {
    return (window.width + COARSE_CELL - 1u) / COARSE_CELL;
}

fn coarse_index(xy: vec2<u32>) -> u32 {
    return (xy.y / COARSE_CELL) * coarse_width() + xy.x / COARSE_CELL;
}

fn near_bristles(xy: vec2<u32>) -> bool {
    let cx = i32(xy.x / COARSE_CELL);
    let cy = i32(xy.y / COARSE_CELL);
    let columns = i32(coarse_width());
    let rows = i32((window.height + COARSE_CELL - 1u) / COARSE_CELL);
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let x = cx + dx;
            let y = cy + dy;
            if (x < 0 || y < 0 || x >= columns || y >= rows) { continue; }
            if (atomicLoad(&coarse_flags[u32(y) * u32(columns) + u32(x)]) > 0u) { return true; }
        }
    }
    return false;
}

@compute @workgroup_size(128)
fn flag_bristle_regions(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let sample = linear_index(id, groups, 128u);
    if (sample >= fluid.sample_count) { return; }
    let bristle_index = sample / fluid.samples_per_bristle;
    let t = f32(sample % fluid.samples_per_bristle) / f32(fluid.samples_per_bristle - 1u);
    let base = bristle_index * 4u;
    let point = hermite(curves[base].xyz, curves[base + 1u].xyz, curves[base + 2u].xyz, curves[base + 3u].xyz, t);
    let local = vec3<i32>(floor(vec3<f32>(point.xy - vec2<f32>(window.origin), point.z)));
    if (!inside(local)) { return; }
    atomicMax(&coarse_flags[coarse_index(vec2<u32>(local.xy))], 1u);
}

fn append_particle(particle: Particle) {
    let slot = atomicAdd(&counters[0], 1u);
    if (slot >= arrayLength(&particles_in)) {
        atomicSub(&counters[0], 1u);
        return;
    }
    particles_in[slot] = particle;
}

@compute @workgroup_size(64)
fn emit_particles(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 64u);
    if (index >= cell_count() || !region_active()) { return; }
    let c = cell_from_index(index);
    let base = index * CHANNELS;
    let density = f32(volume[base]) / PAINT_SCALE;
    if (density <= 0.0 || volume[base + 7u] >= i32(PAINT_SCALE) || !near_bristles(c.xy)) { return; }
    let centre = vec3<f32>(c) + 0.5;
    if (length(sample_velocity(centre)) < RETURN_SPEED) { return; }
    let per_cell = PARTICLES_PER_AXIS * PARTICLES_PER_AXIS * PARTICLES_PER_AXIS;
    let absorption = vec3<f32>(f32(volume[base + 1u]), f32(volume[base + 2u]), f32(volume[base + 3u])) / PAINT_SCALE / density;
    let scattering = vec3<f32>(f32(volume[base + 4u]), f32(volume[base + 5u]), f32(volume[base + 6u])) / PAINT_SCALE / density;
    let mass = density / f32(per_cell);
    for (var n = 0u; n < per_cell; n++) {
        let offset = vec3<u32>(n % PARTICLES_PER_AXIS, (n / PARTICLES_PER_AXIS) % PARTICLES_PER_AXIS, n / (PARTICLES_PER_AXIS * PARTICLES_PER_AXIS));
        let position = vec3<f32>(c) + (vec3<f32>(offset) + 0.5) / f32(PARTICLES_PER_AXIS);
        var particle: Particle;
        particle.position = vec4<f32>(position, mass);
        particle.velocity = vec4<f32>(sample_velocity(position), 0.0);
        particle.absorption = vec4<f32>(absorption, 0.0);
        particle.scattering = vec4<f32>(scattering, 0.0);
        append_particle(particle);
    }
    for (var channel = 0u; channel < 7u; channel++) {
        volume[base + channel] = 0;
    }
}

@compute @workgroup_size(128)
fn rasterize_particles(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 128u);
    if (index >= atomicLoad(&counters[0])) { return; }
    let particle = particles_in[index];
    let mass = particle.position.w;
    if (mass <= 0.0) { return; }
    let c = vec3<i32>(floor(particle.position.xyz));
    if (!inside(c)) { return; }
    let cell = cell_index(vec3<u32>(c));
    let base = cell * CHANNELS;
    atomicAdd(&particle_volume[base], i32(round(mass * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 1u], i32(round(mass * particle.absorption.x * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 2u], i32(round(mass * particle.absorption.y * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 3u], i32(round(mass * particle.absorption.z * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 4u], i32(round(mass * particle.scattering.x * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 5u], i32(round(mass * particle.scattering.y * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 6u], i32(round(mass * particle.scattering.z * PAINT_SCALE)));
    let momentum = cell * 4u;
    atomicAdd(&particle_velocity[momentum], i32(round(mass * PAINT_SCALE)));
    atomicAdd(&particle_velocity[momentum + 1u], i32(round(mass * particle.velocity.x * 256.0)));
    atomicAdd(&particle_velocity[momentum + 2u], i32(round(mass * particle.velocity.y * 256.0)));
    atomicAdd(&particle_velocity[momentum + 3u], i32(round(mass * particle.velocity.z * 256.0)));
}

fn splat_cell(c: vec3<i32>, weight: f32, particle: Particle) {
    if (!inside(c) || weight <= 0.0) { return; }
    let mass = particle.position.w * weight;
    let base = cell_index(vec3<u32>(c)) * CHANNELS;
    atomicAdd(&particle_volume[base], i32(round(mass * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 1u], i32(round(mass * particle.absorption.x * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 2u], i32(round(mass * particle.absorption.y * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 3u], i32(round(mass * particle.absorption.z * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 4u], i32(round(mass * particle.scattering.x * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 5u], i32(round(mass * particle.scattering.y * PAINT_SCALE)));
    atomicAdd(&particle_volume[base + 6u], i32(round(mass * particle.scattering.z * PAINT_SCALE)));
}

@compute @workgroup_size(128)
fn splat_particles(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 128u);
    if (index >= atomicLoad(&counters[0])) { return; }
    let particle = particles_in[index];
    if (particle.position.w <= 0.0) { return; }
    let q = particle.position.xyz - 0.5;
    let base = vec3<i32>(floor(q));
    let f = q - floor(q);
    for (var corner = 0u; corner < 8u; corner++) {
        let offset = vec3<i32>(i32(corner & 1u), i32((corner >> 1u) & 1u), i32((corner >> 2u) & 1u));
        let weight = mix(1.0 - f, f, vec3<f32>(offset));
        splat_cell(base + offset, weight.x * weight.y * weight.z, particle);
    }
}

fn particle_mass_at(c: vec3<i32>) -> f32 {
    if (!inside(c)) { return 0.0; }
    return f32(atomicLoad(&particle_volume[cell_index(vec3<u32>(c)) * CHANNELS])) / PAINT_SCALE;
}

fn particle_momentum(c: vec3<i32>, component: u32) -> vec2<f32> {
    if (!inside(c)) { return vec2<f32>(0.0); }
    let base = cell_index(vec3<u32>(c)) * 4u;
    let mass = f32(atomicLoad(&particle_velocity[base])) / PAINT_SCALE;
    let momentum = f32(atomicLoad(&particle_velocity[base + 1u + component])) / 256.0;
    return vec2<f32>(momentum, mass);
}

fn merged_face(index: u32, left: vec3<i32>, right: vec3<i32>, component: u32) {
    let a = particle_momentum(left, component);
    let b = particle_momentum(right, component);
    let particle_mass = a.y + b.y;
    if (particle_mass <= 0.0) { return; }
    let grid_mass = density_at(left) + density_at(right);
    let particle_speed = (a.x + b.x) / particle_mass;
    velocity[index] = (particle_speed * particle_mass + velocity[index] * grid_mass) / (particle_mass + grid_mass);
}

@compute @workgroup_size(64)
fn merge_particle_velocity(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let faces = (window.width + 1u) * (window.height + 1u) * (window.depth + 1u);
    let index = linear_index(id, groups, 64u);
    if (index >= faces) { return; }
    let i = index % (window.width + 1u);
    let j = (index / (window.width + 1u)) % (window.height + 1u);
    let k = index / ((window.width + 1u) * (window.height + 1u));
    let c = vec3<i32>(i32(i), i32(j), i32(k));
    if (j < window.height && k < window.depth) { merged_face(u_index(i, j, k), c - vec3<i32>(1, 0, 0), c, 0u); }
    if (i < window.width && k < window.depth) { merged_face(v_index(i, j, k), c - vec3<i32>(0, 1, 0), c, 1u); }
    if (i < window.width && j < window.height) { merged_face(w_index(i, j, k), c - vec3<i32>(0, 0, 1), c, 2u); }
}

fn sample_before(p: vec3<f32>) -> vec3<f32> {
    let q = clamp(p, vec3<f32>(0.5), vec3<f32>(f32(window.width), f32(window.height), f32(window.depth)) - 0.5);
    let i = u32(q.x);
    let j = u32(q.y);
    let k = u32(q.z);
    return vec3<f32>(
        0.5 * (velocity_before[u_index(i, j, k)] + velocity_before[u_index(i + 1u, j, k)]),
        0.5 * (velocity_before[v_index(i, j, k)] + velocity_before[v_index(i, j + 1u, k)]),
        0.5 * (velocity_before[w_index(i, j, k)] + velocity_before[w_index(i, j, k + 1u)]),
    );
}

@compute @workgroup_size(128)
fn update_particles(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 128u);
    if (index >= atomicLoad(&counters[0])) { return; }
    var particle = particles_in[index];
    if (particle.position.w <= 0.0) { return; }
    let position = particle.position.xyz;
    let c = vec3<i32>(floor(position));
    var velocity_now = particle.velocity.xyz;
    if (inside(c) && bristle_density(vec3<u32>(c)) > 0.0) {
        velocity_now = bristle_velocity(vec3<u32>(c));
    } else {
        let grid_now = sample_velocity(position);
        let grid_then = sample_before(position);
        velocity_now = PIC_WEIGHT * grid_now + (1.0 - PIC_WEIGHT) * (velocity_now + grid_now - grid_then);
        let friction = clamp(1.0 - position.z / CANVAS_FRICTION_RANGE, 0.0, 1.0);
        velocity_now *= 1.0 - friction * friction * CANVAS_FRICTION;
    }
    var moved = position + velocity_now * fluid.dt;
    moved = clamp(moved, vec3<f32>(0.01), vec3<f32>(f32(window.width), f32(window.height), f32(window.depth)) - 0.01);
    particle.position = vec4<f32>(moved, particle.position.w);
    particle.velocity = vec4<f32>(velocity_now, 0.0);
    particles_in[index] = particle;
}

fn return_particle(index: u32, force: bool) {
    if (index >= atomicLoad(&counters[0])) { return; }
    let particle = particles_in[index];
    let mass = particle.position.w;
    if (mass <= 0.0) { return; }
    let c = vec3<i32>(floor(particle.position.xyz));
    if (!inside(c)) { return; }
    let cell = vec3<u32>(c);
    let far_from_brush = !near_bristles(cell.xy) && bristle_density(cell) <= 0.0;
    let dry_cell = atomicLoad(&volume_atomic[cell_index(cell) * CHANNELS + 7u]) >= i32(PAINT_SCALE);
    if (!force && !far_from_brush && !dry_cell) { return; }
    let q = particle.position.xyz - 0.5;
    let corner_base = vec3<i32>(floor(q));
    let f = q - floor(q);
    var placed = 0.0;
    for (var corner = 0u; corner < 8u; corner++) {
        let offset = vec3<i32>(i32(corner & 1u), i32((corner >> 1u) & 1u), i32((corner >> 2u) & 1u));
        let target_cell = corner_base + offset;
        if (!inside(target_cell)) { continue; }
        let weight = mix(1.0 - f, f, vec3<f32>(offset));
        placed += weight.x * weight.y * weight.z;
    }
    for (var corner = 0u; corner < 8u; corner++) {
        let offset = vec3<i32>(i32(corner & 1u), i32((corner >> 1u) & 1u), i32((corner >> 2u) & 1u));
        let target_cell = corner_base + offset;
        if (!inside(target_cell)) { continue; }
        let weight = mix(1.0 - f, f, vec3<f32>(offset));
        let share = mass * weight.x * weight.y * weight.z / max(placed, 1e-6);
        let base = cell_index(vec3<u32>(target_cell)) * CHANNELS;
        atomicAdd(&volume_atomic[base], i32(round(share * PAINT_SCALE)));
        atomicAdd(&volume_atomic[base + 1u], i32(round(share * particle.absorption.x * PAINT_SCALE)));
        atomicAdd(&volume_atomic[base + 2u], i32(round(share * particle.absorption.y * PAINT_SCALE)));
        atomicAdd(&volume_atomic[base + 3u], i32(round(share * particle.absorption.z * PAINT_SCALE)));
        atomicAdd(&volume_atomic[base + 4u], i32(round(share * particle.scattering.x * PAINT_SCALE)));
        atomicAdd(&volume_atomic[base + 5u], i32(round(share * particle.scattering.y * PAINT_SCALE)));
        atomicAdd(&volume_atomic[base + 6u], i32(round(share * particle.scattering.z * PAINT_SCALE)));
    }
    particles_in[index].position.w = 0.0;
}

@compute @workgroup_size(128)
fn return_resting_particles(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    return_particle(linear_index(id, groups, 128u), false);
}

@compute @workgroup_size(128)
fn return_all_particles(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    return_particle(linear_index(id, groups, 128u), true);
}

@compute @workgroup_size(128)
fn compact_particles(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 128u);
    if (index >= atomicLoad(&counters[0])) { return; }
    let particle = particles_in[index];
    if (particle.position.w <= 0.0) { return; }
    let slot = atomicAdd(&counters[1], 1u);
    particles_out[slot] = particle;
}

@compute @workgroup_size(1)
fn finish_compaction() {
    atomicStore(&counters[0], atomicLoad(&counters[1]));
    atomicStore(&counters[1], 0u);
}

@compute @workgroup_size(64)
fn settle_columns(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 64u);
    let size = region_size();
    if (index >= size.x * size.y) { return; }
    let column = vec2<u32>(region_min().xy) + vec2<u32>(index % size.x, index / size.x);
    var write = 0u;
    for (var z = 0u; z < window.depth; z++) {
        let source = cell_index(vec3<u32>(column, z)) * CHANNELS;
        if (volume[source] <= 0) { continue; }
        if (write != z) {
            let destination = cell_index(vec3<u32>(column, write)) * CHANNELS;
            for (var channel = 0u; channel < 8u; channel++) {
                volume[destination + channel] = volume[source + channel];
                volume[source + channel] = 0;
            }
        }
        write += 1u;
    }
}

fn column_slot(xy: vec2<u32>) -> u32 {
    return xy.y * window.width + xy.x;
}

@compute @workgroup_size(64)
fn snapshot_columns(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 64u);
    if (index >= window.width * window.height) { return; }
    let xy = vec2<u32>(index % window.width, index / window.width);
    var height = 0.0;
    var top_absorption = vec3<f32>(0.0);
    var top_scattering = vec3<f32>(0.0);
    var dry = false;
    for (var z = 0u; z < window.depth; z++) {
        let base = cell_index(vec3<u32>(xy, z)) * CHANNELS;
        let density = f32(volume[base]) / PAINT_SCALE;
        if (density <= 0.0) { continue; }
        height += density;
        top_absorption = vec3<f32>(f32(volume[base + 1u]), f32(volume[base + 2u]), f32(volume[base + 3u])) / PAINT_SCALE / density;
        top_scattering = vec3<f32>(f32(volume[base + 4u]), f32(volume[base + 5u]), f32(volume[base + 6u])) / PAINT_SCALE / density;
        dry = dry || volume[base + 7u] >= i32(PAINT_SCALE);
    }
    columns[index] = vec4<f32>(height, select(0.0, 1.0, dry), top_scattering.x, 0.0);
    column_pigment[index] = vec4<f32>(top_absorption, top_scattering.y);
    columns[index].w = top_scattering.z;
}

fn column_flow(height: f32, xy: vec2<i32>) -> f32 {
    if (xy.x < 0 || xy.y < 0 || xy.x >= i32(window.width) || xy.y >= i32(window.height)) { return 0.0; }
    let other = columns[column_slot(vec2<u32>(xy))];
    if (other.x <= 0.0 || other.y > 0.0) { return 0.0; }
    let flowing = max(max(height, other.x) - LEVEL_MIN_HEIGHT, 0.0);
    let difference = height - other.x;
    let beyond_yield = sign(difference) * max(abs(difference) - LEVEL_YIELD_SLOPE, 0.0);
    return beyond_yield * LEVEL_RATE * 0.25 * min(flowing / LEVEL_MIN_HEIGHT, 1.0);
}

fn take_from_top(xy: vec2<u32>, amount: f32) {
    var remaining = amount;
    for (var step = 0u; step < window.depth; step++) {
        let z = window.depth - 1u - step;
        let base = cell_index(vec3<u32>(xy, z)) * CHANNELS;
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

fn add_on_top(xy: vec2<u32>, amount: f32, absorption: vec3<f32>, scattering: vec3<f32>) {
    for (var z = 0u; z < window.depth; z++) {
        let base = cell_index(vec3<u32>(xy, z)) * CHANNELS;
        let density = f32(volume[base]) / PAINT_SCALE;
        if (density >= 0.999 && z + 1u < window.depth) { continue; }
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

@compute @workgroup_size(64)
fn level_columns(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = linear_index(id, groups, 64u);
    if (index >= window.width * window.height) { return; }
    let xy = vec2<i32>(i32(index % window.width), i32(index / window.width));
    let own = columns[index];
    if (own.x <= 0.0 || own.y > 0.0) { return; }
    let offsets = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    var outgoing = 0.0;
    var incoming = 0.0;
    var incoming_absorption = vec3<f32>(0.0);
    var incoming_scattering = vec3<f32>(0.0);
    for (var n = 0u; n < 4u; n++) {
        let flow = column_flow(own.x, xy + offsets[n]);
        if (flow > 0.0) {
            outgoing += flow;
        } else if (flow < 0.0) {
            let neighbour = column_slot(vec2<u32>(xy + offsets[n]));
            let pigment = column_pigment[neighbour];
            incoming -= flow;
            incoming_absorption += pigment.xyz * -flow;
            incoming_scattering += vec3<f32>(columns[neighbour].z, pigment.w, columns[neighbour].w) * -flow;
        }
    }
    if (outgoing > 0.0) { take_from_top(vec2<u32>(xy), outgoing); }
    if (incoming > 0.0) { add_on_top(vec2<u32>(xy), incoming, incoming_absorption / incoming, incoming_scattering / incoming); }
}
