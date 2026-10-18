use super::flight::Droplet;
use super::fluid::{DENSITY, Fluid};
use super::outflow::orifice_flow;
use super::pendulum::{Damping, Pendulum};
use super::thread::{Laying, falling_thread};
use crate::paint::Mark;
use crate::paint::rheology::{strand, yield_stress};
use crate::pigment::Coefficients;
use crate::rng::SplitMix64;
use crate::units::{GRAVITY, METRES_PER_PIXEL, pixels};
use glam::{Vec2, Vec3};
use std::collections::VecDeque;
use std::f32::consts::PI;

const HEIGHT_PER_RADIUS: f32 = 6.0;
const FILLED: f32 = 0.75;
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
const SURGE: f32 = 0.12;
const SURGE_TIME: f32 = 0.4;
const BREAKUP_SPREAD: f32 = 0.5;
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
    laying: Option<Laying>,
    seed: u64,
    surge: f32,
    lead: Vec2,
    falling: VecDeque<Vec2>,
    unevenness: SplitMix64,
    last_contact: Option<Vec2>,
    bead_debt: f32,
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
            laying: None,
            seed,
            surge: 0.0,
            lead: Vec2::ZERO,
            falling: VecDeque::new(),
            unevenness: SplitMix64::new(seed.rotate_left(17)),
            last_contact: None,
            bead_debt: 0.0,
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
        self.laying = None;
        self.last_contact = None;
        self.falling.clear();
    }

    pub fn covers(&self, point: Vec2) -> bool {
        let up = (self.pendulum.pivot - self.pendulum.bob).normalize();
        let hole = self.pendulum.bob.truncate();
        let along = (self.pendulum.bob + up * self.height()).truncate() - hole;
        let t = ((point - hole).dot(along) / along.length_squared().max(1e-12)).clamp(0.0, 1.0);
        point.distance(hole + along * t) <= self.radius
    }

    pub fn thread_end(&self) -> Option<Vec2> {
        self.last_contact
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

    pub fn forming_drop(&self) -> f32 {
        self.bead_debt
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
        let kick: f32 = (0..3).map(|_| self.unevenness.range(-1.0, 1.0)).sum();
        self.surge += -self.surge * dt / SURGE_TIME + SURGE * (2.0 * dt / SURGE_TIME).sqrt() * kick;
        let flow = if self.pouring { self.flow() * (1.0 + self.surge).max(0.0) } else { 0.0 };
        let released = f64::from(flow * dt).min(self.volume);
        self.volume -= released;
        let released = released as f32;
        let mut out = Released { marks: Vec::new(), droplets: Vec::new(), volume: released };
        let height = self.pendulum.bob.z;
        if released <= 0.0 || height <= 0.0 {
            self.restart_thread();
            return out;
        }
        let thread = falling_thread(flow, height, self.hole_radius, self.fluid);
        self.lead += (self.pendulum.velocity.truncate() * thread.fall_time - self.lead) * (dt / thread.fall_time).min(1.0);
        self.falling.push_back(self.pendulum.bob.truncate() + self.lead);
        let delay = thread.fall_time / dt;
        while self.falling.len() as f32 > delay.ceil() + 1.0 {
            self.falling.pop_front();
        }
        let back = (delay.min(self.falling.len() as f32 - 1.0)).max(0.0);
        let newest = self.falling.len() - 1;
        let (older, newer) = (newest - back.ceil() as usize, newest - back.floor() as usize);
        let disturbed = thread.breakup_time * (BREAKUP_SPREAD * self.surge / SURGE).exp();
        if thread.fall_time > disturbed {
            let diameter = thread.drop_diameter;
            self.laying = None;
            self.last_contact = None;
            self.bead_debt += released;
            let bead = PI * diameter.powi(3) / 6.0;
            while self.bead_debt >= bead {
                self.bead_debt -= bead;
                out.droplets.push(Droplet {
                    position: self.pendulum.bob,
                    velocity: self.pendulum.velocity + Vec3::new(0.0, 0.0, -thread.exit_speed),
                    diameter,
                    paint: self.paint,
                    medium: self.medium,
                    fluid: self.fluid,
                });
            }
            return out;
        }
        let nozzle_velocity = self.pendulum.velocity.truncate();
        let seed = self.seed;
        let laying = self.laying.get_or_insert_with(|| Laying::coiling(thread.coil_radius, seed));
        let landing_below = self.falling[older].lerp(self.falling[newer], back.ceil() - back);
        let contacts = laying.advance(landing_below, nozzle_velocity, &thread, dt);
        let laid_speed = thread.landing_speed.max(nozzle_velocity.length());
        let cross_section = pixels(pixels(flow / laid_speed));
        let line = strand(cross_section, yield_stress(self.medium), pixels(2.0 * thread.radius));
        let share = released / contacts.len() as f32 / METRES_PER_PIXEL.powi(3);
        for contact in contacts {
            let at = contact / METRES_PER_PIXEL;
            out.marks.push(Mark { from: self.last_contact.unwrap_or(at), to: at, radius: line.half_width, volume: share, paint: self.paint, medium: self.medium });
            self.last_contact = Some(at);
        }
        out
    }
}

pub fn capacity(radius: f32) -> f32 {
    PI * radius * radius * radius * HEIGHT_PER_RADIUS
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!((laid - released).abs() < released * 0.02, "paint out of the bottle {released:e} should all be laid or falling, {laid:e}");
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
