use super::CanvasSize;

const BASE_COLOR: [f32; 3] = [0.93, 0.91, 0.86];
const GRAIN_AMPLITUDE: f32 = 0.012;
const THREAD_PERIOD: f32 = crate::units::cm(0.15);
const THREAD_WIDTH_RANGE: (f32, f32) = (0.92, 1.08);
const THREAD_HEIGHT_RANGE: (f32, f32) = (0.85, 1.0);
const THREAD_OFFSET: f32 = 0.06;
const UNDER_LEVEL: f32 = 0.5;
const CROSSING_SHARPNESS: (f32, f32) = (0.3, 0.7);
const KNOT_AMPLITUDE: f32 = 0.06;
const STRIATION_AMPLITUDE: f32 = 0.07;
const STRIATION_ACROSS: f32 = 1.2;
const STRIATION_ALONG: f32 = 9.0;
const WOBBLE_AMPLITUDE: f32 = 0.1;
const WOBBLE_SCALE: f32 = 48.0;
const SAG_AMPLITUDE: f32 = 0.1;
const SAG_SCALE: f32 = 96.0;

pub fn paper(size: CanvasSize, seed: u64) -> Vec<[f32; 4]> {
    let mut pixels = Vec::with_capacity((size.width * size.height) as usize);
    for y in 0..size.height {
        for x in 0..size.width {
            let shade = 1.0 + (hash(x, y, seed) - 0.5) * 2.0 * GRAIN_AMPLITUDE;
            pixels.push([0.0, BASE_COLOR[0] * shade, BASE_COLOR[1] * shade, BASE_COLOR[2] * shade]);
        }
    }
    pixels
}

pub fn relief(size: CanvasSize, seed: u64) -> Vec<f32> {
    let mut heights = Vec::with_capacity((size.width * size.height) as usize);
    for y in 0..size.height {
        for x in 0..size.width {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let warp = thread_height(px, py, 0, seed);
            let weft = thread_height(py, px, 1, seed);
            let sag = (value_noise(px / SAG_SCALE, py / SAG_SCALE, seed ^ 0x77) - 0.5) * 2.0 * SAG_AMPLITUDE;
            let height = warp.max(weft) + sag;
            heights.push(((height - 0.5) * 2.0).clamp(-1.0, 1.0));
        }
    }
    heights
}

fn thread_height(across: f32, along: f32, axis: u32, seed: u64) -> f32 {
    let nearest = (across / THREAD_PERIOD).floor() as i64;
    let mut best: f32 = 0.0;
    for index in nearest - 1..=nearest + 1 {
        let thread_seed = seed ^ ((index as u64) << 8) ^ (u64::from(axis) << 1);
        let offset = (hash1(thread_seed) - 0.5) * 2.0 * THREAD_OFFSET * THREAD_PERIOD;
        let width = lerp(THREAD_WIDTH_RANGE, hash1(thread_seed ^ 0x11)) * THREAD_PERIOD;
        let thickness = lerp(THREAD_HEIGHT_RANGE, hash1(thread_seed ^ 0x22));
        let wobble = (value_noise(along / WOBBLE_SCALE, 0.0, thread_seed ^ 0x33) - 0.5) * 2.0 * WOBBLE_AMPLITUDE * THREAD_PERIOD;
        let centre = (index as f32 + 0.5) * THREAD_PERIOD + offset + wobble;
        let d = (across - centre) / (width * 0.5);
        if d.abs() >= 1.0 {
            continue;
        }
        let profile = (1.0 - d * d).sqrt();
        let phase = along / THREAD_PERIOD - 0.5 + (index.rem_euclid(2) as f32) + axis as f32;
        let lift = plateau(0.5 + 0.5 * (std::f32::consts::PI * phase).cos());
        let cell = (along / THREAD_PERIOD).floor() as i64;
        let knot = (hash((index as i64) as u32, cell as u32, thread_seed ^ 0x44) - 0.5) * 2.0 * KNOT_AMPLITUDE;
        let striation = (value_noise((across - centre) / STRIATION_ACROSS, along / STRIATION_ALONG, thread_seed ^ 0x55) - 0.5) * 2.0 * STRIATION_AMPLITUDE;
        best = best.max(profile * (thickness + knot + striation) * (UNDER_LEVEL + (1.0 - UNDER_LEVEL) * lift));
    }
    best
}

fn plateau(t: f32) -> f32 {
    smooth(((t - CROSSING_SHARPNESS.0) / (CROSSING_SHARPNESS.1 - CROSSING_SHARPNESS.0)).clamp(0.0, 1.0))
}

fn lerp(range: (f32, f32), t: f32) -> f32 {
    range.0 + (range.1 - range.0) * t
}

fn value_noise(x: f32, y: f32, seed: u64) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (smooth(x - x0), smooth(y - y0));
    let corner = |dx: f32, dy: f32| hash((x0 + dx) as i64 as u32, (y0 + dy) as i64 as u32, seed);
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * fx;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * fx;
    top + (bottom - top) * fy
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn hash1(seed: u64) -> f32 {
    hash(0, 0, seed)
}

fn hash(x: u32, y: u32, seed: u64) -> f32 {
    let mut h = seed ^ ((x as u64) << 32) ^ (y as u64);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_is_deterministic_for_a_seed() {
        let size = CanvasSize { width: 16, height: 8 };
        assert_eq!(paper(size, 7), paper(size, 7));
        assert_ne!(paper(size, 7), paper(size, 8));
    }

    #[test]
    fn paper_is_a_flat_dry_layer_near_the_base_colour() {
        for pixel in paper(CanvasSize { width: 32, height: 32 }, 1) {
            assert_eq!(pixel[0], 0.0);
            assert!(pixel[1] > 0.9 && pixel[2] > 0.88 && pixel[3] > 0.83, "{pixel:?}");
        }
    }

    #[test]
    fn relief_is_a_woven_surface_within_unit_range() {
        let size = CanvasSize { width: 96, height: 96 };
        let heights = relief(size, 3);
        let highest = heights.iter().cloned().fold(f32::MIN, f32::max);
        let lowest = heights.iter().cloned().fold(f32::MAX, f32::min);
        assert!(highest > 0.8 && highest <= 1.0 && lowest < -0.5 && lowest >= -1.0, "range {lowest} to {highest}");
        let peaks = heights.iter().filter(|h| **h > 0.6).count();
        let hollows = heights.iter().filter(|h| **h < -0.3).count();
        assert!(peaks > heights.len() / 4 && hollows > heights.len() / 50, "peaks {peaks} hollows {hollows}");
        assert_eq!(relief(size, 3), relief(size, 3));
        assert_ne!(relief(size, 3), relief(size, 4));
    }
}
