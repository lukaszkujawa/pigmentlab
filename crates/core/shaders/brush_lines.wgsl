const VERTICES: u32 = 10u;
const DRAWN_SEGMENTS: u32 = VERTICES - 2u;
const OBLIQUE: f32 = 0.45;
const BLADE_SHADE_HEIGHT: f32 = 40.0;

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
};

@group(0) @binding(0) var<uniform> brush: BrushUniform;
@group(0) @binding(1) var<storage, read> positions: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> solids: array<u32>;

const SILICONE_FLAG: u32 = 0x40000000u;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

fn to_canvas(p: vec3<f32>) -> vec3<f32> {
    let rotation = mat3x3<f32>(brush.rotation_x.xyz, brush.rotation_y.xyz, brush.rotation_z.xyz);
    return transpose(rotation) * p + brush.translation.xyz;
}

@vertex
fn vs_hair(@builtin(vertex_index) index: u32) -> VertexOutput {
    let segment = index / 2u;
    let bristle = segment / DRAWN_SEGMENTS;
    let vertex = segment % DRAWN_SEGMENTS + 1u + (index & 1u);
    let canvas = to_canvas(positions[bristle * VERTICES + vertex].xyz);
    let seen = vec2<f32>(canvas.x, canvas.y - canvas.z * OBLIQUE);
    let ndc = vec2<f32>(seen.x / brush.canvas_width * 2.0 - 1.0, 1.0 - seen.y / brush.canvas_height * 2.0);
    let contact = clamp(1.0 - canvas.z / 4.0, 0.0, 1.0);
    let along = f32(vertex - 1u) / f32(DRAWN_SEGMENTS);
    let hair = mix(vec3<f32>(0.42, 0.33, 0.22), vec3<f32>(0.93, 0.86, 0.70), along);
    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = vec4<f32>(mix(hair, vec3<f32>(0.1, 0.1, 0.12), contact * 0.6), 0.8);
    return out;
}

fn seen_from_above(canvas: vec3<f32>) -> vec4<f32> {
    let seen = vec2<f32>(canvas.x, canvas.y - canvas.z * OBLIQUE);
    return vec4<f32>(seen.x / brush.canvas_width * 2.0 - 1.0, 1.0 - seen.y / brush.canvas_height * 2.0, 0.0, 1.0);
}

fn blade_colour(silicone: bool, along: f32) -> vec3<f32> {
    if (silicone) { return mix(vec3<f32>(0.2, 0.22, 0.25), vec3<f32>(0.36, 0.39, 0.43), along); }
    return mix(vec3<f32>(0.5, 0.53, 0.58), vec3<f32>(0.86, 0.88, 0.91), along);
}

@vertex
fn vs_solid(@builtin(vertex_index) index: u32) -> VertexOutput {
    let id = solids[index];
    var out: VertexOutput;
    let canvas = to_canvas(positions[id & ~SILICONE_FLAG].xyz);
    out.position = seen_from_above(canvas);
    out.color = vec4<f32>(blade_colour((id & SILICONE_FLAG) != 0u, clamp(canvas.z / BLADE_SHADE_HEIGHT, 0.0, 1.0)), 0.9);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
