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
    let egg = select(1.0, 1.0 + EGG, (u32(round(mark.form.z)) & 4u) != 0u);
    return max(max(mark.shape.x, mark.form.x) * (1.0 + mark.form.y) * egg, SMALLEST_RADIUS);
}

fn mark_hash(mark: Mark, salt: u32) -> f32 {
    var hash = bitcast<u32>(mark.ends.x) * 0x8da6b343u ^ bitcast<u32>(mark.ends.y) * 0xd8163841u ^ bitcast<u32>(mark.ends.z) * 0xcb1ab31fu ^ bitcast<u32>(mark.ends.w) * 0x165667b1u ^ salt * 0x9e3779b9u;
    hash ^= hash >> 16u;
    hash *= 0x7feb352du;
    hash ^= hash >> 15u;
    hash *= 0x846ca68bu;
    hash ^= hash >> 16u;
    return f32(hash >> 8u) / 16777216.0;
}

const EGG: f32 = 0.35;
const LOBES = array<f32, 5>(2.0, 3.0, 5.0, 8.0, 13.0);
const LOBE_SIZES = array<f32, 5>(0.3, 0.2, 0.09, 0.05, 0.026);

fn edge(mark: Mark, angle: f32) -> f32 {
    var wobble = 0.0;
    for (var lobe = 0u; lobe < 5u; lobe++) {
        let size = LOBE_SIZES[lobe] * (0.5 + mark_hash(mark, 2u * lobe));
        wobble += size * sin(LOBES[lobe] * angle + 6.2831853 * mark_hash(mark, 2u * lobe + 1u));
    }
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

fn oval_offset(mark: Mark, p: vec2<f32>) -> f32 {
    let along = mark.ends.zw - mark.ends.xy;
    let extent = length(along);
    let axis = select(vec2<f32>(1.0, 0.0), along / extent, extent > 1e-6);
    let away = p - 0.5 * (mark.ends.xy + mark.ends.zw);
    let radius = max(mark.shape.x, SMALLEST_RADIUS);
    let lengthwise = dot(away, axis) / (0.5 * extent + radius);
    let egg = 1.0 + EGG * (2.0 * mark_hash(mark, 11u) - 1.0) * clamp(lengthwise, -1.0, 1.0);
    let local = vec2<f32>(lengthwise, (axis.x * away.y - axis.y * away.x) / (radius * egg));
    return length(local) / edge(mark, atan2(local.y, local.x));
}

fn weight(mark: Mark, pixel: vec2<i32>) -> u32 {
    let p = vec2<f32>(pixel) + 0.5;
    let form = u32(round(mark.form.z));
    var share = 1.0;
    var offset = 0.0;
    if ((form & 4u) != 0u) {
        offset = oval_offset(mark, p);
    } else {
        let along = mark.ends.zw - mark.ends.xy;
        let span = dot(along, along);
        let projected = select(0.0, dot(p - mark.ends.xy, along) / span, span > 1e-6);
        let extent = sqrt(span);
        let fade = min(max(min(mark.shape.x, mark.form.x), SMALLEST_RADIUS), 0.5 * extent);
        if ((form & 1u) != 0u) { share *= clamp(0.5 + projected * extent / fade, 0.0, 1.0); }
        if ((form & 2u) != 0u) { share *= clamp(0.5 - (projected - 1.0) * extent / fade, 0.0, 1.0); }
        let t = clamp(projected, select(0.0, -1.0, (form & 1u) != 0u), select(1.0, 2.0, (form & 2u) != 0u));
        let away = p - (mark.ends.xy + along * t);
        let radius = mix(mark.shape.x, mark.form.x, clamp(t, 0.0, 1.0));
        let travelled = t * extent / max(radius, SMALLEST_RADIUS);
        offset = length(away) / max(radius * edge(mark, atan2(away.y, away.x) + travelled), SMALLEST_RADIUS);
    }
    if (share <= 0.0) { return 0u; }
    let rim = clamp((1.0 - offset) / max(1.0 - mark.shape.w, 0.02), 0.0, 1.0);
    let height = pow(1.0 - (1.0 - rim) * (1.0 - rim), 1.0 - 0.5 * mark.form.w);
    return u32(round(height * share * COVERAGE_SCALE));
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
