pub const PIXELS_PER_CM: f32 = 80.0;

pub const fn cm(value: f32) -> f32 {
    value * PIXELS_PER_CM
}

pub const METRES_PER_PIXEL: f32 = 0.01 / PIXELS_PER_CM;
pub const GRAVITY: f32 = 9.81;

pub fn pixels(metres: f32) -> f32 {
    metres / METRES_PER_PIXEL
}

pub fn metres(pixels: f32) -> f32 {
    pixels * METRES_PER_PIXEL
}
