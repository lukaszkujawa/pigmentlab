mod frame;
mod geometry;
mod preset;
mod simulation;

pub use frame::BrushPose;
pub use geometry::VERTICES_PER_BRISTLE;
pub use preset::{BrushPreset, BrushShape};
pub use simulation::{Brush, SAMPLES_PER_BRISTLE};
