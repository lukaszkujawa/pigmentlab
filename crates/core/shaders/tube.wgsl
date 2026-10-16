struct TubeUniform {
    start: vec2<f32>,
    end: vec2<f32>,
    radius: f32,
    height: f32,
    corner: vec2<i32>,
    absorption: vec4<f32>,
    scattering: vec4<f32>,
};

@group(0) @binding(0) var<uniform> tube: TubeUniform;
@group(0) @binding(1) var<uniform> window: WindowUniform;
@group(0) @binding(2) var<storage, read_write> volume: array<i32>;

fn distance_to_segment(p: vec2<f32>) -> f32 {
    let along = tube.end - tube.start;
    let span = dot(along, along);
    let t = select(0.0, clamp(dot(p - tube.start, along) / span, 0.0, 1.0), span > 1e-6);
    return distance(p, tube.start + along * t);
}

fn add_on_top(local: vec2<u32>, amount: f32) {
    for (var z = 0u; z < window.depth; z++) {
        let base = window_cell(vec3<u32>(local, z));
        let density = f32(volume[base]) / PAINT_SCALE;
        if (density >= FULL_CELL && z + 1u < window.depth) { continue; }
        volume[base] += i32(round(amount * PAINT_SCALE));
        volume[base + 1u] += i32(round(amount * tube.absorption.x * PAINT_SCALE));
        volume[base + 2u] += i32(round(amount * tube.absorption.y * PAINT_SCALE));
        volume[base + 3u] += i32(round(amount * tube.absorption.z * PAINT_SCALE));
        volume[base + 4u] += i32(round(amount * tube.scattering.x * PAINT_SCALE));
        volume[base + 5u] += i32(round(amount * tube.scattering.y * PAINT_SCALE));
        volume[base + 6u] += i32(round(amount * tube.scattering.z * PAINT_SCALE));
        volume[base + 7u] += i32(round(amount * tube.absorption.w * PAINT_SCALE));
        return;
    }
}

@compute @workgroup_size(16, 16)
fn squeeze(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = tube.corner + vec2<i32>(id.xy);
    if (pixel.x < 0 || pixel.y < 0 || pixel.x >= i32(window.canvas_width) || pixel.y >= i32(window.canvas_height)) { return; }
    if (!window_contains(pixel)) { return; }
    let normalised = distance_to_segment(vec2<f32>(pixel) + 0.5) / tube.radius;
    if (normalised >= 1.0) { return; }
    add_on_top(vec2<u32>(pixel - window.origin), tube.height * sqrt(1.0 - normalised * normalised));
}
