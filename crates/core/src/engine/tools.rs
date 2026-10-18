use super::{DEPOSIT_DT, DRIP_TOOL, Engine, FOOTPRINT_SLACK, PaintSettings, SIMULATION_DT, TUBE_TOOL, Tool};
use crate::brush::{Brush, BrushAdjustments, BrushPreset, preset_by_name};
use crate::paint::{ActiveWindow, BrushContents, Deposit, Levelling, Marks, Tube};
use crate::render::{BrushLines, Composite, HeightMap};
use crate::watercolour::WaterBrush;

const FIT_SHRINK: f32 = 0.95;

impl Engine {
    pub fn brush(&self) -> &Brush {
        &self.brush
    }

    pub fn select_brush(&mut self, name: &str) -> bool {
        self.configure_brush(name, self.adjustments)
    }

    pub fn set_brush_adjustments(&mut self, adjustments: BrushAdjustments) {
        if adjustments != self.adjustments {
            self.adjustments = adjustments;
            self.rebuild_brush();
        }
    }

    pub fn configure_brush(&mut self, name: &str, adjustments: BrushAdjustments) -> bool {
        if name.eq_ignore_ascii_case(TUBE_TOOL) || name.eq_ignore_ascii_case(DRIP_TOOL) {
            self.tool = if name.eq_ignore_ascii_case(TUBE_TOOL) { Tool::Tube } else { Tool::Drip };
            self.adjustments = adjustments;
            return true;
        }
        let Some(preset) = preset_by_name(name) else { return false };
        self.tool = Tool::Brush;
        if preset != self.base_preset || adjustments != self.adjustments || self.brush.preset() != &preset.adjusted(adjustments) {
            self.base_preset = preset;
            self.adjustments = adjustments;
            self.rebuild_brush();
        }
        true
    }

    pub fn set_brush_angle(&mut self, degrees: f32) {
        self.brush_angle_degrees = degrees;
    }

    pub fn set_paint(&mut self, paint: PaintSettings) {
        self.paint = paint;
        self.deposit.set_medium(paint.medium);
    }

    pub fn set_param(&mut self, name: &str, value: f32) -> bool {
        match name {
            "size" => self.set_brush_adjustments(BrushAdjustments { scale: value, ..self.adjustments }),
            "bristles" => self.set_brush_adjustments(BrushAdjustments { bristle_count: Some(value as u32), ..self.adjustments }),
            "stiffness" => self.set_brush_adjustments(BrushAdjustments { stiffness: value, ..self.adjustments }),
            "thickness" => self.set_brush_adjustments(BrushAdjustments { thickness: value, ..self.adjustments }),
            "angle" => self.set_brush_angle(value),
            "load" => self.set_paint(PaintSettings { load: value, ..self.paint }),
            "medium" => self.set_paint(PaintSettings { medium: value, ..self.paint }),
            "dilution" => self.set_paint(PaintSettings { dilution: value, ..self.paint }),
            "tooth" => {
                self.canvas.set_tooth(&self.gpu, value);
                if let Some(watercolour) = &self.watercolour {
                    watercolour.set_tooth(&self.gpu, &self.canvas);
                }
                self.mark_whole_canvas_stale();
            }
            _ => return self.set_rig_param(name, value),
        }
        true
    }

    pub(super) fn rebuild_brush(&mut self) {
        let mut adjustments = self.adjustments;
        while self.base_preset.adjusted(adjustments).reach_from_handle() + FOOTPRINT_SLACK > ActiveWindow::largest_footprint() && adjustments.scale > 0.1 {
            adjustments.scale *= FIT_SHRINK;
        }
        self.set_brush(self.base_preset.adjusted(adjustments));
        self.deposit.set_medium(self.paint.medium);
    }

    pub fn set_brush(&mut self, preset: BrushPreset) {
        self.fit_window(preset.reach_from_handle() + FOOTPRINT_SLACK);
        self.brush = Brush::new(&self.gpu, preset, self.seed, &self.window, &self.canvas, &self.stroke_base);
        self.deposit = Deposit::new(&self.gpu, &self.brush, &self.window, &self.canvas, &self.stroke_base, DEPOSIT_DT);
        self.brush_lines = BrushLines::new(&self.gpu, &self.brush, &self.frames);
        self.brush.teleport(&self.gpu, &self.pose);
        self.rebuild_water_brush();
        self.brush_contents = BrushContents::empty();
        self.gauge_version = self.contents_version;
    }

    fn fit_window(&mut self, footprint_radius: f32) {
        let size = ActiveWindow::size_for(footprint_radius);
        if size == self.window.size() {
            return;
        }
        let mut encoder = self.encoder("resize window");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.window = ActiveWindow::new(&self.gpu, &self.canvas, size);
        self.height_map = HeightMap::new(&self.gpu, &self.canvas, &self.window);
        self.levelling = Levelling::new(&self.gpu, &self.canvas, &self.window, &self.height_map);
        self.composite = Composite::new(&self.gpu, &self.canvas, &self.window, &self.height_map, &self.frames);
        self.composite.set_lighting(&self.gpu, self.lighting);
        self.composite.attach_watercolour(&self.gpu, &self.canvas, &self.height_map, self.watercolour.as_ref());
        self.tube = Tube::new(&self.gpu, &self.window, &self.canvas, &self.height_map.heights, self.seed, SIMULATION_DT);
        self.marks = Marks::new(&self.gpu, &self.window, &self.canvas);
        self.mark_whole_canvas_stale();
    }

    pub(super) fn rebuild_water_brush(&mut self) {
        self.water_brush = self.watercolour.as_ref().map(|water| WaterBrush::new(&self.gpu, &self.brush, water, &self.canvas, DEPOSIT_DT));
    }
}
