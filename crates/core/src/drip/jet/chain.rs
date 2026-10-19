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

fn crowded_spacing(radius: f64, nodes: usize, coarseness: f64) -> f64 {
    let crowding = (nodes as f64 / MOST_NODES as f64).max(1.0);
    spacing_for(radius * crowding) * coarseness
}

fn length_between(from: &Node, to: &Node) -> f64 {
    to.position.distance(from.position).max(1e-12)
}

fn radius_of(volume: f64, length: f64) -> f64 {
    (volume / (PI * length)).sqrt()
}

fn first_half_share(radius: f64, before: f64, after: f64) -> f64 {
    let tilt = ((after - before) / 8.0).clamp(-0.5 * radius, 0.5 * radius);
    let (first, second) = ((radius - tilt).powi(2), (radius + tilt).powi(2));
    if first + second > 0.0 { first / (first + second) } else { 0.5 }
}

fn blend(target: &mut Node, own: f64, removed: Node, share: f64) {
    target.velocity = (target.velocity * own + removed.velocity * share) / (own + share);
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
        length_between(&self.nodes[segment], &self.nodes[segment + 1])
    }

    pub fn direction(&self, segment: usize) -> DVec3 {
        (self.nodes[segment + 1].position - self.nodes[segment].position).normalize_or(DVec3::NEG_Z)
    }

    pub fn radius(&self, segment: usize) -> f64 {
        radius_of(self.volumes[segment], self.length(segment))
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

    pub fn caps(&self, segment: usize) -> usize {
        usize::from(segment + 1 == self.segments() && !self.stuck_tail) + usize::from(segment == 0 && !self.pinned && !self.stuck_head)
    }

    pub fn remesh(&mut self, coarseness: f64) {
        self.split_long_segments(coarseness);
        self.merge_short_segments(coarseness);
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

    #[cfg(test)]
    fn spacing(&self, segment: usize, coarseness: f64) -> f64 {
        crowded_spacing(self.radius(segment), self.nodes.len(), coarseness)
    }

    fn split_long_segments(&mut self, coarseness: f64) {
        let start = usize::from(self.pinned);
        if self.segments() <= start {
            return;
        }
        let nodes = std::mem::take(&mut self.nodes);
        let volumes = std::mem::take(&mut self.volumes);
        self.nodes.extend_from_slice(&nodes[..=start]);
        self.volumes.extend_from_slice(&volumes[..start]);
        let mut halves: Vec<(Node, f64)> = Vec::new();
        for segment in start..volumes.len() {
            halves.push((nodes[segment + 1], volumes[segment]));
            let following = volumes.get(segment + 1).map(|volume| (nodes[segment + 2], *volume));
            while let Some(&(end, volume)) = halves.last() {
                let begin = self.nodes[self.nodes.len() - 1];
                let length = length_between(&begin, &end);
                let radius = radius_of(volume, length);
                let count = self.nodes.len() + halves.len() + nodes.len() - segment - 2;
                let capped = (halves.len() == 1 && segment + 1 == volumes.len() && !self.stuck_tail) || (self.volumes.is_empty() && !self.pinned && !self.stuck_head);
                let spaced = SPLIT_AT * crowded_spacing(radius, count, coarseness);
                let longest = if capped { spaced.max(CAP_LENGTH_PER_RADIUS * radius) } else { spaced };
                let splits = length > longest && count < MOST_NODES;
                if !splits {
                    halves.pop();
                    self.nodes.push(end);
                    self.volumes.push(volume);
                    continue;
                }
                let before = self.volumes.last().map_or(radius, |before| radius_of(*before, length_between(&self.nodes[self.nodes.len() - 2], &begin)));
                let next = if halves.len() > 1 { Some(halves[halves.len() - 2]) } else { following };
                let after = next.map_or(radius, |(next, after)| radius_of(after, length_between(&end, &next)));
                let first = first_half_share(radius, before, after);
                let middle = Node { position: (begin.position + end.position) * 0.5, velocity: (begin.velocity + end.velocity) * 0.5 };
                *halves.last_mut().expect("the segment being split") = (end, volume * (1.0 - first));
                halves.push((middle, volume * first));
            }
        }
    }

    fn merge_short_segments(&mut self, coarseness: f64) {
        let first_free = if self.pinned { HELD_NODES } else { 1 };
        if self.segments() <= first_free {
            return;
        }
        let mut segments = self.segments();
        let nodes = std::mem::take(&mut self.nodes);
        let volumes = std::mem::take(&mut self.volumes);
        self.nodes.extend_from_slice(&nodes[..first_free]);
        self.volumes.extend_from_slice(&volumes[..first_free - 1]);
        let mut rest = nodes[first_free..].iter().copied().zip(volumes[first_free - 1..].iter().copied()).peekable();
        let Some(mut current) = rest.next() else { return };
        loop {
            let begin = self.nodes[self.nodes.len() - 1];
            let length = length_between(&begin, &current.0);
            let short = segments > first_free && length < MERGE_AT * crowded_spacing(radius_of(current.1, length), segments + 1, coarseness);
            let has_previous = self.volumes.len() >= first_free;
            let next = rest.peek().copied();
            let into_next = short && next.is_some_and(|next| !has_previous || length_between(&current.0, &next.0) < length_between(&self.nodes[self.nodes.len() - 2], &begin));
            if into_next {
                let (mut after, after_volume) = rest.next().expect("the next segment");
                let share = 0.25 * (current.1 + after_volume);
                let before_volume = self.volumes.last().copied().unwrap_or(0.0);
                let last = self.nodes.len() - 1;
                blend(&mut self.nodes[last], 0.5 * (before_volume + current.1), current.0, share);
                blend(&mut after, 0.5 * (after_volume + rest.peek().map_or(0.0, |beyond| beyond.1)), current.0, share);
                current = (after, current.1 + after_volume);
                segments -= 1;
                continue;
            }
            if short && has_previous {
                let removed = self.nodes.pop().expect("the shared node");
                let previous = self.volumes[self.volumes.len() - 1];
                let share = 0.25 * (previous + current.1);
                let earlier = self.volumes.len().checked_sub(2).map_or(0.0, |earlier| self.volumes[earlier]);
                let last = self.nodes.len() - 1;
                blend(&mut self.nodes[last], 0.5 * (earlier + previous), removed, share);
                blend(&mut current.0, 0.5 * (current.1 + next.map_or(0.0, |next| next.1)), removed, share);
                *self.volumes.last_mut().expect("the previous segment") += current.1;
                self.nodes.push(current.0);
                segments -= 1;
            } else {
                self.nodes.push(current.0);
                self.volumes.push(current.1);
            }
            match rest.next() {
                Some(next) => current = next,
                None => return,
            }
        }
    }

    fn remove_node(&mut self, node: usize) {
        let removed = self.nodes[node];
        let share = 0.5 * self.node_volume(node);
        for neighbour in [node - 1, node + 1] {
            let own = self.node_volume(neighbour);
            blend(&mut self.nodes[neighbour], own, removed, share);
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
        piece.remesh(1.0);
        assert!((piece.volume() - volume).abs() < volume * 1e-12);
        assert!((1..piece.segments() - 1).all(|segment| piece.length(segment) <= SPLIT_AT * piece.spacing(segment, 1.0)));
        let mut thick = column(400, 0.04, 0.003);
        thick.remesh(1.0);
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
