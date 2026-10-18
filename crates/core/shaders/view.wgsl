struct ViewUniform {
    size: vec2<f32>,
    origin: vec2<f32>,
    scale: f32,
    padding: f32,
    more_padding: vec2<f32>,
};

fn canvas_to_clip(point: vec2<f32>) -> vec2<f32> {
    let unit = (view.origin + point * view.scale) / view.size;
    return vec2<f32>(unit.x * 2.0 - 1.0, 1.0 - unit.y * 2.0);
}
