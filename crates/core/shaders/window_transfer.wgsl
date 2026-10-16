@group(0) @binding(0) var<uniform> window: WindowUniform;
@group(0) @binding(1) var<storage, read_write> volume: array<i32>;
@group(0) @binding(2) var<storage, read_write> store: array<i32>;
@group(0) @binding(3) var<storage, read_write> previous: array<i32>;

fn on_canvas(pixel: vec2<i32>) -> bool {
    return pixel.x >= 0 && pixel.y >= 0 && pixel.x < i32(window.canvas_width) && pixel.y < i32(window.canvas_height);
}

fn previous_window_contains(pixel: vec2<i32>) -> bool {
    let local = pixel - window.previous_origin;
    return local.x >= 0 && local.y >= 0 && local.x < i32(window.width) && local.y < i32(window.height);
}

@compute @workgroup_size(16, 16)
fn bake(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    let pixel = vec2<i32>(id.xy) + window.previous_origin;
    let relocating = any(window.previous_origin != window.origin);
    if (!on_canvas(pixel) || (relocating && window_contains(pixel))) { return; }
    var total: array<i32, 7>;
    for (var channel = 0u; channel < 7u; channel++) { total[channel] = 0; }
    for (var z = 0u; z < window.depth; z++) {
        let cell = window_cell(vec3<u32>(id.x, id.y, z));
        for (var channel = 0u; channel < 7u; channel++) {
            total[channel] += previous[cell + channel];
        }
    }
    let destination = store_cell(vec2<u32>(pixel));
    for (var channel = 0u; channel < 7u; channel++) {
        store[destination + channel] = total[channel];
    }
    store[destination + 7u] = 0;
}

fn carry_column(local: vec2<u32>, previous_local: vec2<u32>) {
    for (var z = 0u; z < window.depth; z++) {
        let cell = window_cell(vec3<u32>(local, z));
        let source = window_cell(vec3<u32>(previous_local, z));
        for (var channel = 0u; channel < CHANNELS; channel++) {
            volume[cell + channel] = previous[source + channel];
        }
    }
}

fn unbake_column(local: vec2<u32>, pixel: vec2<i32>) {
    var height = 0.0;
    var absorption = vec3<f32>(0.0);
    var scattering = vec3<f32>(0.0);
    if (on_canvas(pixel)) {
        let source = store_cell(vec2<u32>(pixel));
        height = f32(store[source]) / PAINT_SCALE;
        if (height > 0.0) {
            absorption = vec3<f32>(f32(store[source + 1u]), f32(store[source + 2u]), f32(store[source + 3u])) / PAINT_SCALE / height;
            scattering = vec3<f32>(f32(store[source + 4u]), f32(store[source + 5u]), f32(store[source + 6u])) / PAINT_SCALE / height;
        }
    }
    for (var z = 0u; z < window.depth; z++) {
        let cell = window_cell(vec3<u32>(local, z));
        let fill = select(clamp(height - f32(z), 0.0, 1.0), max(height - f32(z), 0.0), z == window.depth - 1u);
        volume[cell] = i32(round(fill * PAINT_SCALE));
        volume[cell + 1u] = i32(round(fill * absorption.x * PAINT_SCALE));
        volume[cell + 2u] = i32(round(fill * absorption.y * PAINT_SCALE));
        volume[cell + 3u] = i32(round(fill * absorption.z * PAINT_SCALE));
        volume[cell + 4u] = i32(round(fill * scattering.x * PAINT_SCALE));
        volume[cell + 5u] = i32(round(fill * scattering.y * PAINT_SCALE));
        volume[cell + 6u] = i32(round(fill * scattering.z * PAINT_SCALE));
        volume[cell + 7u] = 0;
    }
}

@compute @workgroup_size(16, 16)
fn relocate(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    let pixel = vec2<i32>(id.xy) + window.origin;
    if (previous_window_contains(pixel)) {
        carry_column(id.xy, vec2<u32>(pixel - window.previous_origin));
    } else {
        unbake_column(id.xy, pixel);
    }
}
