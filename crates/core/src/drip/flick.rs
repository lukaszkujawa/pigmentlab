use super::flight::Droplet;
use super::fluid::Fluid;
use crate::pigment::Coefficients;
use crate::rng::SplitMix64;
use glam::{Vec2, Vec3};
use std::f32::consts::PI;

const SLOWEST_FLICK: f32 = 0.3;
const FULL_FLICK: f32 = 2.0;
const CONTACT_TIME: f32 = 0.0025;
pub(super) const BRISTLE_GAP: f32 = 140e-6;
const MEAN_DROP: f32 = 0.0005;
const SIZE_SPREAD: u32 = 4;
const SMALLEST_DROP: f32 = 60e-6;
const FAN: f32 = 0.2;

pub fn shed_share(tip_speed: f32, fluid: Fluid) -> f32 {
    let t = ((tip_speed - SLOWEST_FLICK) / (FULL_FLICK - SLOWEST_FLICK)).clamp(0.0, 1.0);
    let whip = t * t * (3.0 - 2.0 * t);
    let viscous_hold = fluid.kinematic_viscosity() * CONTACT_TIME / (BRISTLE_GAP * BRISTLE_GAP);
    whip / (1.0 + viscous_hold)
}

pub struct Spatter {
    pub tip: Vec3,
    pub velocity: Vec2,
    pub volume: f32,
    pub paint: Coefficients,
    pub medium: f32,
}

pub fn spatter(flick: &Spatter, rng: &mut SplitMix64) -> Vec<Droplet> {
    let fluid = Fluid::of(flick.medium);
    let mut left = flick.volume;
    let mut droplets = Vec::new();
    while left > 0.0 {
        let gamma: f32 = (0..SIZE_SPREAD).map(|_| -rng.unit().max(1e-6).ln()).sum::<f32>() / SIZE_SPREAD as f32;
        let wanted = (MEAN_DROP * gamma).max(SMALLEST_DROP);
        let volume = (PI * wanted.powi(3) / 6.0).min(left);
        left -= volume;
        let diameter = (6.0 * volume / PI).cbrt();
        let launch = Vec2::from_angle(rng.range(-FAN, FAN)).rotate(flick.velocity) * rng.range(0.6, 1.3);
        droplets.push(Droplet {
            position: flick.tip + Vec3::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), 0.0) * 0.003,
            velocity: launch.extend(rng.range(-0.5, 0.3)),
            diameter,
            paint: flick.paint,
            medium: flick.medium,
            fluid,
        });
    }
    droplets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fast_flick_sheds_runny_paint_while_paste_and_slow_lifts_keep_theirs() {
        let runny = Fluid::of(1.0);
        assert_eq!(shed_share(0.2, runny), 0.0, "an ordinary lift should throw nothing");
        assert!(shed_share(1.0, runny) < shed_share(2.0, runny), "a harder flick throws more");
        assert!(shed_share(2.0, runny) > 0.1, "a hard flick of runny paint throws a good share of the load, {}", shed_share(2.0, runny));
        assert!(shed_share(2.0, Fluid::of(0.0)) < 0.001, "stiff paste stays in the brush, {}", shed_share(2.0, Fluid::of(0.0)));
    }

    #[test]
    fn spatter_keeps_its_volume_and_scatters_drops_of_many_sizes_along_the_flick() {
        let flick = Spatter { tip: Vec3::new(0.1, 0.1, 0.01), velocity: Vec2::new(2.0, 0.0), volume: 2e-8, paint: Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium: 1.0 };
        let droplets = spatter(&flick, &mut SplitMix64::new(4));
        let total: f32 = droplets.iter().map(Droplet::volume).sum();
        assert!((total - flick.volume).abs() < flick.volume * 1e-3);
        let sizes: Vec<f32> = droplets.iter().map(|droplet| droplet.diameter).collect();
        let (small, large) = sizes.iter().fold((f32::MAX, 0.0f32), |(low, high), d| (low.min(*d), high.max(*d)));
        assert!(droplets.len() > 50 && large > 3.0 * small, "a flick should throw many drops of varied size: {} drops, {small:e}..{large:e} m", droplets.len());
        assert!(droplets.iter().all(|droplet| droplet.velocity.x > 0.5), "every drop should fly the way the brush was flicked");
    }
}
