struct Frame {
    size: vec2<f32>,
    origin: vec2<f32>,
    scale: f32,
    padding: f32,
    more_padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec2<f32>, @location(1) colour: vec4<f32>) -> VertexOutput {
    var out: VertexOutput;
    let unit = (frame.origin + position * frame.scale) / frame.size;
    out.position = vec4<f32>(unit.x * 2.0 - 1.0, 1.0 - unit.y * 2.0, 0.0, 1.0);
    out.colour = colour;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.colour;
}

@fragment
fn fs_premultiplied(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.colour.rgb * in.colour.a, in.colour.a);
}
