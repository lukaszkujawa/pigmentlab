use super::frame::BrushPose;
use super::geometry::{VERTICES_PER_BRISTLE, rest_positions};
use super::preset::BrushPreset;
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};
use crate::paint::ActiveWindow;
use crate::pigment::Pigment;
use crate::rng::SplitMix64;
use glam::{Mat3, Vec3, Vec4};

const WORKGROUP_SIZE: u32 = 64;
pub const SAMPLES_PER_BRISTLE: u32 = 256;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BrushUniform {
    rotation_columns: [[f32; 4]; 3],
    translation: [f32; 4],
    friction: f32,
    recovery: f32,
    splay: f32,
    bristle_count: u32,
    canvas_width: f32,
    canvas_height: f32,
    sink: f32,
    ferrule_extent: f32,
    cohesion: f32,
    cohesion_radius: f32,
    _padding: [f32; 2],
}

const COHESION_RADIUS_FRACTION: f32 = 0.5;

impl BrushUniform {
    fn new(preset: &BrushPreset, pose: &BrushPose, canvas: [f32; 2]) -> Self {
        let rotation = Mat3::from_quat(pose.canvas_to_brush());
        let column = |v: Vec3| v.extend(0.0).to_array();
        Self {
            rotation_columns: [column(rotation.x_axis), column(rotation.y_axis), column(rotation.z_axis)],
            translation: column(pose.position),
            friction: preset.friction,
            recovery: preset.recovery,
            splay: preset.splay,
            bristle_count: preset.bristle_count,
            canvas_width: canvas[0],
            canvas_height: canvas[1],
            sink: preset.sink,
            ferrule_extent: preset.width.max(preset.thickness) * 0.5,
            cohesion: preset.cohesion,
            cohesion_radius: preset.width.max(preset.thickness) * 0.5 * COHESION_RADIUS_FRACTION,
            _padding: [0.0; 2],
        }
    }
}

pub struct Brush {
    preset: BrushPreset,
    rest: Vec<Vec4>,
    pub uniform: wgpu::Buffer,
    pub positions: wgpu::Buffer,
    pub curves: wgpu::Buffer,
    pub previous_curves: wgpu::Buffer,
    pub sample_load: wgpu::Buffer,
    pub sample_absorption: wgpu::Buffer,
    pub sample_scattering: wgpu::Buffer,
    tips: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    step: wgpu::ComputePipeline,
    cohere: wgpu::ComputePipeline,
}

impl Brush {
    pub fn new(gpu: &Gpu, preset: BrushPreset, seed: u64, window: &ActiveWindow) -> Self {
        let rest = rest_positions(&preset, &mut SplitMix64::new(seed));
        let vertex_bytes = std::mem::size_of_val(rest.as_slice()) as u64;
        let bristles = u64::from(preset.bristle_count);
        let samples = bristles * u64::from(SAMPLES_PER_BRISTLE);
        let rest_buffer = gpu.storage_buffer("bristle rest", vertex_bytes);
        let positions = gpu.storage_buffer("bristle positions", vertex_bytes);
        let tips = gpu.storage_buffer("bristle tips", bristles * 16);
        let curves = gpu.storage_buffer("bristle curves", bristles * 64);
        let previous_curves = gpu.storage_buffer("bristle previous curves", bristles * 64);
        let sample_load = gpu.storage_buffer("sample load", samples * 4);
        let sample_absorption = gpu.storage_buffer("sample absorption", samples * 16);
        let sample_scattering = gpu.storage_buffer("sample scattering", samples * 16);
        gpu.queue.write_buffer(&rest_buffer, 0, bytemuck::cast_slice(&rest));
        gpu.queue.write_buffer(&positions, 0, bytemuck::cast_slice(&rest));
        let uniform = gpu.uniform_buffer("brush uniform", std::mem::size_of::<BrushUniform>() as u64);

        let shader = gpu.shader(
            "bristles",
            &[
                include_str!("../../shaders/units.wgsl"),
                include_str!("../../shaders/hermite.wgsl"),
                include_str!("../../shaders/window.wgsl"),
                include_str!("../../shaders/bristles.wgsl"),
            ],
        );
        let compute = wgpu::ShaderStages::COMPUTE;
        let read_only = wgpu::BufferBindingType::Storage { read_only: true };
        let read_write = wgpu::BufferBindingType::Storage { read_only: false };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bristles"),
            entries: &[
                buffer_layout_entry(0, compute, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(1, compute, read_only),
                buffer_layout_entry(2, compute, read_write),
                buffer_layout_entry(3, compute, read_write),
                buffer_layout_entry(4, compute, read_write),
                buffer_layout_entry(5, compute, wgpu::BufferBindingType::Uniform),
                buffer_layout_entry(6, compute, read_only),
                buffer_layout_entry(7, compute, read_write),
            ],
        });
        let proposals = gpu.storage_buffer("bristle proposals", tips.size());
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bristles"),
            layout: &layout,
            entries: &[
                buffer_binding(0, &uniform),
                buffer_binding(1, &rest_buffer),
                buffer_binding(2, &positions),
                buffer_binding(3, &tips),
                buffer_binding(4, &curves),
                buffer_binding(5, &window.uniform),
                buffer_binding(6, &window.volume),
                buffer_binding(7, &proposals),
            ],
        });
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bristles"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let brush = Self {
            preset,
            rest,
            uniform,
            positions,
            curves,
            previous_curves,
            sample_load,
            sample_absorption,
            sample_scattering,
            tips,
            bind_group,
            step: pipeline("step"),
            cohere: pipeline("cohere"),
        };
        brush.teleport(gpu, &BrushPose::upright(Vec3::ZERO));
        brush
    }

    pub fn clean(&self, gpu: &Gpu) {
        let samples = (self.preset.bristle_count * SAMPLES_PER_BRISTLE) as usize;
        gpu.queue.write_buffer(&self.sample_load, 0, &vec![0u8; samples * 4]);
    }

    pub fn dip(&self, gpu: &Gpu, pigment: Pigment, load_fraction: f32) {
        let samples = (self.preset.bristle_count * SAMPLES_PER_BRISTLE) as usize;
        let loads = vec![self.preset.sample_capacity * load_fraction; samples];
        let absorption = vec![pigment.coefficients.absorption.extend(0.0); samples];
        let scattering = vec![pigment.coefficients.scattering.extend(0.0); samples];
        gpu.queue.write_buffer(&self.sample_load, 0, bytemuck::cast_slice(&loads));
        gpu.queue.write_buffer(&self.sample_absorption, 0, bytemuck::cast_slice(&absorption));
        gpu.queue.write_buffer(&self.sample_scattering, 0, bytemuck::cast_slice(&scattering));
    }

    pub fn preset(&self) -> &BrushPreset {
        &self.preset
    }

    pub fn vertex_count(&self) -> u32 {
        self.preset.bristle_count * VERTICES_PER_BRISTLE
    }

    pub fn teleport(&self, gpu: &Gpu, pose: &BrushPose) {
        gpu.queue.write_buffer(&self.positions, 0, bytemuck::cast_slice(&self.rest));
        let tips: Vec<Vec4> = self
            .rest
            .chunks(VERTICES_PER_BRISTLE as usize)
            .map(|bristle| pose.to_canvas(bristle[VERTICES_PER_BRISTLE as usize - 1].truncate()).extend(0.0))
            .collect();
        gpu.queue.write_buffer(&self.tips, 0, bytemuck::cast_slice(&tips));
    }

    pub fn encode_step(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, pose: &BrushPose, canvas: [f32; 2]) {
        let uniform = BrushUniform::new(&self.preset, pose, canvas);
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        encoder.copy_buffer_to_buffer(&self.curves, 0, &self.previous_curves, 0, self.curves.size());
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("bristles"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        let groups = self.preset.bristle_count.div_ceil(WORKGROUP_SIZE);
        pass.set_pipeline(&self.step);
        pass.dispatch_workgroups(groups, 1, 1);
        pass.set_pipeline(&self.cohere);
        pass.dispatch_workgroups(groups, 1, 1);
    }
}
