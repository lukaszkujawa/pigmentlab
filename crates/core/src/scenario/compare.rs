use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Tolerance {
    pub max_mean_difference: f32,
    pub pixel_threshold: u8,
    pub max_differing_fraction: f32,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self { max_mean_difference: 0.5, pixel_threshold: 8, max_differing_fraction: 0.001 }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Comparison {
    pub mean_difference: f32,
    pub differing_fraction: f32,
    pub passed: bool,
}

pub fn compare(actual: &RgbaImage, expected: &RgbaImage, tolerance: Tolerance) -> Comparison {
    if actual.dimensions() != expected.dimensions() {
        return Comparison { mean_difference: f32::MAX, differing_fraction: 1.0, passed: false };
    }
    let mut total = 0u64;
    let mut differing = 0u64;
    for (a, e) in actual.pixels().zip(expected.pixels()) {
        let channel_diffs = a.0.iter().zip(e.0.iter()).map(|(x, y)| x.abs_diff(*y));
        let max = channel_diffs.clone().max().unwrap_or(0);
        total += channel_diffs.map(u64::from).sum::<u64>();
        differing += u64::from(max > tolerance.pixel_threshold);
    }
    let pixels = u64::from(actual.width()) * u64::from(actual.height());
    let mean_difference = total as f32 / (pixels * 4) as f32;
    let differing_fraction = differing as f32 / pixels as f32;
    Comparison {
        mean_difference,
        differing_fraction,
        passed: mean_difference <= tolerance.max_mean_difference
            && differing_fraction <= tolerance.max_differing_fraction,
    }
}

pub fn difference_image(actual: &RgbaImage, expected: &RgbaImage) -> RgbaImage {
    let mut out = RgbaImage::new(actual.width(), actual.height());
    for ((a, e), o) in actual.pixels().zip(expected.pixels()).zip(out.pixels_mut()) {
        let max = a.0.iter().zip(e.0.iter()).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        let amplified = max.saturating_mul(8);
        *o = image::Rgba([amplified, amplified, amplified, 255]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(value: u8) -> RgbaImage {
        RgbaImage::from_pixel(4, 4, image::Rgba([value, value, value, 255]))
    }

    #[test]
    fn identical_images_pass() {
        let result = compare(&solid(100), &solid(100), Tolerance::default());
        assert!(result.passed);
        assert_eq!(result.mean_difference, 0.0);
    }

    #[test]
    fn large_difference_fails() {
        let result = compare(&solid(100), &solid(200), Tolerance::default());
        assert!(!result.passed);
        assert_eq!(result.differing_fraction, 1.0);
    }

    #[test]
    fn mismatched_sizes_fail() {
        let small = RgbaImage::new(2, 2);
        assert!(!compare(&solid(0), &small, Tolerance::default()).passed);
    }
}
