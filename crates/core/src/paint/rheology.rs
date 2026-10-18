use crate::units::cm;
use std::f32::consts::PI;

pub const YIELD_STRESS_STIFF: f32 = cm(0.8);
pub const YIELD_STRESS_OILY: f32 = 0.05;
pub const PUDDLE_HEIGHT: f32 = cm(0.1);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Strand {
    pub half_width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mound {
    pub radius: f32,
    pub height: f32,
}

pub fn yield_stress(medium: f32) -> f32 {
    let oily = medium.clamp(0.0, 1.0);
    (YIELD_STRESS_STIFF.ln() * (1.0 - oily) + YIELD_STRESS_OILY.ln() * oily).exp()
}

pub fn slump_height(yield_stress: f32, reach: f32) -> f32 {
    (2.0 * yield_stress * reach).sqrt()
}

pub fn strand(area: f32, yield_stress: f32, tallest: f32) -> Strand {
    strand_settling_to(area, yield_stress, tallest, PUDDLE_HEIGHT)
}

pub fn strand_settling_to(area: f32, yield_stress: f32, tallest: f32, puddle: f32) -> Strand {
    if area <= 0.0 {
        return Strand::default();
    }
    let semicircle = (2.0 * area / PI).sqrt();
    let mut half_width = semicircle;
    let mut height = semicircle;
    if height > tallest {
        height = tallest;
        half_width = 2.0 * area / (PI * tallest);
    }
    if slump_height(yield_stress, half_width) < height {
        half_width = (2.0 * area / (PI * (2.0 * yield_stress).sqrt())).powf(2.0 / 3.0);
        height = slump_height(yield_stress, half_width);
    }
    let puddle = puddle.min(semicircle).min(tallest);
    if height < puddle {
        height = puddle;
        half_width = 2.0 * area / (PI * puddle);
    }
    Strand { half_width, height }
}

pub fn mound(volume: f32, yield_stress: f32, tallest: f32) -> Mound {
    mound_settling_to(volume, yield_stress, tallest, PUDDLE_HEIGHT)
}

pub fn mound_settling_to(volume: f32, yield_stress: f32, tallest: f32, puddle: f32) -> Mound {
    if volume <= 0.0 {
        return Mound::default();
    }
    let hemisphere = (1.5 * volume / PI).cbrt();
    let mut radius = hemisphere;
    let mut height = hemisphere;
    if height > tallest {
        height = tallest;
        radius = (1.5 * volume / (PI * tallest)).sqrt();
    }
    if slump_height(yield_stress, radius) < height {
        radius = (1.5 * volume / (PI * (2.0 * yield_stress).sqrt())).powf(0.4);
        height = slump_height(yield_stress, radius);
    }
    let puddle = puddle.min(hemisphere).min(tallest);
    if height < puddle {
        height = puddle;
        radius = (1.5 * volume / (PI * puddle)).sqrt();
    }
    Mound { radius, height }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strand_area(strand: Strand) -> f32 {
        0.5 * PI * strand.half_width * strand.height
    }

    fn mound_volume(mound: Mound) -> f32 {
        2.0 / 3.0 * PI * mound.radius * mound.radius * mound.height
    }

    #[test]
    fn medium_lowers_the_yield_stress_geometrically() {
        assert!((yield_stress(0.0) - YIELD_STRESS_STIFF).abs() < 1e-3);
        assert!((yield_stress(1.0) - YIELD_STRESS_OILY).abs() < 1e-5);
        assert!((yield_stress(0.5) - (YIELD_STRESS_STIFF * YIELD_STRESS_OILY).sqrt()).abs() < 1e-3);
    }

    #[test]
    fn a_stiff_strand_is_a_semicircle_of_the_extruded_area() {
        let strand = strand(1257.0, YIELD_STRESS_STIFF, 60.0);
        assert!((strand.half_width - strand.height).abs() < 1e-3);
        assert!((strand_area(strand) - 1257.0).abs() < 0.5);
    }

    #[test]
    fn thinner_paint_slumps_into_a_wider_lower_strand_of_the_same_area() {
        let stiff = strand(1257.0, YIELD_STRESS_STIFF, 60.0);
        let half = strand(1257.0, yield_stress(0.5), 60.0);
        let runny = strand(1257.0, YIELD_STRESS_OILY, 60.0);
        assert!(stiff.height > half.height && half.height > runny.height);
        assert!(stiff.half_width < half.half_width && half.half_width < runny.half_width);
        assert!((runny.height - PUDDLE_HEIGHT).abs() < 1e-3, "runny paint lies as a capillary puddle, got {} px", runny.height);
        for s in [stiff, half, runny] {
            assert!((strand_area(s) - 1257.0).abs() < 0.5);
        }
    }

    #[test]
    fn a_fat_strand_flattens_at_its_ceiling_and_widens() {
        let fat = strand(20_000.0, YIELD_STRESS_STIFF, 60.0);
        assert_eq!(fat.height, 60.0);
        assert!(fat.half_width > 60.0);
        assert!((strand_area(fat) - 20_000.0).abs() < 1.0);
    }

    #[test]
    fn a_thread_of_runny_paint_is_never_taller_than_it_is_wide() {
        let thread = strand(20.0, YIELD_STRESS_OILY, 60.0);
        assert!(thread.height <= thread.half_width + 1e-3);
    }

    #[test]
    fn a_stiff_pile_grows_as_a_hemisphere_until_it_reaches_its_ceiling_and_then_widens() {
        let small = mound(10_000.0, YIELD_STRESS_STIFF, 60.0);
        let large = mound(1_000_000.0, YIELD_STRESS_STIFF, 60.0);
        assert!((small.radius - small.height).abs() < 1e-3);
        assert_eq!(large.height, 60.0);
        assert!(large.radius > 2.0 * small.radius);
        for m in [small, large] {
            assert!((mound_volume(m) - if m == small { 10_000.0 } else { 1_000_000.0 }).abs() < 1.0);
        }
    }

    #[test]
    fn a_runny_pile_lies_flat_as_a_wide_puddle() {
        let puddle = mound(200_000.0, YIELD_STRESS_OILY, 60.0);
        assert!((puddle.height - PUDDLE_HEIGHT).abs() < 1e-3);
        assert!(puddle.radius > 100.0);
        assert!((mound_volume(puddle) - 200_000.0).abs() < 1.0);
    }
}
