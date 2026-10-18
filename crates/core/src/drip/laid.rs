use super::flight::Droplet;
use super::fluid::{Fluid, wetted_mound, wetted_strand};
use super::jet::{Drop, Landing};
use glam::Vec2;
use crate::paint::{Mark, Profile};
use crate::paint::rheology::{Mound, Strand, yield_stress};
use crate::pigment::Coefficients;
use crate::units::METRES_PER_PIXEL;

const RIBBON_EDGE: f32 = 0.04;
const RIBBON_EDGE_PIXELS: f32 = 0.6;
const SHORTEST_RIBBON: f32 = 4.0;
const SLENDER: f32 = 1.9;
const BLOB_PER_THREAD: f32 = 4.0;
const POOLING_STRETCH: f32 = 2.5;
const WETTED_WIGGLE: f32 = 12.0;
const STILL_REACH: f32 = 6.0;
const WIDTH_EASING: f32 = 0.5;
const POOLED_LENGTH: f32 = 3.0;
const POOL_EDGE: f32 = 0.1;
const RIBBON_LENGTH_PER_HALF_WIDTH: f32 = 2.0;

fn ribbon_edge(radius: f32) -> f32 {
    RIBBON_EDGE.min(RIBBON_EDGE_PIXELS / radius.max(f32::EPSILON))
}

pub fn to_canvas(volume: f64) -> f32 {
    volume as f32 / METRES_PER_PIXEL.powi(3)
}

pub fn falling_droplet(drop: &Drop, paint: Coefficients, medium: f32) -> Droplet {
    Droplet { position: drop.position.as_vec3(), velocity: drop.velocity.as_vec3(), diameter: (6.0 * drop.volume / std::f64::consts::PI).cbrt() as f32, paint, medium, fluid: Fluid::of(medium) }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Threads {
    ribbons: Vec<(u64, Ribbon)>,
}

impl Threads {
    pub fn pending(&self) -> f32 {
        self.ribbons.iter().map(|(_, ribbon)| ribbon.pending()).sum()
    }

    pub fn lay(&mut self, landed: &[Landing], paint: Coefficients, medium: f32) -> Vec<Mark> {
        let mut marks = Vec::new();
        for landing in landed {
            let index = match self.ribbons.iter().position(|(thread, _)| *thread == landing.thread) {
                Some(index) => index,
                None => {
                    self.ribbons.push((landing.thread, Ribbon::default()));
                    self.ribbons.len() - 1
                }
            };
            self.ribbons[index].1.lay(landing, paint, medium, &mut marks);
        }
        for (_, ribbon) in self.ribbons.iter_mut().filter(|(thread, _)| landed.iter().all(|landing| landing.thread != *thread)) {
            ribbon.finish(paint, medium, &mut marks);
        }
        self.ribbons.retain(|(thread, _)| landed.iter().any(|landing| landing.thread == *thread));
        marks
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Ribbon {
    path: Option<(Vec2, Vec2)>,
    weighted: Vec2,
    started: bool,
    start: Option<Vec2>,
    end: Vec2,
    volume: f32,
    still: f32,
    wandered: bool,
    continued: bool,
    width: f32,
}

impl Ribbon {
    fn pending(&self) -> f32 {
        self.volume
    }

    fn lay(&mut self, landing: &Landing, paint: Coefficients, medium: f32, marks: &mut Vec<Mark>) {
        let touched = landing.position.truncate().as_vec2() / METRES_PER_PIXEL;
        let (followed, last) = self.path.unwrap_or((touched, touched));
        let at = followed + (touched - followed) * (touched.distance(last) / WETTED_WIGGLE).min(1.0);
        self.path = Some((at, touched));
        let volume = to_canvas(landing.volume);
        let thread = landing.radius as f32 / METRES_PER_PIXEL;
        if wetted_mound(volume, yield_stress(medium)).radius > BLOB_PER_THREAD * thread.max(1.0) {
            if self.start.is_some() && self.volume > 0.0 {
                self.emit(paint, medium, false, marks);
            }
            marks.push(pool(at, volume, paint, medium));
            *self = Self { path: self.path, start: Some(at), end: at, wandered: true, ..Self::default() };
            return;
        }
        let start = *self.start.get_or_insert(at);
        self.end = at;
        self.volume += volume;
        if self.wandered || start.distance(at) > STILL_REACH {
            self.weighted += at * volume;
        }
        self.wandered |= start.distance(at) > STILL_REACH;
        if !self.wandered {
            self.volume -= volume;
            marks.push(pool(start, volume, paint, medium));
            return;
        }
        let length = start.distance(at);
        if length >= SHORTEST_RIBBON.max(RIBBON_LENGTH_PER_HALF_WIDTH * self.settled(self.volume - self.still, length.max(SHORTEST_RIBBON), medium).half_width) {
            self.emit(paint, medium, true, marks);
        }
    }

    fn finish(&mut self, paint: Coefficients, medium: f32, marks: &mut Vec<Mark>) {
        let Some(start) = self.start else { return };
        let length = start.distance(self.end);
        if !self.continued && length < SHORTEST_RIBBON.max(POOLED_LENGTH * self.settled(self.volume, length.max(SHORTEST_RIBBON), medium).half_width) {
            self.still = self.volume;
        }
        self.emit(paint, medium, false, marks);
        *self = Self::default();
    }

    fn settled(&self, volume: f32, length: f32, medium: f32) -> Strand {
        wetted_strand(volume / length, yield_stress(medium))
    }

    fn emit(&mut self, paint: Coefficients, medium: f32, continues: bool, marks: &mut Vec<Mark>) {
        let start = self.start.expect("a ribbon being laid has a start");
        if self.still > 0.0 {
            marks.push(pool(start, self.still, paint, medium));
        }
        let flowing = self.volume - self.still;
        let length = start.distance(self.end);
        let pooled = flowing > 0.0 && (!self.started || !continues) && POOLING_STRETCH * wetted_mound(flowing, yield_stress(medium)).radius > length.max(SHORTEST_RIBBON);
        if pooled {
            marks.push(pool(self.weighted / flowing, flowing, paint, medium));
        } else if flowing > 0.0 {
            let strand = self.settled(flowing, start.distance(self.end).max(SHORTEST_RIBBON), medium);
            let slender = SLENDER * strand.half_width < start.distance(self.end);
            let from = if self.continued && self.width > 0.0 { self.width } else { strand.half_width };
            let to = from + (strand.half_width - from) * WIDTH_EASING;
            self.width = to;
            let profile = Profile { flatness: (1.0 - strand.height / strand.half_width).clamp(0.0, 1.0), ragged: ribbon_edge(from), open: [self.continued && slender, continues && slender], taper: to / from };
            marks.push(Mark { from: start, to: self.end, radius: from, volume: flowing, paint, medium, profile });
        }
        *self = Self { path: self.path.filter(|_| continues), start: Some(self.end), end: self.end, volume: 0.0, still: 0.0, wandered: true, continued: continues && flowing > 0.0 && !pooled, weighted: Vec2::ZERO, started: self.started || flowing > 0.0, width: if continues { self.width } else { 0.0 } };
    }
}

fn pool(centre: Vec2, volume: f32, paint: Coefficients, medium: f32) -> Mark {
    let puddle = wetted_mound(volume, yield_stress(medium));
    puddle_mark(centre, volume, puddle, paint, medium)
}

fn puddle_mark(centre: Vec2, volume: f32, puddle: Mound, paint: Coefficients, medium: f32) -> Mark {
    let profile = Profile { flatness: (1.0 - puddle.height / puddle.radius).clamp(0.0, 1.0), ragged: POOL_EDGE, open: [false; 2], taper: 1.0 };
    Mark { profile, ..Mark::dot(centre, puddle.radius, volume, paint, medium) }
}
