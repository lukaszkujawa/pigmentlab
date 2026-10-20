use super::{Engine, SIMULATION_DT, Tool};
use crate::film::FILM_CHANGE_CAPACITY;
use crate::paint::OVERLOADED_FILL;
use crate::drip::{Drips, Fluid, Hanging, Rig, Setting, Spatter, Tap, Throw, Tuft, drain_flow, shed_share};
use crate::input::PointerState;
use crate::units::{METRES_PER_PIXEL, cm, metres};
use glam::{Vec2, Vec3};
use std::f32::consts::PI;

const FLICK_HEIGHT: f32 = 0.01;
const HOOK_REACH: f32 = 24.0;
const SPLATTER_VOLUME: f32 = 4e-7;
const TIP_SHARE_OF_HALF_WIDTH: f32 = 0.25;
const SMALLEST_TIP: f32 = 0.0005;
const LARGEST_TIP: f32 = 0.003;

impl Engine {
    pub fn hang_bottle(&mut self, hanging: Hanging) {
        let mut encoder = self.encoder("hang bottle");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.snapshot(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.drips.hang(Rig { pour_at_once: true, ..self.rig }, hanging, self.paint_on_tool.coefficients, self.carried_medium());
    }

    pub(super) fn set_rig_param(&mut self, name: &str, value: f32) -> bool {
        match name {
            "string" => self.rig.string_length = cm(value),
            "pivot" => self.rig.pivot_height = cm(value),
            "hole" => self.rig.hole_radius = cm(value * 0.05),
            "radius" => self.rig.bottle_radius = cm(value),
            "throw" => self.rig.throw = value.max(0.0),
            "pour" => self.rig.pour_at_once = value > 0.5,
            "hook_x" => self.rig.hook.x = value,
            "hook_y" => self.rig.hook.y = value,
            "splatter_height" => self.splatter_height = cm(value),
            "splatter_force" => self.splatter_force = value.max(0.1),
            _ => return false,
        }
        true
    }

    pub(super) fn steer_bottles(&mut self, pointer: &PointerState) {
        let at = Vec2::new(pointer.x, pointer.y);
        if pointer.fresh_press {
            self.holding_hook = at.distance(self.rig.hook) <= HOOK_REACH / self.stage_view.map_or(1.0, |view| view.scale);
            if !self.holding_hook && !self.drips.grab(at) {
                let id = self.drips.hang(self.rig, Hanging { pivot: self.rig.hook, release: at, push: Vec2::ZERO }, self.paint_on_tool.coefficients, self.carried_medium());
                self.drips.take_hold(id);
            }
        }
        if self.holding_hook {
            self.drips.move_hook(self.rig.hook, at);
            self.rig.hook = at;
            self.holding_hook = pointer.pressed;
            return;
        }
        match pointer.pressed {
            true => self.drips.hold(at),
            false => self.drips.let_go(self.rig.throw),
        }
    }

    pub(super) fn aim_splatter(&mut self, pointer: &PointerState) {
        let at = Vec2::new(pointer.x, pointer.y);
        match (pointer.pressed, self.aim) {
            (true, aim) if pointer.fresh_press || aim.is_none() => self.aim = Some([at, at]),
            (true, Some([from, _])) => self.aim = Some([from, at]),
            (false, Some([from, aim])) => {
                self.aim = None;
                self.splatter(from, aim);
            }
            _ => {}
        }
        self.needs_render = true;
    }

    pub fn splatter(&mut self, from: Vec2, aim: Vec2) {
        if self.paint.load > 0.0 {
            let throw = self.throw(from, aim);
            self.drips.splatter(&throw);
        }
    }

    pub(super) fn splatter_reach(&self) -> Option<[Vec2; 2]> {
        self.aim.map(|[from, aim]| self.throw(from, aim).reach().map(|point| point / METRES_PER_PIXEL))
    }

    fn throw(&self, from: Vec2, aim: Vec2) -> Throw {
        let volume = SPLATTER_VOLUME * self.paint.load.clamp(0.0, OVERLOADED_FILL);
        Throw { at: from * METRES_PER_PIXEL, toward: aim * METRES_PER_PIXEL, height: metres(self.splatter_height), speed: self.splatter_force, volume, paint: self.paint_on_tool.coefficients, medium: self.carried_medium() }
    }

    pub fn aim(&self) -> Option<[Vec2; 2]> {
        self.aim
    }

    pub fn splatter_height(&self) -> f32 {
        self.splatter_height
    }

    pub(super) fn flick_brush(&mut self) {
        let velocity = self.touch.velocity() * METRES_PER_PIXEL;
        let medium = self.carried_medium();
        let share = shed_share(velocity.length(), Fluid::of(medium));
        if share <= 0.0 || self.watercolour.is_some() {
            return;
        }
        self.settle_drained();
        let paint = self.brush_pigment();
        let thrown = self.brush_load() * share;
        if thrown <= 0.0 {
            return;
        }
        self.keep_brush_load(1.0 - share);
        let tip = (self.touch.position * METRES_PER_PIXEL).extend(FLICK_HEIGHT);
        self.drips.flick(&Spatter { tip, velocity, volume: thrown * METRES_PER_PIXEL.powi(3), paint, medium });
    }

    pub(super) fn drain_brush(&mut self) {
        if self.tool != Tool::Brush || self.watercolour.is_some() {
            self.drips.tap_brush(None);
            return;
        }
        let preset = *self.brush.preset();
        let capacity = self.brush.paint_capacity();
        let touching = self.contact() > 0.0;
        let held = match touching {
            true => {
                self.lifted_load = None;
                0.0
            }
            false => match self.lifted_load {
                Some(load) => load,
                None => *self.lifted_load.insert(self.brush_load() + self.drained),
            },
        };
        let cubic = METRES_PER_PIXEL.powi(3);
        let along = self.pose.orientation * Vec3::Z;
        let tuft = Tuft { capacity: capacity * cubic, cross_section: PI * metres(preset.width) * metres(preset.thickness) * 0.25, droop: along.z };
        let medium = self.carried_medium();
        let flow = if touching { 0.0 } else { drain_flow((held - self.drained).max(0.0) * cubic, &tuft, Fluid::of(medium)) };
        self.drained += flow * SIMULATION_DT / cubic;
        let tip = (self.pose.position - along * preset.length) * METRES_PER_PIXEL;
        let tip_radius = (metres(preset.width.min(preset.thickness)) * 0.5 * TIP_SHARE_OF_HALF_WIDTH).clamp(SMALLEST_TIP, LARGEST_TIP);
        self.drips.tap_brush(Some(Tap { tip, tip_radius, flow, paint: self.paint_on_tool.coefficients, medium }));
        if touching {
            self.settle_drained();
        }
    }

    pub fn select_bottle(&mut self, id: Option<u32>) {
        self.drips.select(id);
        self.needs_render = true;
    }

    pub fn set_bottle_param(&mut self, id: u32, name: &str, value: f32) -> bool {
        let setting = match name {
            "string" => Setting::String(cm(value)),
            "pivot" => Setting::PivotHeight(cm(value)),
            "hole" => Setting::Hole(cm(value * 0.05)),
            "radius" => Setting::Radius(cm(value)),
            _ => return false,
        };
        self.drips.adjust(id, setting);
        self.needs_render = true;
        true
    }

    pub fn stop_bottles(&mut self, id: Option<u32>) {
        self.drips.stop(id);
        self.needs_render = true;
    }

    pub fn remove_bottle(&mut self, id: u32) {
        self.drips.remove(id);
        self.needs_render = true;
    }

    pub fn set_pouring(&mut self, id: Option<u32>, pouring: bool) {
        self.drips.set_pouring(id, pouring);
    }

    pub fn set_drips_paused(&mut self, paused: bool) {
        self.drips.set_paused(paused);
    }

    pub fn cut_strings(&mut self) {
        self.drips.cut_strings();
    }

    pub fn rig(&self) -> Rig {
        self.rig
    }

    pub fn drips(&self) -> &Drips {
        &self.drips
    }

    pub(super) fn step_drips(&mut self) {
        if !self.drips.is_moving() {
            return;
        }
        let marks = self.drips.step(SIMULATION_DT);
        if self.watercolour.is_none() {
            self.film.lay(&marks);
        }
        self.needs_render = true;
    }

    pub(super) fn settle_film(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.drips.paused() || !self.substeps.is_multiple_of(super::SUBSTEPS_PER_FILM_STEP) {
            return;
        }
        let Some(changes) = self.film.settle(SIMULATION_DT * super::SUBSTEPS_PER_FILM_STEP as f32, FILM_CHANGE_CAPACITY, !self.realtime) else { return };
        if let Some(bounds) = self.film_deltas.encode(&self.gpu, encoder, &changes) {
            self.mark_stale(bounds.expand(1));
        }
    }

    pub fn film_unsettled(&mut self) -> f32 {
        self.film.unsettled()
    }
}
