mod dip;
mod frame;
mod geometry;
mod knife;
mod preset;
mod shaper;
mod simulation;
mod touch;

pub use dip::DipPart;
pub use frame::BrushPose;
pub use knife::{carries_paint, working_point};
pub use geometry::{VERTICES_PER_BRISTLE, blade_grid, clump_count};
pub use preset::{MAX_COMPRESSION, BrushAdjustments, BrushPreset, BrushShape, NamedPreset, PRESETS, preset_by_name};
pub use simulation::{Brush, SAMPLES_PER_BRISTLE};
pub use touch::Touch;
