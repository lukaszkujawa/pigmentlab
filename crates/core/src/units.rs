pub const PIXELS_PER_CM: f32 = 40.0;

pub fn cm(value: f32) -> f32 {
    value * PIXELS_PER_CM
}
