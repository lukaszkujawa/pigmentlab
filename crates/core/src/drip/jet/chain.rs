use super::{BREAKING_RADIUS, HELD_NODES};
use glam::DVec3;
use std::f64::consts::PI;

const SPACING_PER_RADIUS: f64 = 1.0;
const SHORTEST_SPACING: f64 = 150e-6;
const LONGEST_SPACING: f64 = 1e-3;
const SPLIT_AT: f64 = 1.6;
const MERGE_AT: f64 = 0.4;
const CAP_LENGTH_PER_RADIUS: f64 = 3.0;
const STUB_THINNESS: f64 = 0.5;
const MOST_NODES: usize = 3000;

pub fn spacing_for(radius: f64) -> f64 {
    (radius * SPACING_PER_RADIUS).clamp(SHORTEST_SPACING, LONGEST_SPACING)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub position: DVec3,
    pub velocity: DVec3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub nodes: Vec<Node>,
    pub volumes: Vec<f64>,
    pub pinned: bool,
    pub stuck_head: bool,
    pub stuck_tail: bool,
    pub stride: f64,
    pub thread: u64,
}

impl Piece {
    pub fn new(nodes: Vec<Node>, volumes: Vec<f64>, pinned: bool) -> Self {
        Self { nodes, volumes, pinned, stuck_head: false, stuck_tail: false, stride: 0.0, thread: 0 }
    }

    pub fn held_head(&self) -> usize {
        match (self.pinned, self.stuck_head) {
            (true, _) => HELD_NODES,
            (false, true) => 1,
            (false, false) => 0,
        }
    }

    pub fn is_on_canvas(&self) -> bool {
        self.stuck_head || self.stuck_tail
    }

    pub fn segments(&self) -> usize {
        self.volumes.len()
    }

    pub fn length(&self, segment: usize) -> f64 {
        self.nodes[segment + 1].position.distance(self.nodes[segment].position).max(1e-12)
    }

    pub fn direction(&self, segment: usize) -> DVec3 {
        (self.nodes[segment + 1].position - self.nodes[segment].position).normalize_or(DVec3::NEG_Z)
    }

    pub fn radius(&self, segment: usize) -> f64 {
        (self.volumes[segment] / (PI * self.length(segment))).sqrt()
    }

    pub fn node_volume(&self, node: usize) -> f64 {
        let before = if node > 0 { self.volumes[node - 1] } else { 0.0 };
        let after = self.volumes.get(node).copied().unwrap_or(0.0);
        0.5 * (before + after)
    }

    pub fn volume(&self) -> f64 {
        self.volumes.iter().sum()
    }

    pub fn total_length(&self) -> f64 {
        (0..self.segments()).map(|segment| self.length(segment)).sum()
    }

    pub fn centre_of_mass(&self) -> (DVec3, DVec3) {
        let volume = self.volume();
        let weighted = |of: fn(&Node) -> DVec3| (0..self.nodes.len()).map(|node| of(&self.nodes[node]) * self.node_volume(node)).sum::<DVec3>() / volume;
        (weighted(|node| node.position), weighted(|node| node.velocity))
    }

    pub fn has_cap(&self, segment: usize) -> bool {
        self.caps(segment) > 0
    }

    pub fn caps(&self, segment: usize) -> usize {
        usize::from(segment + 1 == self.segments() && !self.stuck_tail) + usize::from(segment == 0 && !self.pinned && !self.stuck_head)
    }

    pub fn remesh(&mut self) {
        self.split_long_segments();
        self.merge_short_segments();
        self.absorb_thin_ends();
    }

    fn absorb_thin_ends(&mut self) {
        let first_free = if self.pinned { HELD_NODES } else { 1 };
        while self.segments() > first_free && self.is_stub(self.segments() - 1, self.segments() - 2) {
            self.remove_node(self.segments() - 1);
        }
        while !self.pinned && !self.stuck_head && self.segments() > 1 && self.is_stub(0, 1) {
            self.remove_node(1);
        }
    }

    fn is_stub(&self, end: usize, neighbour: usize) -> bool {
        let (tip, body) = (self.radius(end), self.radius(neighbour));
        tip < BREAKING_RADIUS || (tip < STUB_THINNESS * body && self.length(end) < CAP_LENGTH_PER_RADIUS * body)
    }

    fn spacing(&self, segment: usize) -> f64 {
        let crowding = (self.nodes.len() as f64 / MOST_NODES as f64).max(1.0);
        spacing_for(self.radius(segment) * crowding)
    }

    fn split_long_segments(&mut self) {
        let mut segment = usize::from(self.pinned);
        while segment < self.segments() {
            let longest = match self.has_cap(segment) {
                true => (SPLIT_AT * self.spacing(segment)).max(CAP_LENGTH_PER_RADIUS * self.radius(segment)),
                false => SPLIT_AT * self.spacing(segment),
            };
            if self.length(segment) > longest && self.nodes.len() < MOST_NODES {
                let (a, b) = (self.nodes[segment], self.nodes[segment + 1]);
                let middle = Node { position: (a.position + b.position) * 0.5, velocity: (a.velocity + b.velocity) * 0.5 };
                let first = self.first_half_share(segment);
                let volume = self.volumes[segment];
                self.nodes.insert(segment + 1, middle);
                self.volumes[segment] = volume * first;
                self.volumes.insert(segment + 1, volume * (1.0 - first));
            } else {
                segment += 1;
            }
        }
    }

    fn first_half_share(&self, segment: usize) -> f64 {
        let radius = self.radius(segment);
        let before = if segment > 0 { self.radius(segment - 1) } else { radius };
        let after = if segment + 1 < self.segments() { self.radius(segment + 1) } else { radius };
        let tilt = ((after - before) / 8.0).clamp(-0.5 * radius, 0.5 * radius);
        let (first, second) = ((radius - tilt).powi(2), (radius + tilt).powi(2));
        first / (first + second)
    }

    fn merge_short_segments(&mut self) {
        let first_free = if self.pinned { HELD_NODES } else { 1 };
        let mut segment = first_free - 1;
        while segment < self.segments() && self.segments() > first_free {
            if self.length(segment) >= MERGE_AT * self.spacing(segment) {
                segment += 1;
                continue;
            }
            let node = match (segment + 1 < self.segments(), segment >= first_free) {
                (true, true) if self.length(segment + 1) < self.length(segment - 1) => segment + 1,
                (_, true) => segment,
                (true, false) => segment + 1,
                (false, false) => {
                    segment += 1;
                    continue;
                }
            };
            self.remove_node(node);
        }
    }

    fn remove_node(&mut self, node: usize) {
        let removed = self.nodes[node];
        let share = 0.5 * self.node_volume(node);
        for neighbour in [node - 1, node + 1] {
            let own = self.node_volume(neighbour);
            let target = &mut self.nodes[neighbour];
            target.velocity = (target.velocity * own + removed.velocity * share) / (own + share);
        }
        self.nodes.remove(node);
        let joined = self.volumes.remove(node);
        self.volumes[node - 1] += joined;
    }

    pub fn cut(&mut self, segment: usize) -> Option<Piece> {
        if self.segments() == 1 {
            return None;
        }
        let volume = self.volumes[segment];
        let mut below = Piece { stuck_tail: self.stuck_tail, stride: self.stride, ..Piece::new(self.nodes.split_off(segment + 1), self.volumes.split_off(segment + 1), false) };
        self.stuck_tail = false;
        self.volumes.pop();
        match (self.volumes.last_mut(), below.volumes.first_mut()) {
            (Some(upper), Some(lower)) => {
                *upper += volume * 0.5;
                *lower += volume * 0.5;
            }
            (Some(upper), None) => *upper += volume,
            (None, Some(lower)) => *lower += volume,
            (None, None) => unreachable!("a piece of two or more segments keeps one on a side"),
        }
        (!below.volumes.is_empty()).then_some(below)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(segments: usize, length: f64, radius: f64) -> Piece {
        let nodes = (0..=segments).map(|k| Node { position: DVec3::new(0.0, 0.0, -length * k as f64 / segments as f64), velocity: DVec3::new(0.0, 0.0, -(k as f64)) }).collect();
        Piece::new(nodes, vec![PI * radius * radius * length / segments as f64; segments], false)
    }

    #[test]
    fn remeshing_keeps_the_volume_and_the_spacing_follows_the_radius() {
        let mut piece = column(4, 0.04, 0.001);
        let volume = piece.volume();
        piece.remesh();
        assert!((piece.volume() - volume).abs() < volume * 1e-12);
        assert!((1..piece.segments() - 1).all(|segment| piece.length(segment) <= SPLIT_AT * piece.spacing(segment)));
        let mut thick = column(400, 0.04, 0.003);
        thick.remesh();
        assert!(thick.segments() < 400, "a thick column is merged into fewer segments, {}", thick.segments());
        assert!((thick.volume() - volume * 9.0).abs() < volume * 1e-9);
    }

    #[test]
    fn cutting_a_neck_shares_its_paint_between_the_two_pieces() {
        let mut piece = Piece { pinned: true, ..column(10, 0.01, 0.001) };
        let volume = piece.volume();
        let below = piece.cut(4).expect("the lower part should fall away");
        assert!(piece.pinned && !below.pinned);
        assert_eq!((piece.segments(), below.segments()), (4, 5));
        assert!((piece.volume() + below.volume() - volume).abs() < volume * 1e-12);
    }
}
