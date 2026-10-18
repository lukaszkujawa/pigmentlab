use super::chain::Piece;
use super::{DENSITY, SURFACE_TENSION};
use glam::DVec3;
use std::f64::consts::PI;

const AIR_DENSITY: f64 = 1.2;
const CYLINDER_DRAG: f64 = 1.2;

pub fn explicit_forces(piece: &Piece, gravity: DVec3, capillary: &[f64]) -> Vec<DVec3> {
    let mut forces: Vec<DVec3> = (0..piece.nodes.len()).map(|node| gravity * DENSITY * piece.node_volume(node)).collect();
    for segment in 0..piece.segments() {
        let along = piece.direction(segment);
        forces[segment] += along * capillary[segment];
        forces[segment + 1] -= along * capillary[segment];
        let crossing = piece.nodes[segment].velocity.lerp(piece.nodes[segment + 1].velocity, 0.5);
        let across = crossing - along * crossing.dot(along);
        let drag = -across * across.length() * 0.5 * AIR_DENSITY * CYLINDER_DRAG * 2.0 * piece.radius(segment) * piece.length(segment);
        forces[segment] += drag * 0.5;
        forces[segment + 1] += drag * 0.5;
    }
    forces
}

pub fn capillary_tensions(piece: &Piece) -> Vec<f64> {
    let segments = piece.segments();
    let radii: Vec<f64> = (0..segments).map(|segment| piece.radius(segment)).collect();
    let slope = |segment: usize| (radii[segment] - radii[segment - 1]) / (0.5 * (piece.length(segment) + piece.length(segment - 1)));
    (0..segments)
        .map(|segment| {
            let after = if segment + 1 < segments { slope(segment + 1) } else { 0.0 };
            let before = if segment > 0 { slope(segment) } else { 0.0 };
            let steepness = 1.0 + (0.5 * (after + before)).powi(2);
            let bend = (after - before) / piece.length(segment);
            let radius = radii[segment];
            let rounding = (1.0 - 2.0 * piece.caps(segment) as f64 * radius / piece.length(segment)).max(0.0);
            PI * SURFACE_TENSION * radius * (rounding + radius * bend / steepness) / steepness.sqrt()
        })
        .collect()
}

pub fn tensions(piece: &Piece, resistance: &[f64], capillary: &[f64]) -> Vec<f64> {
    (0..piece.segments())
        .map(|segment| {
            let stretching = piece.direction(segment).dot(piece.nodes[segment + 1].velocity - piece.nodes[segment].velocity);
            capillary[segment] + resistance[segment] * stretching
        })
        .collect()
}

pub fn stretching_resistance(piece: &Piece, viscosity: f64) -> Vec<f64> {
    (0..piece.segments()).map(|segment| 3.0 * viscosity * piece.volumes[segment] / piece.length(segment).powi(2)).collect()
}

pub fn stable_step(piece: &Piece, viscosity: f64) -> f64 {
    let kinematic = viscosity / DENSITY;
    (0..piece.segments())
        .map(|segment| {
            let wave_number = PI / piece.length(segment);
            let stiffness = SURFACE_TENSION * piece.radius(segment) * wave_number.powi(4) / (2.0 * DENSITY);
            let damping = 3.0 * kinematic * wave_number * wave_number;
            (damping + (damping * damping + 4.0 * stiffness).sqrt()) / stiffness
        })
        .fold(f64::INFINITY, f64::min)
}
