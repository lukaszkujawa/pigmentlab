use super::Gpu;

const MAX_GROUPS_PER_DIMENSION: u32 = 65_535;

#[derive(Clone, Copy)]
pub enum Binding<'a> {
    Uniform(&'a wgpu::Buffer),
    ReadOnly(&'a wgpu::Buffer),
    ReadWrite(&'a wgpu::Buffer),
}

pub struct ComputePass {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    label: &'static str,
}

impl ComputePass {
    pub fn new(gpu: &Gpu, label: &'static str, shader: &wgpu::ShaderModule, entry: &str, bindings: &[(u32, Binding)]) -> Self {
        let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
            .iter()
            .map(|(index, binding)| wgpu::BindGroupLayoutEntry {
                binding: *index,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: match binding {
                        Binding::Uniform(_) => wgpu::BufferBindingType::Uniform,
                        Binding::ReadOnly(_) => wgpu::BufferBindingType::Storage { read_only: true },
                        Binding::ReadWrite(_) => wgpu::BufferBindingType::Storage { read_only: false },
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries });
        let resources: Vec<wgpu::BindGroupEntry> = bindings
            .iter()
            .map(|(index, binding)| {
                let buffer = match binding {
                    Binding::Uniform(b) | Binding::ReadOnly(b) | Binding::ReadWrite(b) => *b,
                };
                wgpu::BindGroupEntry { binding: *index, resource: buffer.as_entire_binding() }
            })
            .collect();
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &layout,
            entries: &resources,
        });
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            module: shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline, bind_group, label }
    }

    pub fn dispatch_linear(&self, encoder: &mut wgpu::CommandEncoder, items: u32, workgroup_size: u32) {
        let groups = items.div_ceil(workgroup_size);
        let columns = groups.min(MAX_GROUPS_PER_DIMENSION);
        self.dispatch(encoder, [columns, groups.div_ceil(columns), 1]);
    }

    pub fn dispatch(&self, encoder: &mut wgpu::CommandEncoder, workgroups: [u32; 3]) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(self.label),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.dispatch_workgroups(workgroups[0], workgroups[1], workgroups[2]);
    }
}
