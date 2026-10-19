use super::{Engine, FOOTPRINT_SLACK, SIMULATION_DT, SUBSTEPS_PER_DEPOSIT, SUBSTEPS_PER_WATER_STEP, Tool, WATER_SETTLE_SECONDS};
use crate::paint::{Mark, Marks, StrokeState, WetBounds};
use glam::IVec2;

const DEPOSIT_REACH: i32 = 24;
const LEVELLING_SETTLE_SECONDS: f64 = 2.0;
const LEVELLING_SETTLE_SECONDS_PER_MEDIUM: f64 = 4.0;
const MAX_SUBSTEPS_PER_ADVANCE: u32 = 32;
const SUBSTEPS_PER_LEVELLING: u64 = 8;
const WET_MARGIN: i32 = 48;

impl Engine {
    pub fn advance_realtime(&mut self, target_time: f64) {
        let dt = f64::from(SIMULATION_DT);
        let needed = ((target_time - self.time) / dt).floor().max(0.0) as u32;
        if needed > MAX_SUBSTEPS_PER_ADVANCE {
            self.time = target_time - dt * f64::from(MAX_SUBSTEPS_PER_ADVANCE);
        }
        self.realtime = true;
        self.advance_steps(needed.min(MAX_SUBSTEPS_PER_ADVANCE));
    }

    pub fn advance_exact(&mut self, target_time: f64) {
        let dt = f64::from(SIMULATION_DT);
        let needed = ((target_time - self.time) / dt).round().max(0.0) as u32;
        self.realtime = false;
        self.advance_steps(needed);
    }

    pub(super) fn advance_steps(&mut self, steps: u32) {
        let dt = f64::from(SIMULATION_DT);
        let canvas = [self.canvas.size.width as f32, self.canvas.size.height as f32];
        for _ in 0..steps {
            self.time += dt;
            if let Some(state) = self.pointer.state_at(self.time) {
                let lifted = self.pressed && !state.pressed;
                self.touch.update(&state, SIMULATION_DT);
                self.pose = self.pose_for();
                self.pressed = state.pressed;
                if lifted && self.tool == Tool::Brush {
                    self.flick_brush();
                }
                if self.touch.engaged() {
                    self.levelling_active_until = self.time + LEVELLING_SETTLE_SECONDS + LEVELLING_SETTLE_SECONDS_PER_MEDIUM * f64::from(self.paint.medium.clamp(0.0, 1.0));
                }
                if state.fresh_press {
                    self.position_at_last_deposit = self.tool_tip();
                    self.tube.press(&self.gpu, self.tool_tip().truncate());
                    self.brush.teleport(&self.gpu, &self.pose);
                    let mut encoder = self.encoder("stroke snapshot");
                    self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
                    self.snapshot(&mut encoder);
                    self.refresh_heights(&mut encoder);
                    self.height_map.encode_tuft_base(&self.gpu, &mut encoder, self.whole_canvas());
                    encoder.copy_buffer_to_buffer(&self.height_map.tuft_base, 0, &self.stroke_base, 0, self.stroke_base.size());
                    if let Some(water_brush) = &self.water_brush {
                        water_brush.start_stroke(&mut encoder, &self.brush);
                    }
                    self.gpu.queue.submit(Some(encoder.finish()));
                }
                self.needs_render |= self.touch.engaged() || self.show_bristles;
                match self.tool {
                    Tool::Drip => self.steer_bottles(&state),
                    Tool::Splatter => self.aim_splatter(&state),
                    Tool::Brush | Tool::Tube => {}
                }
            }
            if let Some(hover) = self.hover.filter(|_| !self.pressed && !self.touch.engaged()) {
                self.touch.position = glam::Vec2::new(hover.x, hover.y);
                self.touch.follow_tilt(hover.tilt, SIMULATION_DT);
                self.touch.rotation = hover.rotation;
                self.pose = self.pose_for();
                self.brush.centre_tuft(self.handle_position());
                self.needs_render |= self.show_bristles;
            }
            self.drain_brush();
            let mut encoder = self
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("substep") });
            let preset = *self.brush.preset();
            let (focus, footprint_radius) = match self.tool {
                Tool::Brush => (self.brush.tuft_centre(), preset.reach_from_handle() + FOOTPRINT_SLACK),
                Tool::Tube => (self.tool_tip().truncate(), self.tube_reach() + FOOTPRINT_SLACK),
                Tool::Drip | Tool::Splatter => (self.tool_tip().truncate(), FOOTPRINT_SLACK),
            };
            if self.window.needs_relocation(focus, footprint_radius) {
                self.window.follow(&self.gpu, &mut encoder, &self.canvas, focus);
            }
            if self.touch.engaged() && matches!(self.tool, Tool::Brush | Tool::Tube) {
                let touched = WetBounds::around(self.tool_tip().truncate().round().as_ivec2(), footprint_radius.ceil() as i32 + WET_MARGIN);
                match &mut self.watercolour {
                    Some(watercolour) => {
                        watercolour.wet(touched);
                        self.water_settles_at = self.time + WATER_SETTLE_SECONDS;
                    }
                    None => self.wet_bounds = Some(self.wet_bounds.map_or(touched, |bounds| bounds.include(touched))),
                }
                self.mark_stale(WetBounds::around(self.tool_tip().truncate().round().as_ivec2(), footprint_radius.ceil() as i32 + DEPOSIT_REACH));
            }
            match self.tool {
                Tool::Brush => {
                    self.brush.encode_step(&self.gpu, &mut encoder, &self.pose, self.touch.pressure, self.canvas.tooth(), canvas);
                    if let Some(water_brush) = self.water_brush.as_ref().filter(|_| self.touch.engaged() && self.substeps.is_multiple_of(SUBSTEPS_PER_DEPOSIT)) {
                        water_brush.encode(&mut encoder, &self.brush);
                        self.contents_version += 1;
                    } else if self.water_brush.is_none() && self.touch.engaged() && self.substeps.is_multiple_of(SUBSTEPS_PER_DEPOSIT) {
                        let travel = self.pose.position.truncate().distance(self.position_at_last_deposit.truncate());
                        let stroke = StrokeState { travel, pressure: self.touch.pressure, contact: self.contact(), lifting: !self.pressed };
                        let region = self.window.region_around(self.brush.tuft_centre(), footprint_radius + DEPOSIT_REACH as f32);
                        self.deposit.encode(&self.gpu, &mut encoder, &self.brush, stroke, region);
                        self.contents_version += 1;
                        self.position_at_last_deposit = self.pose.position;
                    }
                }
                Tool::Tube => {
                    if self.pressed {
                        let path = [self.position_at_last_deposit.truncate(), self.tool_tip().truncate()];
                        let medium = self.paint.medium.clamp(0.0, 1.0);
                        let nozzle_radius = self.nozzle_radius();
                        match (&self.water_tube, &mut self.watercolour) {
                            (Some(tube), Some(water)) => {
                                water.wet(tube.encode_squeeze(&self.gpu, &mut encoder, path, nozzle_radius, self.touch.pressure, self.paint_on_tool, self.paint.dilution.clamp(0.0, 1.0)));
                                self.water_settles_at = self.time + WATER_SETTLE_SECONDS;
                            }
                            _ => self.tube.encode_squeeze(&self.gpu, &mut encoder, path[1], nozzle_radius, self.touch.pressure, self.paint_on_tool.coefficients, medium),
                        }
                    }
                    self.position_at_last_deposit = self.tool_tip();
                }
                Tool::Drip | Tool::Splatter => {}
            }
            self.step_drips();
            self.settle_film(&mut encoder);
            self.lay_pending_marks(&mut encoder);
            if let Some(watercolour) = self.watercolour.as_mut().filter(|water| water.is_wet() && self.substeps.is_multiple_of(SUBSTEPS_PER_WATER_STEP)) {
                let settled = self.time > self.water_settles_at;
                watercolour.encode_step(&self.gpu, &mut encoder, settled);
                if settled {
                    watercolour.settle(&mut encoder);
                }
                self.needs_render = true;
            }
            self.substeps += 1;
            let levelling_due = self.levelling_enabled && self.time <= self.levelling_active_until;
            if let Some(wet) = self.wet_bounds.filter(|_| levelling_due && self.substeps.is_multiple_of(SUBSTEPS_PER_LEVELLING)) {
                self.refresh_heights(&mut encoder);
                self.levelling.encode(&self.gpu, &mut encoder, &self.window, wet);
                self.mark_stale(wet.expand(1));
            }
            self.request_gauge_reading(&mut encoder);
            self.gpu.queue.submit(Some(encoder.finish()));
            self.gauge().after_submit();
            self.collect_brush_contents();
        }
    }

    pub fn lay(&mut self, marks: &[Mark]) {
        self.pending_marks.extend_from_slice(marks);
    }

    fn lay_pending_marks(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.watercolour.is_some() {
            self.pending_marks.clear();
            return;
        }
        let laid: Vec<Mark> = self.pending_marks.drain(..Marks::fitting(&self.pending_marks)).collect();
        let Some(bounds) = self.marks.encode(&self.gpu, encoder, &self.window, &laid) else { return };
        let oiliest = laid.iter().map(|mark| mark.medium).fold(0.0, f32::max).clamp(0.0, 1.0);
        self.levelling_active_until = self.levelling_active_until.max(self.time + LEVELLING_SETTLE_SECONDS + LEVELLING_SETTLE_SECONDS_PER_MEDIUM * f64::from(oiliest));
        let wet = bounds.expand(WET_MARGIN);
        self.wet_bounds = Some(self.wet_bounds.map_or(wet, |bounds| bounds.include(wet)));
        self.mark_stale(bounds.expand(1));
    }

    pub(super) fn request_gauge_reading(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.gauge_version != self.contents_version && self.gauge().encode_copy(encoder) {
            self.gauge_version = self.contents_version;
        }
    }

    fn tube_reach(&self) -> f32 {
        match self.watercolour {
            Some(_) => self.nozzle_radius(),
            None => self.tube.reach(self.nozzle_radius(), self.paint.medium.clamp(0.0, 1.0)),
        }
    }

    pub(super) fn whole_canvas(&self) -> WetBounds {
        WetBounds { min: IVec2::ZERO, max: IVec2::new(self.canvas.size.width as i32, self.canvas.size.height as i32) }
    }

    pub(super) fn mark_whole_canvas_stale(&mut self) {
        self.mark_stale(self.whole_canvas());
    }

    pub(super) fn mark_stale(&mut self, region: WetBounds) {
        self.stale_heights = Some(self.stale_heights.map_or(region, |stale| stale.include(region)));
        self.needs_render = true;
    }

    pub(super) fn refresh_heights(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(region) = self.stale_heights.take() {
            self.height_map.encode_build(&self.gpu, encoder, region);
            self.stale_blur = Some(self.stale_blur.map_or(region, |stale| stale.include(region)));
        }
    }

    pub(super) fn refresh_blur(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.refresh_heights(encoder);
        if let Some(region) = self.stale_blur.take() {
            self.height_map.encode_blur(&self.gpu, encoder, region);
        }
    }
}
