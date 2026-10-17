use crate::units::cm;

pub const MAX_COMPRESSION: f32 = 0.4;
const BEND_REACH_FRACTION: f32 = 0.66;
const SINK_AT_LIGHT: f32 = 0.75;
const SINK_AT_FULL: f32 = 1.0;
const BLADE_CLEARANCE: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrushShape {
    Round,
    Flat,
    Filbert,
    Blade,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushPreset {
    pub shape: BrushShape,
    pub bristle_count: u32,
    pub length: f32,
    pub width: f32,
    pub thickness: f32,
    pub friction: f32,
    pub stiffness: f32,
    pub inertia: f32,
    pub splay: f32,
    pub cohesion: f32,
    pub sink: f32,
    pub sample_capacity: f32,
    pub deposit_rate: f32,
    pub pickup_rate: f32,
    pub blade_layer: f32,
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
            friction: 0.6,
            stiffness: 0.05,
            inertia: 0.05,
            splay: 0.55,
            cohesion: 0.85,
            sink: 0.5,
            sample_capacity: cm(0.1),
            deposit_rate: cm(0.0022),
            pickup_rate: cm(0.00005),
            blade_layer: 0.0,
        }
    }

    pub fn splayed_radius(&self) -> f32 {
        self.width.max(self.thickness) * 0.5 * (1.0 + self.splay)
    }

    pub fn wet_floor_keep(&self, pressure: f32) -> f32 {
        let light_touch = 1.0 - pressure.clamp(0.0, 1.0);
        if self.is_rigid() {
            return light_touch.powi(3);
        }
        let press = 1.0 - light_touch * light_touch;
        1.0 - press * (SINK_AT_LIGHT + (SINK_AT_FULL - SINK_AT_LIGHT) * self.sink)
    }

    pub fn ferrule_area(&self) -> f32 {
        match self.shape {
            BrushShape::Round => std::f32::consts::PI * self.width * self.thickness * 0.25,
            BrushShape::Flat | BrushShape::Filbert | BrushShape::Blade => self.width * self.thickness,
        }
    }

    pub fn bend_reach(&self) -> f32 {
        self.length * BEND_REACH_FRACTION * self.splay
    }

    pub const fn flat(bristle_count: u32, width: f32, thickness: f32, length: f32) -> Self {
        Self { shape: BrushShape::Flat, width, thickness, ..Self::round(bristle_count, 0.0, length) }
    }

    pub const fn filbert(bristle_count: u32, width: f32, thickness: f32, length: f32) -> Self {
        Self { shape: BrushShape::Filbert, ..Self::flat(bristle_count, width, thickness, length) }
    }

    pub const fn shaper(width: f32, thickness: f32, length: f32) -> Self {
        Self {
            shape: BrushShape::Blade,
            friction: 1.0,
            stiffness: 0.9,
            inertia: 0.0,
            splay: 0.0,
            cohesion: 0.0,
            sink: 1.0,
            sample_capacity: cm(0.003),
            deposit_rate: 0.0,
            pickup_rate: cm(0.0002),
            ..Self::flat(160, width, thickness, length)
        }
    }

    pub const fn knife(width: f32, thickness: f32, length: f32) -> Self {
        Self {
            sample_capacity: cm(0.02),
            deposit_rate: cm(0.006),
            pickup_rate: cm(0.0005),
            blade_layer: cm(0.08),
            ..Self::shaper(width, thickness, length)
        }
    }

    pub fn blade_film(&self, pressure: f32) -> f32 {
        (self.blade_layer * (1.0 - pressure.clamp(0.0, 1.0))).max(BLADE_CLEARANCE)
    }

    pub fn is_rigid(&self) -> bool {
        self.shape == BrushShape::Blade
    }

    pub fn default_round() -> Self {
        Self::round(200, cm(0.35), cm(2.2))
    }

    pub fn segment_length(&self) -> f32 {
        self.length / (super::geometry::VERTICES_PER_BRISTLE - 2) as f32
    }

    pub fn adjusted(self, adjustments: BrushAdjustments) -> Self {
        let scale = adjustments.scale.clamp(0.1, 3.0);
        let natural_count = (self.bristle_count as f32 * scale * scale).round() as u32;
        let bristle_count = adjustments.bristle_count.unwrap_or(natural_count).clamp(20, 600);
        let share_per_hair = natural_count as f32 / bristle_count as f32;
        let stiffness = adjustments.stiffness.clamp(0.0, 1.0);
        let scaled = Self { length: self.length * scale.sqrt(), width: self.width * scale, thickness: self.thickness * scale, ..self };
        if self.is_rigid() {
            return Self { bristle_count: (self.bristle_count as f32 * scale * scale).round() as u32, ..scaled };
        }
        Self {
            bristle_count,
            sample_capacity: self.sample_capacity * share_per_hair,
            deposit_rate: self.deposit_rate * share_per_hair,
            pickup_rate: self.pickup_rate * share_per_hair,
            splay: 0.3 + 0.5 * (1.0 - stiffness),
            sink: 0.3 + 0.4 * (1.0 - stiffness),
            friction: 0.4 + 0.4 * stiffness,
            stiffness: 0.02 + 0.06 * stiffness,
            ..scaled
        }
    }
}

pub const PRESETS: [NamedPreset; 8] = [
    NamedPreset { name: "Round small", preset: BrushPreset::round(70, cm(0.15), cm(1.6)) },
    NamedPreset { name: "Round medium", preset: BrushPreset::round(200, cm(0.35), cm(2.2)) },
    NamedPreset { name: "Round large", preset: BrushPreset::round(450, cm(0.6), cm(2.8)) },
    NamedPreset { name: "Flat", preset: BrushPreset::flat(400, cm(1.2), cm(0.3), cm(2.0)) },
    NamedPreset { name: "Filbert", preset: BrushPreset::filbert(350, cm(1.0), cm(0.3), cm(2.2)) },
    NamedPreset { name: "Rigger", preset: BrushPreset::round(40, cm(0.12), cm(3.0)) },
    NamedPreset { name: "Colour shaper", preset: BrushPreset::shaper(cm(1.0), cm(0.12), cm(1.3)) },
    NamedPreset { name: "Painting knife", preset: BrushPreset::knife(cm(1.2), cm(0.35), cm(1.0)) },
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
    fn bristle_count_changes_density_not_paint_flow() {
        let base = BrushPreset::default_round();
        let sparse = base.adjusted(BrushAdjustments { bristle_count: Some(100), ..Default::default() });
        let dense = base.adjusted(BrushAdjustments { bristle_count: Some(400), ..Default::default() });
        let flow = |preset: BrushPreset| preset.deposit_rate * preset.bristle_count as f32;
        let capacity = |preset: BrushPreset| preset.sample_capacity * preset.bristle_count as f32;
        assert!((flow(sparse) - flow(dense)).abs() < flow(dense) * 1e-4);
        assert!((capacity(sparse) - capacity(dense)).abs() < capacity(dense) * 1e-4);
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
