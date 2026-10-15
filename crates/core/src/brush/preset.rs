use crate::units::cm;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrushShape {
    Round,
    Flat,
    Filbert,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushPreset {
    pub shape: BrushShape,
    pub bristle_count: u32,
    pub length: f32,
    pub width: f32,
    pub thickness: f32,
    pub flare: f32,
    pub friction: f32,
    pub recovery: f32,
    pub splay: f32,
    pub sink: f32,
    pub sample_capacity: f32,
    pub deposit_rate: f32,
    pub pickup_rate: f32,
}

impl BrushPreset {
    pub fn round(bristle_count: u32, radius: f32, length: f32) -> Self {
        Self {
            shape: BrushShape::Round,
            bristle_count,
            length,
            width: radius * 2.0,
            thickness: radius * 2.0,
            flare: 0.15,
            friction: 0.6,
            recovery: 0.25,
            splay: 0.55,
            sink: 0.5,
            sample_capacity: 12.0,
            deposit_rate: 0.06,
            pickup_rate: 0.002,
        }
    }

    pub fn flat(bristle_count: u32, width: f32, thickness: f32, length: f32) -> Self {
        Self { shape: BrushShape::Flat, width, thickness, ..Self::round(bristle_count, 0.0, length) }
    }

    pub fn filbert(bristle_count: u32, width: f32, thickness: f32, length: f32) -> Self {
        Self { shape: BrushShape::Filbert, ..Self::flat(bristle_count, width, thickness, length) }
    }

    pub fn default_round() -> Self {
        Self::round(200, cm(0.35), cm(2.2))
    }

    pub fn segment_length(&self) -> f32 {
        self.length / (super::geometry::VERTICES_PER_BRISTLE - 2) as f32
    }
}
