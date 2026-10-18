use super::flick::BRISTLE_GAP;
use super::flight::Droplet;
use super::fluid::{DENSITY, Fluid};
use super::jet::{Jet, Source};
use super::laid::{Threads, falling_droplet};
use crate::paint::Mark;
use crate::pigment::Coefficients;
use crate::units::GRAVITY;
use glam::Vec3;

const RETAINED_SHARE: f32 = 0.6;
const BUNDLE_PERMEABILITY_PER_GAP_SQUARED: f32 = 1.0 / 32.0;
const YIELD_GRADIENT_PER_STRESS: f32 = 4.0 / BRISTLE_GAP;
const FASTEST_HAND: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuft {
    pub capacity: f32,
    pub cross_section: f32,
    pub droop: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
    pub tip: Vec3,
    pub tip_radius: f32,
    pub flow: f32,
    pub paint: Coefficients,
    pub medium: f32,
}

pub fn drain_flow(load: f32, tuft: &Tuft, fluid: Fluid) -> f32 {
    let retained = RETAINED_SHARE * tuft.capacity;
    let overload = ((load - retained) / (tuft.capacity - retained)).clamp(0.0, 1.0);
    let permeability = BUNDLE_PERMEABILITY_PER_GAP_SQUARED * BRISTLE_GAP * BRISTLE_GAP;
    let pull = DENSITY * GRAVITY * tuft.droop.max(0.0) - YIELD_GRADIENT_PER_STRESS * fluid.yield_stress;
    permeability * tuft.cross_section * pull.max(0.0) / fluid.viscosity * overload
}

pub struct Dribble {
    jet: Jet,
    paint: Coefficients,
    medium: f32,
    last_tip: Option<Vec3>,
    threads: Threads,
}

impl Dribble {
    pub fn new(tap: &Tap, seed: u64) -> Self {
        Self { jet: Jet::new(Fluid::of(tap.medium), seed), paint: tap.paint, medium: tap.medium, last_tip: None, threads: Threads::default() }
    }

    pub fn jet(&self) -> &Jet {
        &self.jet
    }

    pub fn airborne(&self) -> f32 {
        self.jet.volume() as f32 + self.threads.pending() * crate::units::METRES_PER_PIXEL.powi(3)
    }

    pub fn is_done(&self) -> bool {
        self.jet.pieces().is_empty() && self.threads.pending() <= 0.0
    }

    pub fn paint(&self) -> Coefficients {
        self.paint
    }

    pub fn carries(&self, tap: &Tap) -> bool {
        tap.medium == self.medium
    }

    pub fn step(&mut self, tap: Option<&Tap>, dt: f32) -> (Vec<Mark>, Vec<Droplet>) {
        let velocity = tap.zip(self.last_tip).map_or(Vec3::ZERO, |(tap, last)| (tap.tip - last) / dt);
        if velocity.length() > FASTEST_HAND {
            self.jet.let_go();
        }
        let source = tap.filter(|tap| self.carries(tap)).map(|tap| {
            let velocity = if velocity.length() > FASTEST_HAND { Vec3::ZERO } else { velocity };
            Source { position: tap.tip.as_dvec3(), velocity: velocity.as_dvec3(), hole_radius: f64::from(tap.tip_radius), flow: f64::from(tap.flow) }
        });
        self.last_tip = tap.map(|tap| tap.tip);
        if let Some(tap) = tap.filter(|tap| self.carries(tap)) {
            self.paint = tap.paint;
        }
        let fallen = self.jet.step(f64::from(dt), source.as_ref());
        let (paint, medium) = (self.paint, self.medium);
        let marks = self.threads.lay(&fallen.landed, paint, medium);
        let droplets = fallen.drops.iter().map(|drop| falling_droplet(drop, self.paint, self.medium)).collect();
        (marks, droplets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_brush(droop: f32) -> Tuft {
        Tuft { capacity: 8e-7, cross_section: std::f32::consts::PI * 0.0035 * 0.0035, droop }
    }

    #[test]
    fn only_paint_beyond_what_capillarity_holds_drains_and_runny_paint_drains_fastest() {
        let tuft = round_brush(1.0);
        let full = tuft.capacity;
        let runny = drain_flow(full, &tuft, Fluid::of(1.0));
        assert!((2e-9..3e-8).contains(&runny), "a soaked round brush of runny paint should drain microlitres a second, {:.2} ul/s", runny * 1e9);
        assert_eq!(drain_flow(0.55 * full, &tuft, Fluid::of(1.0)), 0.0, "a normally loaded brush holds its paint");
        let enamel = drain_flow(full, &tuft, Fluid::of(0.35));
        assert!(enamel < runny * 0.1, "enamel drains far slower, {:.3} vs {:.3} ul/s", enamel * 1e9, runny * 1e9);
        assert_eq!(drain_flow(full, &tuft, Fluid::of(0.0)), 0.0, "stiff paste never drips");
        assert_eq!(drain_flow(full, &round_brush(0.0), Fluid::of(1.0)), 0.0, "a brush held level does not drain");
    }
}
