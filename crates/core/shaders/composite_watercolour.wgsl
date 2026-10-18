const SATURATED_WATER: f32 = 1.0;
const WET_DARKENING: f32 = 0.12;
const DAMP_DARKENING: f32 = 0.05;
const SHEEN_DEPTH: f32 = 0.15;

struct WaterUniform {
    cells: vec2<u32>,
    canvas: vec2<u32>,
    region_origin: vec2<i32>,
    region_size: vec2<u32>,
    dt: f32,
    absorbency: f32,
    granulation: f32,
    tooth: f32,
    force_dry: u32,
    scrub: f32,
    padding_0: f32,
    padding_1: f32,
};


@group(0) @binding(9) var<uniform> water_sim: WaterUniform;
@group(0) @binding(10) var<storage, read> water_cells: array<vec4<f32>>;
@group(0) @binding(11) var<storage, read> floating: array<Pigment>;
@group(0) @binding(12) var<storage, read> settled: array<Pigment>;
@group(0) @binding(13) var<storage, read> paper_cells: array<vec4<f32>>;
@group(0) @binding(14) var<storage, read> paper_relief: array<f32>;
@group(0) @binding(15) var<storage, read> hair_contact: array<u32>;
@group(0) @binding(16) var<storage, read> wetted_concentration: array<f32>;

struct WetSample {
    pigment: Pigment,
    water: vec2<f32>,
}

fn wet_index(cell: vec2<i32>) -> u32 {
    let clamped = clamp(cell, vec2<i32>(0), vec2<i32>(water_sim.cells) - 1);
    return u32(clamped.y) * water_sim.cells.x + u32(clamped.x);
}

fn wet_at(texel: vec2<i32>) -> WetSample {
    let taps = bilinear_cells(texel);
    var sample = WetSample(no_pigment(), vec2<f32>(0.0));
    for (var k = 0u; k < 4u; k++) {
        let index = wet_index(vec2<i32>(taps[k].xy));
        let w = taps[k].z;
        sample.pigment = added(sample.pigment, scaled(added(floating[index], settled[index]), w));
        sample.water += water_cells[index].xy * w;
    }
    return sample;
}

fn hair_touch(texel: vec2<i32>) -> f32 {
    if (texel.x < 0 || texel.y < 0 || u32(texel.x) >= water_sim.canvas.x || u32(texel.y) >= water_sim.canvas.y) { return 0.0; }
    return f32(hair_contact[u32(texel.y) * water_sim.canvas.x + u32(texel.x)] & ~BAKED) / HAIR_TOUCH_SCALE;
}

fn already_baked(texel: vec2<i32>) -> bool {
    if (texel.x < 0 || texel.y < 0 || u32(texel.x) >= water_sim.canvas.x || u32(texel.y) >= water_sim.canvas.y) { return false; }
    return (hair_contact[u32(texel.y) * water_sim.canvas.x + u32(texel.x)] & BAKED) != 0u;
}

fn wetted_inside(texel: vec2<i32>) -> f32 {
    let touch = bridged_touch(texel);
    let taps = bilinear_cells(texel);
    var reach = 0.0;
    for (var k = 0u; k < 4u; k++) {
        let index = wet_index(vec2<i32>(taps[k].xy));
        let state = water_cells[index];
        if (state.z > 1.5) { reach += taps[k].z * state.y / (CAPILLARY_WETTING * paper_cells[index].y); }
    }
    return outline_from(touch, reach);
}

fn pigment_per_pixel(texel: vec2<i32>) -> f32 {
    let taps = bilinear_cells(texel);
    var total = 0.0;
    var weight = 0.0;
    for (var k = 0u; k < 4u; k++) {
        let index = wet_index(vec2<i32>(taps[k].xy));
        if (floating[index].absorption.w + settled[index].absorption.w <= 1e-7) { continue; }
        total += wetted_concentration[index] * taps[k].z;
        weight += taps[k].z;
    }
    return select(0.0, total / weight, weight > 0.0);
}

@fragment
fn fs_watercolour(in: VertexOutput) -> Surface {
    let texel = vec2<i32>(in.position.xy);
    let index = clamped_index(texel);
    var albedo = dry[index].yzw;
    let wet = wet_at(texel);
    let inside = wetted_inside(texel);
    let total = mass(wet.pigment);
    if (total > 1e-7 && inside > 0.0) {
        let height = paper_relief[index] / max(water_sim.tooth, 1e-6) * 0.5 + 0.5;
        let granulation = wet.pigment.scattering.w / total * water_sim.granulation;
        let concentration = pigment_per_pixel(texel);
        let settling = granular_settling(height, granulation, concentration);
        let thickness = granular_thickness(settling, concentration) * inside * OPTICAL_THICKNESS_PER_PIXEL;
        let absorption = separated_absorption(wet.pigment.absorption.xyz / total, granulating_absorption(wet.pigment), settling);
        albedo = layer_reflectance(absorption, wet.pigment.scattering.xyz / total, thickness, albedo);
    }
    let soaked = clamp(wet.water.x / SATURATED_WATER, 0.0, 1.0) * inside;
    let damp = clamp(wet.water.y, 0.0, 1.0) * inside;
    albedo *= 1.0 - WET_DARKENING * soaked - DAMP_DARKENING * damp * (1.0 - soaked);
    return surface_of(albedo, texel, smoothstep(0.0, SHEEN_DEPTH, wet.water.x) * inside * PIXELS_PER_CM * 0.015, 1.0, true);
}
