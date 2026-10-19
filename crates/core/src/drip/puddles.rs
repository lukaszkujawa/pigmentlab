use super::laid::puddle_along;
use crate::paint::{Mark, Profile};
use glam::{Mat2, Vec2};

const RELEASE_EVERY: f32 = 0.15;
const WET_MEMORY: f32 = 3.0;
const JOINING_REACH: f32 = 0.9;
const COALESCED_LENGTH: f32 = 0.7;
const DOT: f32 = 0.5;
const LAYER_REACH: f32 = 0.85;

struct Puddle {
    volume: f32,
    moment: Vec2,
    spread: Mat2,
    released: f32,
    ragged: f32,
    mark: Mark,
    unreleased_for: f32,
    unfed_for: f32,
}

impl Puddle {
    fn new(mark: Mark) -> Self {
        let mut puddle = Self { volume: 0.0, moment: Vec2::ZERO, spread: Mat2::ZERO, released: 0.0, ragged: 0.0, mark, unreleased_for: 0.0, unfed_for: 0.0 };
        puddle.feed(&mark);
        puddle
    }

    fn feed(&mut self, mark: &Mark) {
        let centre = (mark.from + mark.to) * 0.5;
        let along = mark.to - mark.from;
        let segment = Mat2::from_cols(along * along.x, along * along.y) / 12.0;
        self.volume += mark.volume;
        self.moment += centre * mark.volume;
        self.spread += (Mat2::from_cols(centre * centre.x, centre * centre.y) + segment) * mark.volume;
        self.ragged = self.ragged.max(mark.profile.ragged);
        self.unfed_for = 0.0;
        self.mark = self.shape();
    }

    fn shape(&self) -> Mark {
        let centre = self.moment / self.volume;
        let covariance = self.spread / self.volume - Mat2::from_cols(centre * centre.x, centre * centre.y);
        let (variance, axis) = principal(covariance);
        let half = axis * (3.0 * variance.max(0.0)).sqrt() * COALESCED_LENGTH;
        let laid = puddle_along(centre - half, centre + half, self.volume, self.mark.paint, self.mark.medium);
        Mark { profile: Profile { ragged: self.ragged, ..laid.profile }, ..laid }
    }

    fn takes_in(&self, mark: &Mark) -> bool {
        let reach = JOINING_REACH * self.mark.radius;
        self.mark.paint == mark.paint && mark.profile.open == [false; 2] && segment_distance(self.mark.from, self.mark.to, (mark.from + mark.to) * 0.5) < reach
    }

    fn release(&mut self) -> Option<Mark> {
        let fresh = self.volume - self.released;
        let layer = match self.released > 0.0 {
            true => Mark { radius: self.mark.radius * LAYER_REACH, profile: Profile { stiffness: 0.0, ..self.mark.profile }, ..self.mark },
            false => self.mark,
        };
        self.unreleased_for = 0.0;
        self.released = self.volume;
        (fresh > 0.0).then_some(Mark { volume: fresh, ..layer })
    }
}

fn principal(covariance: Mat2) -> (f32, Vec2) {
    let (a, b, d) = (covariance.x_axis.x, covariance.x_axis.y, covariance.y_axis.y);
    let mean = 0.5 * (a + d);
    let variance = mean + (0.25 * (a - d) * (a - d) + b * b).sqrt();
    let axis = match b.abs() > 1e-6 {
        true => Vec2::new(variance - d, b).normalize_or(Vec2::X),
        false if a >= d => Vec2::X,
        false => Vec2::Y,
    };
    (variance, axis)
}

fn segment_distance(from: Vec2, to: Vec2, point: Vec2) -> f32 {
    let along = to - from;
    let t = ((point - from).dot(along) / along.length_squared().max(1e-6)).clamp(0.0, 1.0);
    point.distance(from + along * t)
}

#[derive(Default)]
pub struct Puddles {
    wet: Vec<Puddle>,
}

impl Puddles {
    pub fn volume(&self) -> f32 {
        self.wet.iter().map(|puddle| puddle.volume - puddle.released).sum()
    }

    pub fn has_unlaid(&self) -> bool {
        self.wet.iter().any(|puddle| puddle.released < puddle.volume)
    }

    pub fn settle(&mut self, landed: Vec<Mark>, dt: f32) -> Vec<Mark> {
        let mut laid = Vec::with_capacity(landed.len());
        for mark in landed {
            match self.wet.iter_mut().find(|puddle| puddle.takes_in(&mark)) {
                Some(puddle) => puddle.feed(&mark),
                None if mark.from.distance(mark.to) <= DOT && mark.profile.open == [false; 2] => self.wet.push(Puddle::new(mark)),
                None => laid.push(mark),
            }
        }
        for puddle in &mut self.wet {
            puddle.unreleased_for += dt;
            puddle.unfed_for += dt;
            if puddle.released < puddle.volume && (puddle.unfed_for >= RELEASE_EVERY || puddle.unreleased_for >= RELEASE_EVERY) {
                laid.extend(puddle.release());
            }
        }
        self.wet.retain(|puddle| puddle.released < puddle.volume || puddle.unfed_for < WET_MEMORY);
        laid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pigment::Coefficients;
    use glam::Vec3;

    fn drop_at(centre: Vec2, volume: f32) -> Mark {
        let paint = Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE };
        Mark { radius: 10.0, ..Mark::dot(centre, 10.0, volume, paint, 0.4) }
    }

    fn settle_all(puddles: &mut Puddles, landed: Vec<Mark>) -> Vec<Mark> {
        let mut laid = puddles.settle(landed, 1.0 / 240.0);
        for _ in 0..240 {
            laid.extend(puddles.settle(Vec::new(), 1.0 / 240.0));
        }
        laid
    }

    #[test]
    fn a_drop_landing_in_a_wet_puddle_coalesces_into_one_puddle_stretched_along_the_line_between_them() {
        let mut puddles = Puddles::default();
        let laid = settle_all(&mut puddles, vec![drop_at(Vec2::new(100.0, 100.0), 300.0), drop_at(Vec2::new(106.0, 100.0), 300.0)]);
        assert_eq!(laid.len(), 1, "two overlapping drops should merge");
        let puddle = laid[0];
        assert!((puddle.volume - 600.0).abs() < 1e-3);
        assert!((puddle.from + puddle.to).distance(Vec2::new(206.0, 200.0)) < 1e-3, "the merged puddle should sit between the drops");
        assert!((puddle.to - puddle.from).y.abs() < 1e-3 && (puddle.to - puddle.from).x.abs() > 2.0, "the merged puddle should stretch along the drops, {:?} to {:?}", puddle.from, puddle.to);
    }

    #[test]
    fn paint_poured_onto_a_wet_puddle_spreads_it_wider_instead_of_stacking_a_new_dome() {
        let mut puddles = Puddles::default();
        let first = settle_all(&mut puddles, vec![drop_at(Vec2::new(100.0, 100.0), 300.0)]);
        let second = settle_all(&mut puddles, vec![drop_at(Vec2::new(101.0, 100.0), 900.0)]);
        assert_eq!((first.len(), second.len()), (1, 1));
        assert!((second[0].volume - 900.0).abs() < 1e-3, "only the new paint is laid");
        assert!(second[0].radius > first[0].radius * 1.2, "the new layer covers the grown puddle, {} vs {}", second[0].radius, first[0].radius);
    }

    #[test]
    fn drops_apart_stay_apart() {
        let mut puddles = Puddles::default();
        let laid = settle_all(&mut puddles, vec![drop_at(Vec2::new(100.0, 100.0), 300.0), drop_at(Vec2::new(200.0, 100.0), 300.0)]);
        assert_eq!(laid.len(), 2);
        assert!(puddles.volume() == 0.0);
    }
}
