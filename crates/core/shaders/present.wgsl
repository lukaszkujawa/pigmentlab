const LIGHTING_FLOOR: f32 = 0.35;
const EDGE_SOFTNESS_PIXELS: f32 = 1.5;
const EDGE_SOFTNESS_CELLS: f32 = 1.0;
const ALBEDO_SHARPENING: f32 = 0.6;
const RELIEF_SHARPENING: f32 = 0.5;
const MAX_SUPERSAMPLES: f32 = 3.0;
const OIL_REFLECTANCE: f32 = 0.04;
const LAMP_RADIANCE: f32 = 14.0;
const MATTE_LAMP_RADIUS: f32 = 0.45;
const GLOSSY_LAMP_RADIUS: f32 = 0.12;
const WINDOW_RADIANCE: f32 = 5.0;
const WINDOW_RADIUS: f32 = 0.7;

@group(0) @binding(0) var albedo_map: texture_2d<f32>;
@group(0) @binding(1) var finish_map: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> surface: array<f32>;
@group(0) @binding(3) var<uniform> composite: CompositeUniform;
@group(0) @binding(4) var<uniform> view: ViewUniform;

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

struct Patch {
    value: f32,
    slope: vec2<f32>,
};

struct Look {
    albedo: vec3<f32>,
    finish: vec3<f32>,
};

fn canvas_extent() -> vec2<i32> {
    return vec2<i32>(i32(composite.canvas_width), i32(composite.canvas_height));
}

fn on_canvas_texel(texel: vec2<i32>) -> vec2<i32> {
    return clamp(texel, vec2<i32>(0), canvas_extent() - 1);
}

fn surface_at(texel: vec2<i32>) -> f32 {
    let clamped = on_canvas_texel(texel);
    return surface[u32(clamped.y) * composite.canvas_width + u32(clamped.x)];
}

fn b_spline(t: f32) -> vec4<f32> {
    let u = 1.0 - t;
    return vec4<f32>(u * u * u, 3.0 * t * t * t - 6.0 * t * t + 4.0, -3.0 * t * t * t + 3.0 * t * t + 3.0 * t + 1.0, t * t * t) / 6.0;
}

fn b_spline_slope(t: f32) -> vec4<f32> {
    let u = 1.0 - t;
    return vec4<f32>(-u * u, 3.0 * t * t - 4.0 * t, -3.0 * t * t + 2.0 * t + 1.0, t * t) / 2.0;
}

fn catmull_rom(t: f32) -> vec4<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4<f32>(-t3 + 2.0 * t2 - t, 3.0 * t3 - 5.0 * t2 + 2.0, -3.0 * t3 + 4.0 * t2 + t, t3 - t2) * 0.5;
}

fn catmull_rom_slope(t: f32) -> vec4<f32> {
    let t2 = t * t;
    return vec4<f32>(-3.0 * t2 + 4.0 * t - 1.0, 9.0 * t2 - 10.0 * t, -9.0 * t2 + 8.0 * t + 1.0, 3.0 * t2 - 2.0 * t) * 0.5;
}

fn relief(corner: vec2<i32>, f: vec2<f32>) -> Patch {
    let wx = mix(b_spline(f.x), catmull_rom(f.x), RELIEF_SHARPENING);
    let wy = mix(b_spline(f.y), catmull_rom(f.y), RELIEF_SHARPENING);
    let dx = mix(b_spline_slope(f.x), catmull_rom_slope(f.x), RELIEF_SHARPENING);
    let dy = mix(b_spline_slope(f.y), catmull_rom_slope(f.y), RELIEF_SHARPENING);
    var sampled = Patch(0.0, vec2<f32>(0.0));
    for (var j = 0; j < 4; j++) {
        for (var i = 0; i < 4; i++) {
            let height = surface_at(corner + vec2<i32>(i, j));
            sampled.value += wx[i] * wy[j] * height;
            sampled.slope += vec2<f32>(dx[i] * wy[j], wx[i] * dy[j]) * height;
        }
    }
    return sampled;
}

struct Tally {
    albedo: vec3<f32>,
    finish: vec3<f32>,
    weight: f32,
};

fn tallied(tally: Tally, albedo: vec3<f32>, finish: vec3<f32>, weight: f32) -> Tally {
    return Tally(tally.albedo + albedo * weight, tally.finish + finish * weight, tally.weight + weight);
}

fn averaged(tally: Tally) -> Look {
    return Look(tally.albedo / tally.weight, tally.finish / tally.weight);
}

fn class_look(near: Tally, spread: Tally) -> Look {
    if (near.weight <= 1e-6) { return averaged(spread); }
    let sharp = averaged(near);
    let soft = averaged(spread);
    return Look(max(sharp.albedo + (sharp.albedo - soft.albedo) * ALBEDO_SHARPENING, vec3<f32>(0.0)), sharp.finish);
}

fn look_at(p: vec2<f32>) -> Look {
    let q = p - 0.5;
    let corner = vec2<i32>(floor(q)) - 1;
    let f = fract(q);
    let spread_x = b_spline(f.x);
    let spread_y = b_spline(f.y);
    let spread_dx = b_spline_slope(f.x);
    let spread_dy = b_spline_slope(f.y);
    let near_x = vec4<f32>(0.0, 1.0 - f.x, f.x, 0.0);
    let near_y = vec4<f32>(0.0, 1.0 - f.y, f.y, 0.0);
    let empty = Tally(vec3<f32>(0.0), vec3<f32>(0.0), 0.0);
    var mask = Patch(0.0, vec2<f32>(0.0));
    var near_in = empty;
    var near_out = empty;
    var spread_in = empty;
    var spread_out = empty;
    for (var j = 0; j < 4; j++) {
        for (var i = 0; i < 4; i++) {
            let texel = on_canvas_texel(corner + vec2<i32>(i, j));
            let albedo = textureLoad(albedo_map, texel, 0);
            let finish = textureLoad(finish_map, texel, 0).xyz;
            let covered = albedo.a;
            let near = near_x[i] * near_y[j];
            let spread = spread_x[i] * spread_y[j];
            mask.value += spread * covered;
            mask.slope += vec2<f32>(spread_dx[i] * spread_y[j], spread_x[i] * spread_dy[j]) * covered;
            near_in = tallied(near_in, albedo.rgb, finish, near * covered);
            near_out = tallied(near_out, albedo.rgb, finish, near * (1.0 - covered));
            spread_in = tallied(spread_in, albedo.rgb, finish, spread * covered);
            spread_out = tallied(spread_out, albedo.rgb, finish, spread * (1.0 - covered));
        }
    }
    if (spread_in.weight <= 1e-6) { return class_look(near_out, spread_out); }
    if (spread_out.weight <= 1e-6) { return class_look(near_in, spread_in); }
    let inside = class_look(near_in, spread_in);
    let outside = class_look(near_out, spread_out);
    let distance = (mask.value - 0.5) / max(length(mask.slope), 1e-3);
    let magnification = max(view.scale, 1.0);
    let softness = max(EDGE_SOFTNESS_CELLS / (magnification * magnification), EDGE_SOFTNESS_PIXELS / view.scale);
    let cover = clamp(0.5 + distance / softness, 0.0, 1.0);
    return Look(mix(outside.albedo, inside.albedo, cover), mix(outside.finish, inside.finish, cover));
}

fn disc_seen(away: f32, radius: f32) -> f32 {
    return 1.0 - smoothstep(radius * 0.5, radius * 1.5, away);
}

fn reflection(normal: vec3<f32>, light: vec3<f32>, glossy: f32) -> f32 {
    let mirrored = reflect(vec3<f32>(0.0, 0.0, -1.0), normal);
    let away = acos(clamp(dot(mirrored, light), -1.0, 1.0));
    let lamp_radius = mix(MATTE_LAMP_RADIUS, GLOSSY_LAMP_RADIUS, composite.gloss);
    let lamp = disc_seen(away, lamp_radius) * LAMP_RADIANCE * GLOSSY_LAMP_RADIUS * GLOSSY_LAMP_RADIUS / (lamp_radius * lamp_radius);
    let window = disc_seen(away, WINDOW_RADIUS) * WINDOW_RADIANCE;
    let fresnel = OIL_REFLECTANCE + (1.0 - OIL_REFLECTANCE) * pow(1.0 - normal.z, 5.0);
    return (lamp + window) * fresnel * glossy;
}

fn shaded(p: vec2<f32>) -> vec3<f32> {
    let look = look_at(p);
    let q = p - 0.5;
    let height = relief(vec2<i32>(floor(q)) - 1, fract(q));
    let glossy = mix(look.finish.x, 1.0, composite.varnish);
    let veil = OIL_REFLECTANCE * (1.0 - look.finish.x) * composite.varnish;
    let albedo = max(look.albedo - veil, vec3<f32>(0.0)) / (1.0 - veil);
    let lit = look.finish.y;
    let occlusion = look.finish.z;
    let normal = normalize(vec3<f32>(-height.slope, 1.0));
    let light = light_direction();
    let diffuse = min(max(dot(normal, light), 0.0) / max(light.z, 0.2), 1.5);
    let ambient = composite.ambient * occlusion;
    let lighting = max(ambient + (1.0 - composite.ambient) * diffuse * lit, LIGHTING_FLOOR);
    let specular = reflection(normal, light, glossy) * lit;
    return clamp(albedo * lighting + vec3<f32>(specular), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn coverage(centre: vec2<f32>, left: f32, right: f32) -> f32 {
    let edge = min(min(centre.x - left, right - centre.x), min(centre.y, f32(composite.canvas_height) - centre.y));
    return clamp(edge * view.scale + 0.5, 0.0, 1.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let centre = (in.position.xy - view.origin) / view.scale;
    let extent = vec2<f32>(canvas_extent());
    let shown = max(coverage(centre, 0.0, f32(composite.painting_width)), coverage(centre, f32(composite.palette_start), extent.x));
    if (shown <= 0.0) { return vec4<f32>(0.0); }
    let samples = i32(clamp(ceil(1.0 / view.scale), 1.0, MAX_SUPERSAMPLES));
    let footprint = 1.0 / view.scale;
    var colour = vec3<f32>(0.0);
    for (var j = 0; j < samples; j++) {
        for (var i = 0; i < samples; i++) {
            let offset = (vec2<f32>(f32(i), f32(j)) + 0.5) / f32(samples) - 0.5;
            colour += shaded(clamp(centre + offset * footprint, vec2<f32>(0.0), extent));
        }
    }
    colour /= f32(samples * samples);
    return vec4<f32>(colour * shown, shown);
}
