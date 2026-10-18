use super::flight::Droplet;
use super::fluid::{DENSITY, Fluid};
use super::jet::{Fallen, Jet, Source};
use super::laid::{Threads, falling_droplet};
use crate::units::METRES_PER_PIXEL;
use super::outflow::orifice_flow;
use super::pendulum::{Damping, Pendulum};
use crate::paint::Mark;
use crate::pigment::Coefficients;
use crate::rng::SplitMix64;
use crate::units::GRAVITY;
use glam::Vec2;
use std::f32::consts::PI;

const HEIGHT_PER_RADIUS: f32 = 6.0;
const FILLED: f32 = 0.75;
const LUMP_TIME: f32 = 0.15;
const TREMOR: f32 = 0.0003;
const NARROWEST_CLOG: f32 = 0.4;
const LUMPY_STIFF: f32 = 0.3;
const LUMPY_RUNNY: f32 = 0.1;
pub const SHORTEST_STRING: f32 = 0.05;
pub const CLEARANCE: f32 = 0.01;
const SHELL_MASS_PER_AREA: f32 = 1.5;
const SPOUT_LENGTH: f32 = 0.003;
const AIR_DENSITY: f32 = 1.2;
const SIDE_ON_DRAG: f32 = 0.7;
const STRING_DRAG: f32 = 1.5;
const STRING_THICKNESS: f32 = 0.002;
const HOOK_RUB: f32 = 0.001;
const CORD_LOSS_FACTOR: f32 = 0.015;
const SUSPENSION_UNEVENNESS: f32 = 0.004;
const QUICKEST_SPIN_SETTLING: f32 = 0.3;
const SLOWEST_SPIN_SETTLING: f32 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Bottle {
    pub id: u32,
    pub radius: f32,
    pub pendulum: Pendulum,
    pub paint: Coefficients,
    pub medium: f32,
    pub fluid: Fluid,
    pub hole_radius: f32,
    pub volume: f64,
    pub spin: f32,
    pub turned: f32,
    pub held_over: Option<Vec2>,
    held_from: Option<Vec2>,
    pub pouring: bool,
    seed: u64,
    jet: Jet,
    threads: Threads,
    lumps: SplitMix64,
    clogging: f32,
    shaking: f32,
    tremor: [(f32, f32, Vec2); 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filling {
    pub paint: Coefficients,
    pub medium: f32,
    pub hole_radius: f32,
    pub radius: f32,
    pub pouring: bool,
}

pub struct Released {
    pub marks: Vec<Mark>,
    pub droplets: Vec<Droplet>,
    pub volume: f32,
}

impl Bottle {
    pub fn filled(pendulum: Pendulum, filling: Filling, seed: u64, id: u32) -> Self {
        Self {
            id,
            radius: filling.radius,
            pendulum,
            paint: filling.paint,
            medium: filling.medium,
            fluid: Fluid::of(filling.medium),
            hole_radius: filling.hole_radius,
            volume: f64::from(capacity(filling.radius) * FILLED),
            spin: 0.0,
            turned: 0.0,
            held_over: None,
            held_from: None,
            pouring: filling.pouring,
            seed,
            jet: Jet::new(Fluid::of(filling.medium), seed.rotate_left(17)),
            threads: Threads::default(),
            lumps: SplitMix64::new(seed.rotate_left(29)),
            clogging: 0.0,
            shaking: 0.0,
            tremor: tremor_of(seed),
        }
    }

    pub fn fill(&self) -> f32 {
        self.volume as f32 / capacity(self.radius)
    }

    pub fn resize(&mut self, radius: f32) {
        self.radius = radius;
        self.volume = self.volume.min(f64::from(capacity(radius)));
    }

    pub fn set_string(&mut self, length: f32) {
        let pendulum = self.pendulum;
        let outward = (pendulum.bob - pendulum.pivot).normalize();
        self.pendulum.length = length.clamp(SHORTEST_STRING, pendulum.pivot.z - CLEARANCE);
        self.pendulum.bob = pendulum.pivot + outward * self.pendulum.length;
        self.pendulum.velocity = self.pendulum.tangential(pendulum.velocity);
        self.restart_thread();
    }

    pub fn set_pivot_height(&mut self, height: f32) {
        self.pendulum.pivot.z = height.max(SHORTEST_STRING + CLEARANCE);
        self.set_string(self.pendulum.length);
    }

    pub fn stop(&mut self) {
        let pendulum = self.pendulum;
        self.pendulum = Pendulum::released(pendulum.pivot, pendulum.length, pendulum.pivot.truncate(), Vec2::ZERO);
        self.held_over = None;
        self.spin = 0.0;
        self.restart_thread();
    }

    fn restart_thread(&mut self) {
        self.jet.let_go();
    }

    pub fn refill(&mut self, paint: Coefficients, medium: f32) {
        self.paint = paint;
        self.medium = medium;
        self.fluid = Fluid::of(medium);
        self.jet.set_fluid(self.fluid);
    }

    pub fn covers(&self, point: Vec2) -> bool {
        let up = (self.pendulum.pivot - self.pendulum.bob).normalize();
        let hole = self.pendulum.bob.truncate();
        let along = (self.pendulum.bob + up * self.height()).truncate() - hole;
        let t = ((point - hole).dot(along) / along.length_squared().max(1e-12)).clamp(0.0, 1.0);
        point.distance(hole + along * t) <= self.radius
    }

    pub fn jet(&self) -> &Jet {
        &self.jet
    }

    pub fn inverse_mass(&self) -> f32 {
        match self.held_over {
            Some(_) => 0.0,
            None => 1.0 / self.mass(),
        }
    }

    pub fn inverse_inertia(&self) -> f32 {
        match self.held_over {
            Some(_) => 0.0,
            None => 1.0 / (self.shell_mass() * self.radius * self.radius),
        }
    }

    fn spin_settling(&self) -> f32 {
        (self.radius * self.radius / self.fluid.kinematic_viscosity()).clamp(QUICKEST_SPIN_SETTLING, SLOWEST_SPIN_SETTLING)
    }

    pub fn falling_paint(&self) -> f32 {
        self.jet.volume() as f32 + self.threads.pending() * METRES_PER_PIXEL.powi(3)
    }

    pub fn mass(&self) -> f32 {
        self.shell_mass() + DENSITY * self.volume as f32
    }

    fn shell_mass(&self) -> f32 {
        SHELL_MASS_PER_AREA * 2.0 * PI * self.radius * (self.height() + self.radius)
    }

    pub fn height(&self) -> f32 {
        self.radius * HEIGHT_PER_RADIUS
    }

    pub fn head(&self) -> f32 {
        self.volume as f32 / (PI * self.radius * self.radius)
    }

    pub fn flow(&self) -> f32 {
        orifice_flow(self.head(), self.hole_radius, SPOUT_LENGTH, self.fluid)
    }

    fn damping(&self) -> Damping {
        let length = self.pendulum.length;
        let bottle_drag = SIDE_ON_DRAG * 2.0 * self.radius * self.height();
        let string_drag = STRING_DRAG * STRING_THICKNESS * length / 4.0;
        let sway_angle = (self.seed >> 40) as f32 / (1u64 << 24) as f32 * std::f32::consts::TAU;
        Damping {
            drag_per_mass: 0.5 * AIR_DENSITY * (bottle_drag + string_drag) / self.mass(),
            hook_friction: GRAVITY * HOOK_RUB / length,
            cord_loss: CORD_LOSS_FACTOR * (GRAVITY / length).sqrt(),
            sway: Vec2::from_angle(sway_angle) * SUSPENSION_UNEVENNESS * GRAVITY / length,
        }
    }

    pub fn step(&mut self, dt: f32) -> Released {
        match self.held_over {
            Some(over) => self.pendulum.drag_to(over, self.held_from.unwrap_or(over), dt),
            None => self.pendulum.step(dt, self.damping()),
        }
        self.held_from = self.held_over;
        self.turned += self.spin * dt;
        self.spin *= (-dt / self.spin_settling()).exp();
        let kick: f32 = (0..3).map(|_| self.lumps.range(-1.0, 1.0)).sum();
        self.clogging += -self.clogging * dt / LUMP_TIME + (2.0 * dt / LUMP_TIME).sqrt() * kick;
        let lumpiness = LUMPY_STIFF + (LUMPY_RUNNY - LUMPY_STIFF) * self.medium.clamp(0.0, 1.0);
        let flow = if self.pouring { self.flow() * (1.0 + lumpiness * self.clogging).max(NARROWEST_CLOG) } else { 0.0 };
        let released = f64::from(flow * dt).min(self.volume);
        self.volume -= released;
        self.shaking += dt;
        let shake = self.tremor.iter().map(|(hertz, phase, axis)| *axis * (std::f32::consts::TAU * hertz * self.shaking + phase).sin()).sum::<Vec2>() * TREMOR;
        let source = Source { position: (self.pendulum.bob + shake.extend(0.0)).as_dvec3(), velocity: self.pendulum.velocity.as_dvec3(), hole_radius: f64::from(self.hole_radius), flow: released / f64::from(dt) };
        let fallen = self.jet.step(f64::from(dt), Some(&source));
        self.release(&fallen, released as f32)
    }

    pub fn let_fall(&mut self, dt: f32) -> Released {
        let fallen = self.jet.step(f64::from(dt), None);
        self.release(&fallen, 0.0)
    }

    fn release(&mut self, fallen: &Fallen, volume: f32) -> Released {
        let (paint, medium) = (self.paint, self.medium);
        let marks = self.threads.lay(&fallen.landed, paint, medium);
        let droplets = fallen.drops.iter().map(|drop| falling_droplet(drop, self.paint, self.medium)).collect();
        Released { marks, droplets, volume }
    }
}

fn tremor_of(seed: u64) -> [(f32, f32, Vec2); 3] {
    let mut rng = SplitMix64::new(seed.rotate_left(41));
    std::array::from_fn(|_| (rng.range(6.0, 12.0), rng.range(0.0, std::f32::consts::TAU), Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU)) * rng.range(0.4, 1.0)))
}

pub fn capacity(radius: f32) -> f32 {
    PI * radius * radius * radius * HEIGHT_PER_RADIUS
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    const DT: f32 = 1.0 / 240.0;

    fn bottle(medium: f32, swing: Vec2) -> Bottle {
        let pendulum = Pendulum::released(Vec3::new(0.1, 0.1, 1.1), 0.95, Vec2::new(0.1, 0.1) + swing, Vec2::ZERO);
        Bottle::filled(pendulum, Filling { paint: Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium, hole_radius: 0.002, radius: 0.03, pouring: true }, 1, 1)
    }

    #[test]
    fn everything_that_leaves_the_bottle_is_accounted_for() {
        let mut bottle = bottle(0.35, Vec2::new(0.04, 0.0));
        let start = bottle.volume;
        let mut laid = 0.0f64;
        let mut released = 0.0f64;
        for _ in 0..240 * 5 {
            let out = bottle.step(DT);
            released += f64::from(out.volume);
            laid += f64::from(out.marks.iter().map(|mark| mark.volume).sum::<f32>() * METRES_PER_PIXEL.powi(3) + out.droplets.iter().map(Droplet::volume).sum::<f32>());
        }
        assert!((start - bottle.volume - released).abs() < released * 1e-6);
        let falling = f64::from(bottle.falling_paint());
        assert!((laid + falling - released).abs() < released * 0.001, "paint out of the bottle {released:e} should all be laid or falling, {laid:e} laid and {falling:e} falling");
    }

    fn swing_amplitude(bottle: &mut Bottle, seconds: f32) -> f32 {
        let mut widest = 0.0f32;
        for _ in 0..(seconds / DT) as usize {
            bottle.step(DT);
        }
        for _ in 0..(4.0 / DT) as usize {
            bottle.step(DT);
            widest = widest.max((bottle.pendulum.bob - bottle.pendulum.pivot).truncate().length());
        }
        widest
    }

    #[test]
    fn a_long_swing_halves_within_a_minute_and_stops_within_three() {
        let pendulum = Pendulum::released(Vec3::new(0.0, 0.0, 2.65), 2.5, Vec2::new(0.4, 0.0), Vec2::ZERO);
        let mut bottle = Bottle::filled(pendulum, Filling { paint: Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium: 0.35, hole_radius: 0.002, radius: 0.035, pouring: false }, 1, 1);
        let start = swing_amplitude(&mut bottle, 0.0);
        let after_twenty_seconds = swing_amplitude(&mut bottle, 16.0);
        let after_a_minute = swing_amplitude(&mut bottle, 36.0);
        let after_three_minutes = swing_amplitude(&mut bottle, 116.0);
        assert!(after_twenty_seconds > start * 0.6, "a 40 cm swing should still be wide after twenty seconds, {start:.3} -> {after_twenty_seconds:.3} m");
        assert!(after_a_minute < start * 0.5, "cord and hook losses should halve the swing within a minute, {start:.3} -> {after_a_minute:.3} m");
        assert!(after_three_minutes < 0.005, "hook friction should bring the swing to rest within three minutes, still {after_three_minutes:.3} m");
    }

    #[test]
    fn a_draining_bottle_pours_ever_more_slowly() {
        let mut bottle = bottle(0.35, Vec2::ZERO);
        let first = bottle.flow();
        for _ in 0..240 * 120 {
            bottle.step(DT);
        }
        assert!(bottle.flow() < first * 0.9, "the flow should fall as the paint level drops ({:e} -> {:e})", first, bottle.flow());
    }
}
