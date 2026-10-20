const ITERATIONS: u32 = 4u;
const DAMPING: f32 = 0.45;
const TIP_FLEXIBILITY: f32 = 0.65;
const PARENT_SHARE: f32 = 0.25;
const ORIENTATION_MEMORY: f32 = 0.1;
const AIRBORNE_STIFFNESS: f32 = 0.2;
const GROUNDING_RATE: f32 = 0.05;
const LIFTING_RATE: f32 = 0.25;
const PACKING_GRID: i32 = 64;
const PACKING_SCALE: f32 = 256.0;
const PACKING_RATE: f32 = 0.3;
const CLUMP_SCALE: f32 = 1024.0;
const CLUMP_RATE: f32 = 0.08;
const BRIDGING_WETNESS: f32 = 0.3;
const CONTACT_BAND: f32 = 0.75;
const HAIR_WIDTH: f32 = 0.8;
const HAIR_TIP_WIDTH: f32 = 0.5;
const PRESSED_LOAD: f32 = 0.15;
const PAINT_DRAG: f32 = 0.06;
const IMMERSION_DEPTH: f32 = 6.0;
const WETNESS_SAMPLES: u32 = 8u;

struct BrushUniform {
    rotation_x: vec4<f32>,
    rotation_y: vec4<f32>,
    rotation_z: vec4<f32>,
    translation: vec4<f32>,
    previous_rotation_x: vec4<f32>,
    previous_rotation_y: vec4<f32>,
    previous_rotation_z: vec4<f32>,
    previous_translation: vec4<f32>,
    tuft: vec4<f32>,
    friction: f32,
    stiffness: f32,
    inertia: f32,
    bristle_count: u32,
    canvas_width: f32,
    canvas_height: f32,
    sink: f32,
    cohesion: f32,
    pressure: f32,
    tooth: f32,
    packing_density: f32,
    clump_count: u32,
    sample_capacity: f32,
    wet_floor_keep: f32,
    padding_1: f32,
    padding_2: f32,
};

@group(0) @binding(0) var<uniform> brush: BrushUniform;
@group(0) @binding(1) var<storage, read> rest: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> positions: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> chain: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> velocity: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> relative: array<vec4<f32>>;
@group(0) @binding(6) var<uniform> window: WindowUniform;
@group(0) @binding(7) var<storage, read> stroke_base: array<f32>;
@group(0) @binding(8) var<storage, read> ground: array<f32>;
@group(0) @binding(9) var<storage, read> relief: array<f32>;
@group(0) @binding(10) var<storage, read> columns: array<vec4<f32>>;
@group(0) @binding(11) var<storage, read> sample_load: array<f32>;
@group(0) @binding(12) var<storage, read_write> clumps: array<atomic<i32>>;
@group(0) @binding(13) var<storage, read_write> packing: array<atomic<i32>>;

fn canvas_index(xy: vec2<f32>) -> i32 {
    let pixel = vec2<i32>(floor(xy));
    if (pixel.x < 0 || pixel.y < 0 || pixel.x >= i32(window.canvas_width) || pixel.y >= i32(window.canvas_height)) { return -1; }
    return pixel.y * i32(window.canvas_width) + pixel.x;
}

fn floor_under(xy: vec2<f32>) -> f32 {
    let index = canvas_index(xy);
    if (index < 0) { return 0.0; }
    let dried = ground[u32(index)] - relief[u32(index)];
    let light_touch = 1.0 - brush.pressure;
    let weave_top = brush.tooth * light_touch * light_touch;
    return dried + weave_top + stroke_base[u32(index)] * brush.wet_floor_keep;
}

fn column_at(xy: vec2<f32>) -> vec4<f32> {
    let pixel = vec2<i32>(floor(xy));
    if (!window_contains(pixel)) { return vec4<f32>(0.0); }
    let local = vec2<u32>(pixel - window.origin);
    return columns[local.y * window.width + local.x];
}

fn canvas_to_brush() -> mat3x3<f32> {
    return mat3x3<f32>(brush.rotation_x.xyz, brush.rotation_y.xyz, brush.rotation_z.xyz);
}

fn previous_canvas_to_brush() -> mat3x3<f32> {
    return mat3x3<f32>(brush.previous_rotation_x.xyz, brush.previous_rotation_y.xyz, brush.previous_rotation_z.xyz);
}

fn to_canvas(p: vec3<f32>) -> vec3<f32> {
    return transpose(canvas_to_brush()) * p + brush.translation.xyz;
}

fn direction_to_canvas(d: vec3<f32>) -> vec3<f32> {
    return transpose(canvas_to_brush()) * d;
}

fn to_brush(p: vec3<f32>) -> vec3<f32> {
    return canvas_to_brush() * (p - brush.translation.xyz);
}

fn to_previous_brush(p: vec3<f32>) -> vec3<f32> {
    return previous_canvas_to_brush() * (p - brush.previous_translation.xyz);
}

fn packing_origin() -> vec2<f32> {
    return brush.tuft.xy - f32(PACKING_GRID / 2) * brush.tuft.w;
}

fn packing_slot(cell: vec2<i32>) -> i32 {
    if (cell.x < 0 || cell.y < 0 || cell.x >= PACKING_GRID || cell.y >= PACKING_GRID) { return -1; }
    return cell.y * PACKING_GRID + cell.x;
}

fn packing_count(cell: vec2<i32>) -> f32 {
    let slot = packing_slot(cell);
    if (slot < 0) { return 0.0; }
    return f32(atomicLoad(&packing[slot])) / PACKING_SCALE;
}

struct Splat {
    cell: vec2<i32>,
    weights: array<f32, 4>,
}

fn splat_of(xy: vec2<f32>) -> Splat {
    let grid = (xy - packing_origin()) / brush.tuft.w - 0.5;
    let f = grid - floor(grid);
    return Splat(vec2<i32>(floor(grid)), array<f32, 4>((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y));
}

fn corner(index: u32) -> vec2<i32> {
    return vec2<i32>(i32(index & 1u), i32(index >> 1u));
}

fn packing_at(xy: vec2<f32>, own: vec2<f32>) -> f32 {
    let sample = splat_of(xy);
    let mine = splat_of(own);
    var total = 0.0;
    for (var i = 0u; i < 4u; i++) {
        let cell = sample.cell + corner(i);
        var count = packing_count(cell);
        for (var j = 0u; j < 4u; j++) {
            if (all(mine.cell + corner(j) == cell)) { count -= mine.weights[j]; }
        }
        total += sample.weights[i] * count;
    }
    return total;
}

fn splat_packing(xy: vec2<f32>) {
    let splat = splat_of(xy);
    for (var i = 0u; i < 4u; i++) {
        let slot = packing_slot(splat.cell + corner(i));
        if (slot >= 0) { atomicAdd(&packing[slot], i32(round(splat.weights[i] * PACKING_SCALE))); }
    }
}

fn clump_slot(clump: u32, vertex: u32) -> u32 {
    return (clump * BRISTLE_VERTICES + vertex) * 4u;
}

fn clump_members(clump: u32) -> u32 {
    return (brush.bristle_count - clump + brush.clump_count - 1u) / brush.clump_count;
}

fn clump_centre(clump: u32, vertex: u32) -> vec3<f32> {
    let slot = clump_slot(clump, vertex);
    let sum = vec3<f32>(f32(atomicLoad(&clumps[slot])), f32(atomicLoad(&clumps[slot + 1u])), f32(atomicLoad(&clumps[slot + 2u])));
    return to_canvas(sum / (CLUMP_SCALE * f32(clump_members(clump))));
}

fn wetness(bristle: u32) -> f32 {
    let samples = arrayLength(&sample_load) / brush.bristle_count;
    var total = 0.0;
    for (var i = 0u; i < WETNESS_SAMPLES; i++) {
        total += sample_load[bristle * samples + samples / 2u + i * (samples / 2u) / WETNESS_SAMPLES];
    }
    return clamp(total / (f32(WETNESS_SAMPLES) * brush.sample_capacity), 0.0, 1.0);
}

fn in_contact(p: vec3<f32>) -> bool {
    return p.z < floor_under(p.xy) + CONTACT_BAND;
}

@compute @workgroup_size(64)
fn prepare(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < u32(PACKING_GRID * PACKING_GRID)) { atomicStore(&packing[id.x], 0); }
    if (id.x < brush.clump_count * BRISTLE_VERTICES * 4u) { atomicStore(&clumps[id.x], 0); }
}

@compute @workgroup_size(64)
fn gather(@builtin(global_invocation_id) id: vec3<u32>) {
    let bristle = id.x;
    if (bristle >= brush.bristle_count) { return; }
    let base = bristle * BRISTLE_VERTICES;
    let clump = bristle % brush.clump_count;
    for (var k = 1u; k < BRISTLE_VERTICES; k++) {
        let p = chain[base + k].xyz;
        let slot = clump_slot(clump, k);
        let offset = to_previous_brush(p) * CLUMP_SCALE;
        atomicAdd(&clumps[slot], i32(round(offset.x)));
        atomicAdd(&clumps[slot + 1u], i32(round(offset.y)));
        atomicAdd(&clumps[slot + 2u], i32(round(offset.z)));
        if (k >= 2u && in_contact(p)) { splat_packing(p.xy); }
    }
}

fn tolerated_packing(segment: vec3<f32>, along: f32) -> f32 {
    let lying = clamp(length(segment.xy) / max(length(segment), 1e-6), 0.0, 1.0);
    let width = HAIR_WIDTH * (1.0 + (HAIR_TIP_WIDTH - 1.0) * along * along * along);
    let footprint = width * mix(width, length(segment), lying);
    return brush.packing_density / footprint;
}

fn spread_out(p: vec3<f32>, own: vec2<f32>, segment: vec3<f32>, along: f32) -> vec2<f32> {
    let density = packing_at(p.xy, own);
    let tolerated = tolerated_packing(segment, along);
    if (density <= tolerated) { return vec2<f32>(0.0); }
    let h = brush.tuft.w;
    let gradient = vec2<f32>(
        packing_at(p.xy + vec2<f32>(h, 0.0), own) - packing_at(p.xy - vec2<f32>(h, 0.0), own),
        packing_at(p.xy + vec2<f32>(0.0, h), own) - packing_at(p.xy - vec2<f32>(0.0, h), own),
    );
    let steepness = length(gradient);
    if (steepness < 1e-4) { return vec2<f32>(0.0); }
    let crowding = min((density - tolerated) / tolerated, 1.0);
    return -gradient / steepness * crowding * PACKING_RATE * h;
}

fn transport(v: vec3<f32>, source: vec3<f32>, destination: vec3<f32>) -> vec3<f32> {
    let a = normalize(source);
    let b = normalize(destination);
    let c = dot(a, b);
    if (c < -0.99) { return v; }
    let axis = cross(a, b);
    return v * c + cross(axis, v) + axis * dot(axis, v) / (1.0 + c);
}

@compute @workgroup_size(64)
fn solve(@builtin(global_invocation_id) id: vec3<u32>) {
    let bristle = id.x;
    if (bristle >= brush.bristle_count) { return; }
    let base = bristle * BRISTLE_VERTICES;
    let traits = rest[base];
    let segment = traits.z;
    let grounded = chain[base].w;
    let stiffness = clamp(mix(max(brush.stiffness, AIRBORNE_STIFFNESS), brush.stiffness, grounded) * (0.8 + 0.4 * traits.w), 0.0, 1.0);
    let friction = brush.friction * (0.7 + 0.6 * fract(traits.w * 13.37));
    let clump = bristle % brush.clump_count;
    let cohesion = select(0.0, CLUMP_RATE * brush.cohesion * smoothstep(BRIDGING_WETNESS, 1.0, wetness(bristle)), clump_members(clump) > 1u);
    let keep = 1.0 - DAMPING;
    let rest_tip = to_canvas(rest[base + BRISTLE_VERTICES - 1u].xyz);
    let pressed_load = PRESSED_LOAD * segment * clamp((floor_under(rest_tip.xy) - rest_tip.z) / (segment * f32(BRISTLE_VERTICES - 2u)), 0.0, 1.0);

    var p: array<vec3<f32>, BRISTLE_VERTICES>;
    var start: array<vec3<f32>, BRISTLE_VERTICES>;
    var directions: array<vec3<f32>, BRISTLE_VERTICES>;
    p[1] = to_canvas(rest[base + 1u].xyz);
    start[1] = p[1];
    for (var k = 2u; k < BRISTLE_VERTICES; k++) {
        let x = chain[base + k].xyz;
        start[k] = x;
        directions[k] = direction_to_canvas(rest[base + k].xyz - rest[base + k - 1u].xyz);
        if (in_contact(x)) {
            p[k] = x;
        } else {
            let following = to_canvas(to_previous_brush(x)) + direction_to_canvas(relative[base + k].xyz) * keep;
            let inertial = x + velocity[base + k].xyz * keep;
            p[k] = mix(following, inertial, brush.inertia);
        }
    }

    for (var iteration = 0u; iteration < ITERATIONS; iteration++) {
        for (var k = 2u; k < BRISTLE_VERTICES; k++) {
            let along = f32(k - 1u) / f32(BRISTLE_VERTICES - 2u);
            var aim = p[k - 1u] + directions[k];
            if (k > 2u) { aim = mix(p[k - 1u] + transport(directions[k], directions[k - 1u], p[k - 1u] - p[k - 2u]), aim, ORIENTATION_MEMORY); }
            let bend = (aim - p[k]) * stiffness * (1.0 - TIP_FLEXIBILITY * along);
            p[k] += bend;
            let d = p[k] - p[k - 1u];
            let stretch = d * (1.0 - segment / max(length(d), 1e-6));
            if (k == 2u) {
                p[k] -= stretch;
            } else {
                p[k - 1u] += stretch * PARENT_SHARE;
                p[k] -= stretch * (1.0 - PARENT_SHARE);
            }
        }
        for (var k = 2u; k < BRISTLE_VERTICES; k++) {
            let along = f32(k - 1u) / f32(BRISTLE_VERTICES - 2u);
            if (cohesion > 0.0) { p[k] += (clump_centre(clump, k) - p[k]) * cohesion * along * along; }
            if (in_contact(p[k])) { p[k] = vec3<f32>(p[k].xy + spread_out(p[k], select(vec2<f32>(1e9), start[k].xy, in_contact(start[k])), p[k] - p[k - 1u], along) / f32(ITERATIONS), p[k].z); }
            let floor_height = floor_under(p[k].xy);
            let depth = floor_height - p[k].z;
            if (depth > 0.0) {
                p[k].z = floor_height;
                let slip = p[k].xy - start[k].xy;
                let slip_length = length(slip);
                let held = friction * (depth + pressed_load);
                p[k] = vec3<f32>(start[k].xy + slip * max(slip_length - held, 0.0) / max(slip_length, 1e-6), p[k].z);
            }
        }
    }

    var touching = 0u;
    let reach = brush.tuft.z;
    for (var k = 2u; k < BRISTLE_VERTICES; k++) {
        if (in_contact(p[k])) { touching++; }
        let column = column_at(p[k].xy);
        let immersion = clamp((column.x + column.w - p[k].z) / IMMERSION_DEPTH, 0.0, 1.0);
        let viscosity = 1.0 - MEDIUM_THINNING * column.z;
        p[k] = vec3<f32>(mix(p[k].xy, start[k].xy, PAINT_DRAG * immersion * viscosity), p[k].z);
        let offset = p[k].xy - brush.tuft.xy;
        let distance_from_tuft = length(offset);
        if (distance_from_tuft > reach) { p[k] = vec3<f32>(brush.tuft.xy + offset * reach / distance_from_tuft, p[k].z); }
        p[k].z = max(p[k].z, floor_under(p[k].xy));
    }

    let grounding = select(grounded * (1.0 - LIFTING_RATE), mix(grounded, 1.0, GROUNDING_RATE), touching > 0u);
    chain[base] = vec4<f32>(f32(touching), 0.0, 0.0, grounding);
    positions[base] = rest[base];
    positions[base + 1u] = rest[base + 1u];
    chain[base + 1u] = vec4<f32>(p[1], 0.0);
    for (var k = 2u; k < BRISTLE_VERTICES; k++) {
        chain[base + k] = vec4<f32>(p[k], 0.0);
        velocity[base + k] = vec4<f32>(p[k] - start[k], 0.0);
        relative[base + k] = vec4<f32>(to_brush(p[k]) - to_previous_brush(start[k]), 0.0);
        positions[base + k] = vec4<f32>(to_brush(p[k]), 0.0);
    }
}
