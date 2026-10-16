use super::{DEPOSIT_DT, Engine, PaintSettings, TUBE_TOOL, Tool};
use crate::brush::{Brush, BrushAdjustments, BrushPreset, preset_by_name};
use crate::paint::{BrushContents, Deposit};
use crate::render::BrushLines;
use crate::watercolour::WaterBrush;

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
        if name.eq_ignore_ascii_case(TUBE_TOOL) {
            self.tool = Tool::Tube;
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
            "angle" => self.set_brush_angle(value),
            "load" => self.set_paint(PaintSettings { load: value, ..self.paint }),
            "medium" => self.set_paint(PaintSettings { medium: value, ..self.paint }),
            "tooth" => {
                self.canvas.set_tooth(&self.gpu, value);
                if let Some(watercolour) = &self.watercolour {
                    watercolour.set_tooth(&self.gpu, &self.canvas);
                }
                self.mark_whole_canvas_stale();
            }
            _ => return false,
        }
        true
    }

    pub(super) fn rebuild_brush(&mut self) {
        self.set_brush(self.base_preset.adjusted(self.adjustments));
        self.deposit.set_medium(self.paint.medium);
    }

    pub fn set_brush(&mut self, preset: BrushPreset) {
        self.brush = Brush::new(&self.gpu, preset, self.seed, &self.window, &self.canvas, &self.stroke_base);
        self.deposit = Deposit::new(&self.gpu, &self.brush, &self.window, &self.canvas, &self.stroke_base, DEPOSIT_DT);
        self.brush_lines = BrushLines::new(&self.gpu, &self.brush);
        self.brush.teleport(&self.gpu, &self.pose);
        self.rebuild_water_brush();
        self.brush_contents = BrushContents::empty();
        self.gauge_version = self.contents_version;
    }

    pub(super) fn rebuild_water_brush(&mut self) {
        self.water_brush = self.watercolour.as_ref().map(|water| WaterBrush::new(&self.gpu, &self.brush, water, &self.canvas, DEPOSIT_DT));
    }
}
