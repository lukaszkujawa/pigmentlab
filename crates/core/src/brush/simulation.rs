use super::frame::BrushPose;
use super::geometry::{VERTICES_PER_BRISTLE, clump_count, rest_positions};
use super::preset::BrushPreset;
use crate::gpu::{Gpu, buffer_binding, buffer_layout_entry};
use crate::canvas::Canvas;
use crate::paint::ActiveWindow;
use crate::pigment::{Coefficients, Paint};
use crate::rng::SplitMix64;
use glam::{Mat3, Vec2, Vec3, Vec4};
use std::cell::Cell;

const WORKGROUP_SIZE: u32 = 64;
pub const SAMPLES_PER_BRISTLE: u32 = 256;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BrushUniform {
    rotation_columns: [[f32; 4]; 3],
    translation: [f32; 4],
    previous_rotation_columns: [[f32; 4]; 3],
    previous_translation: [f32; 4],
    tuft: [f32; 4],
    friction: f32,
    stiffness: f32,
    inertia: f32,
    bristle_count: u32,
    canvas_width: f32,
    canvas_height: f32,
    sink: f32,
    cohesion: f32,
    pressure: f32,
    tooth: f32,
    packing_density: f32,
    clump_count: u32,
    sample_capacity: f32,
    wet_floor_keep: f32,
    _padding: [f32; 2],
}

const PACKING_GRID: u32 = 64;
const AREA_PER_HAIR_LAYER: f32 = 6.0;
const MIN_CONTACT_LAYERS: f32 = 0.4;

fn rotation_columns(pose: &BrushPose) -> [[f32; 4]; 3] {
    let rotation = Mat3::from_quat(pose.canvas_to_brush());
    [rotation.x_axis.extend(0.0).to_array(), rotation.y_axis.extend(0.0).to_array(), rotation.z_axis.extend(0.0).to_array()]
}

fn packing_cell(preset: &BrushPreset) -> f32 {
    (2.0 * (preset.ferrule_area() / preset.bristle_count as f32).sqrt()).clamp(3.0, 8.0)
}

fn contact_layers(preset: &BrushPreset) -> f32 {
    (preset.bristle_count as f32 * AREA_PER_HAIR_LAYER / preset.ferrule_area()).max(MIN_CONTACT_LAYERS)
}

#[derive(Clone, Copy)]
struct Motion {
    previous: BrushPose,
    tuft_centre: Vec2,
}

pub struct Brush {
    preset: BrushPreset,
    rest: Vec<Vec4>,
    pub uniform: wgpu::Buffer,
    pub positions: wgpu::Buffer,
    pub chain: wgpu::Buffer,
    pub sample_load: wgpu::Buffer,
    pub sample_absorption: wgpu::Buffer,
    pub sample_scattering: wgpu::Buffer,
    pub sample_behaviour: wgpu::Buffer,
    velocity: wgpu::Buffer,
    relative: wgpu::Buffer,
    pub packing: wgpu::Buffer,
    motion: Cell<Motion>,
    bind_group: wgpu::BindGroup,
    prepare: wgpu::ComputePipeline,
    gather: wgpu::ComputePipeline,
    solve: wgpu::ComputePipeline,
}

impl Brush {
    pub fn new(gpu: &Gpu, preset: BrushPreset, seed: u64, window: &ActiveWindow, canvas: &Canvas, stroke_base: &wgpu::Buffer) -> Self {
        let rest = rest_positions(&preset, &mut SplitMix64::new(seed));
        let vertex_bytes = std::mem::size_of_val(rest.as_slice()) as u64;
        let samples = u64::from(preset.bristle_count) * u64::from(SAMPLES_PER_BRISTLE);
        let rest_buffer = gpu.storage_buffer("bristle rest", vertex_bytes);
        let positions = gpu.storage_buffer("bristle positions", vertex_bytes);
        let chain = gpu.storage_buffer("bristle chain", vertex_bytes);
        let velocity = gpu.storage_buffer("bristle velocity", vertex_bytes);
        let relative = gpu.storage_buffer("bristle relative velocity", vertex_bytes);
        let clumps = gpu.storage_buffer("bristle clumps", u64::from(clump_count(&preset) * VERTICES_PER_BRISTLE) * 16);
        let packing_buffer = gpu.storage_buffer("bristle packing", u64::from(PACKING_GRID * PACKING_GRID) * 4);
        let sample_load = gpu.storage_buffer("sample load", samples * 4);
        let sample_absorption = gpu.storage_buffer("sample absorption", samples * 16);
        let sample_scattering = gpu.storage_buffer("sample scattering", samples * 16);
        let sample_behaviour = gpu.storage_buffer("sample behaviour", samples * 16);
        gpu.queue.write_buffer(&rest_buffer, 0, bytemuck::cast_slice(&rest));
        gpu.queue.write_buffer(&positions, 0, bytemuck::cast_slice(&rest));
        let uniform = gpu.uniform_buffer("brush uniform", std::mem::size_of::<BrushUniform>() as u64);

        let shader = gpu.shader("bristles", &[include_str!("../../shaders/window.wgsl"), include_str!("../../shaders/bristles.wgsl")]);
        let compute = wgpu::ShaderStages::COMPUTE;
        let read_only = wgpu::BufferBindingType::Storage { read_only: true };
        let read_write = wgpu::BufferBindingType::Storage { read_only: false };
        let uniform_type = wgpu::BufferBindingType::Uniform;
        let kinds = [
            uniform_type, read_only, read_write, read_write, read_write, read_write, uniform_type,
            read_only, read_only, read_only, read_only, read_only, read_write, read_write,
        ];
        let entries: Vec<_> = kinds.iter().enumerate().map(|(binding, kind)| buffer_layout_entry(binding as u32, compute, *kind)).collect();
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("bristles"), entries: &entries });
        let buffers = [
            &uniform, &rest_buffer, &positions, &chain, &velocity, &relative, &window.uniform,
            stroke_base, &canvas.ground, &canvas.relief, &window.columns, &sample_load, &clumps, &packing_buffer,
        ];
        let bind_entries: Vec<_> = buffers.iter().enumerate().map(|(binding, buffer)| buffer_binding(binding as u32, buffer)).collect();
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("bristles"), layout: &layout, entries: &bind_entries });
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
        let start = BrushPose::upright(Vec3::ZERO);
        let brush = Self {
            preset,
            rest,
            uniform,
            positions,
            chain,
            sample_load,
            sample_absorption,
            sample_scattering,
            sample_behaviour,
            velocity,
            relative,
            packing: packing_buffer,
            motion: Cell::new(Motion { previous: start, tuft_centre: Vec2::ZERO }),
            bind_group,
            prepare: pipeline("prepare"),
            gather: pipeline("gather"),
            solve: pipeline("solve"),
        };
        brush.teleport(gpu, &start);
        brush
    }

    pub fn clean(&self, gpu: &Gpu) {
        let samples = (self.preset.bristle_count * SAMPLES_PER_BRISTLE) as usize;
        gpu.queue.write_buffer(&self.sample_load, 0, &vec![0u8; samples * 4]);
    }

    pub fn dip(&self, gpu: &Gpu, paint: Coefficients, load_fraction: f32, medium: f32) {
        let samples = (self.preset.bristle_count * SAMPLES_PER_BRISTLE) as usize;
        let loads = vec![self.preset.sample_capacity * load_fraction; samples];
        let absorption = vec![paint.absorption.extend(medium); samples];
        let scattering = vec![paint.scattering.extend(0.0); samples];
        gpu.queue.write_buffer(&self.sample_load, 0, bytemuck::cast_slice(&loads));
        gpu.queue.write_buffer(&self.sample_absorption, 0, bytemuck::cast_slice(&absorption));
        gpu.queue.write_buffer(&self.sample_scattering, 0, bytemuck::cast_slice(&scattering));
    }

    pub fn rest(&self) -> &[Vec4] {
        &self.rest
    }

    pub fn dip_watercolour(&self, gpu: &Gpu, paint: Paint, load_fraction: f32, concentration: f32) {
        let samples = (self.preset.bristle_count * SAMPLES_PER_BRISTLE) as usize;
        let behaviour = paint.behaviour;
        gpu.queue.write_buffer(&self.sample_load, 0, bytemuck::cast_slice(&vec![self.preset.sample_capacity * load_fraction; samples]));
        gpu.queue.write_buffer(&self.sample_absorption, 0, bytemuck::cast_slice(&vec![paint.coefficients.absorption.extend(0.0); samples]));
        gpu.queue.write_buffer(&self.sample_scattering, 0, bytemuck::cast_slice(&vec![paint.coefficients.scattering.extend(concentration); samples]));
        gpu.queue.write_buffer(&self.sample_behaviour, 0, bytemuck::cast_slice(&vec![Vec4::new(behaviour.density, behaviour.staining, behaviour.granulation, 0.0); samples]));
    }

    pub fn preset(&self) -> &BrushPreset {
        &self.preset
    }

    pub fn vertex_count(&self) -> u32 {
        self.preset.bristle_count * VERTICES_PER_BRISTLE
    }

    pub fn teleport(&self, gpu: &Gpu, pose: &BrushPose) {
        let canvas: Vec<Vec4> = self.rest.iter().map(|vertex| pose.to_canvas(vertex.truncate()).extend(0.0)).collect();
        let zeros = vec![Vec4::ZERO; self.rest.len()];
        gpu.queue.write_buffer(&self.positions, 0, bytemuck::cast_slice(&self.rest));
        gpu.queue.write_buffer(&self.chain, 0, bytemuck::cast_slice(&canvas));
        gpu.queue.write_buffer(&self.velocity, 0, bytemuck::cast_slice(&zeros));
        gpu.queue.write_buffer(&self.relative, 0, bytemuck::cast_slice(&zeros));
        self.motion.set(Motion { previous: *pose, tuft_centre: pose.position.truncate() });
    }

    pub fn tuft_centre(&self) -> Vec2 {
        self.motion.get().tuft_centre
    }

    pub fn centre_tuft(&self, position: Vec2) {
        self.motion.set(Motion { tuft_centre: position, ..self.motion.get() });
    }

    pub fn encode_step(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, pose: &BrushPose, pressure: f32, tooth: f32, canvas: [f32; 2]) {
        let motion = self.motion.get();
        let reach = self.preset.bend_reach();
        let handle = pose.position.truncate();
        let offset = handle - motion.tuft_centre;
        let tuft_centre = if offset.length() > reach { handle - offset.normalize() * reach } else { motion.tuft_centre };
        let cell = packing_cell(&self.preset);
        let uniform = BrushUniform {
            rotation_columns: rotation_columns(pose),
            translation: pose.position.extend(0.0).to_array(),
            previous_rotation_columns: rotation_columns(&motion.previous),
            previous_translation: motion.previous.position.extend(0.0).to_array(),
            tuft: [tuft_centre.x, tuft_centre.y, reach + self.preset.splayed_radius(), cell],
            friction: self.preset.friction,
            stiffness: self.preset.stiffness,
            inertia: self.preset.inertia,
            bristle_count: self.preset.bristle_count,
            canvas_width: canvas[0],
            canvas_height: canvas[1],
            sink: self.preset.sink,
            cohesion: self.preset.cohesion,
            pressure,
            tooth,
            packing_density: cell * cell * contact_layers(&self.preset),
            clump_count: clump_count(&self.preset),
            sample_capacity: self.preset.sample_capacity,
            wet_floor_keep: self.preset.wet_floor_keep(pressure),
            _padding: [0.0; 2],
        };
        self.motion.set(Motion { previous: *pose, tuft_centre });
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("bristles"), timestamp_writes: None });
        pass.set_bind_group(0, &self.bind_group, &[]);
        let clump_slots = clump_count(&self.preset) * VERTICES_PER_BRISTLE * 4;
        pass.set_pipeline(&self.prepare);
        pass.dispatch_workgroups((PACKING_GRID * PACKING_GRID).max(clump_slots).div_ceil(WORKGROUP_SIZE), 1, 1);
        let groups = self.preset.bristle_count.div_ceil(WORKGROUP_SIZE);
        pass.set_pipeline(&self.gather);
        pass.dispatch_workgroups(groups, 1, 1);
        pass.set_pipeline(&self.solve);
        pass.dispatch_workgroups(groups, 1, 1);
    }
}
