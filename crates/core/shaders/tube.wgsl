struct TubeUniform {
    start: vec2<f32>,
    end: vec2<f32>,
    nozzle_radius: f32,
    rope_radius: f32,
    corner: vec2<i32>,
    absorption: vec4<f32>,
    scattering: vec4<f32>,
    pile_height: f32,
    padding_1: f32,
    padding_2: f32,
    padding_3: f32,
};

@group(0) @binding(0) var<uniform> tube: TubeUniform;
@group(0) @binding(1) var<uniform> window: WindowUniform;
@group(0) @binding(2) var<storage, read_write> volume: array<i32>;
@group(0) @binding(3) var<storage, read> stroke_base: array<f32>;
@group(0) @binding(4) var<storage, read> ground: array<f32>;

const RESTING_REACH: i32 = 2;

fn distance_to_segment(p: vec2<f32>) -> f32 {
    let along = tube.end - tube.start;
    let span = dot(along, along);
    let t = select(0.0, clamp(dot(p - tube.start, along) / span, 0.0, 1.0), span > 1e-6);
    return distance(p, tube.start + along * t);
}

fn column_height(local: vec2<u32>) -> f32 {
    var height = 0.0;
    for (var z = 0u; z < window.depth; z++) {
        height += f32(volume[window_cell(vec3<u32>(local, z))]) / PAINT_SCALE;
    }
    return height;
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
    volume[cell] += i32(round(amount * PAINT_SCALE));
    volume[cell + 1u] += i32(round(amount * tube.absorption.x * PAINT_SCALE));
    volume[cell + 2u] += i32(round(amount * tube.absorption.y * PAINT_SCALE));
    volume[cell + 3u] += i32(round(amount * tube.absorption.z * PAINT_SCALE));
    volume[cell + 4u] += i32(round(amount * tube.scattering.x * PAINT_SCALE));
    volume[cell + 5u] += i32(round(amount * tube.scattering.y * PAINT_SCALE));
    volume[cell + 6u] += i32(round(amount * tube.scattering.z * PAINT_SCALE));
    volume[cell + 7u] += i32(round(amount * tube.absorption.w * PAINT_SCALE));
}

fn canvas_index(pixel: vec2<i32>) -> u32 {
    let clamped = clamp(pixel, vec2<i32>(0), vec2<i32>(i32(window.canvas_width) - 1, i32(window.canvas_height) - 1));
    return u32(clamped.y) * window.canvas_width + u32(clamped.x);
}

fn resting_surface(pixel: vec2<i32>) -> f32 {
    var highest = 0.0;
    for (var dy = -RESTING_REACH; dy <= RESTING_REACH; dy++) {
        for (var dx = -RESTING_REACH; dx <= RESTING_REACH; dx++) {
            highest = max(highest, ground[canvas_index(pixel + vec2<i32>(dx, dy))]);
        }
    }
    return highest + stroke_base[canvas_index(pixel)];
}

fn rope_paint(distance_from_axis: f32, pixel: vec2<i32>) -> f32 {
    let r = tube.rope_radius;
    if (distance_from_axis >= r) { return 0.0; }
    return resting_surface(pixel) + r + sqrt(r * r - distance_from_axis * distance_from_axis) - ground[canvas_index(pixel)];
}

@compute @workgroup_size(16, 16)
fn squeeze(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = tube.corner + vec2<i32>(id.xy);
    if (pixel.x < 0 || pixel.y < 0 || pixel.x >= i32(window.canvas_width) || pixel.y >= i32(window.canvas_height)) { return; }
    if (!window_contains(pixel)) { return; }
    let from_axis = distance_to_segment(vec2<f32>(pixel) + 0.5);
    if (from_axis >= tube.nozzle_radius) { return; }
    let local = vec2<u32>(pixel - window.origin);
    let normalised = from_axis / tube.nozzle_radius;
    let rope = max(rope_paint(from_axis, pixel) - column_height(local), 0.0);
    let pile = tube.pile_height * sqrt(1.0 - normalised * normalised);
    if (rope + pile > 0.0) { add_on_top(local, rope + pile); }
}
