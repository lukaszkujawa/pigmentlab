const VERTICES: u32 = 10u;
const CURVE_SAMPLES: u32 = 8u;
const TIP_TANGENT_WEIGHT: f32 = 0.5;
const SPLAY_RATE: f32 = 0.3;
const RECOVER_RATE: f32 = 0.04;
const MAX_REACH_FRACTION: f32 = 0.98;
const LIFT_CLEARANCE: f32 = 0.1 * PIXELS_PER_CM;

struct BrushUniform {
    rotation_x: vec4<f32>,
    rotation_y: vec4<f32>,
    rotation_z: vec4<f32>,
    translation: vec4<f32>,
    friction: f32,
    recovery: f32,
    splay: f32,
    bristle_count: u32,
    canvas_width: f32,
    canvas_height: f32,
    sink: f32,
    ferrule_extent: f32,
    cohesion: f32,
    cohesion_radius: f32,
    padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> brush: BrushUniform;
@group(0) @binding(1) var<storage, read> rest: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> positions: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> tips: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> curves: array<vec4<f32>>;
@group(0) @binding(5) var<uniform> window: WindowUniform;
@group(0) @binding(6) var<storage, read> volume: array<i32>;
@group(0) @binding(7) var<storage, read_write> proposals: array<vec4<f32>>;

struct Frame {
    root: vec3<f32>,
    root_tangent: vec3<f32>,
    rest_tip: vec3<f32>,
    previous_tip: vec3<f32>,
    lift: vec3<f32>,
    length: f32,
    natural_reach: f32,
    max_reach: f32,
    contact: bool,
}

fn paint_height_under(xy: vec2<f32>) -> f32 {
    let pixel = vec2<i32>(floor(xy));
    if (!window_contains(pixel)) { return 0.0; }
    let local = vec2<u32>(pixel - window.origin);
    var total = 0;
    for (var z = 0u; z < window.depth; z++) {
        total += volume[window_cell(vec3<u32>(local, z))];
    }
    return f32(total) / PAINT_SCALE;
}

fn canvas_to_brush() -> mat3x3<f32> {
    return mat3x3<f32>(brush.rotation_x.xyz, brush.rotation_y.xyz, brush.rotation_z.xyz);
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

fn arc_length(p0: vec3<f32>, m0: vec3<f32>, p1: vec3<f32>, m1: vec3<f32>) -> f32 {
    var total = 0.0;
    var previous = p0;
    for (var i = 1u; i <= CURVE_SAMPLES; i++) {
        let point = hermite(p0, m0, p1, m1, f32(i) / f32(CURVE_SAMPLES));
        total += distance(previous, point);
        previous = point;
    }
    return total;
}

fn tangent_scale_for_length(p0: vec3<f32>, t0: vec3<f32>, p1: vec3<f32>, t1: vec3<f32>, target_length: f32) -> f32 {
    var low = 0.3;
    var high = 1.2;
    for (var i = 0u; i < 10u; i++) {
        let mid = 0.5 * (low + high);
        if (arc_length(p0, t0 * mid * target_length, p1, t1 * mid * target_length * TIP_TANGENT_WEIGHT) < target_length) {
            low = mid;
        } else {
            high = mid;
        }
    }
    return 0.5 * (low + high);
}

fn rod_reach(angle_below_horizontal: f32, root_height: f32, rod_length: f32) -> f32 {
    let ratio = root_height / rod_length;
    let full_turn = (1.0 - cos(angle_below_horizontal)) / angle_below_horizontal;
    if (ratio <= full_turn) {
        let radius = root_height / max(1.0 - cos(angle_below_horizontal), 1e-4);
        return radius * sin(angle_below_horizontal) + max(rod_length - radius * angle_below_horizontal, 0.0);
    }
    var low = 1e-3;
    var high = angle_below_horizontal;
    for (var i = 0u; i < 16u; i++) {
        let mid = 0.5 * (low + high);
        let drop = (cos(angle_below_horizontal - mid) - cos(angle_below_horizontal)) / mid;
        if (drop > ratio) {
            low = mid;
        } else {
            high = mid;
        }
    }
    let turn = 0.5 * (low + high);
    let radius = rod_length / turn;
    return radius * (sin(angle_below_horizontal) - sin(angle_below_horizontal - turn));
}

fn horizontal_direction(preferred: vec2<f32>, fallback: vec2<f32>) -> vec2<f32> {
    if (length(preferred) > 1e-4) { return normalize(preferred); }
    if (length(fallback) > 1e-4) { return normalize(fallback); }
    return vec2<f32>(1.0, 0.0);
}

fn frame_of(bristle: u32) -> Frame {
    let base = bristle * VERTICES;
    let ferrule_radius = max(length(rest[base + 1u].xy), 1e-3);
    let radial = clamp(ferrule_radius / brush.ferrule_extent, 0.0, 1.0);
    let splay = brush.splay * mix(0.25, 1.0, radial);
    let floor_height = brush.sink * paint_height_under(tips[bristle].xy);
    let lift = vec3<f32>(0.0, 0.0, floor_height);
    let root = to_canvas(rest[base + 1u].xyz) - lift;
    let root_tangent = direction_to_canvas(normalize(rest[base + 2u].xyz - rest[base + 1u].xyz));
    let bristle_length = distance(rest[base + VERTICES - 1u].xyz, rest[base + 1u].xyz);
    let rest_tip = root + root_tangent * bristle_length;
    let contact = rest_tip.z < 0.0 && root.z > 0.0;
    var natural_reach = 0.0;
    if (contact) {
        let angle_below_horizontal = atan2(-root_tangent.z, length(root_tangent.xy));
        natural_reach = rod_reach(angle_below_horizontal, root.z, bristle_length) * splay;
    }
    let max_reach = sqrt(max(bristle_length * bristle_length - root.z * root.z, 0.0)) * MAX_REACH_FRACTION;
    return Frame(root, root_tangent, rest_tip, tips[bristle].xyz - lift, lift, bristle_length, natural_reach, max_reach, contact);
}

fn lean_of(f: Frame) -> vec2<f32> {
    return horizontal_direction(f.root_tangent.xy, f.previous_tip.xy - f.root.xy);
}

@compute @workgroup_size(64)
fn step(@builtin(global_invocation_id) id: vec3<u32>) {
    let bristle = id.x;
    if (bristle >= brush.bristle_count) { return; }
    let f = frame_of(bristle);

    if (f.contact) {
        let equilibrium = f.root.xy + lean_of(f) * f.natural_reach;
        let stuck = f.previous_tip.xy;
        let pull = equilibrium - stuck;
        let pull_length = max(length(pull), 1e-6);
        let slip_limit = brush.friction * -f.rest_tip.z;
        let excess = max(pull_length - slip_limit, 0.0);
        let compressed = length(stuck - f.root.xy) < f.natural_reach;
        let rate = select(RECOVER_RATE, SPLAY_RATE, compressed);
        let slide = min(excess * rate, pull_length);
        proposals[bristle] = vec4<f32>(stuck + pull / pull_length * slide - equilibrium, 0.0, 1.0);
    } else {
        let clear_of_canvas = f.previous_tip.z > LIFT_CLEARANCE;
        let recovered = mix(f.previous_tip, f.rest_tip, brush.recovery);
        let tip = vec3<f32>(select(f.previous_tip.xy, recovered.xy, clear_of_canvas), max(recovered.z, 0.0));
        proposals[bristle] = vec4<f32>(tip - f.rest_tip, 0.0);
    }
}

fn cohered_slip(bristle: u32, own: vec3<f32>, contact: f32) -> vec3<f32> {
    let anchor = rest[bristle * VERTICES + 1u].xy;
    let sigma = max(brush.cohesion_radius, 1e-3);
    var weighted = vec3<f32>(0.0);
    var total = 0.0;
    for (var other = 0u; other < brush.bristle_count; other++) {
        let proposal = proposals[other];
        if (proposal.w != contact) { continue; }
        let d = distance(anchor, rest[other * VERTICES + 1u].xy) / sigma;
        let weight = exp(-0.5 * d * d);
        weighted += proposal.xyz * weight;
        total += weight;
    }
    return mix(own, weighted / max(total, 1e-6), brush.cohesion);
}

@compute @workgroup_size(64)
fn cohere(@builtin(global_invocation_id) id: vec3<u32>) {
    let bristle = id.x;
    if (bristle >= brush.bristle_count) { return; }
    let base = bristle * VERTICES;
    let f = frame_of(bristle);
    let proposal = proposals[bristle];
    let slip = cohered_slip(bristle, proposal.xyz, proposal.w);

    var tip: vec3<f32>;
    var tip_tangent: vec3<f32>;
    if (f.contact) {
        let lean = lean_of(f);
        var planar = lean * f.natural_reach + slip.xy;
        let reach = length(planar);
        if (reach > f.max_reach) { planar *= f.max_reach / reach; }
        tip = vec3<f32>(f.root.xy + planar, 0.0);
        let azimuth = horizontal_direction(planar, lean);
        let flatness = clamp(1.0 - (length(planar) - f.natural_reach) / max(f.max_reach - f.natural_reach, 1e-4), 0.0, 1.0);
        tip_tangent = normalize(mix(vec3<f32>(0.0, 0.0, -1.0), vec3<f32>(azimuth, 0.0), flatness));
    } else {
        tip = f.rest_tip + vec3<f32>(slip.xy, proposal.z);
        tip.z = max(tip.z, 0.0);
        tip_tangent = normalize(mix(f.root_tangent, normalize(tip - f.root), 0.5));
    }

    let scale = tangent_scale_for_length(f.root, f.root_tangent, tip, tip_tangent, f.length);
    let m0 = f.root_tangent * scale * f.length;
    let m1 = tip_tangent * scale * f.length * TIP_TANGENT_WEIGHT;
    positions[base] = rest[base];
    for (var k = 1u; k < VERTICES; k++) {
        let point = above_canvas(hermite(f.root, m0, tip, m1, f32(k - 1u) / f32(VERTICES - 2u))) + f.lift;
        positions[base + k] = vec4<f32>(to_brush(point), 0.0);
    }
    tips[bristle] = vec4<f32>(tip + f.lift, proposal.w);
    curves[bristle * 4u] = vec4<f32>(f.root + f.lift, 0.0);
    curves[bristle * 4u + 1u] = vec4<f32>(m0, 0.0);
    curves[bristle * 4u + 2u] = vec4<f32>(tip + f.lift, 0.0);
    curves[bristle * 4u + 3u] = vec4<f32>(m1, 0.0);
}
