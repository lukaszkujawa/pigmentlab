use super::fluid::{DENSITY, Fluid, SURFACE_TENSION};
use crate::rng::SplitMix64;
use crate::units::GRAVITY;
use glam::Vec2;
use std::f32::consts::{FRAC_PI_2, PI};

const LANDING_FIT: f32 = 3.04;
const BREAKUP_LOGARITHM: f32 = 11.0;
const DROP_PER_JET_DIAMETER: f32 = 1.89;
const TATE_CORRECTION: f32 = 0.6;
const DRIPPING_WEBER: f32 = 4.0;
const STEPS_PER_COIL_RADIUS: f32 = 4.0;
const MOST_STEPS: f32 = 96.0;
const FARTHEST_REACH: f32 = 3.0;
const CURVATURE_SHAPE: f32 = 0.715;
const HEADING_WOBBLE: f32 = 0.01;
const MEANDERS_FADE_FROM: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thread {
    pub exit_speed: f32,
    pub landing_speed: f32,
    pub radius: f32,
    pub coil_radius: f32,
    pub fall_time: f32,
    pub breakup_time: f32,
    pub drop_diameter: f32,
    pub bead_diameter: Option<f32>,
}

pub fn falling_thread(flow: f32, height: f32, hole_radius: f32, fluid: Fluid) -> Thread {
    let exit_speed = flow / (PI * hole_radius * hole_radius);
    let free_fall = (exit_speed * exit_speed + 2.0 * GRAVITY * height).sqrt();
    let nu = fluid.kinematic_viscosity();
    let viscous_length = (nu * nu / GRAVITY).cbrt();
    let scaled_flow = flow * (GRAVITY / nu.powi(5)).cbrt();
    let scaled_height = height / viscous_length;
    let viscous_landing = LANDING_FIT * scaled_flow.sqrt() * scaled_height.powf(0.75) * (GRAVITY * nu).cbrt();
    let landing_speed = viscous_landing.clamp(exit_speed, free_fall);
    let radius = (flow / (PI * landing_speed)).sqrt();
    let viscous_coiling = flow / (height * radius * radius);
    let gravitational_coiling = (GRAVITY * flow.powi(3) / (nu * radius.powi(8))).powf(0.25);
    let coil_radius = landing_speed / viscous_coiling.max(gravitational_coiling);
    let fall_time = (free_fall - exit_speed) / GRAVITY;
    let breakup_time = BREAKUP_LOGARITHM * ((DENSITY * radius.powi(3) / SURFACE_TENSION).sqrt() + 3.0 * fluid.viscosity * radius / SURFACE_TENSION);
    let dripping = DENSITY * exit_speed * exit_speed * 2.0 * hole_radius / SURFACE_TENSION < DRIPPING_WEBER;
    let drop_diameter = match dripping {
        true => (6.0 * TATE_CORRECTION * 2.0 * hole_radius * SURFACE_TENSION / (DENSITY * GRAVITY)).cbrt(),
        false => DROP_PER_JET_DIAMETER * 2.0 * radius,
    };
    let bead_diameter = (fall_time > breakup_time).then_some(drop_diameter);
    Thread { exit_speed, landing_speed, radius, coil_radius, fall_time, breakup_time, drop_diameter, bead_diameter }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Laying {
    offset: Vec2,
    heading: f32,
    wobble: SplitMix64,
    last_under: Option<Vec2>,
}

impl Laying {
    pub fn coiling(coil_radius: f32, seed: u64) -> Self {
        Self { offset: Vec2::new(coil_radius, 0.0), heading: FRAC_PI_2, wobble: SplitMix64::new(seed), last_under: None }
    }

    pub fn advance(&mut self, under_nozzle: Vec2, nozzle_velocity: Vec2, thread: &Thread, dt: f32) -> Vec<Vec2> {
        let laying_speed = thread.landing_speed.max(nozzle_velocity.length());
        let laid = laying_speed * dt;
        let steps = (laid / thread.coil_radius * STEPS_PER_COIL_RADIUS).ceil().clamp(1.0, MOST_STEPS);
        let step = laid / steps / thread.coil_radius;
        let belt = -nozzle_velocity / laying_speed;
        let ratio = nozzle_velocity.length() / thread.landing_speed;
        let t = ((ratio - MEANDERS_FADE_FROM) / (1.0 - MEANDERS_FADE_FROM)).clamp(0.0, 1.0);
        let straightness = t * t * (3.0 - 2.0 * t);
        let trailing = belt.normalize_or_zero() * thread.coil_radius;
        let from = self.last_under.replace(under_nozzle).unwrap_or(under_nozzle - nozzle_velocity * dt);
        (1..=steps as usize)
            .map(|k| {
                let reach = self.offset / thread.coil_radius;
                self.heading += curvature(reach, self.heading) * step + self.wobble.range(-1.0, 1.0) * HEADING_WOBBLE * step.sqrt();
                let moved = (Vec2::from_angle(self.heading) + belt) * step;
                self.offset = ((reach + moved).clamp_length_max(FARTHEST_REACH)) * thread.coil_radius;
                from.lerp(under_nozzle, k as f32 / steps) + self.offset.lerp(trailing, straightness)
            })
            .collect()
    }
}

fn curvature(reach: Vec2, heading: f32) -> f32 {
    let distance = reach.length();
    if distance < 1e-6 {
        return 0.0;
    }
    let lean = heading - reach.y.atan2(reach.x);
    let bend = CURVATURE_SHAPE * CURVATURE_SHAPE * lean.cos() / (1.0 - CURVATURE_SHAPE * lean.cos());
    distance.sqrt() * (1.0 + bend * distance) * lean.sin()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 240.0;

    fn pollock() -> Thread {
        falling_thread(2.0e-7, 0.15, 0.001, Fluid { viscosity: 5.0, yield_stress: 0.0 })
    }

    fn lay(nozzle_speed: f32, seconds: f32) -> Vec<Vec2> {
        let thread = pollock();
        let mut laying = Laying::coiling(thread.coil_radius, 5);
        let velocity = Vec2::new(nozzle_speed, 0.0);
        (0..(seconds / DT) as usize).flat_map(|step| laying.advance(velocity * step as f32 * DT, velocity, &thread, DT)).collect()
    }

    fn sideways_spread(points: &[Vec2]) -> f32 {
        let late = &points[points.len() / 2..];
        late.iter().map(|p| p.y).fold(f32::MIN, f32::max) - late.iter().map(|p| p.y).fold(f32::MAX, f32::min)
    }

    #[test]
    fn pollocks_enamel_lands_at_the_measured_critical_speed_with_millimetre_coils() {
        let thread = pollock();
        assert!((thread.landing_speed - 0.36).abs() < 0.03, "Palacios et al. measure about 0.36 m/s, got {:.3}", thread.landing_speed);
        assert!((0.0002..0.002).contains(&thread.coil_radius), "coils of 0.5-2 mm are seen in the paintings, got {:.2} mm", thread.coil_radius * 1000.0);
        assert!(thread.bead_diameter.is_none(), "enamel poured from 15 cm should stay one continuous thread");
    }

    #[test]
    fn thin_paint_breaks_into_beads_and_a_slow_watery_trickle_drips() {
        let thinned = falling_thread(2.0e-6, 0.15, 0.001, Fluid { viscosity: 0.05, yield_stress: 0.0 });
        let diameter = thinned.bead_diameter.expect("thinned paint should break up before it lands");
        assert!((diameter - DROP_PER_JET_DIAMETER * 2.0 * thinned.radius).abs() < 1e-6);
        let trickle = falling_thread(2.0e-8, 0.15, 0.0015, Fluid { viscosity: 0.01, yield_stress: 0.0 });
        let drop = trickle.bead_diameter.expect("a slow trickle of runny paint should drip");
        assert!((0.0025..0.0045).contains(&drop), "Tate's law gives drops of about 3.3 mm from a 1.5 mm hole, got {:.2} mm", drop * 1000.0);
    }

    #[test]
    fn a_resting_nozzle_coils_and_the_pattern_straightens_as_the_nozzle_speeds_up() {
        let thread = pollock();
        let still = lay(0.0, 1.0);
        let radii: Vec<f32> = still[still.len() / 2..].iter().map(|p| p.length() / thread.coil_radius).collect();
        assert!(radii.iter().all(|r| (r - 1.0).abs() < 0.1), "a still nozzle should lay steady coils of the coil radius: {:?}", &radii[..8]);
        let speeds = [0.1, 0.25, 0.6, 1.2].map(|ratio| ratio * thread.landing_speed);
        let spreads = speeds.map(|speed| sideways_spread(&lay(speed, 1.0)) / thread.coil_radius);
        assert!(spreads[0] > 1.5, "slowly translated coils should sweep about two coil radii sideways: {spreads:?}");
        assert!(spreads[3] < 0.25, "above the landing speed the thread should lay a straight line: {spreads:?}");
        assert!(spreads[2] < spreads[0], "meanders should be narrower than loops: {spreads:?}");
    }

    #[test]
    fn the_laid_thread_moves_over_the_canvas_at_the_landing_speed_while_coiling() {
        let thread = pollock();
        let points = lay(0.3 * thread.landing_speed, 1.0);
        let length: f32 = points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
        assert!((length - thread.landing_speed).abs() < thread.landing_speed * 0.05, "a second of coiling lays {length:.3} m of thread, landing speed {:.3}", thread.landing_speed);
    }
}
