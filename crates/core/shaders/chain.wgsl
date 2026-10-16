fn chain_vertex(previous: bool, bristle: u32, vertex: i32) -> vec3<f32> {
    let index = bristle * BRISTLE_VERTICES + u32(clamp(vertex, 1, i32(BRISTLE_VERTICES) - 1));
    return select(chain[index].xyz, previous_chain[index].xyz, previous);
}

fn curve_point(previous: bool, bristle: u32, t: f32) -> vec3<f32> {
    let along = t * f32(BRISTLE_VERTICES - 2u);
    let span = min(i32(floor(along)), i32(BRISTLE_VERTICES) - 3);
    let f = along - f32(span);
    let before = chain_vertex(previous, bristle, span);
    let p0 = chain_vertex(previous, bristle, span + 1);
    let p1 = chain_vertex(previous, bristle, span + 2);
    let after = chain_vertex(previous, bristle, span + 3);
    return hermite(p0, (p1 - before) * 0.5, p1, (after - p0) * 0.5, f);
}
