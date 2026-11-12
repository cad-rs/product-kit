//! An offscreen color target that can be read back: the color texture and an
//! aligned readback buffer.
//!
//! Every desktop can present a frame this way (render offscreen, copy the
//! pixels back, paint them as an image), so the target is the one piece of the
//! GPU view slot that is not platform-specific. It is a separate crate from
//! the slot because headless render cores (no GUI dependency) read back too:
//! the slot's readback presenter and a core's pixel tests share this
//! implementation instead of keeping two copies of the row-unpadding logic.

/// An offscreen color target that can be read back (color texture + aligned
/// readback buffer).
pub struct ReadbackTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    buf: wgpu::Buffer,
    width: u32,
    height: u32,
    bytes_per_row: u32,
}

impl ReadbackTarget {
    /// Allocates the target at `width` x `height` device pixels (both clamped
    /// to at least 1). `format` is the color format the renderer draws in.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gpu-readback-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let bytes_per_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-readback-buffer"),
            size: bytes_per_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&Default::default());
        Self {
            texture,
            view,
            buf,
            width,
            height,
            bytes_per_row,
        }
    }

    /// Size (device pixels).
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The render target view.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Copies the frame out and blocks until done: tightly packed BGRA rows
    /// (`width * height * 4`, row padding removed).
    pub fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<u8>> {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu-readback-enc"),
        });
        enc.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.bytes_per_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(enc.finish()));
        let (sx, rx) = std::sync::mpsc::channel();
        let slice = self.buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = sx.send(res);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .ok()?;
        rx.recv().ok()?.ok()?;
        let data = slice.get_mapped_range().ok()?;
        let row = self.width as usize * 4;
        let mut out = Vec::with_capacity(row * self.height as usize);
        for chunk in data.chunks(self.bytes_per_row as usize).take(self.height as usize) {
            out.extend_from_slice(&chunk[..row]);
        }
        drop(data);
        self.buf.unmap();
        Some(out)
    }
}
