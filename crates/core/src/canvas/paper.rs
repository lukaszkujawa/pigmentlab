use super::CanvasSize;
use super::noise::{cellular, fractal_noise, hash, lerp, smooth, value_noise};

const FIBRES_PER_PIXEL: f32 = 1.0 / 180.0;
const FIBRE_LENGTH: (f32, f32) = (25.0, 240.0);
const FIBRE_WIDTH: (f32, f32) = (0.6, 1.4);
const FIBRE_CURL: f32 = 2.4;
const FIBRE_CURL_LENGTH: f32 = 45.0;
const FIBRE_STEP_SHARE: f32 = 0.45;

pub fn hot_pressed(size: CanvasSize, seed: u64) -> Vec<f32> {
    normalised(field(size, |x, y| {
        0.55 * fractal_noise(x / 5.0, y / 5.0, 3, seed) + 0.45 * fractal_noise(x / 70.0, y / 70.0, 3, seed ^ 0x11)
    }))
}

pub fn cold_pressed(size: CanvasSize, seed: u64) -> Vec<f32> {
    normalised(field(size, |x, y| {
        0.6 * bumps(x, y, 8.0, seed) + 0.25 * bumps(x, y, 3.5, seed ^ 0x21) + 0.15 * fractal_noise(x / 24.0, y / 24.0, 3, seed ^ 0x22)
    }))
}

pub fn rough(size: CanvasSize, seed: u64) -> Vec<f32> {
    normalised(field(size, |x, y| {
        let body = 0.6 * bumps(x, y, 17.0, seed) + 0.25 * bumps(x, y, 6.5, seed ^ 0x31) + 0.15 * fractal_noise(x / 45.0, y / 45.0, 3, seed ^ 0x32);
        body * body
    }))
}

pub fn rice(size: CanvasSize, seed: u64) -> Vec<f32> {
    let mut heights = field(size, |x, y| {
        let formation = fractal_noise(x / 140.0, y / 140.0, 4, seed);
        0.22 * formation * formation + 0.08 * fractal_noise(x / 9.0, y / 9.0, 2, seed ^ 0x47)
    });
    let fibres = (size.pixel_count() as f32 * FIBRES_PER_PIXEL) as u32;
    for fibre in 0..fibres {
        let pick = |salt: u64| hash(fibre, 0, seed ^ salt);
        let start = (pick(0x41) * size.width as f32, pick(0x42) * size.height as f32);
        let fibre_seed = seed ^ (u64::from(fibre) << 20);
        let shape = Fibre {
            heading: pick(0x43) * std::f32::consts::TAU,
            length: lerp(FIBRE_LENGTH, pick(0x44) * pick(0x48)),
            width: lerp(FIBRE_WIDTH, pick(0x45)),
            strength: 0.08 + 0.2 * pick(0x46),
            seed: fibre_seed,
        };
        lay_fibre(&mut heights, size, start, &shape);
    }
    normalised(heights)
}

struct Fibre {
    heading: f32,
    length: f32,
    width: f32,
    strength: f32,
    seed: u64,
}

fn lay_fibre(heights: &mut [f32], size: CanvasSize, start: (f32, f32), fibre: &Fibre) {
    let steps = fibre.length.ceil() as i32;
    let reach = fibre.width.ceil() as i32 + 1;
    let (mut cx, mut cy) = start;
    for step in 0..=steps {
        let t = step as f32;
        let heading = fibre.heading + (value_noise(t / FIBRE_CURL_LENGTH, 0.5, fibre.seed) - 0.5) * FIBRE_CURL;
        let (dx, dy) = (heading.cos(), heading.sin());
        cx += dx;
        cy += dy;
        let taper = smooth(1.0 - (2.0 * t / fibre.length - 1.0).abs());
        for oy in -reach..=reach {
            for ox in -reach..=reach {
                let (px, py) = (cx.floor() as i32 + ox, cy.floor() as i32 + oy);
                if px < 0 || py < 0 || px >= size.width as i32 || py >= size.height as i32 {
                    continue;
                }
                let across = ((px as f32 + 0.5 - cx) * -dy + (py as f32 + 0.5 - cy) * dx).abs() / fibre.width;
                if across >= 1.0 {
                    continue;
                }
                let index = (py as u32 * size.width + px as u32) as usize;
                heights[index] += (1.0 - across * across) * fibre.strength * taper * FIBRE_STEP_SHARE;
            }
        }
    }
}

fn bumps(x: f32, y: f32, scale: f32, seed: u64) -> f32 {
    smooth((1.0 - cellular(x / scale, y / scale, seed)).clamp(0.0, 1.0))
}

fn field(size: CanvasSize, height: impl Fn(f32, f32) -> f32) -> Vec<f32> {
    let mut heights = Vec::with_capacity(size.pixel_count() as usize);
    for y in 0..size.height {
        for x in 0..size.width {
            heights.push(height(x as f32 + 0.5, y as f32 + 0.5));
        }
    }
    heights
}

fn normalised(mut heights: Vec<f32>) -> Vec<f32> {
    let (low, high) = heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
    let span = (high - low).max(1e-6);
    for h in &mut heights {
        *h = (*h - low) / span * 2.0 - 1.0;
    }
    heights
}
