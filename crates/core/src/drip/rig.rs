use crate::units::cm;
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rig {
    pub string_length: f32,
    pub pivot_height: f32,
    pub hole_radius: f32,
    pub bottle_radius: f32,
    pub hook: Vec2,
    pub throw: f32,
    pub pour_at_once: bool,
}

impl Default for Rig {
    fn default() -> Self {
        Self { string_length: cm(250.0), pivot_height: cm(265.0), hole_radius: cm(0.2), bottle_radius: cm(2.0), hook: Vec2::ZERO, throw: 1.0, pour_at_once: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hanging {
    pub pivot: Vec2,
    pub release: Vec2,
    pub push: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setting {
    String(f32),
    PivotHeight(f32),
    Hole(f32),
    Radius(f32),
}
