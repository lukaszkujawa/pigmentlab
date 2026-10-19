use super::fluid::{DENSITY, Fluid, SURFACE_TENSION};
use crate::paint::{Mark, Profile};
use crate::pigment::Coefficients;
use crate::rng::SplitMix64;
use glam::Vec2;
use std::f32::consts::{PI, TAU};

const FINGERS_PER_IMPACT_REYNOLDS: f32 = 1.0 / 4.2;
const FEWEST_FINGERS: f32 = 3.0;
const MOST_FINGERS: f32 = 64.0;
const COUNT_SPREAD: f32 = 0.35;
const ANGLE_JITTER: f32 = 0.45;
const REACH_PER_RIM: f32 = 0.9;
const LONGEST_STRETCH: f32 = 2.5;
const SHORTEST_STRETCH: f32 = 0.1;
const ROOT_INSET: f32 = 0.8;
const FINGER_FLATNESS: f32 = 0.6;
const FINGER_EDGE: f32 = 0.1;
const SEGMENT_PER_ROOT: f32 = 2.5;
const MOST_SEGMENTS: usize = 8;
const CURL: f32 = 0.9;
const THINNING: f32 = 0.75;
const THINNEST: f32 = 0.6;
const BEADED: f32 = 0.55;
const BEAD_SHARE: f32 = 0.35;
const SHEDDING: f32 = 0.5;
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
    pub vigour: f32,
    pub volume: f32,
    pub paint: Coefficients,
    pub medium: f32,
}

struct Finger {
    root: Vec2,
    heading: Vec2,
    reach: f32,
    width: f32,
    volume: f32,
}

pub fn fingers(splash: &Splash, rng: &mut SplitMix64) -> Vec<Mark> {
    let heading = splash.tail.normalize_or(Vec2::X);
    let lean = splash.lean.clamp(0.0, LONGEST_LEAN);
    let count = ((splash.count as f32 * rng.range(1.0 - COUNT_SPREAD, 1.0 + COUNT_SPREAD)).round() as usize).max(FEWEST_FINGERS as usize);
    let weights: Vec<f32> = (0..count).map(|_| rng.unit().powi(3) + 0.03).collect();
    let total: f32 = weights.iter().sum();
    let mut marks = Vec::new();
    for (finger, weight) in weights.iter().enumerate() {
        let direction = Vec2::from_angle(TAU * (finger as f32 + rng.range(-ANGLE_JITTER, ANGLE_JITTER)) / count as f32);
        let downwind = direction.dot(heading);
        let stretch = (1.0 + lean * downwind).clamp(SHORTEST_STRETCH, LONGEST_STRETCH);
        let finger = Finger {
            root: splash.centre + direction * splash.rim * ROOT_INSET + splash.tail * 0.5 * downwind,
            heading: direction,
            reach: splash.rim * (REACH_PER_RIM * splash.vigour.sqrt() * weight.sqrt() * rng.range(0.5, 1.5) + (1.0 - ROOT_INSET)) * stretch,
            width: (ROOT_WIDTH_SHARE * PI * splash.rim / count as f32) * (0.5 + weight.sqrt()),
            volume: splash.volume * weight / total,
        };
        lay_finger(&finger, splash, rng, &mut marks);
    }
    marks
}

fn lay_finger(finger: &Finger, splash: &Splash, rng: &mut SplitMix64, marks: &mut Vec<Mark>) {
    let segments = ((finger.reach / (SEGMENT_PER_ROOT * finger.width)).ceil() as usize).clamp(1, MOST_SEGMENTS);
    let curl = rng.range(-CURL, CURL) / finger.reach.max(1.0);
    let beaded = rng.unit() < BEADED;
    let shed = rng.unit() < SHEDDING;
    let in_bead = if beaded { BEAD_SHARE } else { 0.0 };
    let in_drop = if shed { SHED_SHARE * (1.0 - in_bead) } else { 0.0 };
    let along_finger = finger.volume * (1.0 - in_bead - in_drop);
    let step = finger.reach / segments as f32;
    let width_at = |t: f32| finger.width * ((1.0 - t).powf(THINNING)).max(THINNEST / finger.width.max(THINNEST));
    let weights: Vec<f32> = (0..segments).map(|k| width_at((k as f32 + 0.5) / segments as f32).powi(2)).collect();
    let total: f32 = weights.iter().sum();
    let flat = Profile { flatness: FINGER_FLATNESS, stiffness: 0.0, ragged: FINGER_EDGE, open: [true, true], taper: 1.0, oval: false };
    let mut at = finger.root;
    let mut heading = finger.heading;
    for k in 0..segments {
        let turned = heading.rotate(Vec2::from_angle(curl * step));
        let next = at + turned * step;
        let (from_width, to_width) = (width_at(k as f32 / segments as f32), width_at((k + 1) as f32 / segments as f32));
        let last = k + 1 == segments;
        let profile = Profile { open: [true, !last || beaded], taper: to_width / from_width, ..flat };
        marks.push(Mark { from: at, to: next, radius: from_width, volume: along_finger * weights[k] / total, paint: splash.paint, medium: splash.medium, profile });
        at = next;
        heading = turned;
    }
    let dot = |centre: Vec2, volume: f32| {
        let radius = (2.0 * volume / (PI * DROPLET_SLOPE)).cbrt();
        Mark { profile: Profile { flatness: FINGER_FLATNESS, stiffness: 0.0, ragged: FINGER_EDGE, open: [false; 2], taper: 1.0, oval: false }, ..Mark::dot(centre, radius, volume, splash.paint, splash.medium) }
    };
    if beaded {
        marks.push(dot(at, finger.volume * in_bead));
    }
    if shed {
        let flight = at + heading.rotate(Vec2::from_angle(rng.range(-0.3, 0.3))) * finger.reach * rng.range(0.2, 1.8);
        marks.push(dot(flight, finger.volume * in_drop));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    const WATER: Fluid = Fluid { viscosity: 1e-3, yield_stress: 0.0 };

    fn splash(tail: Vec2, lean: f32) -> Splash {
        Splash { centre: Vec2::new(500.0, 500.0), tail, rim: 20.0, lean, count: 16, vigour: 1.0, volume: 300.0, paint: Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium: 0.8 }
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
        let reach = |marks: &[Mark]| marks.iter().map(|mark| mark.to.x - 500.0).fold(0.0f32, f32::max) / -marks.iter().map(|mark| mark.to.x - 500.0).fold(0.0f32, f32::min);
        let mean = |tail: Vec2, lean: f32| (0..32).map(|seed| reach(&fingers(&splash(tail, lean), &mut SplitMix64::new(seed)))).sum::<f32>() / 32.0;
        let (straight, slanted) = (mean(Vec2::ZERO, 0.0), mean(Vec2::new(30.0, 0.0), 1.0));
        assert!((0.7..1.4).contains(&straight), "a straight impact splashes evenly, downwind/upwind reach {straight:.2}");
        assert!(slanted > 2.0, "a slanted impact throws its fingers downwind, downwind/upwind reach {slanted:.2}");
    }
}
