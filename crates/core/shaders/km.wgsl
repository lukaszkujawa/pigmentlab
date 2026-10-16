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
    let scattering_depth = safe_s * thickness * (1.0 - substrate_color);
    let non_absorbing = (substrate_color + scattering_depth) / (1.0 + scattering_depth);
    let absorbing_only = substrate_color * exp(-2.0 * k * thickness);
    let opaque = opaque_reflectance(k, safe_s);
    let general = select(layered, non_absorbing, b < vec3<f32>(1e-3));
    let thin_scatter = select(general, absorbing_only, s <= vec3<f32>(1e-5));
    return clamp(select(thin_scatter, opaque, argument >= vec3<f32>(20.0)), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn transmittance(k: vec3<f32>, s: vec3<f32>, thickness: f32) -> vec3<f32> {
    let safe_s = max(s, vec3<f32>(1e-5));
    let a = 1.0 + k / safe_s;
    let b = sqrt(max(a * a - 1.0, vec3<f32>(0.0)));
    let argument = min(b * safe_s * thickness, vec3<f32>(20.0));
    let layered = b / (a * sinh(argument) + b * cosh(argument));
    let non_absorbing = 1.0 / (1.0 + safe_s * thickness);
    let absorbing_only = exp(-k * thickness);
    let general = select(layered, non_absorbing, b < vec3<f32>(1e-3));
    return clamp(select(general, absorbing_only, s <= vec3<f32>(1e-5)), vec3<f32>(0.0), vec3<f32>(1.0));
}

struct VisiblePigment {
    absorption: vec3<f32>,
    scattering: vec3<f32>,
    weight: vec3<f32>,
    transmission: vec3<f32>,
    height: f32,
    medium: f32,
};

fn no_visible_pigment() -> VisiblePigment {
    return VisiblePigment(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(1.0), 0.0, 0.0);
}

fn add_layer_below(visible: ptr<function, VisiblePigment>, density: f32, k: vec3<f32>, s: vec3<f32>, medium_mass: f32) {
    let reaching = (*visible).transmission;
    let passing = transmittance(k, s, density * OPTICAL_THICKNESS_PER_PIXEL);
    let weight = reaching * reaching * (1.0 - passing);
    (*visible).absorption += k * weight;
    (*visible).scattering += s * weight;
    (*visible).weight += weight;
    (*visible).transmission = reaching * passing;
    (*visible).height += density;
    (*visible).medium += medium_mass;
}

fn visible_absorption(visible: VisiblePigment) -> vec3<f32> {
    return visible.absorption / max(visible.weight, vec3<f32>(1e-9));
}

fn visible_scattering(visible: VisiblePigment) -> vec3<f32> {
    return visible.scattering / max(visible.weight, vec3<f32>(1e-9));
}
