use super::flight::Droplet;
use super::fluid::{Fluid, Wetted, wetted_mound, wetted_strand};
use super::jet::{Drop, Landing};
use glam::Vec2;
use crate::paint::{Mark, Profile};
use crate::paint::rheology::yield_stress;
use crate::pigment::Coefficients;
use crate::units::METRES_PER_PIXEL;

const RIBBON_EDGE: f32 = 0.04;
const RIBBON_EDGE_PIXELS: f32 = 0.6;
const SHORTEST_RIBBON: f32 = 4.0;
const SLENDER: f32 = 1.0;
const SLENDER_START: f32 = 2.5;
const BLOB_PER_THREAD: f32 = 4.0;
const WETTED_WIGGLE: f32 = 12.0;
const WIDTH_EASING: f32 = 0.5;
const POOL_EDGE: f32 = 0.2;
const RIBBON_LENGTH_PER_HALF_WIDTH: f32 = 2.0;
const AREA_MEMORY: f32 = 6.0;
const COILING: f32 = 3.0;
const ABSORBED_SHARE: f32 = 0.5;
const LONGEST_STEP: f32 = 40.0;
const SHARPEST_JOIN: f32 = 0.5;
const STILL_STEPS: u32 = 8;
const PAUSE_STEPS: u32 = 6;

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
        for (thread, ribbon) in &mut self.ribbons {
            match landed.iter().any(|landing| landing.thread == *thread) {
                true => {
                    ribbon.idle = 0;
                    ribbon.settle(paint, medium, &mut marks);
                }
                false => {
                    ribbon.idle += 1;
                    if ribbon.idle > PAUSE_STEPS {
                        ribbon.finish(paint, medium, &mut marks);
                    }
                }
            }
        }
        self.ribbons.retain(|(_, ribbon)| ribbon.idle <= PAUSE_STEPS);
        marks
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Ribbon {
    path: Option<(Vec2, Vec2)>,
    start: Option<Vec2>,
    end: Vec2,
    volume: f32,
    weighted: Vec2,
    travelled: f32,
    continued: bool,
    width: f32,
    area: f32,
    steps: u32,
    idle: u32,
    held: Option<Mark>,
}

impl Ribbon {
    fn pending(&self) -> f32 {
        self.volume + self.held.map_or(0.0, |held| held.volume)
    }

    fn lay(&mut self, landing: &Landing, paint: Coefficients, medium: f32, marks: &mut Vec<Mark>) {
        let touched = landing.position.truncate().as_vec2() / METRES_PER_PIXEL;
        if self.path.is_some_and(|(_, last)| last.distance(touched) > LONGEST_STEP) {
            self.emit(paint, medium, false, marks);
            *self = Self::default();
        }
        let (followed, last) = self.path.unwrap_or((touched, touched));
        let at = followed + (touched - followed) * (touched.distance(last) / WETTED_WIGGLE).min(1.0);
        self.path = Some((at, touched));
        let volume = to_canvas(landing.volume);
        let thread = landing.radius as f32 / METRES_PER_PIXEL;
        if wetted_mound(volume, yield_stress(medium)).reach > BLOB_PER_THREAD * thread.max(1.0) {
            self.emit(paint, medium, false, marks);
            marks.push(pool(at, volume, paint, medium));
            *self = Self { path: self.path, ..Self::default() };
            return;
        }
        if self.start.is_none() {
            self.start = Some(at);
            self.end = at;
        }
        self.travelled += self.end.distance(at);
        self.end = at;
        self.volume += volume;
        self.weighted += at * volume;
        if self.travelled >= SHORTEST_RIBBON.max(RIBBON_LENGTH_PER_HALF_WIDTH * self.half_width(medium)) {
            self.emit(paint, medium, true, marks);
        }
    }

    fn settle(&mut self, paint: Coefficients, medium: f32, marks: &mut Vec<Mark>) {
        if self.volume <= 0.0 {
            return;
        }
        self.steps += 1;
        let chord = self.start.map_or(0.0, |start| start.distance(self.end));
        let pooling_still = self.travelled < SHORTEST_RIBBON && self.steps >= STILL_STEPS;
        let coiling = self.travelled > COILING * chord.max(self.half_width(medium));
        if pooling_still || coiling {
            self.emit(paint, medium, true, marks);
        }
    }

    fn half_width(&self, medium: f32) -> f32 {
        self.settled(self.volume, self.travelled.max(SHORTEST_RIBBON), medium).reach
    }

    fn finish(&mut self, paint: Coefficients, medium: f32, marks: &mut Vec<Mark>) {
        self.emit(paint, medium, false, marks);
        *self = Self::default();
    }

    fn settled(&self, volume: f32, length: f32, medium: f32) -> Wetted {
        wetted_strand(volume / length, yield_stress(medium))
    }

    fn emit(&mut self, paint: Coefficients, medium: f32, continues: bool, marks: &mut Vec<Mark>) {
        let Some(start) = self.start.filter(|_| self.volume > 0.0) else {
            if !continues {
                marks.extend(self.held.take().map(closed));
            }
            return;
        };
        let chord = start.distance(self.end);
        let strand = self.settled(self.volume, chord.max(SHORTEST_RIBBON), medium);
        let slender = chord >= strand.reach * if self.continued { SLENDER } else { SLENDER_START };
        let mut kept = 0.0;
        if slender {
            let measured = self.volume / chord;
            let area = match self.continued && self.area > 0.0 && continues {
                true => self.area + (measured - self.area) * (1.0 - (-chord / (AREA_MEMORY * strand.reach)).exp()),
                false => measured,
            };
            let laid = (area * chord).min(self.volume);
            kept = self.volume - laid;
            let strand = self.settled(laid, chord, medium);
            let from = if self.continued && self.width > 0.0 { self.width } else { strand.reach };
            let to = from + (strand.reach - from) * WIDTH_EASING;
            let profile = Profile { open: [self.continued, continues], taper: to / from, ..strand.profile(ribbon_edge(from)) };
            let mut mark = Mark { from: start, to: self.end, radius: from, volume: laid, paint, medium, profile };
            if let Some(held) = self.held.as_mut().filter(|held| (held.to - held.from).normalize_or_zero().dot((mark.to - mark.from).normalize_or_zero()) < SHARPEST_JOIN) {
                held.profile.open[1] = false;
                mark.profile.open[0] = false;
            }
            marks.extend(self.held.replace(mark));
            self.width = to;
            self.area = area;
        } else if let Some(held) = self.held.as_mut().filter(|held| self.volume <= ABSORBED_SHARE * held.volume) {
            held.to = self.end;
            held.volume += self.volume;
        } else {
            let centre = self.weighted / self.volume;
            let half = (self.end - start) * 0.5;
            marks.extend(self.held.take().map(closed));
            marks.push(puddle_along(centre - half, centre + half, self.volume, paint, medium));
        }
        let continued = continues && slender;
        if !continued {
            marks.extend(self.held.take().map(closed));
        }
        *self = Self {
            held: self.held,
            path: self.path.filter(|_| continues),
            start: continues.then_some(self.end),
            end: self.end,
            volume: kept,
            weighted: self.end * kept,
            continued,
            width: if continued { self.width } else { 0.0 },
            area: if continued { self.area } else { 0.0 },
            ..Self::default()
        };
    }
}

fn closed(held: Mark) -> Mark {
    match held.profile.open[0] {
        true => Mark { profile: Profile { open: [true, false], ..held.profile }, ..held },
        false => oval_of(held),
    }
}

fn oval_of(capsule: Mark) -> Mark {
    let half_length = 0.5 * capsule.from.distance(capsule.to);
    let area = capsule.radius * (std::f32::consts::PI * capsule.radius + 4.0 * half_length) / std::f32::consts::PI;
    let radius = 0.5 * ((half_length * half_length + 4.0 * area).sqrt() - half_length);
    Mark { radius, profile: Profile { oval: true, open: [false; 2], taper: 1.0, ..capsule.profile }, ..capsule }
}

fn pool(centre: Vec2, volume: f32, paint: Coefficients, medium: f32) -> Mark {
    puddle_along(centre, centre, volume, paint, medium)
}

pub fn puddle_along(from: Vec2, to: Vec2, volume: f32, paint: Coefficients, medium: f32) -> Mark {
    let puddle = wetted_mound(volume, yield_stress(medium));
    let half_length = 0.5 * from.distance(to);
    let radius = 0.5 * ((half_length * half_length + 4.0 * puddle.reach * puddle.reach).sqrt() - half_length);
    let profile = Profile { oval: true, ..Wetted { reach: radius, ..puddle }.profile(POOL_EDGE) };
    Mark { from, to, radius, volume, paint, medium, profile }
}

