const VERTICES: u32 = 10u;
const CURVE_SAMPLES: u32 = 8u;
const TIP_TANGENT_WEIGHT: f32 = 0.5;
const SPLAY_RATE: f32 = 0.3;
const RECOVER_RATE: f32 = 0.04;
const MAX_REACH_FRACTION: f32 = 0.98;

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
};

@group(0) @binding(0) var<uniform> brush: BrushUniform;
@group(0) @binding(1) var<storage, read> rest: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> positions: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> tips: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> curves: array<vec4<f32>>;
@group(0) @binding(5) var<uniform> window: WindowUniform;
@group(0) @binding(6) var<storage, read> volume: array<i32>;

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

@compute @workgroup_size(64)
fn step(@builtin(global_invocation_id) id: vec3<u32>) {
    let bristle = id.x;
    if (bristle >= brush.bristle_count) { return; }
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
    let previous_tip = tips[bristle].xyz - lift;

    var tip: vec3<f32>;
    var tip_tangent: vec3<f32>;
    var contact = 0.0;
    if (rest_tip.z < 0.0 && root.z > 0.0) {
        let lean = horizontal_direction(root_tangent.xy, previous_tip.xy - root.xy);
        let angle_below_horizontal = atan2(-root_tangent.z, length(root_tangent.xy));
        let natural_reach = rod_reach(angle_below_horizontal, root.z, bristle_length) * splay;
        let max_reach = sqrt(max(bristle_length * bristle_length - root.z * root.z, 0.0)) * MAX_REACH_FRACTION;
        let equilibrium = root.xy + lean * natural_reach;
        let stuck = previous_tip.xy;
        let pull = equilibrium - stuck;
        let pull_length = max(length(pull), 1e-6);
        let slip_limit = brush.friction * -rest_tip.z;
        let excess = max(pull_length - slip_limit, 0.0);
        let compressed = length(stuck - root.xy) < natural_reach;
        let rate = select(RECOVER_RATE, SPLAY_RATE, compressed);
        let slide = min(excess * rate, pull_length);
        var offset = stuck + pull / pull_length * slide - root.xy;
        let reach = length(offset);
        if (reach > max_reach) { offset *= max_reach / reach; }
        tip = vec3<f32>(root.xy + offset, 0.0);
        let azimuth = horizontal_direction(offset, lean);
        let flatness = clamp(1.0 - (length(offset) - natural_reach) / max(max_reach - natural_reach, 1e-4), 0.0, 1.0);
        tip_tangent = normalize(mix(vec3<f32>(0.0, 0.0, -1.0), vec3<f32>(azimuth, 0.0), flatness));
        contact = 1.0;
    } else {
        tip = mix(previous_tip, rest_tip, brush.recovery);
        tip.z = max(tip.z, 0.0);
        tip_tangent = normalize(mix(root_tangent, normalize(tip - root), 0.5));
    }

    let scale = tangent_scale_for_length(root, root_tangent, tip, tip_tangent, bristle_length);
    let m0 = root_tangent * scale * bristle_length;
    let m1 = tip_tangent * scale * bristle_length * TIP_TANGENT_WEIGHT;
    positions[base] = rest[base];
    for (var k = 1u; k < VERTICES; k++) {
        let point = above_canvas(hermite(root, m0, tip, m1, f32(k - 1u) / f32(VERTICES - 2u))) + lift;
        positions[base + k] = vec4<f32>(to_brush(point), 0.0);
    }
    tips[bristle] = vec4<f32>(tip + lift, contact);
    curves[bristle * 4u] = vec4<f32>(root + lift, 0.0);
    curves[bristle * 4u + 1u] = vec4<f32>(m0, 0.0);
    curves[bristle * 4u + 2u] = vec4<f32>(tip + lift, 0.0);
    curves[bristle * 4u + 3u] = vec4<f32>(m1, 0.0);
}
