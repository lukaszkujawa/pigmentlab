
const OCCLUSION_STRENGTH: f32 = 0.6;
const EDGE_PAINT: f32 = 0.02;
const GLOSSY_PAINT: f32 = 0.015 * PIXELS_PER_CM;
const DRIED_SHEEN: f32 = 0.4;
const SHADOW_STEPS: i32 = 24;
const OCCLUSION_RADII: array<f32, 3> = array<f32, 3>(0.05 * PIXELS_PER_CM, 0.1 * PIXELS_PER_CM, 0.18 * PIXELS_PER_CM);

@group(0) @binding(1) var<storage, read> paint: array<i32>;
@group(0) @binding(2) var<uniform> composite: CompositeUniform;
@group(0) @binding(3) var<uniform> window: WindowUniform;
@group(0) @binding(4) var<storage, read> volume: array<i32>;
@group(0) @binding(5) var<storage, read> heights: array<f32>;
@group(0) @binding(6) var<storage, read> smooth_surface: array<f32>;
@group(0) @binding(7) var<storage, read> smooth_paint: array<f32>;
@group(0) @binding(8) var<storage, read> dry: array<vec4<f32>>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

fn volume_vec3(cell: u32, offset: u32) -> vec3<f32> {
    return vec3<f32>(f32(volume[cell + offset]), f32(volume[cell + offset + 1u]), f32(volume[cell + offset + 2u])) / PAINT_SCALE;
}

fn paint_vec3(cell: u32, offset: u32) -> vec3<f32> {
    return vec3<f32>(f32(paint[cell + offset]), f32(paint[cell + offset + 1u]), f32(paint[cell + offset + 2u])) / PAINT_SCALE;
}

fn column_pigment(texel: vec2<i32>) -> VisiblePigment {
    var visible = no_visible_pigment();
    if (texel.x < 0 || texel.y < 0 || texel.x >= i32(composite.canvas_width) || texel.y >= i32(composite.canvas_height)) {
        return visible;
    }
    if (window_contains(texel)) {
        let local = vec2<u32>(texel - window.origin);
        for (var step = 0u; step < window.depth; step++) {
            let cell = window_cell(vec3<u32>(local, window.depth - 1u - step));
            let density = f32(volume[cell]) / PAINT_SCALE;
            if (density <= 0.0) { continue; }
            add_layer_below(&visible, density, volume_vec3(cell, 1u) / density, volume_vec3(cell, 4u) / density, f32(volume[cell + 7u]) / PAINT_SCALE);
        }
    } else {
        let cell = store_cell(vec2<u32>(texel));
        let height = f32(paint[cell]) / PAINT_SCALE;
        if (height > 0.0) {
            add_layer_below(&visible, height, paint_vec3(cell, 1u) / height, paint_vec3(cell, 4u) / height, f32(paint[cell + 7u]) / PAINT_SCALE);
        }
    }
    return visible;
}

fn clamped_index(texel: vec2<i32>) -> u32 {
    let clamped = clamp(texel, vec2<i32>(0), vec2<i32>(i32(composite.canvas_width) - 1, i32(composite.canvas_height) - 1));
    return u32(clamped.y) * composite.canvas_width + u32(clamped.x);
}

fn height_at(texel: vec2<i32>) -> f32 {
    return smooth_surface[clamped_index(texel)];
}

fn occlusion(texel: vec2<i32>, height: f32) -> f32 {
    var total = 0.0;
    for (var direction = 0; direction < 8; direction++) {
        let angle = f32(direction) * 0.78539816;
        let step = vec2<f32>(cos(angle), sin(angle));
        for (var ring = 0; ring < 3; ring++) {
            let radius = OCCLUSION_RADII[ring];
            let neighbour = height_at(texel + vec2<i32>(round(step * radius)));
            total += clamp((neighbour - height - 0.1) / radius, 0.0, 1.0);
        }
    }
    return 1.0 - OCCLUSION_STRENGTH * total / 24.0;
}

fn shadow(texel: vec2<i32>, height: f32, light: vec3<f32>) -> f32 {
    let horizontal = length(light.xy);
    if (horizontal < 1e-3) { return 1.0; }
    let step = light.xy / horizontal;
    let rise = light.z / horizontal;
    var blocked = 0.0;
    for (var i = 1; i <= SHADOW_STEPS; i++) {
        let distance = f32(i);
        let sample_height = height_at(texel + vec2<i32>(round(step * distance)));
        let ray_height = height + distance * rise;
        blocked = max(blocked, clamp(sample_height - ray_height, 0.0, 1.0));
    }
    return 1.0 - composite.shadow_strength * blocked;
}

struct Surface {
    @location(0) albedo: vec4<f32>,
    @location(1) finish: vec4<f32>,
};

fn surface_of(albedo: vec3<f32>, texel: vec2<i32>, paint_height: f32, medium: f32, covered: bool) -> Surface {
    let height = height_at(texel);
    let wet = smoothstep(0.0, GLOSSY_PAINT, paint_height) * mix(0.7, 1.0, medium);
    let glossy = max(wet, smoothstep(0.0, GLOSSY_PAINT, dry[clamped_index(texel)].x) * DRIED_SHEEN);
    var surface: Surface;
    surface.albedo = vec4<f32>(clamp(albedo, vec3<f32>(0.0), vec3<f32>(1.0)), select(0.0, 1.0, covered));
    surface.finish = vec4<f32>(glossy, shadow(texel, height, light_direction()), occlusion(texel, height), 0.0);
    return surface;
}

fn painted(texel: vec2<i32>) -> bool {
    return heights[clamped_index(texel)] > EDGE_PAINT;
}

@fragment
fn fs_main(in: VertexOutput) -> Surface {
    let texel = vec2<i32>(in.position.xy);
    let base = dry[clamped_index(texel)].yzw;
    let covered = painted(texel);
    let column = column_pigment(texel);
    let height = column.height;
    if (height <= 1e-4) { return surface_of(base, texel, 0.0, 0.0, covered); }
    let k = max(visible_absorption(column), vec3<f32>(0.0));
    let s = max(visible_scattering(column), vec3<f32>(0.0));
    let thickness = max(mix(height, smooth_paint[clamped_index(texel)], 0.3), 1e-4);
    let albedo = layer_reflectance(k, s, thickness * OPTICAL_THICKNESS_PER_PIXEL, base);
    return surface_of(albedo, texel, height, clamp(column.medium / height, 0.0, 1.0), covered);
}
