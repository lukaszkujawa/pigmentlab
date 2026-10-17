use super::preset::{BrushPreset, BrushShape};
use crate::rng::SplitMix64;
use glam::{Vec2, Vec3, Vec4};

pub const VERTICES_PER_BRISTLE: u32 = 10;
const LENGTH_JITTER: f32 = 0.02;
const DIRECTION_JITTER: f32 = 0.02;
const BRISTLES_PER_CLUMP: u32 = 7;
const CLUMP_SPREAD: f32 = 0.35;
const TIP_RADIUS_FRACTION: f32 = 0.2;
const BELLY: f32 = 0.35;
const MAX_LEAN: f32 = 0.5;
const CURL: f32 = 0.04;
const CURL_RANDOMNESS: f32 = 0.35;

pub fn rest_positions(preset: &BrushPreset, rng: &mut SplitMix64) -> Vec<Vec4> {
    let mut positions = Vec::with_capacity((preset.bristle_count * VERTICES_PER_BRISTLE) as usize);
    let clump_count = clump_count(preset);
    let clumps: Vec<Vec2> = (0..clump_count).map(|index| root_position(preset, index, clump_count, rng)).collect();
    let clump_radius = clump_extent(preset, clump_count);
    for index in 0..preset.bristle_count {
        let clump = clumps[(index % clump_count) as usize];
        let root = match preset.shape {
            BrushShape::Blade => grid_root(preset, index),
            _ => clamp_to_ferrule(preset, clump + jitter(rng, clump_radius)),
        };
        let length = bristle_length(preset, root) * (1.0 + rng.range(-LENGTH_JITTER, LENGTH_JITTER));
        let wobble = jitter(rng, DIRECTION_JITTER) * preset.length;
        let curl = curl_direction(preset, root, jitter(rng, 1.0)) * CURL * preset.length;
        let segment = length / (VERTICES_PER_BRISTLE - 2) as f32;
        positions.push(Vec4::new(root.x, root.y, segment, rng.unit()));
        let mut point = root.extend(0.0);
        positions.push(point.extend(0.0));
        for vertex in 2..VERTICES_PER_BRISTLE {
            let along = (vertex - 1) as f32 / (VERTICES_PER_BRISTLE - 2) as f32;
            let across = profile(preset, root, along) + wobble * along + curl * (std::f32::consts::PI * along).sin();
            let step = (across - point.truncate()).clamp_length_max(segment * MAX_LEAN);
            let drop = (segment * segment - step.length_squared()).sqrt();
            point = Vec3::new(point.x + step.x, point.y + step.y, point.z - drop);
            positions.push(point.extend(0.0));
        }
    }
    positions
}

fn curl_direction(preset: &BrushPreset, root: Vec2, random: Vec2) -> Vec2 {
    let half = Vec2::new(preset.width, preset.thickness) * 0.5;
    let outward = match preset.shape {
        BrushShape::Round => root / half.x,
        BrushShape::Flat | BrushShape::Filbert => Vec2::new(0.0, root.y / half.y),
        BrushShape::Blade => return Vec2::ZERO,
    };
    (outward + random * CURL_RANDOMNESS).normalize_or_zero()
}

pub fn clump_count(preset: &BrushPreset) -> u32 {
    preset.bristle_count.div_ceil(BRISTLES_PER_CLUMP).max(1)
}

fn profile(preset: &BrushPreset, root: Vec2, along: f32) -> Vec2 {
    let belly = 1.0 + BELLY * preset.splay * (std::f32::consts::PI * along.powf(0.8)).sin();
    let taper = |fraction: f32| 1.0 + (fraction - 1.0) * along * along;
    match preset.shape {
        BrushShape::Round => root * belly * taper(TIP_RADIUS_FRACTION),
        BrushShape::Flat | BrushShape::Filbert => Vec2::new(root.x, root.y * belly * taper(TIP_RADIUS_FRACTION)),
        BrushShape::Blade => root,
    }
}

fn root_position(preset: &BrushPreset, index: u32, count: u32, rng: &mut SplitMix64) -> Vec2 {
    let half = Vec2::new(preset.width, preset.thickness) * 0.5;
    match preset.shape {
        BrushShape::Round => sunflower(index, count, half.x) + jitter(rng, half.x * 0.05),
        BrushShape::Flat | BrushShape::Filbert | BrushShape::Blade => {
            Vec2::new(rng.range(-half.x, half.x), rng.range(-half.y, half.y))
        }
    }
}

pub fn blade_grid(preset: &BrushPreset) -> (u32, u32) {
    let rows = (preset.bristle_count as f32 * preset.thickness / preset.width).sqrt().round().max(1.0) as u32;
    (rows, preset.bristle_count.div_ceil(rows))
}

fn grid_root(preset: &BrushPreset, index: u32) -> Vec2 {
    let (rows, columns) = blade_grid(preset);
    let column = index % columns;
    let row = index / columns;
    Vec2::new(
        ((column as f32 + 0.5) / columns as f32 - 0.5) * preset.width,
        ((row as f32 + 0.5) / rows as f32 - 0.5) * preset.thickness,
    )
}

fn clump_extent(preset: &BrushPreset, clump_count: u32) -> f32 {
    let area = preset.width * preset.thickness;
    (area / clump_count as f32).sqrt() * CLUMP_SPREAD
}

fn clamp_to_ferrule(preset: &BrushPreset, root: Vec2) -> Vec2 {
    let half = Vec2::new(preset.width, preset.thickness) * 0.5;
    match preset.shape {
        BrushShape::Round => root.clamp_length_max(half.x),
        BrushShape::Flat | BrushShape::Filbert | BrushShape::Blade => root.clamp(-half, half),
    }
}

fn sunflower(index: u32, count: u32, radius: f32) -> Vec2 {
    let golden_angle = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    let r = radius * ((index as f32 + 0.5) / count as f32).sqrt();
    let theta = index as f32 * golden_angle;
    Vec2::new(r * theta.cos(), r * theta.sin())
}

fn jitter(rng: &mut SplitMix64, amount: f32) -> Vec2 {
    Vec2::new(rng.range(-amount, amount), rng.range(-amount, amount))
}

fn bristle_length(preset: &BrushPreset, root: Vec2) -> f32 {
    match preset.shape {
        BrushShape::Filbert => {
            let across = (root.x / (preset.width * 0.5)).clamp(-1.0, 1.0);
            preset.length * (0.75 + 0.25 * (1.0 - across * across).sqrt())
        }
        BrushShape::Round | BrushShape::Flat | BrushShape::Blade => preset.length,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::cm;

    #[test]
    fn generates_expected_vertex_count() {
        let preset = BrushPreset::round(12, cm(0.3), cm(2.0));
        let positions = rest_positions(&preset, &mut SplitMix64::new(1));
        assert_eq!(positions.len(), 12 * VERTICES_PER_BRISTLE as usize);
    }

    #[test]
    fn segments_have_uniform_length() {
        let preset = BrushPreset::flat(5, cm(1.0), cm(0.2), cm(2.0));
        let positions = rest_positions(&preset, &mut SplitMix64::new(9));
        for bristle in positions.chunks(VERTICES_PER_BRISTLE as usize) {
            let first = bristle[1].distance(bristle[2]);
            for pair in bristle[1..].windows(2) {
                assert!((pair[0].distance(pair[1]) - first).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn roots_stay_inside_ferrule() {
        let preset = BrushPreset::round(300, cm(0.4), cm(2.0));
        let positions = rest_positions(&preset, &mut SplitMix64::new(5));
        for bristle in positions.chunks(VERTICES_PER_BRISTLE as usize) {
            assert!(bristle[1].truncate().length() <= cm(0.4) * 1.001);
            assert!(bristle[0].z > 0.0 && bristle[1].z == 0.0);
        }
    }
}
