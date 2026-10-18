mod bottle;
mod collide;
mod flick;
mod flight;
mod fluid;
mod outflow;
mod pendulum;
mod rig;
mod thread;

pub use bottle::Bottle;
pub use flick::{Spatter, shed_share};
pub use flight::Droplet;
pub use fluid::Fluid;
pub use rig::{Hanging, Rig, Setting};

use crate::paint::Mark;
use crate::pigment::Coefficients;
use crate::rng::SplitMix64;
use crate::units::{METRES_PER_PIXEL, metres};
use bottle::{CLEARANCE, Filling, SHORTEST_STRING};
use glam::Vec2;
use pendulum::Pendulum;

const SAME_HOOK: f32 = 1.0;
const HARDEST_THROW: f32 = 1.5;

pub struct Drips {
    bottles: Vec<Bottle>,
    droplets: Vec<Droplet>,
    held: Option<u32>,
    selected: Option<u32>,
    paused: bool,
    next_id: u32,
    rng: SplitMix64,
}

impl Drips {
    pub fn new(seed: u64) -> Self {
        Self { bottles: Vec::new(), droplets: Vec::new(), held: None, selected: None, paused: false, next_id: 1, rng: SplitMix64::new(seed ^ 0xd21b_0771e) }
    }

    pub fn hang(&mut self, rig: Rig, hanging: Hanging, paint: Coefficients, medium: f32) -> u32 {
        let pivot_height = metres(rig.pivot_height).max(SHORTEST_STRING + CLEARANCE);
        let length = metres(rig.string_length).clamp(SHORTEST_STRING, pivot_height - CLEARANCE);
        let pendulum = Pendulum::released((hanging.pivot * METRES_PER_PIXEL).extend(pivot_height), length, hanging.release * METRES_PER_PIXEL, hanging.push * METRES_PER_PIXEL);
        let filling = Filling { paint, medium, hole_radius: metres(rig.hole_radius), radius: metres(rig.bottle_radius), pouring: rig.pour_at_once };
        let id = self.next_id;
        self.next_id += 1;
        let seed = self.rng.next_u64();
        self.bottles.push(Bottle::filled(pendulum, filling, seed, id));
        self.selected = Some(id);
        id
    }

    pub fn flick(&mut self, spatter: &Spatter) {
        self.droplets.extend(flick::spatter(spatter, &mut self.rng));
    }

    pub fn cut_strings(&mut self) {
        self.bottles.clear();
        self.held = None;
        self.selected = None;
    }

    pub fn remove(&mut self, id: u32) {
        self.bottles.retain(|bottle| bottle.id != id);
        self.held = self.held.filter(|held| *held != id);
        self.selected = self.selected.filter(|selected| *selected != id);
    }

    pub fn select(&mut self, id: Option<u32>) {
        self.selected = id.filter(|id| self.bottle(*id).is_some());
    }

    pub fn selected(&self) -> Option<u32> {
        self.selected
    }

    pub fn bottle(&self, id: u32) -> Option<&Bottle> {
        self.bottles.iter().find(|bottle| bottle.id == id)
    }

    fn bottle_mut(&mut self, id: Option<u32>) -> Option<&mut Bottle> {
        self.bottles.iter_mut().find(|bottle| Some(bottle.id) == id)
    }

    pub fn adjust(&mut self, id: u32, setting: Setting) {
        let Some(bottle) = self.bottle_mut(Some(id)) else { return };
        match setting {
            Setting::String(length) => bottle.set_string(metres(length)),
            Setting::PivotHeight(height) => bottle.set_pivot_height(metres(height)),
            Setting::Hole(radius) => bottle.hole_radius = metres(radius),
            Setting::Radius(radius) => bottle.resize(metres(radius)),
        }
    }

    pub fn stop(&mut self, id: Option<u32>) {
        for bottle in self.bottles.iter_mut().filter(|bottle| id.is_none_or(|id| bottle.id == id)) {
            bottle.stop();
        }
    }

    pub fn set_pouring(&mut self, id: Option<u32>, pouring: bool) {
        for bottle in self.bottles.iter_mut().filter(|bottle| id.is_none_or(|id| bottle.id == id)) {
            bottle.pouring = pouring;
        }
    }

    pub fn move_hook(&mut self, from: Vec2, to: Vec2) {
        for bottle in self.bottles.iter_mut().filter(|bottle| (bottle.pendulum.pivot.truncate() / METRES_PER_PIXEL).distance(from) < SAME_HOOK) {
            bottle.pendulum.pivot = (to * METRES_PER_PIXEL).extend(bottle.pendulum.pivot.z);
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn grab(&mut self, at: Vec2) -> bool {
        let point = at * METRES_PER_PIXEL;
        self.held = self.bottles.iter().rev().find(|bottle| bottle.covers(point)).map(|bottle| bottle.id);
        if self.held.is_some() {
            self.selected = self.held;
        }
        self.held.is_some()
    }

    pub fn take_hold(&mut self, id: u32) {
        self.held = self.bottle(id).map(|bottle| bottle.id);
        self.selected = self.held.or(self.selected);
    }

    pub fn hold(&mut self, at: Vec2) {
        if let Some(bottle) = self.bottle_mut(self.held) {
            bottle.held_over = Some(at * METRES_PER_PIXEL);
        }
    }

    pub fn let_go(&mut self, throw: f32) {
        let held = self.held.take();
        if let Some(bottle) = self.bottle_mut(held) {
            bottle.held_over = None;
            bottle.pendulum.velocity = (bottle.pendulum.velocity * throw).clamp_length_max(HARDEST_THROW);
        }
    }

    pub fn bottles(&self) -> &[Bottle] {
        &self.bottles
    }

    pub fn droplets(&self) -> &[Droplet] {
        &self.droplets
    }

    pub fn is_moving(&self) -> bool {
        !self.paused && (!self.bottles.is_empty() || !self.droplets.is_empty())
    }

    pub fn step(&mut self, dt: f32) -> Vec<Mark> {
        let mut marks = Vec::new();
        if self.paused {
            return marks;
        }
        for bottle in &mut self.bottles {
            let released = bottle.step(dt);
            marks.extend(released.marks);
            self.droplets.extend(released.droplets);
        }
        collide::collide(&mut self.bottles);
        for droplet in &mut self.droplets {
            droplet.fly(dt);
        }
        let (landed, flying): (Vec<Droplet>, Vec<Droplet>) = self.droplets.drain(..).partition(Droplet::landed);
        self.droplets = flying;
        for droplet in landed {
            marks.extend(droplet.impact(&mut self.rng));
        }
        marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::cm;
    use glam::Vec3;

    fn relative_motion_after_a_minute(second_string: f32) -> f32 {
        let mut drips = Drips::new(7);
        let paint = Coefficients { absorption: Vec3::ONE, scattering: Vec3::ONE };
        let rig = Rig { bottle_radius: cm(4.0), ..Rig::default() };
        drips.hang(rig, Hanging { pivot: Vec2::ZERO, release: Vec2::new(800.0, 0.0), push: Vec2::new(0.0, 3000.0) }, paint, 0.35);
        drips.hang(Rig { string_length: second_string, ..rig }, Hanging { pivot: Vec2::ZERO, release: Vec2::new(-300.0, 500.0), push: Vec2::new(2500.0, -800.0) }, paint, 0.35);
        let (mut apart, mut speed) = (0.0f32, 0.0f32);
        for step in 0..240 * 60 {
            drips.step(1.0 / 240.0);
            if step >= 240 * 20 {
                let [a, b] = [&drips.bottles[0].pendulum, &drips.bottles[1].pendulum];
                apart += (a.velocity - b.velocity).length();
                speed += 0.5 * (a.velocity.length() + b.velocity.length());
            }
        }
        apart / speed
    }

    #[test]
    fn big_bottles_on_equal_strings_from_one_hook_knock_together_until_they_swing_as_a_pair() {
        let relative = relative_motion_after_a_minute(Rig::default().string_length);
        assert!(relative < 0.2, "bottles of one length on one hook collide every half swing and soon move together, relative motion {relative:.2}");
    }

    #[test]
    fn strings_of_different_length_keep_their_bottles_swinging_on_their_own() {
        let relative = relative_motion_after_a_minute(cm(180.0));
        assert!(relative > 0.8, "a shorter string hangs its bottle higher and on its own rhythm, relative motion {relative:.2}");
    }
}
