fn make_room_on_top(local: vec2<u32>) {
    let bottom = window_cell(vec3<u32>(local, 0u));
    let above_bottom = window_cell(vec3<u32>(local, 1u));
    for (var channel = 0u; channel < CHANNELS; channel++) {
        volume[bottom + channel] += volume[above_bottom + channel];
    }
    for (var z = 1u; z + 1u < window.depth; z++) {
        let cell = window_cell(vec3<u32>(local, z));
        let above = window_cell(vec3<u32>(local, z + 1u));
        for (var channel = 0u; channel < CHANNELS; channel++) {
            volume[cell + channel] = volume[above + channel];
        }
    }
    let top = window_cell(vec3<u32>(local, window.depth - 1u));
    for (var channel = 0u; channel < CHANNELS; channel++) {
        volume[top + channel] = 0;
    }
}

fn free_cell(local: vec2<u32>) -> u32 {
    for (var z = 0u; z < window.depth; z++) {
        let cell = window_cell(vec3<u32>(local, z));
        if (f32(volume[cell]) / PAINT_SCALE < FULL_CELL) { return cell; }
    }
    make_room_on_top(local);
    return window_cell(vec3<u32>(local, window.depth - 1u));
}
