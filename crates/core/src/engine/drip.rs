use super::{Engine, SIMULATION_DT};
use crate::drip::{Drips, Fluid, Hanging, Rig, Setting, Spatter, shed_share};
use crate::input::PointerState;
use crate::units::{METRES_PER_PIXEL, cm};
use glam::Vec2;

const FLICK_HEIGHT: f32 = 0.01;
const HOOK_REACH: f32 = 24.0;

impl Engine {
    pub fn hang_bottle(&mut self, hanging: Hanging) {
        let mut encoder = self.encoder("hang bottle");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.snapshot(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.drips.hang(Rig { pour_at_once: true, ..self.rig }, hanging, self.paint_on_tool.coefficients, self.paint.medium.clamp(0.0, 1.0));
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
            _ => return false,
        }
        true
    }

    pub(super) fn steer_bottles(&mut self, pointer: &PointerState) {
        let at = Vec2::new(pointer.x, pointer.y);
        if pointer.fresh_press {
            self.holding_hook = at.distance(self.rig.hook) <= HOOK_REACH / self.stage_view.map_or(1.0, |view| view.scale);
            if !self.holding_hook && !self.drips.grab(at) {
                let id = self.drips.hang(self.rig, Hanging { pivot: self.rig.hook, release: at, push: Vec2::ZERO }, self.paint_on_tool.coefficients, self.paint.medium.clamp(0.0, 1.0));
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

    pub(super) fn flick_brush(&mut self) {
        let velocity = self.touch.velocity() * METRES_PER_PIXEL;
        let medium = self.paint.medium.clamp(0.0, 1.0);
        let share = shed_share(velocity.length(), Fluid::of(medium));
        if share <= 0.0 || self.watercolour.is_some() {
            return;
        }
        let paint = self.brush_pigment();
        let thrown = self.brush_load() * share;
        if thrown <= 0.0 {
            return;
        }
        self.keep_brush_load(1.0 - share);
        let tip = (self.touch.position * METRES_PER_PIXEL).extend(FLICK_HEIGHT);
        self.drips.flick(&Spatter { tip, velocity, volume: thrown * METRES_PER_PIXEL.powi(3), paint, medium });
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
        self.pending_marks.extend(marks);
        self.needs_render = true;
    }
}
