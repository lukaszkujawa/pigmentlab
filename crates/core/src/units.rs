pub const PIXELS_PER_CM: f32 = 80.0;

pub const fn cm(value: f32) -> f32 {
    value * PIXELS_PER_CM
}
