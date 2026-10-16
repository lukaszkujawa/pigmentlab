const HAIR_TOUCH_SCALE: f32 = 65535.0;
const OUTLINE_LEVEL: f32 = 0.5;
const OUTLINE_SOFTNESS: f32 = 0.45;
const CAPILLARY_WETTING: f32 = 0.35;
const CAPILLARY_SOFTNESS: f32 = 0.2;
const SOAKED_MASK: f32 = 2.0;
const PAPER_MID_HEIGHT: f32 = 0.5;

fn outline_from(touch: f32, capillary_reach: f32) -> f32 {
    let brushed = smoothstep(OUTLINE_LEVEL - OUTLINE_SOFTNESS, OUTLINE_LEVEL + OUTLINE_SOFTNESS, touch);
    let soaked = smoothstep(1.0 - CAPILLARY_SOFTNESS, 1.0 + CAPILLARY_SOFTNESS, capillary_reach);
    return max(brushed, soaked);
}

fn bilinear_cells(pixel: vec2<i32>) -> array<vec3<f32>, 4> {
    let position = (vec2<f32>(pixel) + 0.5) / 2.0 - 0.5;
    let base = floor(position);
    let f = position - base;
    return array<vec3<f32>, 4>(
        vec3<f32>(base, (1.0 - f.x) * (1.0 - f.y)),
        vec3<f32>(base + vec2<f32>(1.0, 0.0), f.x * (1.0 - f.y)),
        vec3<f32>(base + vec2<f32>(0.0, 1.0), (1.0 - f.x) * f.y),
        vec3<f32>(base + vec2<f32>(1.0, 1.0), f.x * f.y),
    );
}
