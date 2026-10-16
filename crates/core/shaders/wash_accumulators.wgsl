const ACCUMULATOR_SCALE: f32 = 65536.0;
const ACCUMULATORS: u32 = 13u;

fn fixed(value: f32) -> i32 {
    return i32(round(value * ACCUMULATOR_SCALE));
}

fn accumulate(cell: u32, channel: u32, value: f32) {
    atomicAdd(&accumulators[cell * ACCUMULATORS + channel], fixed(value));
}

fn add_to_wash(cell: u32, water: f32, pigment: f32, absorption: vec3<f32>, scattering: vec3<f32>, behaviour: vec3<f32>) {
    accumulate(cell, 0u, water);
    accumulate(cell, 1u, absorption.x * pigment);
    accumulate(cell, 2u, absorption.y * pigment);
    accumulate(cell, 3u, absorption.z * pigment);
    accumulate(cell, 4u, pigment);
    accumulate(cell, 5u, scattering.x * pigment);
    accumulate(cell, 6u, scattering.y * pigment);
    accumulate(cell, 7u, scattering.z * pigment);
    accumulate(cell, 8u, behaviour.z * pigment);
    accumulate(cell, 9u, behaviour.x * pigment);
    accumulate(cell, 10u, behaviour.y * pigment);
}
