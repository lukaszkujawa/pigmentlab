@group(0) @binding(0) var layer: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let colour = textureLoad(layer, vec2<i32>(at.xy), 0);
    if colour.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(colour.rgb / colour.a, colour.a);
}
