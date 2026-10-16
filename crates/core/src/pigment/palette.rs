use super::Coefficients;
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pigment {
    pub name: &'static str,
    pub coefficients: Coefficients,
}

const OIL_BODY: f32 = 3.0;

const fn pigment(name: &'static str, k: [f32; 3], s: [f32; 3]) -> Pigment {
    Pigment {
        name,
        coefficients: Coefficients {
            absorption: Vec3::from_array(k),
            scattering: Vec3::new(s[0] * OIL_BODY, s[1] * OIL_BODY, s[2] * OIL_BODY),
        },
    }
}

pub const PALETTE: [Pigment; 10] = [
    pigment("titanium_white", [0.01, 0.01, 0.01], [1.2, 1.2, 1.2]),
    pigment("cadmium_yellow", [0.10, 0.36, 3.45], [0.97, 0.65, 0.007]),
    pigment("cadmium_red", [0.14, 1.08, 1.68], [0.77, 0.015, 0.018]),
    pigment("quinacridone_rose", [0.22, 1.47, 0.57], [0.05, 0.003, 0.03]),
    pigment("french_ultramarine", [1.5, 0.5, 0.06], [0.005, 0.005, 0.09]),
    pigment("cerulean_blue", [1.52, 0.32, 0.25], [0.06, 0.26, 0.40]),
    pigment("phthalo_green", [1.55, 0.47, 0.63], [0.01, 0.05, 0.035]),
    pigment("burnt_umber", [0.74, 1.54, 2.10], [0.09, 0.09, 0.004]),
    pigment("indian_red", [0.46, 1.07, 1.50], [1.28, 0.38, 0.21]),
    pigment("ivory_black", [2.5, 2.5, 2.5], [0.02, 0.02, 0.02]),
];

pub fn by_name(name: &str) -> Option<Pigment> {
    PALETTE.iter().copied().find(|p| p.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_findable() {
        for pigment in PALETTE {
            assert_eq!(by_name(pigment.name), Some(pigment));
        }
        assert!(by_name("unobtainium").is_none());
    }

    #[test]
    fn yellow_and_blue_make_green() {
        let yellow = by_name("cadmium_yellow").unwrap().coefficients;
        let blue = by_name("french_ultramarine").unwrap().coefficients;
        let green = yellow.mix(1.0, blue, 1.0).opaque_reflectance();
        assert!(green.y > green.x && green.y > green.z, "got {green}");
    }

    #[test]
    fn white_is_light_and_black_is_dark() {
        let white = by_name("titanium_white").unwrap().coefficients.opaque_reflectance();
        let black = by_name("ivory_black").unwrap().coefficients.opaque_reflectance();
        assert!(white.min_element() > 0.8);
        assert!(black.max_element() < 0.05);
    }

    #[test]
    fn primaries_have_expected_hue() {
        let red = by_name("cadmium_red").unwrap().coefficients.opaque_reflectance();
        let blue = by_name("french_ultramarine").unwrap().coefficients.opaque_reflectance();
        assert!(red.x > red.y && red.x > red.z);
        assert!(blue.z > blue.x && blue.z > blue.y);
    }
}
