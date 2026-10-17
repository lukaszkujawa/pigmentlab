use super::rheology::{Mound, Strand, YIELD_STRESS_STIFF, mound, strand};
use crate::rng::SplitMix64;
use glam::Vec2;
use std::f32::consts::{PI, TAU};

pub const MAX_REACH: f32 = 150.0;
const FASTEST_EXTRUSION: f32 = crate::units::cm(5.0);
const TRICKLE: f32 = crate::units::cm(1.0);
const ROPE_HEIGHT_PER_NOZZLE_RADIUS: f32 = 1.6;
const PILE_HEIGHT_PER_NOZZLE_RADIUS: f32 = 3.0;
const PILE_FADE_PER_NOZZLE_RADIUS: f32 = 0.15;
const MEMORY_PER_REACH: f32 = 3.0;
const STROKE_GAP: f32 = 1000.0;
const SETTLING_SECONDS: f32 = 0.1;
const ROPE_STIFFNESS_SECONDS: f32 = 0.02;
const BACKLOG_SECONDS: f32 = 1.0;
const CLOSED_STRAND: f32 = 0.01;
const WOBBLE_WAVELENGTHS_PER_NOZZLE_RADIUS: [f32; 3] = [2.3, 3.7, 6.1];
const WOBBLE: f32 = 0.06;
const WOBBLE_DRIFT_PER_SECOND: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stamp {
    pub path: [Vec2; 2],
    pub strand: Strand,
    pub strand_core: f32,
    pub strand_drape: f32,
    pub mound: Mound,
    pub mound_core: f32,
    pub mound_drape: f32,
    pub squeezed: f32,
    pub backlog_limit: f32,
    pub arc: f32,
    pub memory: f32,
}

impl Stamp {
    pub fn reach(&self) -> f32 {
        self.strand.half_width.max(self.mound.radius)
    }
}

pub struct Squeeze {
    rng: SplitMix64,
    pointer: Vec2,
    nozzle: Vec2,
    arc: f32,
    strand_area: f32,
    pile_volume: f32,
    stroke_volume: f32,
    wobble: [(f32, f32); 3],
}

impl Squeeze {
    pub fn new(seed: u64) -> Self {
        Self { rng: SplitMix64::new(seed), pointer: Vec2::ZERO, nozzle: Vec2::ZERO, arc: STROKE_GAP, strand_area: 0.0, pile_volume: 0.0, stroke_volume: 0.0, wobble: [(0.0, 0.0); 3] }
    }

    pub fn press(&mut self, nozzle: Vec2) {
        self.pointer = nozzle;
        self.nozzle = nozzle;
        self.arc += STROKE_GAP;
        self.strand_area = 0.0;
        self.pile_volume = 0.0;
        self.stroke_volume = 0.0;
        for harmonic in &mut self.wobble {
            *harmonic = (self.rng.range(-WOBBLE, WOBBLE), self.rng.range(0.0, TAU));
        }
    }

    fn width_wobble(&mut self, nozzle_radius: f32, dt: f32) -> f32 {
        for (_, phase) in &mut self.wobble {
            *phase += self.rng.range(-1.0, 1.0) * WOBBLE_DRIFT_PER_SECOND * dt;
        }
        let total: f32 = self.wobble.iter().zip(WOBBLE_WAVELENGTHS_PER_NOZZLE_RADIUS).map(|((amplitude, phase), wavelength)| amplitude * (TAU * self.arc / (wavelength * nozzle_radius) + phase).sin()).sum();
        1.0 + total / 3.0f32.sqrt()
    }

    pub fn advance(&mut self, pointer: Vec2, nozzle_radius: f32, pressure: f32, yield_stress: f32, dt: f32) -> Option<Stamp> {
        let travelled = pointer.distance(self.pointer);
        self.pointer = pointer;
        let nozzle = self.nozzle + (pointer - self.nozzle) * (1.0 - (-dt / ROPE_STIFFNESS_SECONDS).exp());
        let path = [self.nozzle, nozzle];
        self.arc += nozzle.distance(self.nozzle);
        self.nozzle = nozzle;
        let opening = PI * nozzle_radius * nozzle_radius * pressure.clamp(0.0, 1.0);
        let laid = (opening * travelled).min(opening * FASTEST_EXTRUSION * dt);
        let squeezed = laid + opening * TRICKLE * dt;
        if squeezed <= 0.0 {
            return None;
        }
        self.stroke_volume += squeezed;
        if travelled > 0.0 {
            let closed = self.strand_area < CLOSED_STRAND * PI * nozzle_radius * nozzle_radius;
            self.strand_area += (laid / travelled - self.strand_area) * if closed { 1.0 } else { settling(dt) };
        } else {
            self.strand_area -= self.strand_area * settling(dt);
        }
        self.pile_volume = self.pile_volume * (-travelled / (PILE_FADE_PER_NOZZLE_RADIUS * nozzle_radius)).exp() + squeezed - laid;
        let rope_tallest = ROPE_HEIGHT_PER_NOZZLE_RADIUS * nozzle_radius;
        let pile_tallest = PILE_HEIGHT_PER_NOZZLE_RADIUS * nozzle_radius;
        let wobble = self.width_wobble(nozzle_radius, dt);
        let covered = mound(self.stroke_volume, yield_stress, pile_tallest).radius;
        let strand_stiff = wobbled(strand(self.strand_area, YIELD_STRESS_STIFF, rope_tallest), wobble);
        let strand = no_wider_than(widest(wobbled(strand(self.strand_area, yield_stress, rope_tallest), wobble), self.strand_area), covered);
        let mound_stiff = mound(self.pile_volume, YIELD_STRESS_STIFF, pile_tallest);
        let mound = roundest(mound(self.pile_volume, yield_stress, pile_tallest), self.pile_volume);
        let strand_drape = ratio(strand.height, strand_stiff.height);
        let mound_drape = ratio(mound.height, mound_stiff.height);
        let stamp = Stamp {
            path,
            strand,
            strand_core: (strand_stiff.half_width * strand_drape).max(nozzle_radius),
            strand_drape,
            mound,
            mound_core: (mound_stiff.radius * mound_drape).max(nozzle_radius),
            mound_drape,
            squeezed,
            backlog_limit: squeezed * BACKLOG_SECONDS / dt,
            arc: self.arc,
            memory: 0.0,
        };
        Some(Stamp { memory: MEMORY_PER_REACH * stamp.reach().max(nozzle_radius), ..stamp })
    }
}

fn wobbled(strand: Strand, factor: f32) -> Strand {
    Strand { half_width: strand.half_width * factor, height: strand.height * factor.sqrt() }
}

fn settling(dt: f32) -> f32 {
    1.0 - (-dt / SETTLING_SECONDS).exp()
}

fn ratio(slumped: f32, stiff: f32) -> f32 {
    if stiff <= 0.0 { 1.0 } else { (slumped / stiff).clamp(0.0, 1.0) }
}

fn no_wider_than(strand: Strand, reach: f32) -> Strand {
    Strand { half_width: strand.half_width.min(reach), height: strand.height.min(reach) }
}

fn widest(strand: Strand, area: f32) -> Strand {
    if strand.half_width <= MAX_REACH {
        return strand;
    }
    Strand { half_width: MAX_REACH, height: 2.0 * area / (PI * MAX_REACH) }
}

fn roundest(mound: Mound, volume: f32) -> Mound {
    if mound.radius <= MAX_REACH {
        return mound;
    }
    Mound { radius: MAX_REACH, height: 1.5 * volume / (PI * MAX_REACH * MAX_REACH) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 240.0;
    const NOZZLE: f32 = 20.0;

    fn stamps(path: impl Fn(f32) -> Vec2, seconds: f32, yield_stress: f32) -> Vec<Stamp> {
        let mut squeeze = Squeeze::new(1);
        squeeze.press(path(0.0));
        (1..=(seconds / DT) as usize).filter_map(|step| squeeze.advance(path(step as f32 * DT), NOZZLE, 1.0, yield_stress, DT)).collect()
    }

    fn along(speed: f32) -> impl Fn(f32) -> Vec2 {
        move |t| Vec2::new(speed * t, 0.0)
    }

    fn area(strand: Strand) -> f32 {
        0.5 * PI * strand.half_width * strand.height
    }

    #[test]
    fn a_hand_slower_than_the_fastest_extrusion_gets_a_nozzle_sized_rope() {
        for speed in [FASTEST_EXTRUSION * 0.1, FASTEST_EXTRUSION * 0.5, FASTEST_EXTRUSION] {
            let rope = stamps(along(speed), 600.0 / speed, YIELD_STRESS_STIFF);
            let mean = rope[rope.len() / 2..].iter().map(|s| area(s.strand)).sum::<f32>() / (rope.len() / 2) as f32;
            let last = rope.last().unwrap();
            assert!((mean / (PI * NOZZLE * NOZZLE) - 1.0).abs() < 0.08, "mean strand area {mean} should match the nozzle at speed {speed}");
            assert!(last.mound.radius < NOZZLE, "a moving nozzle leaves no visible pile, got radius {} at speed {speed}", last.mound.radius);
        }
    }

    #[test]
    fn a_hand_faster_than_the_tube_can_feed_draws_a_thinner_rope() {
        let width = |speed: f32| stamps(along(speed), 1.0, YIELD_STRESS_STIFF).pop().unwrap().strand.half_width;
        assert!(width(FASTEST_EXTRUSION * 4.0) < width(FASTEST_EXTRUSION) * 0.6);
    }

    #[test]
    fn the_strand_width_settles_over_a_few_hundredths_of_a_second() {
        let mut squeeze = Squeeze::new(1);
        squeeze.press(Vec2::ZERO);
        let mut x = 0.0;
        let mut widths = Vec::new();
        for step in 0..96 {
            x += if step < 48 { FASTEST_EXTRUSION } else { FASTEST_EXTRUSION * 3.0 } * DT;
            widths.push(squeeze.advance(Vec2::new(x, 0.0), NOZZLE, 1.0, YIELD_STRESS_STIFF, DT).unwrap().strand.half_width);
        }
        let jump = widths[47] - widths[48];
        let full = widths[47] - widths[95];
        assert!(jump < full * 0.25 && widths[95] < widths[47] * 0.8, "speeding up should thin the strand gradually: {:?}", &widths[44..56]);
    }

    #[test]
    fn the_rope_width_wobbles_gently_along_its_length_and_repeats_for_the_same_seed() {
        let rope = stamps(along(FASTEST_EXTRUSION), 1.0, YIELD_STRESS_STIFF);
        let widths: Vec<f32> = rope[rope.len() / 2..].iter().map(|s| s.strand.half_width).collect();
        let (lo, hi) = widths.iter().fold((f32::MAX, f32::MIN), |(lo, hi), w| (lo.min(*w), hi.max(*w)));
        assert!(hi > lo * 1.02 && hi < lo * 1.3, "the width should vary a little, {lo} to {hi}");
        assert_eq!(rope, stamps(along(FASTEST_EXTRUSION), 1.0, YIELD_STRESS_STIFF));
    }

    #[test]
    fn a_strand_is_never_wider_than_the_paint_squeezed_so_far_could_cover() {
        let runny = stamps(along(FASTEST_EXTRUSION), 0.5, 0.05);
        let widths: Vec<f32> = runny.iter().map(|s| s.strand.half_width).collect();
        assert!(widths[0] < 15.0, "a strand starts narrow, got {}", widths[0]);
        assert!(widths[12] < widths.last().unwrap() * 0.5);
        assert!(*widths.last().unwrap() > 60.0, "after half a second the runny strand should be wide, got {}", widths.last().unwrap());
    }

    #[test]
    fn half_pressure_squeezes_half_the_paint() {
        let mut full = Squeeze::new(1);
        let mut half = Squeeze::new(1);
        full.press(Vec2::ZERO);
        half.press(Vec2::ZERO);
        let squeezed = |squeeze: &mut Squeeze, pressure: f32| (1..=240).map(|step| squeeze.advance(Vec2::new(step as f32, 0.0), NOZZLE, pressure, YIELD_STRESS_STIFF, DT).unwrap().squeezed).sum::<f32>();
        let (a, b) = (squeezed(&mut full, 1.0), squeezed(&mut half, 0.5));
        assert!((b / a - 0.5).abs() < 1e-3);
    }

    #[test]
    fn a_still_nozzle_piles_a_mound_that_keeps_growing() {
        let still = stamps(|_| Vec2::new(100.0, 100.0), 2.0, YIELD_STRESS_STIFF);
        let at = |seconds: f32| still[(seconds / DT) as usize - 1].mound;
        assert!(at(0.5).radius < at(1.0).radius && at(1.0).radius < at(2.0).radius);
        assert!(at(2.0).radius > NOZZLE, "two seconds of holding still should pile a blob wider than the nozzle, got {}", at(2.0).radius);
        assert!(still.last().unwrap().strand.half_width < 1.0, "a strand closes while the nozzle rests");
    }

    #[test]
    fn the_pile_lets_go_of_the_nozzle_within_a_nozzle_radius_and_the_rope_keeps_its_width() {
        let mut squeeze = Squeeze::new(1);
        squeeze.press(Vec2::ZERO);
        for _ in 0..240 {
            squeeze.advance(Vec2::ZERO, NOZZLE, 1.0, YIELD_STRESS_STIFF, DT);
        }
        let mut x = 0.0;
        let mut radii = Vec::new();
        let mut widths = Vec::new();
        while x < NOZZLE {
            x += FASTEST_EXTRUSION * DT;
            let stamp = squeeze.advance(Vec2::new(x, 0.0), NOZZLE, 1.0, YIELD_STRESS_STIFF, DT).unwrap();
            radii.push(stamp.mound.radius);
            widths.push(stamp.strand.half_width);
        }
        assert!(radii[0] > NOZZLE && *radii.last().unwrap() < radii[0] * 0.3, "the pile should let go quickly: {radii:?}");
        assert!(radii.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(widths[0] > NOZZLE, "the rope leaving a pile starts at full width, got {}", widths[0]);
    }

    #[test]
    fn diluted_paint_lands_only_under_the_nozzle_and_drapes_as_a_film() {
        let runny = stamps(along(FASTEST_EXTRUSION), 1.0, 0.05).pop().unwrap();
        let stiff = stamps(along(FASTEST_EXTRUSION), 1.0, YIELD_STRESS_STIFF).pop().unwrap();
        assert_eq!(runny.strand_core, NOZZLE);
        assert!(runny.strand_drape < 0.4 && stiff.strand_drape == 1.0);
        assert!((stiff.strand_core - stiff.strand.half_width).abs() < 1e-3);
    }

    #[test]
    fn the_arc_keeps_counting_across_strokes_so_earlier_paint_is_never_fresh() {
        let mut squeeze = Squeeze::new(1);
        squeeze.press(Vec2::ZERO);
        let first = squeeze.advance(Vec2::new(1.0, 0.0), NOZZLE, 1.0, YIELD_STRESS_STIFF, DT).unwrap();
        squeeze.press(Vec2::new(1.0, 0.0));
        let second = squeeze.advance(Vec2::new(2.0, 0.0), NOZZLE, 1.0, YIELD_STRESS_STIFF, DT).unwrap();
        assert!(second.arc - first.arc > second.memory);
    }

    #[test]
    fn no_pressure_squeezes_nothing() {
        let mut squeeze = Squeeze::new(1);
        squeeze.press(Vec2::ZERO);
        assert!(squeeze.advance(Vec2::new(5.0, 0.0), NOZZLE, 0.0, YIELD_STRESS_STIFF, DT).is_none());
    }
}
