use super::fluid::{DENSITY, Fluid, SURFACE_TENSION, Wetted, wetted_mound};
use super::splash::{Splash, finger_count, fingers};
use crate::paint::rheology::yield_stress;
use crate::paint::{Mark, Profile};
use crate::pigment::Coefficients;
use crate::rng::SplitMix64;
use crate::units::{GRAVITY, METRES_PER_PIXEL, pixels};
use glam::{Vec2, Vec3};
use std::f32::consts::PI;

const AIR_DENSITY: f32 = 1.2;
const AIR_VISCOSITY: f32 = 1.8e-5;
const SPLASH_THRESHOLD: f32 = 57.7;
const SPLASHED_SHARE: f32 = 0.15;
const MOST_SPLASHED_SHARE: f32 = 0.4;
const LONGEST_STAIN: f32 = 6.0;
const QUIET_EDGE: f32 = 0.05;
const WETTING_SPREAD: f32 = 0.2;
const BODY_SHARE: f32 = 0.6;
const SPINE_WIDTH: f32 = 0.35;
const SPINE_VOLUME: f32 = 0.15;
const SPLASHED_EDGE: f32 = 0.12;
const SPINE_TIP: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Droplet {
    pub position: Vec3,
    pub velocity: Vec3,
    pub diameter: f32,
    pub paint: Coefficients,
    pub medium: f32,
    pub fluid: Fluid,
}

impl Droplet {
    pub fn fly(&mut self, dt: f32) {
        let relative_speed = self.velocity.length();
        let air_reynolds = AIR_DENSITY * relative_speed * self.diameter / AIR_VISCOSITY;
        let stokes_time = DENSITY * self.diameter * self.diameter / (18.0 * AIR_VISCOSITY);
        let drag = (1.0 + 0.15 * air_reynolds.powf(0.687)) / stokes_time;
        self.velocity = (self.velocity + Vec3::new(0.0, 0.0, -GRAVITY) * dt) / (1.0 + drag * dt);
        self.position += self.velocity * dt;
    }

    pub fn landed(&self) -> bool {
        self.position.z <= 0.0
    }

    pub fn volume(&self) -> f32 {
        PI * self.diameter.powi(3) / 6.0
    }

    pub fn impact(&self, rng: &mut SplitMix64) -> Vec<Mark> {
        let normal_speed = (-self.velocity.z).max(1e-3);
        let sideways = self.velocity.truncate();
        let impact = Impact::of(normal_speed, self.diameter, self.fluid);
        let width = impact.spread * self.diameter;
        let along = sideways.normalize_or(Vec2::X);
        let elongation = (sideways.length() / normal_speed).hypot(1.0).min(LONGEST_STAIN);
        let landing = pixels_of(self.position.truncate());
        let volume = self.volume() / METRES_PER_PIXEL.powi(3);
        let splashed = (SPLASHED_SHARE * impact.splash_number / SPLASH_THRESHOLD).min(MOST_SPLASHED_SHARE);
        let kept = if impact.splashes { 1.0 - splashed } else { 1.0 };
        let puddle = wetted_mound(volume * kept, yield_stress(self.medium));
        let wetting = rng.range(1.0 - WETTING_SPREAD, 1.0 + WETTING_SPREAD);
        let rim = pixels(width * 0.5).max(puddle.reach) * wetting;
        let tail = along * (elongation - 1.0) * 2.0 * rim;
        let profile = Wetted { reach: rim, height: puddle.height * wetting.recip().powi(2), ..puddle }.profile(if impact.splashes { SPLASHED_EDGE } else { QUIET_EDGE * rng.range(0.4, 2.5) });
        let stain = Mark { from: landing, to: landing + tail, radius: rim, volume: volume * kept, paint: self.paint, medium: self.medium, profile };
        let mut marks = match tail.length() > rim * SPINE_WIDTH {
            true => {
                let body_end = landing + tail * BODY_SHARE;
                let spine = Mark { from: body_end, to: landing + tail + along * rim * (1.0 - SPINE_WIDTH), radius: rim * SPINE_WIDTH, volume: stain.volume * SPINE_VOLUME, profile: Profile { ragged: QUIET_EDGE, taper: SPINE_TIP, ..profile }, ..stain };
                vec![Mark { to: body_end, volume: stain.volume - spine.volume, profile: Profile { oval: true, ..profile }, ..stain }, spine]
            }
            false => vec![stain],
        };
        if impact.splashes {
            let splash = Splash { centre: landing + tail * 0.5, tail, rim, lean: sideways.length() / normal_speed, count: finger_count(normal_speed, self.diameter, self.fluid), vigour: impact.splash_number / SPLASH_THRESHOLD, volume: volume * splashed, paint: self.paint, medium: self.medium };
            marks.extend(fingers(&splash, rng));
        }
        marks
    }
}

fn pixels_of(position: Vec2) -> Vec2 {
    position / METRES_PER_PIXEL
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub spread: f32,
    pub splash_number: f32,
    pub splashes: bool,
}

impl Impact {
    pub fn of(speed: f32, diameter: f32, fluid: Fluid) -> Self {
        let weber = DENSITY * speed * speed * diameter / SURFACE_TENSION;
        let reynolds = DENSITY * speed * diameter / fluid.viscosity;
        let impact_number = weber * reynolds.powf(-0.4);
        let spread = (reynolds.powf(0.2) * impact_number.sqrt() / (1.24 + impact_number.sqrt())).max(1.0);
        let splash_number = weber.sqrt() * reynolds.powf(0.25);
        Self { spread, splash_number, splashes: splash_number > SPLASH_THRESHOLD }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn droplet(velocity: Vec3, viscosity: f32) -> Droplet {
        Droplet { position: Vec3::new(0.05, 0.05, 0.0), velocity, diameter: 0.003, paint: Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium: 0.5, fluid: Fluid { viscosity, yield_stress: 0.0 } }
    }

    #[test]
    fn enamel_drops_land_as_round_dots_and_thinned_paint_splashes() {
        let speed = (2.0 * GRAVITY * 0.3f32).sqrt();
        let enamel = Impact::of(speed, 0.003, Fluid { viscosity: 1.0, yield_stress: 0.0 });
        let thinned = Impact::of(speed, 0.003, Fluid { viscosity: 0.05, yield_stress: 0.0 });
        assert!(!enamel.splashes && (1.2..1.6).contains(&enamel.spread), "enamel from 30 cm: {enamel:?}");
        assert!(thinned.splashes && (2.0..2.7).contains(&thinned.spread), "thinned paint from 30 cm: {thinned:?}");
        let mut rng = SplitMix64::new(1);
        assert_eq!(droplet(Vec3::new(0.0, 0.0, -speed), 1.0).impact(&mut rng).len(), 1, "an enamel drop should land as one round dot");
    }

    #[test]
    fn a_slanted_drop_leaves_a_stain_as_long_as_one_over_the_sine_of_its_angle() {
        let mut rng = SplitMix64::new(1);
        let marks = droplet(Vec3::new(2.0, 0.0, -2.0), 2.0).impact(&mut rng);
        let (body, spine) = (marks[0], marks[1]);
        let width = body.radius * 2.0;
        let length = (spine.to.x + spine.radius) - (body.from.x - body.radius);
        assert!((width / length - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01, "a 45 degree drop should leave a stain with width/length = sin 45, got {:.3}", width / length);
        assert!(spine.radius < body.radius && spine.to.x > body.to.x, "the narrow spine should point the way the drop was travelling");
    }

    #[test]
    fn splashing_keeps_the_drops_volume() {
        let mut rng = SplitMix64::new(7);
        let drop = droplet(Vec3::new(0.5, 0.2, -3.0), 0.02);
        let marks = drop.impact(&mut rng);
        assert!(marks.len() > 3, "a fast runny drop should throw satellites");
        let laid: f32 = marks.iter().map(|mark| mark.volume).sum();
        let expected = drop.volume() / METRES_PER_PIXEL.powi(3);
        assert!((laid - expected).abs() < expected * 1e-4);
    }

    #[test]
    fn fine_spray_is_slowed_by_the_air_while_big_drops_fly_on() {
        let mut fine = droplet(Vec3::new(3.0, 0.0, 0.0), 1.0);
        fine.diameter = 0.00014;
        let mut big = droplet(Vec3::new(3.0, 0.0, 0.0), 1.0);
        for _ in 0..24 {
            fine.fly(1.0 / 240.0);
            big.fly(1.0 / 240.0);
        }
        assert!(fine.velocity.x < 1.5, "140 micron spray should lose most of its speed in 0.1 s, still at {:.2} m/s", fine.velocity.x);
        assert!(big.velocity.x > 2.8, "a 3 mm drop should keep its speed, at {:.2} m/s", big.velocity.x);
    }
}
