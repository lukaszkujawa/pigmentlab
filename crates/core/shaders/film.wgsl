struct FilmBatch {
    count: u32,
};

@group(0) @binding(0) var<uniform> window: WindowUniform;
@group(0) @binding(1) var<storage, read_write> volume: array<i32>;
@group(0) @binding(2) var<storage, read_write> store: array<i32>;
@group(0) @binding(3) var<storage, read> changes: array<i32>;
@group(0) @binding(4) var<uniform> batch: FilmBatch;

const ENTRY: u32 = 9u;

fn top_layer(local: vec2<u32>) -> u32 {
    var top = 0u;
    for (var z = 0u; z < window.depth; z++) {
        if (volume[window_cell(vec3<u32>(local, z))] > 0) { top = z; }
    }
    return top;
}

fn apply_to_column(local: vec2<u32>, entry: u32) {
    var remaining: array<f32, 8>;
    for (var channel = 0u; channel < CHANNELS; channel++) {
        remaining[channel] = f32(changes[entry + 1u + channel]);
    }
    var z = top_layer(local);
    loop {
        let base = window_cell(vec3<u32>(local, z));
        let held = f32(volume[base]);
        let room = FULL_CELL * PAINT_SCALE - held;
        var share = 1.0;
        if (remaining[0] > room && z + 1u < window.depth && remaining[0] > 0.0) {
            share = max(room, 0.0) / remaining[0];
        } else if (remaining[0] < -held && z > 0u) {
            share = held / -remaining[0];
        }
        for (var channel = 0u; channel < CHANNELS; channel++) {
            let part = round(remaining[channel] * share);
            volume[base + channel] = max(volume[base + channel] + i32(part), 0);
            remaining[channel] -= part;
        }
        if (share >= 1.0 || abs(remaining[0]) < 1.0) { return; }
        if (remaining[0] > 0.0) { z += 1u; } else { z -= 1u; }
    }
}

@compute @workgroup_size(64)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= batch.count) { return; }
    let entry = id.x * ENTRY;
    let index = u32(changes[entry]);
    let pixel = vec2<i32>(i32(index % window.canvas_width), i32(index / window.canvas_width));
    if (window_contains(pixel)) {
        apply_to_column(vec2<u32>(pixel - window.origin), entry);
        return;
    }
    let cell = store_cell(vec2<u32>(pixel));
    for (var channel = 0u; channel < CHANNELS; channel++) {
        store[cell + channel] = max(store[cell + channel] + changes[entry + 1u + channel], 0);
    }
}
