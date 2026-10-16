use super::preset::{BrushPreset, BrushShape};
use crate::rng::SplitMix64;
use glam::{Vec2, Vec3, Vec4};

pub const VERTICES_PER_BRISTLE: u32 = 10;
const LENGTH_JITTER: f32 = 0.04;
const DIRECTION_JITTER: f32 = 0.02;
const BRISTLES_PER_CLUMP: u32 = 7;
const CLUMP_SPREAD: f32 = 0.35;
const TIP_RADIUS_FRACTION: f32 = 0.3;

pub fn rest_positions(preset: &BrushPreset, rng: &mut SplitMix64) -> Vec<Vec4> {
    let mut positions = Vec::with_capacity((preset.bristle_count * VERTICES_PER_BRISTLE) as usize);
    let clump_count = preset.bristle_count.div_ceil(BRISTLES_PER_CLUMP).max(1);
    let clumps: Vec<Vec2> = (0..clump_count).map(|index| root_position(preset, index, clump_count, rng)).collect();
    let clump_radius = clump_extent(preset, clump_count);
    for index in 0..preset.bristle_count {
        let clump = clumps[(index % clump_count) as usize];
        let root = match preset.shape {
            BrushShape::Shaper => grid_root(preset, index),
            _ => clamp_to_ferrule(preset, clump + jitter(rng, clump_radius)),
        };
        let length = bristle_length(preset, root) * (1.0 + rng.range(-LENGTH_JITTER, LENGTH_JITTER));
        let direction = rest_direction(preset, root, rng);
        let segment = length / (VERTICES_PER_BRISTLE - 2) as f32;
        positions.push(Vec3::new(root.x, root.y, segment).extend(0.0));
        for vertex in 1..VERTICES_PER_BRISTLE {
            let along = segment * (vertex - 1) as f32;
            positions.push((root.extend(0.0) + direction * along).extend(0.0));
        }
    }
    positions
}

fn root_position(preset: &BrushPreset, index: u32, count: u32, rng: &mut SplitMix64) -> Vec2 {
    let half = Vec2::new(preset.width, preset.thickness) * 0.5;
    match preset.shape {
        BrushShape::Round => sunflower(index, count, half.x) + jitter(rng, half.x * 0.05),
        BrushShape::Flat | BrushShape::Filbert | BrushShape::Shaper => {
            Vec2::new(rng.range(-half.x, half.x), rng.range(-half.y, half.y))
        }
    }
}

fn grid_root(preset: &BrushPreset, index: u32) -> Vec2 {
    let rows = (preset.bristle_count as f32 * preset.thickness / preset.width).sqrt().round().max(1.0) as u32;
    let columns = preset.bristle_count.div_ceil(rows);
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
        BrushShape::Flat | BrushShape::Filbert | BrushShape::Shaper => root.clamp(-half, half),
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
        BrushShape::Round | BrushShape::Flat | BrushShape::Shaper => preset.length,
    }
}

fn rest_tip(preset: &BrushPreset, root: Vec2) -> Vec2 {
    match preset.shape {
        BrushShape::Round => root * TIP_RADIUS_FRACTION,
        BrushShape::Flat | BrushShape::Filbert => Vec2::new(root.x, root.y * TIP_RADIUS_FRACTION),
        BrushShape::Shaper => root,
    }
}

fn rest_direction(preset: &BrushPreset, root: Vec2, rng: &mut SplitMix64) -> Vec3 {
    let wobble = jitter(rng, DIRECTION_JITTER) * preset.length;
    let towards_tip = rest_tip(preset, root) - root + wobble;
    Vec3::new(towards_tip.x, towards_tip.y, -preset.length).normalize()
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
