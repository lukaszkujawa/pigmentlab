use super::window::{ActiveWindow, WINDOW_SIZE};
use crate::brush::{Brush, SAMPLES_PER_BRISTLE};
use crate::canvas::Canvas;
use crate::gpu::{Binding, ComputePass, Gpu};

const SAMPLE_WORKGROUP: u32 = 128;
const CELL_WORKGROUP: u32 = 64;
const PROJECTION_ROUNDS: u32 = 3;
const DIFFUSION_ROUNDS: u32 = 1;
const MAX_ADVECTION_SUBSTEPS: u32 = 12;
pub const PARTICLE_CAPACITY: u32 = 262_144;
const PARTICLE_STRIDE: u64 = 64;
const COARSE_CELL: u32 = 16;
const MIN_SPEED_FOR_SUBSTEPS: f32 = crate::units::cm(1.5);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FluidUniform {
    dt: f32,
    damping: f32,
    viscosity: f32,
    dryness_rate: f32,
    sample_count: u32,
    samples_per_bristle: u32,
    advection_dt: f32,
    gravity: f32,
    yield_speed: f32,
    _padding: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidSettings {
    pub dt: f32,
    pub damping: f32,
    pub viscosity: f32,
    pub dryness_rate: f32,
    pub gravity: f32,
    pub yield_speed: f32,
    pub particles: bool,
}

pub struct Fluid {
    settings: FluidSettings,
    uniform: wgpu::Buffer,
    pub velocity: wgpu::Buffer,
    velocity_before: wgpu::Buffer,
    previous_curves: wgpu::Buffer,
    bristle: wgpu::Buffer,
    pub particle_volume: wgpu::Buffer,
    particle_velocity: wgpu::Buffer,
    particles_in: wgpu::Buffer,
    particles_out: wgpu::Buffer,
    pub counters: wgpu::Buffer,
    coarse_flags: wgpu::Buffer,
    stroke_base: wgpu::Buffer,
    region: wgpu::Buffer,
    indirect: wgpu::Buffer,
    snapshot_columns: ComputePass,
    level_columns: ComputePass,
    grow_region: ComputePass,
    finalize_bounds: ComputePass,
    scan_region: ComputePass,
    finalize_depth: ComputePass,
    rasterize: ComputePass,
    flag_regions: ComputePass,
    emit: ComputePass,
    rasterize_particles: ComputePass,
    splat_particles: ComputePass,
    merge_velocity: ComputePass,
    update_particles: ComputePass,
    return_resting: ComputePass,
    return_all: ComputePass,
    compact: ComputePass,
    finish_compaction: ComputePass,
    advect_forward: ComputePass,
    advect_backward: ComputePass,
    boundaries: ComputePass,
    diffuse_forward: ComputePass,
    diffuse_backward: ComputePass,
    divergence: ComputePass,
    jacobi_forward: ComputePass,
    jacobi_backward: ComputePass,
    gradient: ComputePass,
    dry: ComputePass,
    dry_all: ComputePass,
    settle: ComputePass,
    sample_count: u32,
    cells: u32,
    face_slots: u32,
}

impl Fluid {
    pub fn new(gpu: &Gpu, canvas: &Canvas, window: &ActiveWindow, brush: &Brush, settings: FluidSettings) -> Self {
        let [w, h, d] = [WINDOW_SIZE.x, WINDOW_SIZE.y, WINDOW_SIZE.z];
        let cells = w * h * d;
        let velocity_len = (w + 1) * h * d + w * (h + 1) * d + w * h * (d + 1);
        let face_slots = (w + 1) * (h + 1) * (d + 1);
        let sample_count = brush.preset().bristle_count * SAMPLES_PER_BRISTLE;

        let uniform = gpu.uniform_buffer("fluid uniform", std::mem::size_of::<FluidUniform>() as u64);
        let volume_next = gpu.storage_buffer("fluid volume next", window.volume.size());
        let velocity = gpu.storage_buffer("fluid velocity", u64::from(velocity_len) * 4);
        let velocity_next = gpu.storage_buffer("fluid velocity next", u64::from(velocity_len) * 4);
        let pressure = gpu.storage_buffer("fluid pressure", u64::from(cells) * 4);
        let pressure_next = gpu.storage_buffer("fluid pressure next", u64::from(cells) * 4);
        let divergence = gpu.storage_buffer("fluid divergence", u64::from(cells) * 4);
        let bristle = gpu.storage_buffer("fluid bristle occupancy", u64::from(cells) * 16);
        let previous_curves = gpu.storage_buffer("fluid previous curves", brush.curves.size());
        let velocity_before = gpu.storage_buffer("fluid velocity before projection", u64::from(velocity_len) * 4);
        let particle_volume = gpu.storage_buffer("particle volume", window.volume.size());
        let particle_velocity = gpu.storage_buffer("particle momentum", u64::from(cells) * 16);
        let particles_in = gpu.storage_buffer("particles", u64::from(PARTICLE_CAPACITY) * PARTICLE_STRIDE);
        let particles_out = gpu.storage_buffer("particles compacted", u64::from(PARTICLE_CAPACITY) * PARTICLE_STRIDE);
        let counters = gpu.storage_buffer("particle counters", 8);
        let coarse_cells = u64::from(w.div_ceil(COARSE_CELL)) * u64::from(h.div_ceil(COARSE_CELL));
        let coarse_flags = gpu.storage_buffer("bristle region flags", coarse_cells * 4);
        let stroke_base = gpu.storage_buffer("stroke base heights", canvas.size.pixel_count() * 4);
        let region = gpu.storage_buffer("fluid region", 7 * 4);
        let columns = gpu.storage_buffer("column snapshot", u64::from(w * h) * 16);
        let column_pigment = gpu.storage_buffer("column pigment snapshot", u64::from(w * h) * 16);
        let indirect = gpu.indirect_buffer("fluid indirect dispatch", 9 * 4);

        let shader = gpu.shader(
            "fluid",
            &[
                include_str!("../../shaders/units.wgsl"),
                include_str!("../../shaders/hermite.wgsl"),
                include_str!("../../shaders/window.wgsl"),
                include_str!("../../shaders/fluid.wgsl"),
            ],
        );
        let uniforms = [(0, Binding::Uniform(&uniform)), (1, Binding::Uniform(&window.uniform))];
        let pass = |label: &'static str, entry: &str, extra: &[(u32, Binding)]| {
            let bindings: Vec<(u32, Binding)> = uniforms.iter().copied().chain(extra.iter().copied()).collect();
            ComputePass::new(gpu, label, &shader, entry, &bindings)
        };
        let volume = Binding::ReadWrite(&window.volume);
        let velocity_rw = Binding::ReadWrite(&velocity);
        let bristle_rw = Binding::ReadWrite(&bristle);
        let particle_volume_rw = Binding::ReadWrite(&particle_volume);
        let particle_velocity_rw = Binding::ReadWrite(&particle_velocity);
        let particles_in_rw = Binding::ReadWrite(&particles_in);
        let counters_rw = Binding::ReadWrite(&counters);
        let coarse_rw = Binding::ReadWrite(&coarse_flags);
        let region_rw = Binding::ReadWrite(&region);

        let rasterize = pass(
            "rasterize bristles",
            "rasterize_bristles",
            &[(9, bristle_rw), (11, Binding::ReadOnly(&brush.curves)), (12, Binding::ReadOnly(&previous_curves)), (22, region_rw)],
        );
        let advect_forward = pass(
            "advect forward",
            "advect_forward",
            &[(2, volume), (3, Binding::ReadWrite(&volume_next)), (4, velocity_rw), (22, region_rw)],
        );
        let advect_backward = pass(
            "advect backward",
            "advect_backward",
            &[(2, volume), (3, Binding::ReadWrite(&volume_next)), (4, velocity_rw), (22, region_rw)],
        );
        let boundaries = pass(
            "apply boundaries",
            "apply_boundaries",
            &[(2, volume), (4, velocity_rw), (9, bristle_rw), (16, particle_volume_rw), (21, Binding::ReadWrite(&stroke_base)), (22, region_rw)],
        );
        let grow_region = pass("grow region", "grow_region", &[(11, Binding::ReadOnly(&brush.curves)), (22, region_rw)]);
        let column_bindings = [(2, volume), (24, Binding::ReadWrite(&columns)), (25, Binding::ReadWrite(&column_pigment))];
        let snapshot_columns = pass("snapshot columns", "snapshot_columns", &column_bindings);
        let level_columns = pass("level columns", "level_columns", &column_bindings);
        let finalize_bounds = pass("finalize bounds", "finalize_bounds", &[(22, region_rw), (23, Binding::ReadWrite(&indirect))]);
        let scan_region = pass("scan region", "scan_region", &[(21, Binding::ReadWrite(&stroke_base)), (22, region_rw)]);
        let finalize_depth = pass("finalize depth", "finalize_depth", &[(22, region_rw), (23, Binding::ReadWrite(&indirect))]);
        let flag_regions = pass("flag bristle regions", "flag_bristle_regions", &[(11, Binding::ReadOnly(&brush.curves)), (18, coarse_rw)]);
        let emit = pass(
            "emit particles",
            "emit_particles",
            &[(2, volume), (4, velocity_rw), (13, particles_in_rw), (15, counters_rw), (18, coarse_rw), (22, region_rw)],
        );
        let rasterize_particles = pass(
            "rasterize particles",
            "rasterize_particles",
            &[(13, particles_in_rw), (15, counters_rw), (16, particle_volume_rw), (17, particle_velocity_rw)],
        );
        let splat_particles = pass(
            "splat particles",
            "splat_particles",
            &[(13, particles_in_rw), (15, counters_rw), (16, particle_volume_rw)],
        );
        let merge_velocity = pass(
            "merge particle velocity",
            "merge_particle_velocity",
            &[(2, volume), (4, velocity_rw), (16, particle_volume_rw), (17, particle_velocity_rw)],
        );
        let update_particles = pass(
            "update particles",
            "update_particles",
            &[(4, velocity_rw), (9, bristle_rw), (13, particles_in_rw), (15, counters_rw), (19, Binding::ReadWrite(&velocity_before))],
        );
        let return_bindings = [(9, bristle_rw), (13, particles_in_rw), (15, counters_rw), (18, coarse_rw), (20, volume)];
        let return_resting = pass("return resting particles", "return_resting_particles", &return_bindings);
        let return_all = pass("return all particles", "return_all_particles", &return_bindings);
        let compact = pass(
            "compact particles",
            "compact_particles",
            &[(13, particles_in_rw), (14, Binding::ReadWrite(&particles_out)), (15, counters_rw)],
        );
        let finish_compaction = pass("finish compaction", "finish_compaction", &[(15, counters_rw)]);
        let diffuse_forward = pass("diffuse forward", "diffuse_forward", &[(4, velocity_rw), (5, Binding::ReadWrite(&velocity_next)), (22, region_rw)]);
        let diffuse_backward = pass("diffuse backward", "diffuse_backward", &[(4, velocity_rw), (5, Binding::ReadWrite(&velocity_next)), (22, region_rw)]);
        let projection_bindings = |pressure_pair: bool| -> Vec<(u32, Binding)> {
            let mut list = vec![
                (2, volume),
                (4, velocity_rw),
                (6, Binding::ReadWrite(&pressure)),
                (8, Binding::ReadWrite(&divergence)),
                (9, bristle_rw),
                (16, particle_volume_rw),
                (22, region_rw),
            ];
            if pressure_pair {
                list.push((7, Binding::ReadWrite(&pressure_next)));
            }
            list
        };
        let divergence_pass = pass("compute divergence", "compute_divergence", &projection_bindings(false));
        let jacobi_forward = pass("jacobi forward", "jacobi_forward", &projection_bindings(true));
        let jacobi_backward = pass("jacobi backward", "jacobi_backward", &projection_bindings(true));
        let gradient = pass("apply gradient", "apply_gradient", &projection_bindings(false));
        let dry = pass("dry", "dry", &[(2, volume), (22, region_rw)]);
        let dry_all = pass("dry all", "dry_all", &[(2, volume)]);
        let settle = pass("settle columns", "settle_columns", &[(2, volume), (22, region_rw)]);

        Self {
            settings,
            uniform,
            velocity,
            velocity_before,
            previous_curves,
            bristle,
            particle_volume,
            particle_velocity,
            particles_in,
            particles_out,
            counters,
            coarse_flags,
            stroke_base,
            region,
            indirect,
            snapshot_columns,
            level_columns,
            grow_region,
            finalize_bounds,
            scan_region,
            finalize_depth,
            rasterize,
            flag_regions,
            emit,
            rasterize_particles,
            splat_particles,
            merge_velocity,
            update_particles,
            return_resting,
            return_all,
            compact,
            finish_compaction,
            advect_forward,
            advect_backward,
            boundaries,
            diffuse_forward,
            diffuse_backward,
            divergence: divergence_pass,
            jacobi_forward,
            jacobi_backward,
            gradient,
            dry,
            dry_all,
            settle,
            sample_count,
            cells,
            face_slots,
        }
    }

    pub fn clear_velocity(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.velocity, 0, None);
    }

    pub fn encode_levelling(&self, encoder: &mut wgpu::CommandEncoder) {
        let column_count = WINDOW_SIZE.x * WINDOW_SIZE.y;
        self.snapshot_columns.dispatch_linear(encoder, column_count, CELL_WORKGROUP);
        self.level_columns.dispatch_linear(encoder, column_count, CELL_WORKGROUP);
    }

    pub fn encode_capture_stroke_base(&self, encoder: &mut wgpu::CommandEncoder, heights: &wgpu::Buffer) {
        encoder.copy_buffer_to_buffer(heights, 0, &self.stroke_base, 0, self.stroke_base.size());
    }

    pub fn encode_step(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, brush: &Brush, brush_speed: f32, dt: f32) {
        let substeps = ((brush_speed.max(MIN_SPEED_FOR_SUBSTEPS) * dt).ceil() as u32)
            .clamp(2, MAX_ADVECTION_SUBSTEPS)
            .next_multiple_of(2);
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&FluidUniform {
                dt,
                damping: self.settings.damping,
                viscosity: self.settings.viscosity,
                dryness_rate: self.settings.dryness_rate,
                sample_count: self.sample_count,
                samples_per_bristle: SAMPLES_PER_BRISTLE,
                advection_dt: dt / substeps as f32,
                gravity: self.settings.gravity,
                yield_speed: self.settings.yield_speed,
                _padding: [0.0; 3],
            }),
        );
        gpu.queue.write_buffer(&self.region, 0, bytemuck::cast_slice(&[i32::MAX, i32::MAX, 0, i32::MIN, i32::MIN, 0, 0]));
        self.grow_region.dispatch_linear(encoder, self.sample_count, SAMPLE_WORKGROUP);
        self.finalize_bounds.dispatch(encoder, [1, 1, 1]);
        self.scan_region.dispatch_indirect(encoder, &self.indirect, 24);
        self.finalize_depth.dispatch(encoder, [1, 1, 1]);
        encoder.clear_buffer(&self.bristle, 0, None);
        encoder.clear_buffer(&self.coarse_flags, 0, None);
        self.rasterize.dispatch_linear(encoder, self.sample_count, SAMPLE_WORKGROUP);
        if self.settings.particles {
            self.flag_regions.dispatch_linear(encoder, self.sample_count, SAMPLE_WORKGROUP);
            self.emit.dispatch_linear(encoder, self.cells, CELL_WORKGROUP);
            self.encode_particle_rasterization(encoder);
            self.merge_velocity.dispatch_linear(encoder, self.face_slots, CELL_WORKGROUP);
            encoder.copy_buffer_to_buffer(&self.velocity, 0, &self.velocity_before, 0, self.velocity.size());
        }
        self.boundaries.dispatch_indirect(encoder, &self.indirect, 12);
        for _ in 0..DIFFUSION_ROUNDS {
            self.diffuse_forward.dispatch_indirect(encoder, &self.indirect, 12);
            self.diffuse_backward.dispatch_indirect(encoder, &self.indirect, 12);
        }
        self.boundaries.dispatch_indirect(encoder, &self.indirect, 12);
        self.encode_projection(encoder);
        if self.settings.particles {
            self.update_particles.dispatch_linear(encoder, PARTICLE_CAPACITY, SAMPLE_WORKGROUP);
            self.return_resting.dispatch_linear(encoder, PARTICLE_CAPACITY, SAMPLE_WORKGROUP);
            self.encode_compaction(encoder);
            encoder.clear_buffer(&self.particle_volume, 0, None);
            self.splat_particles.dispatch_linear(encoder, PARTICLE_CAPACITY, SAMPLE_WORKGROUP);
        }
        for _ in 0..substeps / 2 {
            self.advect_forward.dispatch_indirect(encoder, &self.indirect, 0);
            self.advect_backward.dispatch_indirect(encoder, &self.indirect, 0);
        }
        self.settle.dispatch_indirect(encoder, &self.indirect, 24);
        self.dry.dispatch_indirect(encoder, &self.indirect, 0);
        encoder.copy_buffer_to_buffer(&brush.curves, 0, &self.previous_curves, 0, brush.curves.size());
    }

    fn encode_particle_rasterization(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.particle_volume, 0, None);
        encoder.clear_buffer(&self.particle_velocity, 0, None);
        self.rasterize_particles.dispatch_linear(encoder, PARTICLE_CAPACITY, SAMPLE_WORKGROUP);
    }

    fn encode_compaction(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.counters, 4, Some(4));
        self.compact.dispatch_linear(encoder, PARTICLE_CAPACITY, SAMPLE_WORKGROUP);
        encoder.copy_buffer_to_buffer(&self.particles_out, 0, &self.particles_in, 0, self.particles_in.size());
        self.finish_compaction.dispatch(encoder, [1, 1, 1]);
    }

    pub fn submit_return_all_particles(&self, gpu: &Gpu) {
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("return particles") });
        self.return_all.dispatch_linear(&mut encoder, PARTICLE_CAPACITY, SAMPLE_WORKGROUP);
        self.encode_compaction(&mut encoder);
        encoder.clear_buffer(&self.particle_volume, 0, None);
        encoder.clear_buffer(&self.particle_velocity, 0, None);
        gpu.queue.submit(Some(encoder.finish()));
    }

    pub fn encode_dry_all(&self, encoder: &mut wgpu::CommandEncoder) {
        self.dry_all.dispatch_linear(encoder, self.cells, CELL_WORKGROUP);
    }

    pub fn encode_projection(&self, encoder: &mut wgpu::CommandEncoder) {
        for _ in 0..PROJECTION_ROUNDS {
            self.divergence.dispatch_indirect(encoder, &self.indirect, 0);
            self.jacobi_forward.dispatch_indirect(encoder, &self.indirect, 0);
            self.jacobi_backward.dispatch_indirect(encoder, &self.indirect, 0);
            self.gradient.dispatch_indirect(encoder, &self.indirect, 0);
        }
    }
}
