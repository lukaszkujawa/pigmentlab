struct Pigment {
    absorption: vec4<f32>,
    scattering: vec4<f32>,
    behaviour: vec4<f32>,
    settling: vec4<f32>,
};

fn no_pigment() -> Pigment {
    return Pigment(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
}

fn scaled(p: Pigment, factor: f32) -> Pigment {
    return Pigment(p.absorption * factor, p.scattering * factor, p.behaviour * factor, p.settling * factor);
}

fn added(a: Pigment, b: Pigment) -> Pigment {
    return Pigment(a.absorption + b.absorption, a.scattering + b.scattering, a.behaviour + b.behaviour, a.settling + b.settling);
}

fn clamped(p: Pigment) -> Pigment {
    return Pigment(max(p.absorption, vec4<f32>(0.0)), max(p.scattering, vec4<f32>(0.0)), max(p.behaviour, vec4<f32>(0.0)), max(p.settling, vec4<f32>(0.0)));
}

fn mass(p: Pigment) -> f32 {
    return p.absorption.w;
}

fn granulating_absorption(p: Pigment) -> vec3<f32> {
    return select(p.absorption.xyz / max(p.absorption.w, 1e-9), p.settling.xyz / p.scattering.w, p.scattering.w > 1e-9);
}
