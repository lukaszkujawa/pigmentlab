use super::Engine;
use crate::canvas::{PAINT_CHANNELS, PAINT_SCALE};
use crate::gpu::read_buffer;
use crate::paint::{BrushContents, Gauge};
use crate::pigment::{Coefficients, Paint};
use glam::{Vec3, Vec4};

const DILUTION_PER_MEDIUM: f32 = 0.9;
const WATERCOLOUR_DILUTION: f32 = 0.95;
const WATERCOLOUR_DRIEST_LOAD: f32 = 0.2;
const WATERCOLOUR_PIGMENT: f32 = 0.08;

impl Engine {
    pub fn dip(&mut self, paint: impl Into<Paint>) {
        let paint = paint.into();
        let load = self.paint.load.clamp(0.0, 1.0);
        let medium = self.paint.medium.clamp(0.0, 1.0);
        if self.watercolour.is_some() {
            let concentration = WATERCOLOUR_PIGMENT * (1.0 - WATERCOLOUR_DILUTION * medium) / load.max(WATERCOLOUR_DRIEST_LOAD);
            self.paint_on_tool = paint;
            self.brush.dip_watercolour(&self.gpu, paint, load, concentration);
            self.gauge().invalidate();
            self.brush_contents = BrushContents { fill: load, paint: paint.coefficients, concentration };
            return;
        }
        let dilution = 1.0 - DILUTION_PER_MEDIUM * medium;
        let thinned = Coefficients { absorption: paint.coefficients.absorption * dilution, scattering: paint.coefficients.scattering * dilution };
        self.paint_on_tool = Paint { coefficients: thinned, ..paint };
        self.brush.dip(&self.gpu, thinned, load, medium);
        self.gauge().invalidate();
        self.brush_contents = BrushContents { fill: load, paint: thinned, concentration: 1.0 };
    }

    pub fn dip_water(&mut self) {
        let load = self.paint.load.clamp(0.0, 1.0);
        let clear = Paint::from(Coefficients { absorption: glam::Vec3::ZERO, scattering: glam::Vec3::ZERO });
        if self.watercolour.is_some() {
            self.brush.dip_watercolour(&self.gpu, clear, load, 0.0);
        } else {
            self.brush.clean(&self.gpu);
        }
        self.gauge().invalidate();
        self.brush_contents = BrushContents { fill: if self.watercolour.is_some() { load } else { 0.0 }, paint: clear.coefficients, concentration: 0.0 };
    }

    pub(super) fn gauge(&self) -> &Gauge {
        self.water_brush.as_ref().map_or(self.deposit.gauge(), |water| water.gauge())
    }

    pub fn brush_contents(&mut self) -> BrushContents {
        self.collect_brush_contents();
        self.brush_contents
    }

    pub(super) fn collect_brush_contents(&mut self) {
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        if let Some(contents) = self.gauge().take() {
            self.brush_contents = contents;
        }
        if self.gauge_version != self.contents_version {
            let mut encoder = self.encoder("brush gauge");
            self.request_gauge_reading(&mut encoder);
            self.gpu.queue.submit(Some(encoder.finish()));
            self.gauge().after_submit();
        }
    }

    pub fn clean(&mut self) {
        self.brush.clean(&self.gpu);
        self.gauge().invalidate();
        self.brush_contents = BrushContents { fill: 0.0, ..self.brush_contents };
    }

    pub fn brush_load(&self) -> f32 {
        read_buffer::<f32>(&self.gpu, &self.brush.sample_load).iter().map(|load| f64::from(*load)).sum::<f64>() as f32
    }

    pub fn brush_sample_absorption(&self) -> Vec<Vec3> {
        read_buffer::<Vec4>(&self.gpu, &self.brush.sample_absorption).iter().map(|v| v.truncate()).collect()
    }

    pub fn brush_pigment(&self) -> Coefficients {
        let loads = read_buffer::<f32>(&self.gpu, &self.brush.sample_load);
        let absorption = read_buffer::<Vec4>(&self.gpu, &self.brush.sample_absorption);
        let scattering = read_buffer::<Vec4>(&self.gpu, &self.brush.sample_scattering);
        let total: f32 = loads.iter().sum::<f32>().max(1e-6);
        let weighted = |values: &[Vec4]| values.iter().zip(&loads).map(|(v, load)| v.truncate() * *load).sum::<Vec3>() / total;
        Coefficients { absorption: weighted(&absorption), scattering: weighted(&scattering) }
    }

    pub fn paint_columns(&mut self) -> Vec<[f32; 8]> {
        let mut encoder = self.encoder("bake");
        self.window.encode_bake(&self.gpu, &mut encoder, &self.canvas);
        self.gpu.queue.submit(Some(encoder.finish()));
        let store = read_buffer::<i32>(&self.gpu, &self.canvas.paint);
        store.chunks(PAINT_CHANNELS as usize).map(|pixel| std::array::from_fn(|channel| pixel[channel] as f32 / PAINT_SCALE)).collect()
    }

    pub fn paint_heights(&mut self) -> Vec<f32> {
        self.paint_columns().iter().map(|column| column[0]).collect()
    }

    pub fn paint_mass(&mut self) -> f32 {
        self.paint_heights().iter().map(|h| f64::from(*h)).sum::<f64>() as f32
    }
}
