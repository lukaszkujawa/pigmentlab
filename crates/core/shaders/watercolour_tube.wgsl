struct PasteUniform {
    start: vec2<f32>,
    end: vec2<f32>,
    corner: vec2<i32>,
    canvas: vec2<u32>,
    cells: vec2<u32>,
    radius: f32,
    water: f32,
    pigment: f32,
    absorption: vec4<f32>,
    scattering: vec4<f32>,
    behaviour: vec4<f32>,
};

@group(0) @binding(0) var<uniform> paste: PasteUniform;
@group(0) @binding(1) var<storage, read_write> accumulators: array<atomic<i32>>;
@group(0) @binding(2) var<storage, read_write> touched: array<atomic<u32>>;

fn distance_to_nozzle_path(p: vec2<f32>) -> f32 {
    let along = paste.end - paste.start;
    let span = dot(along, along);
    let t = select(0.0, clamp(dot(p - paste.start, along) / span, 0.0, 1.0), span > 1e-6);
    return distance(p, paste.start + along * t);
}

@compute @workgroup_size(16, 16)
fn squeeze_onto_paper(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = paste.corner + vec2<i32>(id.xy);
    if (pixel.x < 0 || pixel.y < 0 || u32(pixel.x) >= paste.canvas.x || u32(pixel.y) >= paste.canvas.y) { return; }
    let normalised = distance_to_nozzle_path(vec2<f32>(pixel) + 0.5) / paste.radius;
    if (normalised >= 1.0) { return; }
    let profile = sqrt(1.0 - normalised * normalised);
    let cell = vec2<u32>(pixel) / 2u;
    let pixels_per_cell = 4.0;
    add_to_wash(
        cell.y * paste.cells.x + cell.x,
        paste.water * profile / pixels_per_cell,
        paste.pigment * profile / pixels_per_cell,
        paste.absorption.xyz,
        paste.scattering.xyz,
        paste.behaviour.xyz,
    );
    let slot = u32(pixel.y) * paste.canvas.x + u32(pixel.x);
    atomicAnd(&touched[slot], ~BAKED);
    atomicMax(&touched[slot], u32(profile * HAIR_TOUCH_SCALE));
}
