mod palette;

pub use palette::{PALETTE, Pigment, by_name};

use glam::Vec3;

pub const OPTICAL_THICKNESS_PER_PIXEL: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefficients {
    pub absorption: Vec3,
    pub scattering: Vec3,
}

impl Coefficients {
    pub fn mix(self, mass: f32, other: Coefficients, other_mass: f32) -> Coefficients {
        let total = mass + other_mass;
        if total <= 0.0 {
            return self;
        }
        Coefficients {
            absorption: (self.absorption * mass + other.absorption * other_mass) / total,
            scattering: (self.scattering * mass + other.scattering * other_mass) / total,
        }
    }

    pub fn opaque_reflectance(self) -> Vec3 {
        Vec3::from_array(std::array::from_fn(|i| opaque_reflectance(self.absorption[i], self.scattering[i])))
    }

    pub fn layer_reflectance(self, thickness: f32, substrate: Vec3) -> Vec3 {
        Vec3::from_array(std::array::from_fn(|i| {
            layer_reflectance(self.absorption[i], self.scattering[i], thickness, substrate[i])
        }))
    }
}

pub fn opaque_reflectance(k: f32, s: f32) -> f32 {
    if s <= 1e-6 {
        return 0.0;
    }
    let ratio = k / s;
    1.0 + ratio - (ratio * ratio + 2.0 * ratio).sqrt()
}

pub fn layer_reflectance(k: f32, s: f32, thickness: f32, substrate: f32) -> f32 {
    if thickness <= 0.0 {
        return substrate;
    }
    if s <= 1e-5 {
        return substrate * (-2.0 * k * thickness).exp();
    }
    let a = 1.0 + k / s;
    let b = (a * a - 1.0).max(0.0).sqrt();
    let argument = b * s * thickness;
    if argument > 20.0 {
        return opaque_reflectance(k, s);
    }
    let coth = 1.0 / argument.tanh();
    (1.0 - substrate * (a - b * coth)) / (a - substrate + b * coth)
}

pub fn linear_to_srgb(linear: f32) -> f32 {
    let clamped = linear.clamp(0.0, 1.0);
    if clamped <= 0.003_130_8 { clamped * 12.92 } else { 1.055 * clamped.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white() -> Coefficients {
        Coefficients { absorption: Vec3::splat(0.01), scattering: Vec3::splat(1.0) }
    }

    fn black() -> Coefficients {
        Coefficients { absorption: Vec3::splat(3.0), scattering: Vec3::splat(0.02) }
    }

    #[test]
    fn zero_thickness_shows_substrate() {
        let substrate = Vec3::new(0.9, 0.8, 0.7);
        assert_eq!(white().layer_reflectance(0.0, substrate), substrate);
    }

    #[test]
    fn thick_layer_approaches_opaque_reflectance() {
        let thick = white().layer_reflectance(100.0, Vec3::ZERO);
        let opaque = white().opaque_reflectance();
        assert!((thick - opaque).length() < 1e-3);
        assert!(opaque.x > 0.85);
    }

    #[test]
    fn thin_white_over_black_is_grey() {
        let thin = white().layer_reflectance(0.5, Vec3::ZERO).x;
        assert!(thin > 0.1 && thin < 0.8, "got {thin}");
    }

    #[test]
    fn black_is_dark_when_opaque() {
        assert!(black().opaque_reflectance().x < 0.05);
    }

    #[test]
    fn mixing_is_mass_weighted() {
        let mixed = white().mix(1.0, black(), 3.0);
        assert!((mixed.absorption.x - (0.01 + 9.0) / 4.0).abs() < 1e-6);
        assert!((mixed.scattering.x - (1.0 + 0.06) / 4.0).abs() < 1e-6);
    }

    #[test]
    fn mixing_with_nothing_keeps_coefficients() {
        assert_eq!(white().mix(0.0, black(), 0.0), white());
    }

    #[test]
    fn reflectance_is_monotonic_in_thickness_for_dark_over_light() {
        let mut previous = 1.0;
        for step in 1..20 {
            let r = black().layer_reflectance(step as f32 * 0.1, Vec3::splat(0.9)).x;
            assert!(r <= previous + 1e-6, "step {step}: {r} > {previous}");
            previous = r;
        }
    }
}
