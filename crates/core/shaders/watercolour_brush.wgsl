const MAX_SEGMENT_STEPS: u32 = 16u;
const JUMP_DISTANCE: f32 = 0.4 * PIXELS_PER_CM;
const CONTACT_BAND: f32 = 0.75;
const SATURATED_DEPTH: f32 = 1.0;
const GIVE_RATE: f32 = 3.0;
const TAKE_RATE: f32 = 2.0;
const WATER_PER_LOAD: f32 = 0.25;
const MAX_LIFT_FRACTION: f32 = 0.5;
const WICKING_PER_SECOND: f32 = 6.0;
const BLEED_RATE: f32 = 1.5;
const BLEED_ONSET: f32 = 4.0;
const ACCUMULATOR_SCALE: f32 = 65536.0;
const ACCUMULATORS: u32 = 13u;

struct BrushWaterUniform {
    sample_count: u32,
    samples_per_bristle: u32,
    sample_capacity: f32,
    dt: f32,
    cells: vec2<u32>,
    canvas: vec2<u32>,
};

struct Pigment {
    absorption: vec4<f32>,
    scattering: vec4<f32>,
    behaviour: vec4<f32>,
};

@group(0) @binding(0) var<uniform> exchange_uniform: BrushWaterUniform;
@group(0) @binding(1) var<storage, read> chain: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> previous_chain: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> sample_load: array<f32>;
@group(0) @binding(4) var<storage, read_write> sample_absorption: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> sample_scattering: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read_write> sample_behaviour: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read_write> accumulators: array<atomic<i32>>;
@group(0) @binding(8) var<storage, read> water: array<vec4<f32>>;
@group(0) @binding(9) var<storage, read> suspended: array<Pigment>;
@group(0) @binding(10) var<storage, read_write> lift_demand: array<atomic<i32>>;
@group(0) @binding(11) var<storage, read_write> lift: array<Pigment>;
@group(0) @binding(12) var<storage, read_write> sample_request: array<vec2<f32>>;
@group(0) @binding(13) var<storage, read> ground: array<f32>;
@group(0) @binding(14) var<storage, read_write> sample_concentration_at_start: array<f32>;
@group(0) @binding(15) var<storage, read> deposited: array<Pigment>;

struct Touch {
    cell: u32,
    point: vec3<f32>,
    give: f32,
    take: f32,
    bleed: f32,
}

fn fixed(value: f32) -> i32 {
    return i32(round(value * ACCUMULATOR_SCALE));
}

fn accumulate(cell: u32, channel: u32, value: f32) {
    atomicAdd(&accumulators[cell * ACCUMULATORS + channel], fixed(value));
}

fn cell_wetness(state: vec4<f32>) -> f32 {
    return clamp((state.x + 0.5 * state.y) / SATURATED_DEPTH, 0.0, 1.0);
}

fn segment_point(sample: u32, step: u32, steps: u32) -> vec3<f32> {
    let bristle = sample / exchange_uniform.samples_per_bristle;
    let t = f32(sample % exchange_uniform.samples_per_bristle) / f32(exchange_uniform.samples_per_bristle - 1u);
    let end = curve_point(false, bristle, t);
    var start = curve_point(true, bristle, t);
    if (distance(start.xy, end.xy) > JUMP_DISTANCE) { start = end; }
    return mix(start, end, f32(step) / f32(steps));
}

fn segment_steps(sample: u32) -> u32 {
    let bristle = sample / exchange_uniform.samples_per_bristle;
    let t = f32(sample % exchange_uniform.samples_per_bristle) / f32(exchange_uniform.samples_per_bristle - 1u);
    let travelled = distance(curve_point(false, bristle, t).xy, curve_point(true, bristle, t).xy);
    return clamp(u32(ceil(select(travelled, 0.0, travelled > JUMP_DISTANCE))), 1u, MAX_SEGMENT_STEPS);
}

fn touch_at(point: vec3<f32>, load: f32, share: f32, sample_concentration: f32) -> Touch {
    let pixel = vec2<i32>(floor(point.xy));
    var touch = Touch(0u, point, 0.0, 0.0, 0.0);
    if (pixel.x < 0 || pixel.y < 0 || u32(pixel.x) >= exchange_uniform.canvas.x || u32(pixel.y) >= exchange_uniform.canvas.y) { return touch; }
    if (point.z > ground[u32(pixel.y) * exchange_uniform.canvas.x + u32(pixel.x)] + CONTACT_BAND) { return touch; }
    let cell = vec2<u32>(pixel) / 2u;
    touch.cell = cell.y * exchange_uniform.cells.x + cell.x;
    let difference = load / exchange_uniform.sample_capacity - cell_wetness(water[touch.cell]);
    let rate = exchange_uniform.sample_capacity * exchange_uniform.dt * share;
    touch.give = min(max(difference, 0.0) * GIVE_RATE * rate, load);
    touch.take = max(-difference, 0.0) * TAKE_RATE * rate;
    let state = water[touch.cell];
    let wetter_paper = clamp(-difference * BLEED_ONSET, 0.0, 1.0);
    if (state.x > 0.0 && wetter_paper > 0.0) {
        let cell_pigment = suspended[touch.cell].absorption.w + deposited[touch.cell].absorption.w;
        let cell_concentration = cell_pigment / (state.x / WATER_PER_LOAD);
        touch.bleed = min(wetter_paper * BLEED_RATE * exchange_uniform.dt * share, 1.0) * load;
        touch.bleed *= max(1.0 - cell_concentration / max(sample_concentration, 1e-9), 0.0);
    }
    return touch;
}

@compute @workgroup_size(128)
fn exchange(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x;
    if (sample >= exchange_uniform.sample_count) { return; }
    let initial_load = sample_load[sample];
    var load = initial_load;
    let absorption = sample_absorption[sample].xyz;
    let scattering = sample_scattering[sample].xyz;
    let concentration = sample_scattering[sample].w;
    let behaviour = sample_behaviour[sample];
    let steps = segment_steps(sample);
    let share = 1.0 / f32(steps);
    var previous = segment_point(sample, 0u, steps);
    var requested = 0.0;
    var bled = 0.0;
    for (var step = 1u; step <= steps; step++) {
        let point = segment_point(sample, step, steps);
        let motion = point.xy - previous.xy;
        previous = point;
        let touch = touch_at(point, load, share, concentration);
        if (touch.give <= 0.0 && touch.take <= 0.0 && touch.bleed <= 0.0) { continue; }
        if (touch.bleed > 0.0) {
            let pigment = touch.bleed * concentration;
            accumulate(touch.cell, 1u, absorption.x * pigment);
            accumulate(touch.cell, 2u, absorption.y * pigment);
            accumulate(touch.cell, 3u, absorption.z * pigment);
            accumulate(touch.cell, 4u, pigment);
            accumulate(touch.cell, 5u, scattering.x * pigment);
            accumulate(touch.cell, 6u, scattering.y * pigment);
            accumulate(touch.cell, 7u, scattering.z * pigment);
            accumulate(touch.cell, 8u, behaviour.z * pigment);
            accumulate(touch.cell, 9u, behaviour.x * pigment);
            accumulate(touch.cell, 10u, behaviour.y * pigment);
            bled += pigment;
        }
        if (touch.give > 0.0) {
            let position = point.xy / 2.0 - 0.5;
            let corner = vec2<i32>(floor(position));
            let f = position - floor(position);
            let weights = array<f32, 4>((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
            for (var k = 0u; k < 4u; k++) {
                let target_cell = clamp(corner + vec2<i32>(i32(k & 1u), i32(k >> 1u)), vec2<i32>(0), vec2<i32>(exchange_uniform.cells) - 1);
                let index = u32(target_cell.y) * exchange_uniform.cells.x + u32(target_cell.x);
                let given = touch.give * weights[k];
                let pigment = given * concentration;
                accumulate(index, 0u, given * WATER_PER_LOAD);
                accumulate(index, 1u, absorption.x * pigment);
                accumulate(index, 2u, absorption.y * pigment);
                accumulate(index, 3u, absorption.z * pigment);
                accumulate(index, 4u, pigment);
                accumulate(index, 5u, scattering.x * pigment);
                accumulate(index, 6u, scattering.y * pigment);
                accumulate(index, 7u, scattering.z * pigment);
                accumulate(index, 8u, behaviour.z * pigment);
                accumulate(index, 9u, behaviour.x * pigment);
                accumulate(index, 10u, behaviour.y * pigment);
            }
            load -= touch.give;
        }
        if (touch.take > 0.0) {
            atomicAdd(&lift_demand[touch.cell], fixed(touch.take));
            requested += touch.take;
        }
        accumulate(touch.cell, 11u, motion.x * share);
        accumulate(touch.cell, 12u, motion.y * share);
    }
    sample_load[sample] = load;
    sample_scattering[sample].w = max(concentration * initial_load - concentration * (initial_load - load) - bled, 0.0) / max(load, 1e-9);
    sample_request[sample] = vec2<f32>(initial_load, requested);
    sample_concentration_at_start[sample] = concentration;
}

@compute @workgroup_size(16, 16)
fn grant_lift(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= exchange_uniform.cells.x || id.y >= exchange_uniform.cells.y) { return; }
    let cell = id.y * exchange_uniform.cells.x + id.x;
    let demand = f32(atomicExchange(&lift_demand[cell], 0)) / ACCUMULATOR_SCALE;
    lift[cell] = Pigment(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
    if (demand <= 0.0) { return; }
    let state = water[cell];
    let available = state.x / WATER_PER_LOAD;
    if (available <= 0.0) { return; }
    let granted = min(demand, available * MAX_LIFT_FRACTION);
    let removed = granted / available;
    let floating = suspended[cell];
    let taken = Pigment(floating.absorption * removed, floating.scattering * removed, floating.behaviour * removed);
    accumulate(cell, 0u, -state.x * removed);
    accumulate(cell, 1u, -taken.absorption.x);
    accumulate(cell, 2u, -taken.absorption.y);
    accumulate(cell, 3u, -taken.absorption.z);
    accumulate(cell, 4u, -taken.absorption.w);
    accumulate(cell, 5u, -taken.scattering.x);
    accumulate(cell, 6u, -taken.scattering.y);
    accumulate(cell, 7u, -taken.scattering.z);
    accumulate(cell, 8u, -taken.scattering.w);
    accumulate(cell, 9u, -taken.behaviour.x);
    accumulate(cell, 10u, -taken.behaviour.y);
    lift[cell] = Pigment(taken.absorption / granted, taken.scattering / granted, vec4<f32>(taken.behaviour.xy / granted, 0.0, granted / demand));
}

@compute @workgroup_size(128)
fn apply_lift(@builtin(global_invocation_id) id: vec3<u32>) {
    let sample = id.x;
    if (sample >= exchange_uniform.sample_count) { return; }
    let request = sample_request[sample];
    if (request.y <= 0.0) { return; }
    let steps = segment_steps(sample);
    let share = 1.0 / f32(steps);
    var load = sample_load[sample];
    var pigment_mass = load * sample_scattering[sample].w;
    var absorption = sample_absorption[sample].xyz * pigment_mass;
    var scattering = sample_scattering[sample].xyz * pigment_mass;
    var behaviour = sample_behaviour[sample].xyz * pigment_mass;
    var replay = request.x;
    let request_concentration = sample_concentration_at_start[sample];
    for (var step = 1u; step <= steps; step++) {
        let touch = touch_at(segment_point(sample, step, steps), replay, share, request_concentration);
        replay -= touch.give;
        if (touch.take <= 0.0) { continue; }
        let granted = lift[touch.cell];
        let taken = touch.take * granted.behaviour.w;
        load += taken;
        pigment_mass += granted.absorption.w * taken;
        absorption += granted.absorption.xyz * taken;
        scattering += granted.scattering.xyz * taken;
        behaviour += vec3<f32>(granted.behaviour.x, granted.behaviour.y, granted.scattering.w) * taken;
    }
    let mass = max(pigment_mass, 1e-9);
    sample_load[sample] = load;
    sample_absorption[sample] = vec4<f32>(absorption / mass, sample_absorption[sample].w);
    sample_scattering[sample] = vec4<f32>(scattering / mass, pigment_mass / max(load, 1e-9));
    sample_behaviour[sample] = vec4<f32>(behaviour / mass, 0.0);
}

@compute @workgroup_size(64)
fn wick(@builtin(global_invocation_id) id: vec3<u32>) {
    let bristle = id.x;
    if (bristle * exchange_uniform.samples_per_bristle >= exchange_uniform.sample_count) { return; }
    let base = bristle * exchange_uniform.samples_per_bristle;
    var liquid = 0.0;
    var pigment = 0.0;
    var absorption = vec3<f32>(0.0);
    var scattering = vec3<f32>(0.0);
    var behaviour = vec3<f32>(0.0);
    for (var i = 0u; i < exchange_uniform.samples_per_bristle; i++) {
        let load = sample_load[base + i];
        let mass = load * sample_scattering[base + i].w;
        liquid += load;
        pigment += mass;
        absorption += sample_absorption[base + i].xyz * mass;
        scattering += sample_scattering[base + i].xyz * mass;
        behaviour += sample_behaviour[base + i].xyz * mass;
    }
    let mean_load = liquid / f32(exchange_uniform.samples_per_bristle);
    let mean_concentration = pigment / max(liquid, 1e-9);
    let rate = min(WICKING_PER_SECOND * exchange_uniform.dt, 1.0);
    let mass = max(pigment, 1e-9);
    for (var i = 0u; i < exchange_uniform.samples_per_bristle; i++) {
        let here = base + i;
        let load = mix(sample_load[here], mean_load, rate);
        let concentration = mix(sample_scattering[here].w, mean_concentration, rate);
        sample_load[here] = load;
        sample_absorption[here] = vec4<f32>(mix(sample_absorption[here].xyz, absorption / mass, rate), sample_absorption[here].w);
        sample_scattering[here] = vec4<f32>(mix(sample_scattering[here].xyz, scattering / mass, rate), concentration);
        sample_behaviour[here] = vec4<f32>(mix(sample_behaviour[here].xyz, behaviour / mass, rate), 0.0);
    }
}
