use super::CanvasSize;

const BASE_COLOR: [f32; 3] = [0.93, 0.91, 0.86];
const GRAIN_AMPLITUDE: f32 = 0.025;
const WEAVE_PERIOD: f32 = crate::units::cm(0.15);
const RELIEF_GRAIN: f32 = 0.25;

pub fn paper(size: CanvasSize, seed: u64) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size.width * size.height * 4) as usize);
    for y in 0..size.height {
        for x in 0..size.width {
            let shade = 1.0 + (hash(x, y, seed) - 0.5) * 2.0 * GRAIN_AMPLITUDE;
            for channel in BASE_COLOR {
                pixels.push(to_srgb8(channel * shade));
            }
            pixels.push(255);
        }
    }
    pixels
}

pub fn relief(size: CanvasSize, seed: u64) -> Vec<f32> {
    let mut heights = Vec::with_capacity((size.width * size.height) as usize);
    for y in 0..size.height {
        for x in 0..size.width {
            let grain = (hash(x, y, seed ^ 0x9e37_79b9) - 0.5) * 2.0 * RELIEF_GRAIN;
            heights.push((weave(x as f32, y as f32) + grain).clamp(-1.0, 1.0));
        }
    }
    heights
}

fn weave(x: f32, y: f32) -> f32 {
    let warp = (x * std::f32::consts::TAU / WEAVE_PERIOD).sin();
    let weft = (y * std::f32::consts::TAU / WEAVE_PERIOD).sin();
    0.5 * (warp + weft)
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

fn to_srgb8(linear: f32) -> u8 {
    let clamped = linear.clamp(0.0, 1.0);
    let srgb = if clamped <= 0.003_130_8 {
        clamped * 12.92
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
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
    fn paper_stays_near_base_colour() {
        let pixels = paper(CanvasSize { width: 32, height: 32 }, 1);
        for pixel in pixels.chunks(4) {
            assert!(pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 190);
            assert_eq!(pixel[3], 255);
        }
    }

    #[test]
    fn relief_is_a_unit_amplitude_weave() {
        let size = CanvasSize { width: 64, height: 64 };
        let heights = relief(size, 3);
        let highest = heights.iter().cloned().fold(f32::MIN, f32::max);
        let lowest = heights.iter().cloned().fold(f32::MAX, f32::min);
        assert!(highest > 0.8 && highest <= 1.0 && lowest < -0.8 && lowest >= -1.0);
        assert_eq!(relief(size, 3), relief(size, 3));
    }

    #[test]
    fn srgb_encoding_endpoints() {
        assert_eq!(to_srgb8(0.0), 0);
        assert_eq!(to_srgb8(1.0), 255);
        assert_eq!(to_srgb8(2.0), 255);
    }
}
