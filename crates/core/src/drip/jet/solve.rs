use super::DENSITY;
use super::chain::Piece;
use glam::{DMat3, DVec3};
use std::f64::consts::PI;

const BAND: usize = 2;

pub struct Stiffness<'a> {
    pub resistance: &'a [f64],
    pub tension: &'a [f64],
    pub viscosity: f64,
}

pub struct Ends {
    pub head: usize,
    pub head_velocity: DVec3,
    pub tail_stuck: bool,
}

struct Banded {
    rows: Vec<[DMat3; 2 * BAND + 1]>,
    right: Vec<DVec3>,
}

impl Banded {
    fn add(&mut self, row: usize, column: usize, block: DMat3) {
        self.rows[row][column + BAND - row] += block;
    }

    fn at(&self, row: usize, column: usize) -> DMat3 {
        self.rows[row][column + BAND - row]
    }

    fn fix(&mut self, node: usize, velocity: DVec3) {
        let count = self.rows.len();
        for other in node.saturating_sub(BAND)..(node + BAND + 1).min(count) {
            if other != node {
                let coupling = self.at(other, node);
                self.right[other] -= coupling * velocity;
                self.rows[other][node + BAND - other] = DMat3::ZERO;
                self.rows[node][other + BAND - node] = DMat3::ZERO;
            }
        }
        self.rows[node][BAND] = DMat3::IDENTITY;
        self.right[node] = velocity;
    }

    fn solve(mut self) -> Vec<DVec3> {
        let count = self.rows.len();
        let mut inverses = Vec::with_capacity(count);
        for pivot in 0..count {
            let inverse = self.at(pivot, pivot).inverse();
            inverses.push(inverse);
            for row in pivot + 1..(pivot + BAND + 1).min(count) {
                let factor = self.at(row, pivot) * inverse;
                if factor == DMat3::ZERO {
                    continue;
                }
                for column in pivot + 1..(pivot + BAND + 1).min(count) {
                    let eliminated = self.at(row, column) - factor * self.at(pivot, column);
                    self.rows[row][column + BAND - row] = eliminated;
                }
                self.right[row] = self.right[row] - factor * self.right[pivot];
            }
        }
        let mut velocities = vec![DVec3::ZERO; count];
        for row in (0..count).rev() {
            let known = (row + 1..(row + BAND + 1).min(count)).fold(self.right[row], |known, column| known - self.at(row, column) * velocities[column]);
            velocities[row] = inverses[row] * known;
        }
        velocities
    }
}

pub fn implicit_velocities(piece: &Piece, forces: &[DVec3], stiffness: &Stiffness, ends: &Ends, dt: f64) -> Vec<DVec3> {
    let count = piece.nodes.len();
    let mut system = Banded { rows: vec![[DMat3::ZERO; 2 * BAND + 1]; count], right: vec![DVec3::ZERO; count] };
    for (node, force) in forces.iter().enumerate() {
        let mass = DENSITY * piece.node_volume(node);
        system.add(node, node, DMat3::IDENTITY * mass);
        system.right[node] = piece.nodes[node].velocity * mass + *force * dt;
    }
    for segment in 0..piece.segments() {
        let along = piece.direction(segment);
        let axial = DMat3::from_cols(along * along.x, along * along.y, along * along.z);
        let sideways = stiffness.tension[segment].max(0.0) / piece.length(segment) * dt;
        let coupling = axial * (stiffness.resistance[segment] * dt) + (DMat3::IDENTITY - axial) * (sideways * dt);
        system.add(segment, segment, coupling);
        system.add(segment + 1, segment + 1, coupling);
        system.add(segment, segment + 1, -coupling);
        system.add(segment + 1, segment, -coupling);
    }
    for node in 1..count.saturating_sub(1) {
        let (before, after) = (piece.length(node - 1), piece.length(node));
        let span = 0.5 * (before + after);
        let radius = 0.5 * (piece.radius(node - 1) + piece.radius(node));
        let rigidity = 3.0 * stiffness.viscosity * PI * radius.powi(4) / 4.0;
        let tangent = (piece.nodes[node + 1].position - piece.nodes[node - 1].position).normalize_or(DVec3::NEG_Z);
        let across = DMat3::IDENTITY - DMat3::from_cols(tangent * tangent.x, tangent * tangent.y, tangent * tangent.z);
        let weight = rigidity / span.powi(3) * dt;
        let stencil = [(node - 1, 1.0), (node, -2.0), (node + 1, 1.0)];
        for (row, a) in stencil {
            for (column, b) in stencil {
                system.add(row, column, across * (weight * a * b));
            }
        }
    }
    for node in 0..ends.head {
        system.fix(node, ends.head_velocity);
    }
    if ends.tail_stuck {
        system.fix(count - 1, DVec3::ZERO);
    }
    system.solve()
}
