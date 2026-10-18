use crate::paint::rheology::yield_stress;
use crate::units::{GRAVITY, metres};

pub const DENSITY: f32 = 1000.0;
pub const SURFACE_TENSION: f32 = 0.04;
const VISCOSITY_STIFF: f32 = 20.0;
const VISCOSITY_OILY: f32 = 0.05;
pub const POURED_YIELD_SHARE: f32 = 0.1;
pub const POURED_PUDDLE: f32 = crate::units::cm(0.04);
const WETTING_SLOPE: f32 = 0.36;

pub fn wetted_mound(volume: f32, yield_stress: f32) -> crate::paint::rheology::Mound {
    let settled = crate::paint::rheology::mound_settling_to(volume, yield_stress * POURED_YIELD_SHARE, f32::INFINITY, POURED_PUDDLE);
    let cap_radius = (2.0 * volume / (std::f32::consts::PI * WETTING_SLOPE)).cbrt();
    let radius = settled.radius.max(cap_radius);
    crate::paint::rheology::Mound { radius, height: 2.0 * volume / (std::f32::consts::PI * radius * radius) }
}

pub fn wetted_strand(area: f32, yield_stress: f32) -> crate::paint::rheology::Strand {
    let settled = crate::paint::rheology::strand_settling_to(area, yield_stress * POURED_YIELD_SHARE, f32::INFINITY, POURED_PUDDLE);
    let cap_half_width = (0.75 * area / WETTING_SLOPE).sqrt();
    let half_width = settled.half_width.max(cap_half_width);
    crate::paint::rheology::Strand { half_width, height: 0.75 * area / half_width }
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
