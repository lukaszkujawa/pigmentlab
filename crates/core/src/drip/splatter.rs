use super::drain::Slug;
use super::jet::Node;
use crate::pigment::Coefficients;
use crate::rng::SplitMix64;
use crate::units::GRAVITY;
use glam::{Vec2, Vec3};
use std::f32::consts::PI;

const SEGMENTS: usize = 16;
const LIGAMENTS: usize = 8;
const SMALLEST_SHARE: f32 = 0.1;
const FAN: f32 = 0.06;
const SLOWEST: f32 = 0.7;
const FASTEST: f32 = 1.15;
const SLOWEST_PAINT: f32 = SLOWEST * (1.0 - STRETCH);
const LENGTH_PER_DIAMETER: f32 = 4.0;
const STRETCH: f32 = 0.3;
const SCATTER: f32 = 0.04;
const LOWEST: f32 = 0.01;
const RELEASE_TIME: f32 = 0.04;
const FASTEST_PAINT: f32 = FASTEST * (1.0 + STRETCH);
const SHALLOWEST: f32 = 0.05;
const STEEPEST: f32 = 1.45;
const ANGLE_SEARCH: usize = 24;
const LONGEST_LIGAMENT: f32 = 0.06;
const HIGHEST_HEAD: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Throw {
    pub at: Vec2,
    pub toward: Vec2,
    pub height: f32,
    pub speed: f32,
    pub volume: f32,
    pub paint: Coefficients,
    pub medium: f32,
}

impl Throw {
    pub fn release(&self) -> (Vec3, Vec3) {
        let angle = self.angle();
        (self.release_point(angle), self.velocity(angle))
    }

    pub fn reach(&self) -> [Vec2; 2] {
        let angle = self.angle();
        [self.landing(angle, SLOWEST_PAINT, 0.0), self.landing(angle, FASTEST_PAINT, self.ligament_length(angle))]
    }

    pub fn angle(&self) -> f32 {
        let wanted = self.at.distance(self.toward);
        let spread = |angle: f32| self.landing(angle, FASTEST_PAINT, self.ligament_length(angle)).distance(self.at);
        let (mut shallow, mut steep) = (SHALLOWEST, STEEPEST);
        for _ in 0..ANGLE_SEARCH {
            let middle = 0.5 * (shallow + steep);
            match spread(middle) > wanted {
                true => shallow = middle,
                false => steep = middle,
            }
        }
        0.5 * (shallow + steep)
    }

    fn heading(&self) -> Vec2 {
        (self.toward - self.at).normalize_or(Vec2::X)
    }

    fn velocity(&self, angle: f32) -> Vec3 {
        (self.heading() * self.speed * angle.cos()).extend(-self.speed * angle.sin())
    }

    fn release_point(&self, angle: f32) -> Vec3 {
        let height = self.height.max(LOWEST);
        let slowest = self.velocity(angle) * SLOWEST_PAINT;
        (self.at - slowest.truncate() * fall_time(-slowest.z, height)).extend(height)
    }

    fn ligament_length(&self, angle: f32) -> f32 {
        let drop = HIGHEST_HEAD * self.height.max(LOWEST) / angle.sin().max(f32::EPSILON);
        (self.speed * RELEASE_TIME).min(LONGEST_LIGAMENT).min(drop)
    }

    fn landing(&self, angle: f32, scale: f32, ahead: f32) -> Vec2 {
        let velocity = self.velocity(angle);
        let position = self.release_point(angle) + velocity.normalize_or(Vec3::X) * ahead;
        let velocity = velocity * scale;
        position.truncate() + velocity.truncate() * fall_time(-velocity.z, position.z)
    }

    pub fn slugs(&self, rng: &mut SplitMix64) -> Vec<Slug> {
        let angle = self.angle();
        let (centre, velocity) = (self.release_point(angle), self.velocity(angle));
        let longest = self.ligament_length(angle);
        let shares: Vec<f32> = (0..LIGAMENTS).map(|_| rng.unit().powi(2) + SMALLEST_SHARE).collect();
        let total: f32 = shares.iter().sum();
        shares
            .iter()
            .map(|share| {
                let turn = glam::Quat::from_rotation_z(rng.range(-FAN, FAN));
                let velocity = turn * velocity * rng.range(SLOWEST, FASTEST);
                self.ligament(centre, velocity, longest, self.volume * share / total, rng)
            })
            .collect()
    }

    fn ligament(&self, centre: Vec3, velocity: Vec3, longest: f32, volume: f32, rng: &mut SplitMix64) -> Slug {
        let heading = velocity.normalize_or(Vec3::X);
        let speed = velocity.length();
        let length = (LENGTH_PER_DIAMETER * (6.0 * volume / PI).cbrt()).max(longest * rng.range(0.5, 1.0));
        let nodes = (0..=SEGMENTS)
            .map(|node| {
                let along = node as f32 / SEGMENTS as f32;
                let scatter = Vec3::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)) * SCATTER * speed;
                Node { position: (centre + heading * along * length).as_dvec3(), velocity: (velocity * (1.0 + 2.0 * STRETCH * (along - 0.5)) + scatter).as_dvec3() }
            })
            .collect::<Vec<_>>();
        Slug { nodes, volumes: vec![f64::from(volume) / SEGMENTS as f64; SEGMENTS] }
    }
}

fn fall_time(down: f32, height: f32) -> f32 {
    ((down * down + 2.0 * GRAVITY * height).sqrt() - down) / GRAVITY
}

#[cfg(test)]
mod tests {
    use super::*;

    fn throw(toward: Vec2, height: f32, speed: f32) -> Throw {
        Throw { at: Vec2::ZERO, toward, height, speed, volume: 2e-7, paint: Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE }, medium: 0.5 }
    }

    fn landing(throw: &Throw, scale: f32) -> Vec2 {
        let (mut position, velocity) = throw.release();
        let mut velocity = velocity * scale;
        while position.z > 0.0 {
            velocity.z -= GRAVITY * 1e-4;
            position += velocity * 1e-4;
        }
        position.truncate()
    }

    #[test]
    fn however_hard_the_throw_the_paint_lands_between_the_arrow_start_and_tip() {
        for height in [0.1, 0.3, 1.0] {
            for speed in [1.0, 4.0, 9.0] {
                let throw = throw(Vec2::new(0.06, 0.08), height, speed);
                let far = |angle: f32| throw.landing(angle, FASTEST_PAINT, throw.ligament_length(angle)).length();
                let reachable = far(SHALLOWEST) > 0.1 && far(STEEPEST) < 0.1;
                assert!(landing(&throw, SLOWEST_PAINT).length() < 0.002, "{speed} m/s from {height} m: the slowest paint should land at the arrow start, {:?}", landing(&throw, SLOWEST_PAINT));
                if reachable {
                    assert!((throw.reach()[1] - throw.toward).length() < 0.003, "{speed} m/s from {height} m: the farthest paint should land at the arrow tip, {:?}", throw.reach()[1]);
                }
            }
        }
    }

    #[test]
    fn a_harder_throw_into_the_same_arrow_hits_the_canvas_harder() {
        let impact = |speed: f32| {
            let throw = throw(Vec2::new(0.1, 0.0), 0.3, speed);
            let (position, velocity) = throw.release();
            (velocity.length_squared() + 2.0 * GRAVITY * position.z).sqrt()
        };
        let speeds = [1.0, 3.0, 9.0].map(impact);
        assert!(speeds[0] < speeds[1] && speeds[1] < speeds[2], "harder throws must arrive faster: {speeds:?}");
    }

    #[test]
    fn the_ligaments_carry_the_thrown_paint_and_stretch_along_the_throw() {
        let throw = throw(Vec2::new(0.0, 0.1), 0.3, 3.0);
        let slugs = throw.slugs(&mut SplitMix64::new(3));
        let volume: f64 = slugs.iter().flat_map(|slug| &slug.volumes).sum();
        assert!((volume - f64::from(throw.volume)).abs() < 1e-12);
        for slug in &slugs {
            let (back, front) = (slug.nodes[0], slug.nodes[SEGMENTS]);
            assert!(front.position.y > back.position.y && front.velocity.y > back.velocity.y, "each ligament should fly front first along the throw");
        }
    }
}
