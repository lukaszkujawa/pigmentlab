struct CompositeUniform {
    canvas_width: u32,
    canvas_height: u32,
    optical_thickness_per_pixel: f32,
    light_azimuth: f32,
    light_elevation: f32,
    gloss: f32,
    ambient: f32,
    shadow_strength: f32,
};

const OCCLUSION_STRENGTH: f32 = 0.6;
const SHADOW_STEPS: i32 = 24;
const OCCLUSION_RADII: array<f32, 3> = array<f32, 3>(0.05 * PIXELS_PER_CM, 0.1 * PIXELS_PER_CM, 0.18 * PIXELS_PER_CM);

@group(0) @binding(0) var substrate: texture_2d<f32>;
@group(0) @binding(1) var<storage, read> paint: array<i32>;
@group(0) @binding(2) var<uniform> composite: CompositeUniform;
@group(0) @binding(3) var<uniform> window: WindowUniform;
@group(0) @binding(4) var<storage, read> volume: array<i32>;
@group(0) @binding(5) var<storage, read> particle_volume: array<i32>;
@group(0) @binding(6) var<storage, read> heights: array<f32>;

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

fn opaque_reflectance(k: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    let ratio = k / max(s, vec3<f32>(1e-6));
    return 1.0 + ratio - sqrt(ratio * ratio + 2.0 * ratio);
}

fn layer_reflectance(k: vec3<f32>, s: vec3<f32>, thickness: f32, substrate_color: vec3<f32>) -> vec3<f32> {
    let safe_s = max(s, vec3<f32>(1e-5));
    let a = 1.0 + k / safe_s;
    let b = sqrt(max(a * a - 1.0, vec3<f32>(0.0)));
    let argument = min(b * safe_s * thickness, vec3<f32>(20.0));
    let coth = 1.0 / tanh(max(argument, vec3<f32>(1e-6)));
    let layered = (1.0 - substrate_color * (a - b * coth)) / (a - substrate_color + b * coth);
    let absorbing_only = substrate_color * exp(-2.0 * k * thickness);
    let opaque = opaque_reflectance(k, safe_s);
    let thin_scatter = select(layered, absorbing_only, s <= vec3<f32>(1e-5));
    return select(thin_scatter, opaque, argument >= vec3<f32>(20.0));
}

fn column_sum(texel: vec2<i32>) -> array<i32, 7> {
    var column: array<i32, 7>;
    for (var channel = 0u; channel < 7u; channel++) { column[channel] = 0; }
    if (texel.x < 0 || texel.y < 0 || texel.x >= i32(composite.canvas_width) || texel.y >= i32(composite.canvas_height)) {
        return column;
    }
    if (window_contains(texel)) {
        let local = vec2<u32>(texel - window.origin);
        for (var z = 0u; z < window.depth; z++) {
            let cell = window_cell(vec3<u32>(local, z));
            for (var channel = 0u; channel < 7u; channel++) { column[channel] += volume[cell + channel] + particle_volume[cell + channel]; }
        }
    } else {
        let cell = store_cell(vec2<u32>(texel));
        for (var channel = 0u; channel < 7u; channel++) { column[channel] = paint[cell + channel]; }
    }
    return column;
}

fn height_at(texel: vec2<i32>) -> f32 {
    let clamped = clamp(texel, vec2<i32>(0), vec2<i32>(i32(composite.canvas_width) - 1, i32(composite.canvas_height) - 1));
    return heights[u32(clamped.y) * composite.canvas_width + u32(clamped.x)];
}

fn smooth_height(texel: vec2<i32>) -> f32 {
    var total = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            total += height_at(texel + vec2<i32>(dx, dy));
        }
    }
    return total / 9.0;
}

fn light_direction() -> vec3<f32> {
    let elevation = radians(composite.light_elevation);
    let azimuth = radians(composite.light_azimuth);
    return vec3<f32>(cos(elevation) * cos(azimuth), cos(elevation) * sin(azimuth), sin(elevation));
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

fn shade(albedo: vec3<f32>, texel: vec2<i32>, height: f32) -> vec3<f32> {
    let relief = smoothstep(0.0, 0.015 * PIXELS_PER_CM, height);
    let slope_x = (smooth_height(texel + vec2<i32>(1, 0)) - smooth_height(texel - vec2<i32>(1, 0))) * 0.5 * relief;
    let slope_y = (smooth_height(texel + vec2<i32>(0, 1)) - smooth_height(texel - vec2<i32>(0, 1))) * 0.5 * relief;
    let normal = normalize(vec3<f32>(-slope_x, -slope_y, 1.0));
    let light = light_direction();
    let diffuse = min(max(dot(normal, light), 0.0) / max(light.z, 0.2), 1.5);
    let halfway = normalize(light + vec3<f32>(0.0, 0.0, 1.0));
    let shininess = mix(8.0, 96.0, composite.gloss);
    let specular = pow(max(dot(normal, halfway), 0.0), shininess) * composite.gloss * 0.5 * relief;
    let lit = shadow(texel, height, light);
    let ambient = composite.ambient * occlusion(texel, height);
    return albedo * (ambient + (1.0 - composite.ambient) * diffuse * lit) + vec3<f32>(specular * lit);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(in.position.xy);
    let paper = textureLoad(substrate, texel, 0).rgb;
    let column = column_sum(texel);
    let height = f32(column[0]) / PAINT_SCALE;
    if (height <= 1e-4) {
        return vec4<f32>(shade(paper, texel, 0.0), 1.0);
    }
    let k = vec3<f32>(f32(column[1]), f32(column[2]), f32(column[3])) / PAINT_SCALE / height;
    let s = vec3<f32>(f32(column[4]), f32(column[5]), f32(column[6])) / PAINT_SCALE / height;
    let thickness = max(mix(height, smooth_height(texel), 0.3), 1e-4);
    let albedo = layer_reflectance(max(k, vec3<f32>(0.0)), max(s, vec3<f32>(0.0)), thickness * composite.optical_thickness_per_pixel, paper);
    let color = shade(albedo, texel, height);
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
