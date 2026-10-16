const BRIDGE_SPAN: i32 = 5;

fn bridged_touch(pixel: vec2<i32>) -> f32 {
    let directions = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(0, 1), vec2<i32>(1, 1), vec2<i32>(1, -1));
    if (already_baked(pixel)) { return 0.0; }
    var touch = hair_touch(pixel);
    if (touch >= OUTLINE_LEVEL + OUTLINE_SOFTNESS) { return touch; }
    for (var d = 0u; d < 4u; d++) {
        for (var span = 1; span <= BRIDGE_SPAN; span += 2) {
            let offset = directions[d] * span;
            touch = max(touch, min(hair_touch(pixel - offset), hair_touch(pixel + offset)));
        }
    }
    return touch;
}
