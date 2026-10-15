use super::Gpu;

const ROW_ALIGNMENT: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

pub fn read_rgba8(gpu: &Gpu, texture: &wgpu::Texture) -> image::RgbaImage {
    let size = texture.size();
    let unpadded_row = size.width * 4;
    let padded_row = unpadded_row.div_ceil(ROW_ALIGNMENT) * ROW_ALIGNMENT;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded_row * size.height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    gpu.queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| sender.send(result).unwrap());
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll failed");
    receiver.recv().unwrap().expect("readback map failed");

    let data = slice.get_mapped_range().expect("readback range");
    let mut pixels = Vec::with_capacity((unpadded_row * size.height) as usize);
    for row in data.chunks(padded_row as usize) {
        pixels.extend_from_slice(&row[..unpadded_row as usize]);
    }
    drop(data);
    buffer.unmap();
    image::RgbaImage::from_raw(size.width, size.height, pixels).expect("readback size mismatch")
}

pub fn read_buffer<T: bytemuck::Pod>(gpu: &Gpu, source: &wgpu::Buffer) -> Vec<T> {
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("buffer readback"),
        size: source.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, source.size());
    gpu.queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| sender.send(result).unwrap());
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll failed");
    receiver.recv().unwrap().expect("readback map failed");
    let data = slice.get_mapped_range().expect("readback range");
    let values = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    staging.unmap();
    values
}
