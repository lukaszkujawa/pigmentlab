use crate::paint::Profile;
use crate::paint::rheology::{mound_settling_to, strand_settling_to, yield_stress};
use crate::units::{GRAVITY, metres, pixels};
use std::f32::consts::PI;

pub const DENSITY: f32 = 1000.0;
pub const SURFACE_TENSION: f32 = 0.04;
const VISCOSITY_STIFF: f32 = 20.0;
const VISCOSITY_OILY: f32 = 0.05;
pub const POURED_YIELD_SHARE: f32 = 0.1;
pub const POURED_PUDDLE: f32 = crate::units::cm(0.04);
pub const WETTING_SLOPE: f32 = 0.36;
const YIELD_HOLD_RANGE: f32 = 0.5;
const EDGE_SLOPE: f32 = 0.8;

fn puddle_height() -> f32 {
    let capillary_length = (SURFACE_TENSION / (DENSITY * GRAVITY)).sqrt();
    pixels(2.0 * capillary_length * (0.5 * WETTING_SLOPE.atan()).sin())
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wetted {
    pub reach: f32,
    pub height: f32,
    pub stiffness: f32,
}

impl Wetted {
    pub fn flatness(&self) -> f32 {
        (1.0 - 2.0 * self.height / (EDGE_SLOPE * self.reach.max(f32::EPSILON))).clamp(0.0, 1.0)
    }

    pub fn profile(&self, ragged: f32) -> Profile {
        Profile { flatness: self.flatness(), stiffness: self.stiffness, ragged, open: [false; 2], taper: 1.0, oval: false }
    }
}

fn held_by_yield(yielded: f32, wetted: f32) -> f32 {
    ((wetted / yielded.max(f32::EPSILON) - 1.0) / YIELD_HOLD_RANGE).clamp(0.0, 1.0)
}

pub fn wetted_mound(volume: f32, yield_stress: f32) -> Wetted {
    if volume <= 0.0 {
        return Wetted::default();
    }
    let settled = mound_settling_to(volume, yield_stress * POURED_YIELD_SHARE, f32::INFINITY, POURED_PUDDLE);
    let capillary = (4.0 * volume / (PI * WETTING_SLOPE)).cbrt();
    let pancake = (volume / (PI * puddle_height())).sqrt();
    let wetted = capillary.max(pancake);
    let reach = settled.radius.min(wetted);
    Wetted { reach, height: (2.0 * volume / (PI * reach * reach)).min(puddle_height().max(settled.height)), stiffness: held_by_yield(settled.radius, wetted) }
}

pub fn wetted_strand(area: f32, yield_stress: f32) -> Wetted {
    if area <= 0.0 {
        return Wetted::default();
    }
    let settled = strand_settling_to(area, yield_stress * POURED_YIELD_SHARE, f32::INFINITY, POURED_PUDDLE);
    let capillary = (1.5 * area / WETTING_SLOPE).sqrt();
    let pancake = 0.5 * area / puddle_height();
    let wetted = capillary.max(pancake);
    let reach = settled.half_width.min(wetted);
    Wetted { reach, height: (0.75 * area / reach).min(puddle_height().max(settled.height)), stiffness: held_by_yield(settled.half_width, wetted) }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fluid {
    pub viscosity: f32,
    pub yield_stress: f32,
}

impl Fluid {
    pub fn of(medium: f32) -> Self {
        let oily = medium.clamp(0.0, 1.0);
        Self {
            viscosity: (VISCOSITY_STIFF.ln() * (1.0 - oily) + VISCOSITY_OILY.ln() * oily).exp(),
            yield_stress: DENSITY * GRAVITY * metres(yield_stress(oily)),
        }
    }

    pub fn kinematic_viscosity(&self) -> f32 {
        self.viscosity / DENSITY
    }

    pub fn ohnesorge(&self, diameter: f32) -> f32 {
        self.viscosity / (DENSITY * SURFACE_TENSION * diameter).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn medium_thins_paint_from_a_paste_to_a_runny_liquid() {
        let stiff = Fluid::of(0.0);
        let enamel = Fluid::of(0.35);
        let runny = Fluid::of(1.0);
        assert!(stiff.viscosity > 10.0 && stiff.yield_stress > 50.0, "tube paint is a stiff paste: {stiff:?}");
        assert!((2.0..8.0).contains(&enamel.viscosity), "around a third medium paint pours like Pollock's enamel (3-5 Pa s): {enamel:?}");
        assert!(runny.viscosity < 0.1 && runny.yield_stress < 1.0, "all medium pours freely: {runny:?}");
    }
}
