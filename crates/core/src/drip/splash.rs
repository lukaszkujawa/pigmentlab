use super::fluid::{DENSITY, Fluid, SURFACE_TENSION};
use crate::paint::{Mark, Profile};
use crate::pigment::Coefficients;
use crate::rng::SplitMix64;
use glam::Vec2;
use std::f32::consts::{PI, TAU};

const FINGERS_PER_IMPACT_REYNOLDS: f32 = 1.0 / 4.2;
const FEWEST_FINGERS: f32 = 3.0;
const MOST_FINGERS: f32 = 64.0;
const ANGLE_JITTER: f32 = 0.45;
const REACH_PER_RIM: f32 = 1.1;
const ROOT_INSET: f32 = 0.8;
const FINGER_FLATNESS: f32 = 0.6;
const FINGER_EDGE: f32 = 0.1;
const FINGER_TIP: f32 = 0.4;
const SHEDDING: f32 = 0.7;
const SHED_SHARE: f32 = 0.5;
const DROPLET_SLOPE: f32 = 0.36;
const LONGEST_LEAN: f32 = 2.0;
const ROOT_WIDTH_SHARE: f32 = 0.3;

pub fn finger_count(speed: f32, diameter: f32, fluid: Fluid) -> usize {
    let impact_reynolds = speed * (PI * PI * DENSITY * diameter.powi(3) / (16.0 * SURFACE_TENSION)).powf(0.25) / fluid.kinematic_viscosity().sqrt();
    (FINGERS_PER_IMPACT_REYNOLDS * impact_reynolds.powf(0.75)).round().clamp(FEWEST_FINGERS, MOST_FINGERS) as usize
}

pub struct Splash {
    pub centre: Vec2,
    pub tail: Vec2,
    pub rim: f32,
    pub lean: f32,
    pub count: usize,
    pub volume: f32,
    pub paint: Coefficients,
    pub medium: f32,
}

pub fn fingers(splash: &Splash, rng: &mut SplitMix64) -> Vec<Mark> {
    let heading = splash.tail.normalize_or(Vec2::X);
    let lean = splash.lean.clamp(0.0, LONGEST_LEAN);
    let weights: Vec<f32> = (0..splash.count).map(|_| rng.unit().powi(2) + 0.05).collect();
    let total: f32 = weights.iter().sum();
    let flat = Profile { flatness: FINGER_FLATNESS, ragged: FINGER_EDGE, open: [true, false], taper: FINGER_TIP };
    let mut marks = Vec::with_capacity(2 * splash.count);
    for (finger, weight) in weights.iter().enumerate() {
        let direction = Vec2::from_angle(TAU * (finger as f32 + rng.range(-ANGLE_JITTER, ANGLE_JITTER)) / splash.count as f32);
        let downwind = direction.dot(heading);
        let root = splash.centre + direction * splash.rim * ROOT_INSET + splash.tail * 0.5 * downwind;
        let reach = splash.rim * (REACH_PER_RIM * weight.sqrt() + (1.0 - ROOT_INSET)) * (1.0 + lean * downwind).max(0.1).powi(2);
        let tip = root + direction * reach;
        let volume = splash.volume * weight / total;
        let shed = rng.unit() < SHEDDING * weight.sqrt();
        let kept = if shed { volume * (1.0 - SHED_SHARE) } else { volume };
        let root_radius = (ROOT_WIDTH_SHARE * PI * splash.rim / splash.count as f32) * (0.5 + weight.sqrt());
        marks.push(Mark { from: root, to: tip, radius: root_radius, volume: kept, paint: splash.paint, medium: splash.medium, profile: flat });
        if shed {
            let flight = tip + direction.rotate(Vec2::from_angle(rng.range(-0.2, 0.2))) * reach * rng.range(0.3, 1.6);
            let droplet = volume - kept;
            let radius = (2.0 * droplet / (PI * DROPLET_SLOPE)).cbrt();
            marks.push(Mark { profile: Profile { flatness: FINGER_FLATNESS, ragged: FINGER_EDGE, open: [false; 2], taper: 1.0 }, ..Mark::dot(flight, radius, droplet, splash.paint, splash.medium) });
        }
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    const WATER: Fluid = Fluid { viscosity: 1e-3, yield_stress: 0.0 };

    fn splash(tail: Vec2, lean: f32) -> Splash {
        Splash { centre: Vec2::new(500.0, 500.0), tail, rim: 20.0, lean, count: 16, volume: 300.0, paint: Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium: 0.8 }
    }

    #[test]
    fn the_finger_count_follows_marmanis_and_thoroddsen_and_agrees_with_rayleigh_taylor_fingering() {
        let (speed, diameter) = (2.0, 0.003);
        let weber = DENSITY * speed * speed * diameter / SURFACE_TENSION;
        let reynolds = DENSITY * speed * diameter / WATER.viscosity;
        let bhola_chandra = weber.sqrt() * reynolds.powf(0.25) / (4.0 * 3.0f32.sqrt());
        let counted = finger_count(speed, diameter, WATER) as f32;
        assert!((counted - bhola_chandra).abs() < 0.25 * bhola_chandra, "a 3 mm watery drop at 2 m/s throws about {bhola_chandra:.0} fingers, counted {counted}");
        assert!(finger_count(3.0, 0.003, WATER) > finger_count(2.0, 0.003, WATER), "faster drops throw more fingers");
        assert!(finger_count(2.0, 0.005, WATER) > finger_count(2.0, 0.003, WATER), "bigger drops throw more fingers");
    }

    #[test]
    fn fingers_keep_the_splashed_paint() {
        let splash = splash(Vec2::ZERO, 0.0);
        let marks = fingers(&splash, &mut SplitMix64::new(3));
        let laid: f32 = marks.iter().map(|mark| mark.volume).sum();
        assert!((laid - splash.volume).abs() < splash.volume * 1e-4);
    }

    #[test]
    fn fingers_of_a_slanted_impact_reach_downwind() {
        let straight = fingers(&splash(Vec2::ZERO, 0.0), &mut SplitMix64::new(3));
        let slanted = fingers(&splash(Vec2::new(30.0, 0.0), 1.0), &mut SplitMix64::new(3));
        let reach = |marks: &[Mark]| marks.iter().map(|mark| mark.to.x - 500.0).fold(0.0f32, f32::max) / -marks.iter().map(|mark| mark.to.x - 500.0).fold(0.0f32, f32::min);
        assert!((0.7..1.4).contains(&reach(&straight)), "a straight impact splashes evenly, downwind/upwind reach {:.2}", reach(&straight));
        assert!(reach(&slanted) > 2.0, "a slanted impact throws its fingers downwind, downwind/upwind reach {:.2}", reach(&slanted));
    }
}
