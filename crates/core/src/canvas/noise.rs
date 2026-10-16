pub fn lerp(range: (f32, f32), t: f32) -> f32 {
    range.0 + (range.1 - range.0) * t
}

pub fn value_noise(x: f32, y: f32, seed: u64) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (smooth(x - x0), smooth(y - y0));
    let corner = |dx: f32, dy: f32| hash((x0 + dx) as i64 as u32, (y0 + dy) as i64 as u32, seed);
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * fx;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * fx;
    top + (bottom - top) * fy
}

pub fn fractal_noise(x: f32, y: f32, octaves: u32, seed: u64) -> f32 {
    let (mut total, mut amplitude, mut frequency, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for octave in 0..octaves {
        total += value_noise(x * frequency, y * frequency, seed ^ u64::from(octave).wrapping_mul(0x9e37)) * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    total / norm
}

pub fn cellular(x: f32, y: f32, seed: u64) -> f32 {
    let (cx, cy) = (x.floor() as i64, y.floor() as i64);
    let mut nearest = f32::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (gx, gy) = ((cx + dx) as u32, (cy + dy) as u32);
            let px = (cx + dx) as f32 + hash(gx, gy, seed);
            let py = (cy + dy) as f32 + hash(gx, gy, seed ^ 0x5bd1);
            nearest = nearest.min((px - x).powi(2) + (py - y).powi(2));
        }
    }
    nearest.sqrt()
}

pub fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

pub fn hash1(seed: u64) -> f32 {
    hash(0, 0, seed)
}

pub fn hash(x: u32, y: u32, seed: u64) -> f32 {
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
    fn noises_stay_in_unit_range_and_are_deterministic() {
        for i in 0..500 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 0.91);
            for value in [value_noise(x, y, 3), fractal_noise(x, y, 4, 3)] {
                assert!((0.0..=1.0).contains(&value));
            }
            assert!(cellular(x, y, 3) < 1.5);
            assert_eq!(cellular(x, y, 3), cellular(x, y, 3));
        }
    }
}
