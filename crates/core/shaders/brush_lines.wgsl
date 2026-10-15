const VERTICES: u32 = 10u;

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
@group(0) @binding(1) var<storage, read> positions: array<vec4<f32>>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

fn to_canvas(p: vec3<f32>) -> vec3<f32> {
    let rotation = mat3x3<f32>(brush.rotation_x.xyz, brush.rotation_y.xyz, brush.rotation_z.xyz);
    return transpose(rotation) * p + brush.translation.xyz;
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let segment = index / 2u;
    let bristle = segment / (VERTICES - 1u);
    let vertex = segment % (VERTICES - 1u) + (index & 1u);
    let canvas = to_canvas(positions[bristle * VERTICES + vertex].xyz);
    let ndc = vec2<f32>(canvas.x / brush.canvas_width * 2.0 - 1.0, 1.0 - canvas.y / brush.canvas_height * 2.0);
    let contact = clamp(1.0 - canvas.z / 4.0, 0.0, 1.0);
    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = mix(vec4<f32>(0.15, 0.15, 0.18, 0.6), vec4<f32>(0.8, 0.1, 0.1, 0.9), contact);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
