struct Mark {
    ends: vec4<f32>,
    shape: vec4<f32>,
    absorption: vec4<f32>,
    scattering: vec4<f32>,
    form: vec4<f32>,
};

struct MarkBatch {
    region: vec4<u32>,
    count: u32,
    tiles: u32,
};

@group(0) @binding(0) var<uniform> batch: MarkBatch;
@group(0) @binding(1) var<uniform> window: WindowUniform;
@group(0) @binding(2) var<storage, read> marks: array<Mark>;
@group(0) @binding(3) var<storage, read_write> coverage: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read_write> film: array<atomic<i32>>;
@group(0) @binding(5) var<storage, read_write> store: array<atomic<i32>>;
@group(0) @binding(6) var<storage, read_write> volume: array<i32>;
@group(0) @binding(7) var<storage, read> tiles: array<vec2<u32>>;

const COVERAGE_SCALE: f32 = 1024.0;
const SMALLEST_RADIUS: f32 = 0.75;

fn radius_of(mark: Mark) -> f32 {
    return max(max(mark.shape.x, mark.form.x) * (1.0 + mark.form.y), SMALLEST_RADIUS);
}

fn edge(mark: Mark, angle: f32) -> f32 {
    let seed = fract(sin(dot(mark.ends.xy + mark.ends.zw * 0.37, vec2<f32>(12.9898, 78.233))) * 43758.5453) * 6.2831853;
    let wobble = 0.35 * sin(3.0 * angle + seed) + 0.3 * sin(7.0 * angle + seed * 2.3) * fract(seed * 1.7) + 0.2 * sin(11.0 * angle + seed * 3.7) + 0.15 * sin(17.0 * angle + seed * 5.1) * fract(seed * 3.1);
    return 1.0 + mark.form.y * wobble;
}

fn corner_of(mark: Mark) -> vec2<i32> {
    return vec2<i32>(floor(min(mark.ends.xy, mark.ends.zw) - radius_of(mark)));
}

fn outside_box(mark: Mark, offset: vec2<u32>) -> bool {
    let far = vec2<i32>(ceil(max(mark.ends.xy, mark.ends.zw) + radius_of(mark)));
    return any(vec2<i32>(offset) > far - corner_of(mark));
}

const TILE_ROW: u32 = 65535u;

fn tile_of(group: vec3<u32>) -> u32 {
    return group.x + group.y * TILE_ROW;
}

fn tile_offset(tile: vec2<u32>, local: vec3<u32>) -> vec2<u32> {
    return vec2<u32>(tile.y & 0xffffu, tile.y >> 16u) * 16u + local.xy;
}

fn weight(mark: Mark, pixel: vec2<i32>) -> u32 {
    let p = vec2<f32>(pixel) + 0.5;
    let along = mark.ends.zw - mark.ends.xy;
    let span = dot(along, along);
    let projected = select(0.0, dot(p - mark.ends.xy, along) / span, span > 1e-6);
    let open = u32(round(mark.form.z));
    let extent = sqrt(span);
    let fade = min(max(min(mark.shape.x, mark.form.x), SMALLEST_RADIUS), 0.5 * extent);
    var share = 1.0;
    if ((open & 1u) != 0u) { share *= clamp(0.5 + projected * extent / fade, 0.0, 1.0); }
    if ((open & 2u) != 0u) { share *= clamp(0.5 - (projected - 1.0) * extent / fade, 0.0, 1.0); }
    if (share <= 0.0) { return 0u; }
    let t = clamp(projected, 0.0, 1.0);
    let away = p - (mark.ends.xy + along * t);
    let radius = mix(mark.shape.x, mark.form.x, t);
    let travelled = t * sqrt(span) / max(radius, SMALLEST_RADIUS);
    let reach = max(radius * edge(mark, atan2(away.y, away.x) + travelled), SMALLEST_RADIUS);
    let offset = length(away) / reach;
    let steepness = 2.0 + 10.0 * mark.shape.w;
    let rounding = 0.5 + 0.5 * mark.shape.w;
    return u32(round(pow(max(1.0 - pow(offset, steepness), 0.0), rounding) * share * COVERAGE_SCALE));
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
fn measure(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    let index = tile_of(group);
    if (index >= batch.tiles) { return; }
    let tile = tiles[index];
    let mark = marks[tile.x];
    let offset = tile_offset(tile, local);
    if (outside_box(mark, offset)) { return; }
    let covered = weight(mark, corner_of(mark) + vec2<i32>(offset));
    if (covered > 0u) { atomicAdd(&coverage[tile.x], covered); }
}

@compute @workgroup_size(16, 16)
fn spread(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    let index = tile_of(group);
    if (index >= batch.tiles) { return; }
    let tile = tiles[index];
    let mark = marks[tile.x];
    let offset = tile_offset(tile, local);
    let pixel = corner_of(mark) + vec2<i32>(offset);
    if (outside_box(mark, offset) || !on_canvas(pixel)) { return; }
    let covered = weight(mark, pixel);
    let total = atomicLoad(&coverage[tile.x]);
    if (covered == 0u || total == 0u) { return; }
    let amount = mark.shape.y * f32(covered) / f32(total);
    if (window_contains(pixel)) {
        let local_pixel = vec2<u32>(pixel - window.origin);
        add_paint((local_pixel.y * window.width + local_pixel.x) * CHANNELS, true, amount, mark);
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
