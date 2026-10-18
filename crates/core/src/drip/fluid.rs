use crate::paint::rheology::yield_stress;
use crate::units::{GRAVITY, metres};

pub const DENSITY: f32 = 1000.0;
pub const SURFACE_TENSION: f32 = 0.04;
const VISCOSITY_STIFF: f32 = 20.0;
const VISCOSITY_OILY: f32 = 0.05;

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
