//! [`create_buffer_filled`]: a buffer holding given bytes, filled through the
//! queue. Every buffer this crate fills when it makes it goes through here
//! (the SSAA blit's uniforms and quad, the blend-mode proxy blit's uniforms
//! through `texture_life`, the tumble cube's vertices and indices).
//!
//! Why not `wgpu::util::DeviceExt::create_buffer_init`: it makes the buffer
//! mapped and copies the bytes in through its mapped range, and wgpu has no
//! fallible way to take a mapped range. Once the device is lost every buffer
//! made on it is invalid, so that copy panics the app on the first frame
//! drawn after the loss, before the shell has looked at `device_lost` and
//! rebuilt. `queue.write_buffer` on the same invalid buffer reports an error
//! to the device's error handler instead (the shells install
//! [`crate::UncapturedErrors`], which logs and counts it), so the frame is
//! lost and the app carries on.

/// Make a buffer holding `desc.contents` (with `desc.usage` plus
/// `COPY_DST`), filled by `queue.write_buffer`.
///
/// `MAP_READ` and `MAP_WRITE` usages are not supported: wgpu allows
/// `MAP_WRITE` only beside `COPY_SRC`, so the added `COPY_DST` makes
/// `MAP_WRITE | COPY_DST` an invalid buffer, and a mapped buffer is what
/// this function exists to avoid.
///
/// The buffer has the size `create_buffer_init` gives it: the contents
/// rounded up to wgpu's copy alignment, the padding zero, so the bytes a
/// shader reads are the same. Empty contents make an empty buffer with
/// nothing written. The write lands before the commands of the next
/// `queue.submit`, so commands recorded with this buffer into an encoder
/// submitted after this call see the contents.
pub fn create_buffer_filled(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    desc: &wgpu::util::BufferInitDescriptor<'_>,
) -> wgpu::Buffer {
    let usage = desc.usage | wgpu::BufferUsages::COPY_DST;
    if desc.contents.is_empty() {
        return device.create_buffer(&wgpu::BufferDescriptor {
            label: desc.label,
            size: 0,
            usage,
            mapped_at_creation: false,
        });
    }
    let unpadded = desc.contents.len() as u64;
    let align_mask = wgpu::COPY_BUFFER_ALIGNMENT - 1;
    let padded = ((unpadded + align_mask) & !align_mask).max(wgpu::COPY_BUFFER_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: desc.label,
        size: padded,
        usage,
        mapped_at_creation: false,
    });
    if padded == unpadded {
        queue.write_buffer(&buffer, 0, desc.contents);
    } else {
        // `write_buffer` takes whole copy-alignment units; the tail is zero,
        // as `create_buffer_init` leaves it.
        let mut bytes = Vec::with_capacity(padded as usize);
        bytes.extend_from_slice(desc.contents);
        bytes.resize(padded as usize, 0);
        queue.write_buffer(&buffer, 0, &bytes);
    }
    buffer
}
