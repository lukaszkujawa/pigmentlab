@group(0) @binding(0) var<uniform> window: WindowUniform;
@group(0) @binding(1) var<storage, read> store: array<i32>;
@group(0) @binding(2) var<storage, read> volume: array<i32>;
@group(0) @binding(3) var<storage, read> particle_volume: array<i32>;
@group(0) @binding(4) var<storage, read_write> heights: array<f32>;

@compute @workgroup_size(16, 16)
fn build(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.canvas_width || id.y >= window.canvas_height) { return; }
    let texel = vec2<i32>(id.xy);
    var total = 0;
    if (window_contains(texel)) {
        let local = vec2<u32>(texel - window.origin);
        for (var z = 0u; z < window.depth; z++) {
            let cell = window_cell(vec3<u32>(local, z));
            total += volume[cell] + particle_volume[cell];
        }
    } else {
        total = store[store_cell(id.xy)];
    }
    heights[id.y * window.canvas_width + id.x] = f32(total) / PAINT_SCALE;
}
