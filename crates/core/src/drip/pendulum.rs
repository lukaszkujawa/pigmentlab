use crate::units::GRAVITY;
use glam::{Vec2, Vec3};

const SHORTEST_DROP: f32 = 0.05;
const TAUT: f32 = 1.0 - 1e-4;
const STICKING_SPEED: f32 = 0.002;
const HAND_SMOOTHING: f32 = 0.04;
const SLACKENING: f32 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pendulum {
    pub pivot: Vec3,
    pub length: f32,
    pub bob: Vec3,
    pub velocity: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Damping {
    pub drag_per_mass: f32,
    pub hook_friction: f32,
    pub cord_loss: f32,
    pub sway: Vec2,
}

impl Pendulum {
    pub fn released(pivot: Vec3, length: f32, over: Vec2, push: Vec2) -> Self {
        let reach = (over - pivot.truncate()).clamp_length_max(length * (1.0 - SHORTEST_DROP * SHORTEST_DROP).sqrt());
        let bob = pivot + reach.extend(-(length * length - reach.length_squared()).sqrt());
        let mut pendulum = Self { pivot, length, bob, velocity: push.extend(0.0) };
        pendulum.velocity = pendulum.tangential(pendulum.velocity);
        pendulum
    }

    pub fn step(&mut self, dt: f32, damping: Damping) {
        let closing_in = self.velocity.dot((self.bob - self.pivot).normalize()) < -SLACKENING;
        match self.taut() && !closing_in && self.tension_per_mass(self.bob, self.velocity, damping) >= 0.0 {
            true => self.swing(dt, damping),
            false => self.fly(dt, damping),
        }
    }

    pub fn taut(&self) -> bool {
        (self.bob - self.pivot).length() >= self.length * TAUT
    }

    fn swing(&mut self, dt: f32, damping: Damping) {
        let derivative = |bob: Vec3, velocity: Vec3| (velocity, self.held_acceleration(bob, velocity, damping));
        let (bob, velocity) = runge_kutta(self.bob, self.velocity, dt, derivative);
        self.velocity = velocity;
        self.bob = self.pivot + (bob - self.pivot).normalize() * self.length;
        self.velocity = self.tangential(self.velocity);
    }

    fn fly(&mut self, dt: f32, damping: Damping) {
        let (bob, velocity) = runge_kutta(self.bob, self.velocity, dt, |_, velocity| (velocity, free_acceleration(velocity, damping)));
        self.bob = bob;
        self.velocity = velocity;
        let reach = self.bob - self.pivot;
        if reach.length() > self.length {
            let outward = reach.normalize();
            self.bob = self.pivot + outward * self.length;
            let stretching = self.velocity.dot(outward);
            if stretching > 0.0 {
                self.velocity -= outward * stretching;
            }
        }
    }

    pub fn drag_to(&mut self, over: Vec2, from: Vec2, dt: f32) {
        let held = |at: Vec2| Self::released(self.pivot, self.length, at, Vec2::ZERO).bob;
        let measured = (held(over) - held(from)) / dt;
        self.velocity += (measured - self.velocity) * (dt / HAND_SMOOTHING).min(1.0);
        self.bob = held(over);
    }

    pub fn tangential(&self, velocity: Vec3) -> Vec3 {
        let outward = (self.bob - self.pivot).normalize();
        velocity - outward * velocity.dot(outward)
    }

    fn suspended_acceleration(&self, bob: Vec3, velocity: Vec3, damping: Damping) -> Vec3 {
        let speed = velocity.length();
        let rubbing = if speed > 0.0 { -velocity / speed * damping.hook_friction * (speed / STICKING_SPEED).min(1.0) } else { Vec3::ZERO };
        let axis = damping.sway.normalize_or_zero();
        let leaning = (bob - self.pivot).truncate().dot(axis);
        free_acceleration(velocity, damping) + rubbing - velocity * damping.cord_loss - (axis * leaning * damping.sway.length()).extend(0.0)
    }

    fn tension_per_mass(&self, bob: Vec3, velocity: Vec3, damping: Damping) -> f32 {
        let outward = (bob - self.pivot).normalize();
        let sideways = velocity - outward * velocity.dot(outward);
        self.suspended_acceleration(bob, velocity, damping).dot(outward) + sideways.length_squared() / self.length
    }

    fn held_acceleration(&self, bob: Vec3, velocity: Vec3, damping: Damping) -> Vec3 {
        let outward = (bob - self.pivot).normalize();
        self.suspended_acceleration(bob, velocity, damping) - outward * self.tension_per_mass(bob, velocity, damping)
    }

    pub fn energy_per_mass(&self) -> f32 {
        0.5 * self.velocity.length_squared() + GRAVITY * self.bob.z
    }
}

fn free_acceleration(velocity: Vec3, damping: Damping) -> Vec3 {
    Vec3::new(0.0, 0.0, -GRAVITY) - velocity * damping.drag_per_mass * velocity.length()
}

fn runge_kutta(position: Vec3, velocity: Vec3, dt: f32, derivative: impl Fn(Vec3, Vec3) -> (Vec3, Vec3)) -> (Vec3, Vec3) {
    let (k1x, k1v) = derivative(position, velocity);
    let (k2x, k2v) = derivative(position + k1x * dt * 0.5, velocity + k1v * dt * 0.5);
    let (k3x, k3v) = derivative(position + k2x * dt * 0.5, velocity + k2v * dt * 0.5);
    let (k4x, k4v) = derivative(position + k3x * dt, velocity + k3v * dt);
    (position + (k1x + 2.0 * k2x + 2.0 * k3x + k4x) * dt / 6.0, velocity + (k1v + 2.0 * k2v + 2.0 * k3v + k4v) * dt / 6.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const DT: f32 = 1.0 / 240.0;
    const UNDAMPED: Damping = Damping { drag_per_mass: 0.0, hook_friction: 0.0, cord_loss: 0.0, sway: Vec2::ZERO };

    fn hung(length: f32, over: Vec2, push: Vec2) -> Pendulum {
        Pendulum::released(Vec3::new(0.0, 0.0, 2.0), length, over, push)
    }

    fn crossings_of_x(pendulum: &mut Pendulum, seconds: f32) -> Vec<f32> {
        let mut times = Vec::new();
        let mut previous = pendulum.bob.x;
        for step in 1..=(seconds / DT) as usize {
            pendulum.step(DT, UNDAMPED);
            if previous < 0.0 && pendulum.bob.x >= 0.0 {
                times.push(step as f32 * DT);
            }
            previous = pendulum.bob.x;
        }
        times
    }

    #[test]
    fn a_small_swing_keeps_the_textbook_period() {
        let length = 1.2;
        let mut pendulum = hung(length, Vec2::new(0.03, 0.0), Vec2::ZERO);
        let crossings = crossings_of_x(&mut pendulum, 12.0);
        let period = (crossings.last().unwrap() - crossings[0]) / (crossings.len() - 1) as f32;
        let expected = 2.0 * PI * (length / GRAVITY).sqrt();
        assert!((period - expected).abs() < expected * 0.002, "period {period:.4} s vs {expected:.4} s");
    }

    #[test]
    fn the_string_never_stretches_and_an_undamped_swing_keeps_its_energy() {
        let mut pendulum = hung(1.0, Vec2::new(0.3, 0.1), Vec2::new(0.0, 0.8));
        let energy = pendulum.energy_per_mass();
        for _ in 0..240 * 60 {
            pendulum.step(DT, UNDAMPED);
            assert!(((pendulum.bob - pendulum.pivot).length() - 1.0).abs() < 1e-5);
        }
        assert!((pendulum.energy_per_mass() - energy).abs() < 0.001 * GRAVITY, "energy drifted from {energy} to {}", pendulum.energy_per_mass());
    }

    #[test]
    fn an_elliptical_orbit_precesses_at_three_eighths_omega_ab_over_l_squared() {
        let length = 1.0;
        let omega = (GRAVITY / length).sqrt();
        let (a, b) = (0.15, 0.06);
        let mut pendulum = hung(length, Vec2::new(a, 0.0), Vec2::new(0.0, b * omega));
        let mut apsis_angles = Vec::new();
        let mut previous_radius = pendulum.bob.truncate().length();
        let mut growing = false;
        for _ in 0..240 * 40 {
            pendulum.step(DT, UNDAMPED);
            let radius = pendulum.bob.truncate().length();
            if growing && radius < previous_radius {
                apsis_angles.push(pendulum.bob.y.atan2(pendulum.bob.x));
            }
            growing = radius > previous_radius;
            previous_radius = radius;
        }
        let turned: f32 = apsis_angles.windows(2).map(|pair| (pair[1] - pair[0] + PI).rem_euclid(PI) - PI).map(|step| if step < -PI / 2.0 { step + PI } else { step }).sum();
        let rate = turned / 40.0;
        let expected = 3.0 / 8.0 * omega * a * b / (length * length);
        assert!((rate - expected).abs() < expected * 0.15, "apsides turned at {rate:.5} rad/s, expected {expected:.5}");
    }

    #[test]
    fn a_bottle_thrown_up_towards_its_hook_flies_free_until_the_string_snaps_taut() {
        let mut pendulum = hung(1.0, Vec2::new(0.3, 0.0), Vec2::ZERO);
        pendulum.velocity = Vec3::new(-1.5, 0.0, 2.5);
        let start = pendulum.bob;
        let launch = pendulum.velocity;
        let mut snapped = false;
        for step in 1..240 {
            pendulum.step(DT, UNDAMPED);
            let t = step as f32 * DT;
            if pendulum.taut() {
                snapped = step > 10;
                break;
            }
            let ballistic = start + launch * t + Vec3::new(0.0, 0.0, -0.5 * GRAVITY * t * t);
            assert!(pendulum.bob.distance(ballistic) < 1e-3, "a slack string must not pull: {:?} vs {ballistic:?}", pendulum.bob);
        }
        assert!(snapped, "the bottle should fly freely for a while and then be caught by the string");
    }

    #[test]
    fn a_string_snapping_taut_takes_energy_out() {
        let mut pendulum = hung(1.0, Vec2::ZERO, Vec2::ZERO);
        pendulum.bob = pendulum.pivot + Vec3::new(0.0, 0.0, -0.6);
        pendulum.velocity = Vec3::ZERO;
        let before = pendulum.energy_per_mass();
        for _ in 0..240 {
            pendulum.step(DT, UNDAMPED);
        }
        assert!((pendulum.bob - pendulum.pivot).length() > 0.999, "a dropped bottle should end up hanging on its string");
        assert!(pendulum.energy_per_mass() < before - 0.5 * GRAVITY * 0.4 * 0.9, "the jerk of the string should absorb most of the fall");
    }

    #[test]
    fn air_drag_slowly_dies_the_swing_down() {
        let mut pendulum = hung(1.0, Vec2::new(0.2, 0.0), Vec2::ZERO);
        let damping = Damping { drag_per_mass: 0.01, hook_friction: 0.001, cord_loss: 0.0, sway: Vec2::ZERO };
        let resting = GRAVITY * (pendulum.pivot.z - pendulum.length);
        let start = pendulum.energy_per_mass() - resting;
        for _ in 0..240 * 60 {
            pendulum.step(DT, damping);
        }
        let left = pendulum.energy_per_mass() - resting;
        assert!(left < start * 0.9 && left > start * 0.3, "a minute of swinging should lose some but not all of its energy ({start:.4} -> {left:.4})");
    }
}
