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

struct WetPigment {
    absorption: vec4<f32>,
    scattering: vec4<f32>,
    behaviour: vec4<f32>,
};

@group(0) @binding(9) var<uniform> water_sim: WaterUniform;
@group(0) @binding(10) var<storage, read> water_cells: array<vec4<f32>>;
@group(0) @binding(11) var<storage, read> floating: array<WetPigment>;
@group(0) @binding(12) var<storage, read> settled: array<WetPigment>;
@group(0) @binding(13) var<storage, read> paper_cells: array<vec4<f32>>;
@group(0) @binding(14) var<storage, read> paper_relief: array<f32>;
@group(0) @binding(15) var<storage, read> wash_coverage: array<vec2<f32>>;

struct WetSample {
    absorption: vec4<f32>,
    scattering: vec4<f32>,
    water: vec2<f32>,
    paper: f32,
    coverage: vec2<f32>,
}

fn wet_cell(cell: vec2<i32>) -> WetSample {
    let clamped = clamp(cell, vec2<i32>(0), vec2<i32>(water_sim.cells) - 1);
    let index = u32(clamped.y) * water_sim.cells.x + u32(clamped.x);
    let a = floating[index];
    let b = settled[index];
    return WetSample(a.absorption + b.absorption, a.scattering + b.scattering, water_cells[index].xy, paper_cells[index].x, wash_coverage[index]);
}

fn wet_at(texel: vec2<i32>) -> WetSample {
    let position = (vec2<f32>(texel) + 0.5) / 2.0 - 0.5;
    let base = vec2<i32>(floor(position));
    let f = position - floor(position);
    let s00 = wet_cell(base);
    let s10 = wet_cell(base + vec2<i32>(1, 0));
    let s01 = wet_cell(base + vec2<i32>(0, 1));
    let s11 = wet_cell(base + vec2<i32>(1, 1));
    let w = vec4<f32>((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
    return WetSample(
        s00.absorption * w.x + s10.absorption * w.y + s01.absorption * w.z + s11.absorption * w.w,
        s00.scattering * w.x + s10.scattering * w.y + s01.scattering * w.z + s11.scattering * w.w,
        s00.water * w.x + s10.water * w.y + s01.water * w.z + s11.water * w.w,
        s00.paper * w.x + s10.paper * w.y + s01.paper * w.z + s11.paper * w.w,
        s00.coverage * w.x + s10.coverage * w.y + s01.coverage * w.z + s11.coverage * w.w,
    );
}

@fragment
fn fs_watercolour(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(in.position.xy);
    let index = clamped_index(texel);
    var albedo = dry[index].yzw;
    let wet = wet_at(texel);
    let total = wet.absorption.w;
    if (total > 1e-7) {
        let height = paper_relief[index] / max(water_sim.tooth, 1e-6) * 0.5 + 0.5;
        let granulation = wet.scattering.w / total * water_sim.granulation;
        let weight = max(1.0 + granulation * (wet.paper - height) * 2.5, 0.3);
        let inside = smoothstep(0.35, 0.6, wet.coverage.y) / max(wet.coverage.y, 0.25);
        let thickness = total / 4.0 * weight * min(inside, 2.0) * OPTICAL_THICKNESS_PER_PIXEL;
        albedo = layer_reflectance(wet.absorption.xyz / total, wet.scattering.xyz / total, thickness, albedo);
    }
    let soaked = clamp(wet.water.x / SATURATED_WATER, 0.0, 1.0);
    let damp = clamp(wet.water.y, 0.0, 1.0);
    albedo *= 1.0 - WET_DARKENING * soaked - DAMP_DARKENING * damp * (1.0 - soaked);
    let colour = shade(albedo, texel, smoothstep(0.0, SHEEN_DEPTH, wet.water.x) * PIXELS_PER_CM * 0.015, 1.0);
    return vec4<f32>(clamp(colour, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
