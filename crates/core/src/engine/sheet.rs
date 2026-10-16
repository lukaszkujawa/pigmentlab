use super::{Engine, SIMULATION_DT, SUBSTEPS_PER_WATER_STEP, WATER_SETTLE_SECONDS};
use crate::canvas::{Medium, Surface};
use crate::gpu::read_buffer;
use crate::watercolour::Watercolour;
use glam::Vec4;

const WATERCOLOUR_SCRUB: f32 = 0.002;
const WATER_DT: f32 = SIMULATION_DT * SUBSTEPS_PER_WATER_STEP as f32;

impl Engine {
    pub fn canvas_relief(&self) -> Vec<f32> {
        self.canvas.relief_heights()
    }

    pub fn dry_layer(&self) -> Vec<Vec4> {
        read_buffer::<Vec4>(&self.gpu, &self.canvas.dry)
    }

    pub fn dry_heights(&self) -> Vec<f32> {
        self.dry_layer().iter().map(|layer| layer.x).collect()
    }

    pub(super) fn snapshot(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.history.push(encoder, &self.canvas.paint);
        self.dry_history.push(encoder, &self.canvas.dry);
        if let Some(watercolour) = &mut self.watercolour {
            watercolour.snapshot(encoder);
        }
    }

    pub fn surface(&self) -> Surface {
        self.canvas.surface()
    }

    pub fn set_surface(&mut self, surface: Surface) {
        self.canvas.set_surface(&self.gpu, surface);
        match surface.medium() {
            Medium::Watercolour => match &mut self.watercolour {
                Some(watercolour) => watercolour.set_paper(&self.gpu, &self.canvas),
                None => self.watercolour = Some(Watercolour::new(&self.gpu, &self.canvas, WATER_DT, WATERCOLOUR_SCRUB)),
            },
            Medium::Oil => self.watercolour = None,
        }
        self.rebuild_water_brush();
        self.composite.attach_watercolour(&self.gpu, &self.canvas, &self.height_map, self.watercolour.as_ref());
        let mut encoder = self.encoder("new sheet");
        self.window.encode_unbake(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.history.forget();
        self.dry_history.forget();
        self.wet_bounds = None;
        self.mark_whole_canvas_stale();
    }

    pub fn clear(&mut self) {
        if let Some(watercolour) = &mut self.watercolour {
            watercolour.clear(&self.gpu);
        }
        let mut encoder = self.encoder("clear");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.snapshot(&mut encoder);
        self.canvas.clear(&self.gpu, &mut encoder);
        self.window.encode_unbake(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.wet_bounds = None;
        self.mark_whole_canvas_stale();
    }

    pub fn dry(&mut self) {
        let mut encoder = self.encoder("dry");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.snapshot(&mut encoder);
        if let Some(watercolour) = &mut self.watercolour {
            watercolour.encode_step(&self.gpu, &mut encoder, true);
            watercolour.settle(&mut encoder);
        }
        self.canvas.encode_dry(&mut encoder);
        self.window.encode_unbake(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.wet_bounds = None;
        self.mark_whole_canvas_stale();
    }

    pub fn undo(&mut self) -> bool {
        let mut encoder = self.encoder("undo");
        let restored = self.history.pop(&mut encoder, &self.canvas.paint);
        if restored {
            self.dry_history.pop(&mut encoder, &self.canvas.dry);
            if let Some(watercolour) = &mut self.watercolour {
                watercolour.undo(&mut encoder);
                self.water_settles_at = self.time + WATER_SETTLE_SECONDS;
            }
            self.canvas.encode_refresh_ground(&mut encoder);
            self.window.encode_unbake(&mut encoder);
            self.wet_bounds = Some(self.whole_canvas());
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        self.mark_whole_canvas_stale();
        restored
    }

    pub fn save_png(&mut self, path: &std::path::Path) -> Result<(), image::ImageError> {
        self.render();
        self.read_back().save(path)
    }
}
