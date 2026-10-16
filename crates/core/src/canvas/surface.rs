use super::noise::hash;
use super::{CanvasSize, linen, paper};
use crate::units::cm;
use serde::{Deserialize, Serialize};

const GRAIN_AMPLITUDE: f32 = 0.012;
const FIBRE_SHEEN: f32 = 0.025;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Medium {
    Oil,
    Watercolour,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Linen,
    HotPressed,
    ColdPressed,
    Rough,
    Rice,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaperProperties {
    pub absorbency: f32,
    pub capacity: (f32, f32),
    pub granulation: f32,
}

impl Surface {
    pub const ALL: [Surface; 5] = [Surface::Linen, Surface::HotPressed, Surface::ColdPressed, Surface::Rough, Surface::Rice];

    pub fn name(self) -> &'static str {
        match self {
            Surface::Linen => "Linen canvas",
            Surface::HotPressed => "Hot-pressed paper",
            Surface::ColdPressed => "Cold-pressed paper",
            Surface::Rough => "Rough paper",
            Surface::Rice => "Rice paper",
        }
    }

    pub fn medium(self) -> Medium {
        match self {
            Surface::Linen => Medium::Oil,
            _ => Medium::Watercolour,
        }
    }

    pub fn tooth(self) -> f32 {
        match self {
            Surface::Linen => super::DEFAULT_TOOTH,
            Surface::HotPressed => cm(0.003),
            Surface::ColdPressed => cm(0.008),
            Surface::Rough => cm(0.015),
            Surface::Rice => cm(0.004),
        }
    }

    pub fn colour(self) -> [f32; 3] {
        match self {
            Surface::Linen => [0.93, 0.91, 0.86],
            Surface::HotPressed => [0.95, 0.945, 0.92],
            Surface::ColdPressed => [0.94, 0.93, 0.895],
            Surface::Rough => [0.935, 0.92, 0.88],
            Surface::Rice => [0.95, 0.935, 0.89],
        }
    }

    pub fn paper(self) -> PaperProperties {
        match self {
            Surface::Linen => PaperProperties { absorbency: 0.0, capacity: (0.0, 0.0), granulation: 0.0 },
            Surface::HotPressed => PaperProperties { absorbency: 0.1, capacity: (0.25, 0.45), granulation: 0.2 },
            Surface::ColdPressed => PaperProperties { absorbency: 0.15, capacity: (0.3, 0.6), granulation: 0.5 },
            Surface::Rough => PaperProperties { absorbency: 0.18, capacity: (0.35, 0.7), granulation: 0.8 },
            Surface::Rice => PaperProperties { absorbency: 0.6, capacity: (0.5, 0.9), granulation: 0.15 },
        }
    }

    pub fn relief(self, size: CanvasSize, seed: u64) -> Vec<f32> {
        match self {
            Surface::Linen => linen::relief(size, seed),
            Surface::HotPressed => paper::hot_pressed(size, seed),
            Surface::ColdPressed => paper::cold_pressed(size, seed),
            Surface::Rough => paper::rough(size, seed),
            Surface::Rice => paper::rice(size, seed),
        }
    }

    pub fn colour_layer(self, relief: &[f32], size: CanvasSize, seed: u64) -> Vec<[f32; 4]> {
        let base = self.colour();
        let sheen = if self == Surface::Rice { FIBRE_SHEEN } else { 0.0 };
        relief
            .iter()
            .enumerate()
            .map(|(index, height)| {
                let (x, y) = (index as u32 % size.width, index as u32 / size.width);
                let shade = 1.0 + (hash(x, y, seed) - 0.5) * 2.0 * GRAIN_AMPLITUDE + height * sheen;
                [0.0, base[0] * shade, base[1] * shade, base[2] * shade]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: CanvasSize = CanvasSize { width: 160, height: 120 };

    fn roughness(surface: Surface) -> f32 {
        let heights: Vec<f32> = surface.relief(SIZE, 5).iter().map(|h| h * surface.tooth()).collect();
        let w = SIZE.width as usize;
        let mut total = 0.0;
        for y in 0..SIZE.height as usize {
            for x in 1..w {
                total += (heights[y * w + x] - heights[y * w + x - 1]).abs();
            }
        }
        total / heights.len() as f32
    }

    #[test]
    fn every_surface_is_deterministic_and_in_unit_range() {
        for surface in Surface::ALL {
            let heights = surface.relief(SIZE, 5);
            assert_eq!(heights, surface.relief(SIZE, 5), "{surface:?}");
            assert_ne!(heights, surface.relief(SIZE, 6), "{surface:?}");
            let (low, high) = heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
            assert!(low >= -1.0 && high <= 1.0 && high - low > 1.0, "{surface:?} spans {low}..{high}");
        }
    }

    #[test]
    fn papers_get_rougher_from_hot_pressed_to_rough() {
        let (hot, cold, rough) = (roughness(Surface::HotPressed), roughness(Surface::ColdPressed), roughness(Surface::Rough));
        assert!(hot < cold && cold < rough, "hot {hot} cold {cold} rough {rough}");
    }

    #[test]
    fn papers_are_watercolour_and_linen_is_oil() {
        for surface in Surface::ALL {
            let expected = if surface == Surface::Linen { Medium::Oil } else { Medium::Watercolour };
            assert_eq!(surface.medium(), expected);
        }
        assert!(Surface::Rice.paper().absorbency > Surface::ColdPressed.paper().absorbency * 3.0);
    }

    #[test]
    fn the_colour_layer_is_a_pale_dry_layer() {
        for surface in Surface::ALL {
            let relief = surface.relief(SIZE, 5);
            for pixel in surface.colour_layer(&relief, SIZE, 5) {
                assert_eq!(pixel[0], 0.0);
                assert!(pixel[1] > 0.85 && pixel[3] > 0.8, "{surface:?} {pixel:?}");
            }
        }
    }
}
