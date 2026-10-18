struct CompositeUniform {
    canvas_width: u32,
    canvas_height: u32,
    light_azimuth: f32,
    light_elevation: f32,
    gloss: f32,
    ambient: f32,
    shadow_strength: f32,
    padding: f32,
};

fn light_direction() -> vec3<f32> {
    let elevation = radians(composite.light_elevation);
    let azimuth = radians(composite.light_azimuth);
    return vec3<f32>(cos(elevation) * cos(azimuth), cos(elevation) * sin(azimuth), sin(elevation));
}
