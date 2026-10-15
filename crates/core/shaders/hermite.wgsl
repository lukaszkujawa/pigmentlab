fn hermite(p0: vec3<f32>, m0: vec3<f32>, p1: vec3<f32>, m1: vec3<f32>, t: f32) -> vec3<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return (2.0 * t3 - 3.0 * t2 + 1.0) * p0 + (t3 - 2.0 * t2 + t) * m0 + (-2.0 * t3 + 3.0 * t2) * p1 + (t3 - t2) * m1;
}

fn above_canvas(p: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(p.xy, max(p.z, 0.0));
}
