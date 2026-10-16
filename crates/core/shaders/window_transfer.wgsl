@group(0) @binding(0) var<uniform> window: WindowUniform;
@group(0) @binding(1) var<storage, read_write> volume: array<i32>;
@group(0) @binding(2) var<storage, read_write> store: array<i32>;
@group(0) @binding(3) var<storage, read_write> previous: array<i32>;

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
    var visible = no_visible_pigment();
    var height = 0;
    var medium = 0;
    for (var step = 0u; step < window.depth; step++) {
        let cell = window_cell(vec3<u32>(id.x, id.y, window.depth - 1u - step));
        let density_fixed = previous[cell];
        if (density_fixed <= 0) { continue; }
        let density = f32(density_fixed) / PAINT_SCALE;
        add_layer_below(&visible, density, previous_vec3(cell, 1u) / density, previous_vec3(cell, 4u) / density, 0.0);
        height += density_fixed;
        medium += previous[cell + 7u];
    }
    let destination = store_cell(vec2<u32>(pixel));
    let total = f32(height) / PAINT_SCALE;
    let absorption = visible_absorption(visible) * total;
    let scattering = visible_scattering(visible) * total;
    store[destination] = height;
    store[destination + 1u] = i32(round(absorption.x * PAINT_SCALE));
    store[destination + 2u] = i32(round(absorption.y * PAINT_SCALE));
    store[destination + 3u] = i32(round(absorption.z * PAINT_SCALE));
    store[destination + 4u] = i32(round(scattering.x * PAINT_SCALE));
    store[destination + 5u] = i32(round(scattering.y * PAINT_SCALE));
    store[destination + 6u] = i32(round(scattering.z * PAINT_SCALE));
    store[destination + 7u] = medium;
}

fn previous_vec3(cell: u32, offset: u32) -> vec3<f32> {
    return vec3<f32>(f32(previous[cell + offset]), f32(previous[cell + offset + 1u]), f32(previous[cell + offset + 2u])) / PAINT_SCALE;
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
    var medium = 0.0;
    if (on_canvas(pixel)) {
        let source = store_cell(vec2<u32>(pixel));
        height = f32(store[source]) / PAINT_SCALE;
        if (height > 0.0) {
            absorption = vec3<f32>(f32(store[source + 1u]), f32(store[source + 2u]), f32(store[source + 3u])) / PAINT_SCALE / height;
            scattering = vec3<f32>(f32(store[source + 4u]), f32(store[source + 5u]), f32(store[source + 6u])) / PAINT_SCALE / height;
            medium = f32(store[source + 7u]) / PAINT_SCALE / height;
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
        volume[cell + 7u] = i32(round(fill * medium * PAINT_SCALE));
    }
}

@compute @workgroup_size(16, 16)
fn unbake_all(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= window.width || id.y >= window.height) { return; }
    unbake_column(id.xy, vec2<i32>(id.xy) + window.origin);
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
