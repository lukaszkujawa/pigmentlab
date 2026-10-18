mod chain;
mod forces;
mod solve;

pub use chain::{Node, Piece};

use super::fluid::Fluid;
use crate::rng::SplitMix64;
use forces::{capillary_tensions, explicit_forces, stable_step, stretching_resistance, tensions};
use glam::DVec3;
use solve::{Ends, Stiffness, implicit_velocities};
use std::f64::consts::PI;

pub const DENSITY: f64 = super::fluid::DENSITY as f64;
pub const SURFACE_TENSION: f64 = super::fluid::SURFACE_TENSION as f64;
pub const HELD_NODES: usize = 2;
const GRAVITY: f64 = crate::units::GRAVITY as f64;
pub const BREAKING_RADIUS: f64 = 8e-6;
const VANISHED_RADIUS: f64 = 2e-6;
const NECK_REACH: usize = 4;
const NECK_DEPTH: f64 = 0.5;
const SAFETY: f64 = 0.2;
const MOST_SUBSTEPS: usize = 400;
const FEWEST_SUBSTEPS: usize = 8;
const STRIDE_GROWTH: f64 = 2.0;
const ROUNDEST_DROP: f64 = 2.5;
const LONGEST_SPLAT: f64 = 3.0;
const NOZZLE_COLUMN_PER_RADIUS: f64 = 0.2;
const NOZZLE_JITTER: f64 = 1e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub position: DVec3,
    pub velocity: DVec3,
    pub hole_radius: f64,
    pub flow: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    pub position: DVec3,
    pub span: [DVec3; 2],
    pub velocity: DVec3,
    pub volume: f64,
    pub radius: f64,
    pub attached: bool,
    pub thread: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drop {
    pub position: DVec3,
    pub velocity: DVec3,
    pub volume: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fallen {
    pub landed: Vec<Landing>,
    pub drops: Vec<Drop>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Jet {
    pieces: Vec<Piece>,
    viscosity: f64,
    gravity: DVec3,
    jitter: SplitMix64,
    nozzle: Option<DVec3>,
    threads: u64,
}

impl Source {
    pub fn exit_velocity(&self) -> DVec3 {
        self.velocity + DVec3::NEG_Z * self.flow / (PI * self.hole_radius * self.hole_radius)
    }
}

impl Jet {
    pub fn new(fluid: Fluid, seed: u64) -> Self {
        Self { pieces: Vec::new(), viscosity: f64::from(fluid.viscosity), gravity: DVec3::new(0.0, 0.0, -GRAVITY), jitter: SplitMix64::new(seed), nozzle: None, threads: 0 }
    }

    pub fn set_fluid(&mut self, fluid: Fluid) {
        self.viscosity = f64::from(fluid.viscosity);
    }

    pub fn let_go(&mut self) {
        self.nozzle = None;
        self.pieces.iter_mut().for_each(|piece| piece.pinned = false);
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub fn volume(&self) -> f64 {
        self.pieces.iter().map(Piece::volume).sum()
    }

    pub fn step(&mut self, dt: f64, source: Option<&Source>) -> Fallen {
        let mut fallen = Fallen::default();
        let travelled = source.map(|source| {
            let start = self.nozzle.unwrap_or(source.position - source.velocity * dt);
            Source { velocity: (source.position - start) / dt, ..*source }
        });
        let source = travelled.as_ref();
        match source {
            Some(source) if source.flow > 0.0 && !self.pieces.iter().any(|piece| piece.pinned) => {
                let still = Node { position: source.position - source.velocity * dt, velocity: source.exit_velocity() };
                let thread = self.fresh_thread();
                self.pieces.push(Piece { thread, ..Piece::new(vec![still, still], vec![0.0], true) });
            }
            None => self.let_go(),
            _ => {}
        }
        self.nozzle = source.map(|source| source.position);
        let mut waiting: Vec<(Piece, f64)> = self.pieces.drain(..).map(|piece| (piece, 0.0)).collect();
        while let Some((piece, clock)) = waiting.pop() {
            self.follow(piece, clock, dt, source, &mut waiting, &mut fallen);
        }
        fallen
    }

    fn fresh_thread(&mut self) -> u64 {
        self.threads += 1;
        self.threads
    }

    fn follow(&mut self, mut piece: Piece, mut clock: f64, frame: f64, source: Option<&Source>, waiting: &mut Vec<(Piece, f64)>, fallen: &mut Fallen) {
        let longest = frame / FEWEST_SUBSTEPS as f64;
        let shortest = frame / MOST_SUBSTEPS as f64;
        loop {
            if piece.stuck_head && piece.stuck_tail {
                fallen.landed.extend(lay_down(&piece));
                return;
            }
            if touches_ground(&piece) && !piece.pinned && !piece.is_on_canvas() && is_compact(&piece) {
                let (position, velocity) = piece.centre_of_mass();
                fallen.drops.push(Drop { position: position.with_z(0.0), velocity, volume: piece.volume() });
                return;
            }
            if touches_ground(&piece) {
                let fresh = self.fresh_thread();
                let (landing, rest) = land_on_ground(piece, fresh);
                fallen.landed.push(landing);
                waiting.extend(rest.into_iter().map(|part| (part, clock)));
                return;
            }
            if !piece.pinned && !piece.is_on_canvas() && is_round(&piece) {
                let (position, velocity) = piece.centre_of_mass();
                fallen.drops.push(Drop { position, velocity, volume: piece.volume() });
                return;
            }
            if clock >= frame {
                break;
            }
            let dt = (SAFETY * stable_step(&piece, self.viscosity)).min(STRIDE_GROWTH * piece.stride).clamp(shortest, longest).min(frame - clock);
            piece.stride = dt;
            let nozzle = source.filter(|_| piece.pinned).map(|source| Source { position: source.position - source.velocity * (frame - clock), ..*source });
            if let Some(nozzle) = &nozzle {
                feed_from_nozzle(&mut piece, nozzle, nozzle.flow * dt);
            }
            let shaking = DVec3::new(self.jitter.range(-1.0, 1.0).into(), self.jitter.range(-1.0, 1.0).into(), self.jitter.range(-1.0, 1.0).into()) * NOZZLE_JITTER;
            let held = nozzle.map(|nozzle| nozzle.velocity + (nozzle.exit_velocity() - nozzle.velocity).length() * (DVec3::NEG_Z + shaking));
            advance(&mut piece, held, self.viscosity, self.gravity, dt);
            clock += dt;
            piece.remesh();
            let neck = (0..piece.segments()).find(|segment| is_breaking_neck(&piece, *segment));
            if let Some(below) = neck.and_then(|segment| piece.cut(segment)) {
                let thread = self.fresh_thread();
                waiting.push((Piece { thread, ..below }, clock));
            }
        }
        self.pieces.push(piece);
    }
}

fn advance(piece: &mut Piece, held: Option<DVec3>, viscosity: f64, gravity: DVec3, dt: f64) {
    let capillary = capillary_tensions(piece);
    let forces = explicit_forces(piece, gravity, &capillary);
    let resistance = stretching_resistance(piece, viscosity);
    let tension = tensions(piece, &resistance, &capillary);
    let ends = Ends { head: piece.held_head(), head_velocity: held.unwrap_or(DVec3::ZERO), tail_stuck: piece.stuck_tail };
    let velocities = implicit_velocities(piece, &forces, &Stiffness { resistance: &resistance, tension: &tension, viscosity }, &ends, dt);
    for (node, velocity) in piece.nodes.iter_mut().zip(velocities) {
        node.velocity = velocity;
        node.position += velocity * dt;
    }
}

fn feed_from_nozzle(piece: &mut Piece, source: &Source, poured: f64) {
    let area = PI * source.hole_radius * source.hole_radius;
    let column = area * NOZZLE_COLUMN_PER_RADIUS * source.hole_radius;
    let exit = source.exit_velocity();
    let mouth = |depth: f64| Node { position: source.position + DVec3::NEG_Z * depth, velocity: exit };
    let filling = poured.min(column - piece.volumes[0]).max(0.0);
    piece.volumes[0] += filling;
    piece.nodes[0] = mouth(0.0);
    piece.nodes[1] = mouth(piece.volumes[0] / area);
    let overflow = poured - filling;
    if overflow <= 0.0 {
        return;
    }
    match piece.segments() {
        1 => {
            piece.nodes.push(mouth(piece.volumes[0] / area + overflow / area));
            piece.volumes.push(overflow);
        }
        _ => piece.volumes[1] += overflow,
    }
}

fn touches_ground(piece: &Piece) -> bool {
    (held_nodes(piece)..piece.nodes.len()).any(|node| is_touching(piece, node)) || (!piece.pinned && piece.is_on_canvas() && piece.segments() <= 1)
}

fn is_touching(piece: &Piece, node: usize) -> bool {
    let position = piece.nodes[node].position;
    let resting = (node == 0 && piece.stuck_head) || (node + 1 == piece.nodes.len() && piece.stuck_tail);
    let beside = |segment: usize| (segment < piece.segments()).then(|| piece.radius(segment));
    let reach = [node.checked_sub(1).and_then(beside), beside(node)].into_iter().flatten().fold(0.0, f64::max);
    !resting && position.z < 2.0 * reach
}

fn land_on_ground(piece: Piece, fresh: u64) -> (Landing, Vec<Piece>) {
    let node = (held_nodes(&piece)..piece.nodes.len()).rfind(|node| is_touching(&piece, *node)).unwrap_or(piece.nodes.len() - 1);
    let touching = piece.nodes[node];
    let on_canvas = |node: Node| Node { position: node.position.with_z(0.0), velocity: DVec3::ZERO };
    let midway = |other: usize| on_canvas(Node { position: touching.position.lerp(piece.nodes[other].position, 0.5), velocity: DVec3::ZERO });
    let grounded = |other: usize| piece.nodes[other].position.z <= 0.0 || is_touching(&piece, other);
    let mut landed = 0.0;
    let mut radius = 0.0f64;
    let mut parts = Vec::new();
    let mut span = [touching.position, touching.position];
    if node > 0 {
        let segment = node - 1;
        radius = radius.max(piece.radius(segment));
        let mut upper = Piece { stuck_head: piece.stuck_head, stuck_tail: true, stride: piece.stride, thread: piece.thread, ..Piece::new(piece.nodes[..node].to_vec(), piece.volumes[..segment].to_vec(), piece.pinned) };
        match grounded(segment) && segment >= held_nodes(&piece) {
            true => {
                landed += piece.volumes[segment];
                span[0] = piece.nodes[segment].position;
                let last = upper.nodes.len() - 1;
                upper.nodes[last] = on_canvas(upper.nodes[last]);
            }
            false => {
                landed += 0.5 * piece.volumes[segment];
                upper.nodes.push(midway(segment));
                upper.volumes.push(0.5 * piece.volumes[segment]);
                span[0] = midway(segment).position;
            }
        }
        parts.push(upper);
    }
    if node < piece.segments() {
        let segment = node;
        radius = radius.max(piece.radius(segment));
        let mut lower = Piece { stuck_head: true, stuck_tail: piece.stuck_tail, stride: piece.stride, thread: fresh, ..Piece::new(piece.nodes[node + 1..].to_vec(), piece.volumes[node + 1..].to_vec(), false) };
        match grounded(node + 1) {
            true => {
                landed += piece.volumes[segment];
                span[1] = piece.nodes[node + 1].position;
                lower.nodes[0] = on_canvas(lower.nodes[0]);
            }
            false => {
                landed += 0.5 * piece.volumes[segment];
                lower.nodes.insert(0, midway(node + 1));
                lower.volumes.insert(0, 0.5 * piece.volumes[segment]);
                span[1] = midway(node + 1).position;
            }
        }
        parts.push(lower);
    }
    parts.retain(|part| part.segments() > 0 && !(part.is_on_canvas() && part.volume() <= 0.0));
    (Landing { position: touching.position.with_z(0.0), span: span.map(|end| end.with_z(0.0)), velocity: touching.velocity, volume: landed, radius, attached: piece.pinned, thread: piece.thread }, parts)
}

fn lay_down(piece: &Piece) -> impl Iterator<Item = Landing> + '_ {
    (0..piece.segments()).map(move |segment| {
        let [from, to] = [piece.nodes[segment], piece.nodes[segment + 1]];
        Landing { position: from.position.lerp(to.position, 0.5).with_z(0.0), span: [from.position.with_z(0.0), to.position.with_z(0.0)], velocity: from.velocity.lerp(to.velocity, 0.5), volume: piece.volumes[segment], radius: piece.radius(segment), attached: false, thread: piece.thread }
    })
}

fn is_breaking_neck(piece: &Piece, segment: usize) -> bool {
    let radius = piece.radius(segment);
    if segment == 0 || segment + 1 >= piece.segments() || radius >= BREAKING_RADIUS || radius >= piece.radius(segment - 1) || radius > piece.radius(segment + 1) {
        return false;
    }
    let around = segment.saturating_sub(NECK_REACH)..(segment + NECK_REACH + 1).min(piece.segments());
    let shoulders = around.map(|other| piece.radius(other)).fold(0.0, f64::max);
    radius < NECK_DEPTH * shoulders || radius < VANISHED_RADIUS
}

fn held_nodes(piece: &Piece) -> usize {
    if piece.pinned { HELD_NODES } else { 0 }
}

fn is_compact(piece: &Piece) -> bool {
    let (low, high) = piece.nodes.iter().fold((DVec3::MAX, DVec3::MIN), |(low, high), node| (low.min(node.position), high.max(node.position)));
    (high - low).truncate().length() < LONGEST_SPLAT * (6.0 * piece.volume() / PI).cbrt()
}

fn is_round(piece: &Piece) -> bool {
    let diameter = (6.0 * piece.volume() / PI).cbrt();
    piece.segments() <= 2 || piece.total_length() < ROUNDEST_DROP * diameter
}

#[cfg(test)]
mod tests;
