struct CanvasUniform {
    width: u32,
    height: u32,
    shrink: f32,
    padding: f32,
};

@group(0) @binding(0) var<uniform> canvas: CanvasUniform;
@group(0) @binding(1) var<storage, read_write> store: array<i32>;
@group(0) @binding(2) var<storage, read_write> dry: array<i32>;
@group(0) @binding(3) var<storage, read> relief: array<f32>;
@group(0) @binding(4) var<storage, read_write> ground: array<f32>;

@compute @workgroup_size(16, 16)
fn bake_dry(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= canvas.width || id.y >= canvas.height) { return; }
    let cell = (id.y * canvas.width + id.x) * CHANNELS;
    for (var channel = 0u; channel < 7u; channel++) {
        dry[cell + channel] += i32(round(f32(store[cell + channel]) * canvas.shrink));
        store[cell + channel] = 0;
    }
}

@compute @workgroup_size(16, 16)
fn refresh_ground(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= canvas.width || id.y >= canvas.height) { return; }
    let index = id.y * canvas.width + id.x;
    ground[index] = relief[index] + f32(dry[index * CHANNELS]) / PAINT_SCALE;
}
