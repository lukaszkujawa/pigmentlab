use crate::paint::marks::{EGG, SMALLEST_RADIUS};
use crate::paint::Mark;
use glam::{IVec2, Vec2};
use std::f32::consts::TAU;

const COVERAGE_SCALE: f32 = 1024.0;
const LOBES: [f32; 5] = [2.0, 3.0, 5.0, 8.0, 13.0];
const LOBE_SIZES: [f32; 5] = [0.3, 0.2, 0.09, 0.05, 0.026];

pub struct Covered {
    pub pixel: IVec2,
    pub volume: f32,
}

struct Outline<'a> {
    mark: &'a Mark,
    lobe_sizes: [f32; 5],
    lobe_phases: [f32; 5],
    egg: f32,
}

impl<'a> Outline<'a> {
    fn of(mark: &'a Mark) -> Self {
        Self {
            mark,
            lobe_sizes: std::array::from_fn(|lobe| LOBE_SIZES[lobe] * (0.5 + mark_hash(mark, 2 * lobe as u32))),
            lobe_phases: std::array::from_fn(|lobe| TAU * mark_hash(mark, 2 * lobe as u32 + 1)),
            egg: 2.0 * mark_hash(mark, 11) - 1.0,
        }
    }

    fn edge(&self, angle: f32) -> f32 {
        if self.mark.profile.ragged == 0.0 {
            return 1.0;
        }
        let wobble: f32 = (0..LOBES.len()).map(|lobe| self.lobe_sizes[lobe] * (LOBES[lobe] * angle + self.lobe_phases[lobe]).sin()).sum();
        1.0 + self.mark.profile.ragged * wobble
    }

    fn oval_offset(&self, p: Vec2) -> f32 {
        let mark = self.mark;
        let along = mark.to - mark.from;
        let extent = along.length();
        let axis = if extent > 1e-6 { along / extent } else { Vec2::X };
        let away = p - 0.5 * (mark.from + mark.to);
        let radius = mark.radius.max(SMALLEST_RADIUS);
        let lengthwise = away.dot(axis) / (0.5 * extent + radius);
        let egg = 1.0 + EGG * self.egg * lengthwise.clamp(-1.0, 1.0);
        let local = Vec2::new(lengthwise, axis.perp_dot(away) / (radius * egg));
        local.length() / self.edge(local.y.atan2(local.x))
    }

    fn capsule_offset(&self, p: Vec2) -> (f32, f32) {
        let mark = self.mark;
        let [open_start, open_end] = mark.profile.open;
        let tip = mark.radius * mark.profile.taper;
        let along = mark.to - mark.from;
        let span = along.length_squared();
        let projected = if span > 1e-6 { (p - mark.from).dot(along) / span } else { 0.0 };
        let extent = span.sqrt();
        let fade = mark.radius.min(tip).max(SMALLEST_RADIUS).min(0.5 * extent);
        let mut share = 1.0;
        if open_start {
            share *= (0.5 + projected * extent / fade).clamp(0.0, 1.0);
        }
        if open_end {
            share *= (0.5 - (projected - 1.0) * extent / fade).clamp(0.0, 1.0);
        }
        let t = projected.clamp(if open_start { -1.0 } else { 0.0 }, if open_end { 2.0 } else { 1.0 });
        let away = p - (mark.from + along * t);
        let radius = mark.radius + (tip - mark.radius) * t.clamp(0.0, 1.0);
        let travelled = t * extent / radius.max(SMALLEST_RADIUS);
        (away.length() / (radius * self.edge(away.y.atan2(away.x) + travelled)).max(SMALLEST_RADIUS), share)
    }

    fn weight(&self, pixel: IVec2) -> u32 {
        let profile = &self.mark.profile;
        let p = pixel.as_vec2() + 0.5;
        let (offset, share) = if profile.oval { (self.oval_offset(p), 1.0) } else { self.capsule_offset(p) };
        if share <= 0.0 {
            return 0;
        }
        let rim = ((1.0 - offset) / (1.0 - profile.flatness).max(0.02)).clamp(0.0, 1.0);
        let height = (1.0 - (1.0 - rim) * (1.0 - rim)).powf(1.0 - 0.5 * profile.stiffness);
        (height * share * COVERAGE_SCALE).round() as u32
    }
}

pub fn rasterise(mark: &Mark) -> Vec<Covered> {
    let outline = Outline::of(mark);
    let reach = reach_of(mark);
    let corner = (mark.from.min(mark.to) - reach).floor().as_ivec2();
    let far = (mark.from.max(mark.to) + reach).ceil().as_ivec2();
    let weights: Vec<(IVec2, u32)> = (corner.y..=far.y)
        .flat_map(|y| (corner.x..=far.x).map(move |x| IVec2::new(x, y)))
        .map(|pixel| (pixel, outline.weight(pixel)))
        .filter(|(_, weight)| *weight > 0)
        .collect();
    let total: u64 = weights.iter().map(|(_, weight)| u64::from(*weight)).sum();
    if total == 0 {
        return Vec::new();
    }
    weights.into_iter().map(|(pixel, weight)| Covered { pixel, volume: mark.volume * weight as f32 / total as f32 }).collect()
}

fn reach_of(mark: &Mark) -> f32 {
    let egg = if mark.profile.oval { 1.0 + EGG } else { 1.0 };
    (mark.radius.max(mark.radius * mark.profile.taper) * (1.0 + mark.profile.ragged) * egg).max(SMALLEST_RADIUS)
}

fn mark_hash(mark: &Mark, salt: u32) -> f32 {
    let mut hash = mark.from.x.to_bits().wrapping_mul(0x8da6_b343) ^ mark.from.y.to_bits().wrapping_mul(0xd816_3841) ^ mark.to.x.to_bits().wrapping_mul(0xcb1a_b31f) ^ mark.to.y.to_bits().wrapping_mul(0x1656_67b1) ^ salt.wrapping_mul(0x9e37_79b9);
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x7feb_352d);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x846c_a68b);
    hash ^= hash >> 16;
    (hash >> 8) as f32 / 16_777_216.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Profile;
    use crate::pigment::Coefficients;
    use glam::Vec3;

    fn mark(from: Vec2, to: Vec2, profile: Profile) -> Mark {
        Mark { from, to, radius: 6.0, volume: 100.0, paint: Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium: 0.5, profile }
    }

    #[test]
    fn a_rasterised_mark_lays_all_its_paint_inside_its_reach() {
        for profile in [Profile::ROUND, Profile { oval: true, ragged: 0.2, ..Profile::ROUND }, Profile { open: [true, true], taper: 0.4, ragged: 0.1, ..Profile::ROUND }] {
            let mark = mark(Vec2::new(40.5, 30.0), Vec2::new(70.0, 44.0), profile);
            let covered = rasterise(&mark);
            let laid: f32 = covered.iter().map(|cover| cover.volume).sum();
            assert!((laid - mark.volume).abs() < 1e-3, "{profile:?} laid {laid}");
            let reach = reach_of(&mark) + 1.0;
            let nearest = |pixel: IVec2| {
                let p = pixel.as_vec2() + 0.5;
                let along = mark.to - mark.from;
                let t = ((p - mark.from).dot(along) / along.length_squared()).clamp(-1.0, 2.0);
                p.distance(mark.from + along * t)
            };
            assert!(covered.iter().all(|cover| nearest(cover.pixel) <= reach), "{profile:?} spilled past its reach");
        }
    }
}
