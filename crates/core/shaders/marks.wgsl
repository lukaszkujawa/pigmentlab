struct Mark {
    ends: vec4<f32>,
    shape: vec4<f32>,
    absorption: vec4<f32>,
    scattering: vec4<f32>,
};

struct MarkBatch {
    region: vec4<u32>,
    count: u32,
};

@group(0) @binding(0) var<uniform> batch: MarkBatch;
@group(0) @binding(1) var<uniform> window: WindowUniform;
@group(0) @binding(2) var<storage, read> marks: array<Mark>;
@group(0) @binding(3) var<storage, read_write> coverage: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read_write> film: array<atomic<i32>>;
@group(0) @binding(5) var<storage, read_write> store: array<atomic<i32>>;
@group(0) @binding(6) var<storage, read_write> volume: array<i32>;

const COVERAGE_SCALE: f32 = 1024.0;
const SMALLEST_RADIUS: f32 = 0.75;

fn radius_of(mark: Mark) -> f32 {
    return max(mark.shape.x, SMALLEST_RADIUS);
}

fn corner_of(mark: Mark) -> vec2<i32> {
    return vec2<i32>(floor(min(mark.ends.xy, mark.ends.zw) - radius_of(mark)));
}

fn outside_box(mark: Mark, id: vec3<u32>) -> bool {
    let far = vec2<i32>(ceil(max(mark.ends.xy, mark.ends.zw) + radius_of(mark)));
    return any(vec2<i32>(id.xy) > far - corner_of(mark));
}

fn weight(mark: Mark, pixel: vec2<i32>) -> u32 {
    let p = vec2<f32>(pixel) + 0.5;
    let along = mark.ends.zw - mark.ends.xy;
    let span = dot(along, along);
    let t = select(0.0, clamp(dot(p - mark.ends.xy, along) / span, 0.0, 1.0), span > 1e-6);
    let offset = distance(p, mark.ends.xy + along * t) / radius_of(mark);
    return u32(round(sqrt(max(1.0 - offset * offset, 0.0)) * COVERAGE_SCALE));
}

fn fixed(value: f32) -> i32 {
    return i32(round(value * PAINT_SCALE));
}

fn add_paint(buffer_cell: u32, in_window: bool, amount: f32, mark: Mark) {
    let channels = array<f32, 8>(amount, amount * mark.absorption.x, amount * mark.absorption.y, amount * mark.absorption.z, amount * mark.scattering.x, amount * mark.scattering.y, amount * mark.scattering.z, amount * mark.shape.z);
    for (var channel = 0u; channel < CHANNELS; channel++) {
        if (in_window) {
            atomicAdd(&film[buffer_cell + channel], fixed(channels[channel]));
        } else {
            atomicAdd(&store[buffer_cell + channel], fixed(channels[channel]));
        }
    }
}

@compute @workgroup_size(16, 16)
fn measure(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.z >= batch.count) { return; }
    let mark = marks[id.z];
    if (outside_box(mark, id)) { return; }
    let covered = weight(mark, corner_of(mark) + vec2<i32>(id.xy));
    if (covered > 0u) { atomicAdd(&coverage[id.z], covered); }
}

@compute @workgroup_size(16, 16)
fn spread(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.z >= batch.count) { return; }
    let mark = marks[id.z];
    let pixel = corner_of(mark) + vec2<i32>(id.xy);
    if (outside_box(mark, id) || !on_canvas(pixel)) { return; }
    let covered = weight(mark, pixel);
    let total = atomicLoad(&coverage[id.z]);
    if (covered == 0u || total == 0u) { return; }
    let amount = mark.shape.y * f32(covered) / f32(total);
    if (window_contains(pixel)) {
        let local = vec2<u32>(pixel - window.origin);
        add_paint((local.y * window.width + local.x) * CHANNELS, true, amount, mark);
    } else {
        add_paint(store_cell(vec2<u32>(pixel)), false, amount, mark);
    }
}

@compute @workgroup_size(16, 16)
fn settle(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= batch.region.zw)) { return; }
    let local = batch.region.xy + id.xy;
    let slot = (local.y * window.width + local.x) * CHANNELS;
    if (atomicLoad(&film[slot]) <= 0) { return; }
    let cell = free_cell(local);
    for (var channel = 0u; channel < CHANNELS; channel++) {
        volume[cell + channel] += atomicExchange(&film[slot + channel], 0);
    }
}
