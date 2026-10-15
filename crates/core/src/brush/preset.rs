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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NamedPreset {
    pub name: &'static str,
    pub preset: BrushPreset,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushAdjustments {
    pub scale: f32,
    pub bristle_count: Option<u32>,
    pub stiffness: f32,
}

impl Default for BrushAdjustments {
    fn default() -> Self {
        Self { scale: 1.0, bristle_count: None, stiffness: 0.5 }
    }
}

impl BrushPreset {
    pub const fn round(bristle_count: u32, radius: f32, length: f32) -> Self {
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
            sample_capacity: cm(0.3),
            deposit_rate: cm(0.0015),
            pickup_rate: cm(0.00005),
        }
    }

    pub const fn flat(bristle_count: u32, width: f32, thickness: f32, length: f32) -> Self {
        Self { shape: BrushShape::Flat, width, thickness, ..Self::round(bristle_count, 0.0, length) }
    }

    pub const fn filbert(bristle_count: u32, width: f32, thickness: f32, length: f32) -> Self {
        Self { shape: BrushShape::Filbert, ..Self::flat(bristle_count, width, thickness, length) }
    }

    pub fn default_round() -> Self {
        Self::round(200, cm(0.35), cm(2.2))
    }

    pub fn segment_length(&self) -> f32 {
        self.length / (super::geometry::VERTICES_PER_BRISTLE - 2) as f32
    }

    pub fn adjusted(self, adjustments: BrushAdjustments) -> Self {
        let scale = adjustments.scale.clamp(0.3, 3.0);
        let bristle_count = adjustments
            .bristle_count
            .unwrap_or_else(|| (self.bristle_count as f32 * scale * scale).round() as u32)
            .clamp(20, 600);
        let stiffness = adjustments.stiffness.clamp(0.0, 1.0);
        Self {
            bristle_count,
            length: self.length * scale.sqrt(),
            width: self.width * scale,
            thickness: self.thickness * scale,
            splay: 0.3 + 0.5 * (1.0 - stiffness),
            sink: 0.3 + 0.4 * (1.0 - stiffness),
            friction: 0.4 + 0.4 * stiffness,
            ..self
        }
    }
}

pub const PRESETS: [NamedPreset; 6] = [
    NamedPreset { name: "Round small", preset: BrushPreset::round(70, cm(0.15), cm(1.6)) },
    NamedPreset { name: "Round medium", preset: BrushPreset::round(200, cm(0.35), cm(2.2)) },
    NamedPreset { name: "Round large", preset: BrushPreset::round(450, cm(0.6), cm(2.8)) },
    NamedPreset { name: "Flat", preset: BrushPreset::flat(400, cm(1.2), cm(0.3), cm(2.0)) },
    NamedPreset { name: "Filbert", preset: BrushPreset::filbert(350, cm(1.0), cm(0.3), cm(2.2)) },
    NamedPreset { name: "Rigger", preset: BrushPreset::round(40, cm(0.12), cm(3.0)) },
];

pub fn preset_by_name(name: &str) -> Option<BrushPreset> {
    PRESETS.iter().find(|named| named.name.eq_ignore_ascii_case(name)).map(|named| named.preset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_findable_by_name() {
        for named in PRESETS {
            assert_eq!(preset_by_name(named.name), Some(named.preset));
        }
        assert!(preset_by_name("mop").is_none());
    }

    #[test]
    fn scaling_grows_width_and_bristles() {
        let base = BrushPreset::default_round();
        let bigger = base.adjusted(BrushAdjustments { scale: 2.0, ..Default::default() });
        assert!((bigger.width - base.width * 2.0).abs() < 1e-4);
        assert!(bigger.bristle_count >= base.bristle_count * 2);
        assert!(bigger.bristle_count <= 600);
    }

    #[test]
    fn stiff_brushes_splay_and_sink_less() {
        let base = BrushPreset::default_round();
        let stiff = base.adjusted(BrushAdjustments { stiffness: 1.0, ..Default::default() });
        let soft = base.adjusted(BrushAdjustments { stiffness: 0.0, ..Default::default() });
        assert!(stiff.splay < soft.splay);
        assert!(stiff.sink < soft.sink);
    }
}
