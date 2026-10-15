mod frame;
mod geometry;
mod preset;
mod simulation;

pub use frame::BrushPose;
pub use geometry::VERTICES_PER_BRISTLE;
pub use preset::{BrushAdjustments, BrushPreset, BrushShape, NamedPreset, PRESETS, preset_by_name};
pub use simulation::{Brush, SAMPLES_PER_BRISTLE};
