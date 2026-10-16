struct CanvasUniform {
    width: u32,
    height: u32,
    shrink: f32,
    optical_thickness_per_pixel: f32,
};

const MIN_DRY_HEIGHT: f32 = 1e-3;

@group(0) @binding(0) var<uniform> canvas: CanvasUniform;
@group(0) @binding(1) var<storage, read_write> store: array<i32>;
@group(0) @binding(2) var<storage, read_write> dry: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> relief: array<f32>;
@group(0) @binding(4) var<storage, read_write> ground: array<f32>;

@compute @workgroup_size(16, 16)
fn bake_dry(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= canvas.width || id.y >= canvas.height) { return; }
    let index = id.y * canvas.width + id.x;
    let cell = index * CHANNELS;
    let height = f32(store[cell]) / PAINT_SCALE;
    if (height <= 0.0) { return; }
    let k = max(vec3<f32>(f32(store[cell + 1u]), f32(store[cell + 2u]), f32(store[cell + 3u])) / PAINT_SCALE / height, vec3<f32>(0.0));
    let s = max(vec3<f32>(f32(store[cell + 4u]), f32(store[cell + 5u]), f32(store[cell + 6u])) / PAINT_SCALE / height, vec3<f32>(0.0));
    for (var channel = 0u; channel < CHANNELS; channel++) {
        store[cell + channel] = 0;
    }
    if (height < MIN_DRY_HEIGHT) { return; }
    let previous = dry[index];
    let colour = layer_reflectance(k, s, height * canvas.optical_thickness_per_pixel, previous.yzw);
    dry[index] = vec4<f32>(previous.x + height * canvas.shrink, colour);
}

@compute @workgroup_size(16, 16)
fn refresh_ground(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= canvas.width || id.y >= canvas.height) { return; }
    let index = id.y * canvas.width + id.x;
    ground[index] = relief[index] + dry[index].x;
}
