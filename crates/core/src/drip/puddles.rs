use super::fluid::wetted_mound;
use crate::paint::rheology::yield_stress;
use crate::paint::{Mark, Profile};

const STILL_WET: f32 = 0.15;
const JOINING_REACH: f32 = 0.9;
const DOT: f32 = 0.5;

struct Puddle {
    mark: Mark,
    drying: f32,
}

#[derive(Default)]
pub struct Puddles {
    open: Vec<Puddle>,
}

impl Puddles {
    pub fn volume(&self) -> f32 {
        self.open.iter().map(|puddle| puddle.mark.volume).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    pub fn settle(&mut self, landed: Vec<Mark>, dt: f32) -> Vec<Mark> {
        let mut laid = Vec::with_capacity(landed.len());
        for mark in landed {
            let dot = mark.from.distance(mark.to) <= DOT;
            let inside = |puddle: &Puddle| {
                let reach = JOINING_REACH * if dot { puddle.mark.radius.max(mark.radius) } else { puddle.mark.radius };
                mark.profile.open == [false; 2] && puddle.mark.paint == mark.paint && puddle.mark.from.distance(mark.from) < reach && puddle.mark.from.distance(mark.to) < reach
            };
            match self.open.iter_mut().find(|puddle| inside(puddle)) {
                Some(puddle) => puddle.mark = merged(&puddle.mark, &mark),
                None if dot => self.open.push(Puddle { mark, drying: 0.0 }),
                None => laid.push(mark),
            }
        }
        self.open.iter_mut().for_each(|puddle| puddle.drying += dt);
        let (dry, wet): (Vec<Puddle>, Vec<Puddle>) = self.open.drain(..).partition(|puddle| puddle.drying >= STILL_WET);
        self.open = wet;
        laid.extend(dry.into_iter().map(|puddle| puddle.mark));
        laid
    }
}

fn merged(puddle: &Mark, joining: &Mark) -> Mark {
    let volume = puddle.volume + joining.volume;
    let mound = wetted_mound(volume, yield_stress(puddle.medium));
    let radius = mound.radius.max(puddle.radius);
    let profile = Profile { flatness: (1.0 - mound.height / radius).clamp(0.0, 1.0), ragged: puddle.profile.ragged.max(joining.profile.ragged), ..puddle.profile };
    Mark { radius, volume, profile, ..*puddle }
}
