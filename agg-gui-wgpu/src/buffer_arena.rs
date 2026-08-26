//! Per-frame GPU buffer pool — a chunked, growable arena that recycles
//! `wgpu::Buffer` allocations across frames.
//!
//! ## Why this exists
//!
//! Before this module, `end_frame_prepare.rs` was calling
//! `device.create_buffer_init(...)` *per [`DrawCommand`]*: one vertex buffer,
//! one index buffer, and one uniform buffer for each Solid / AaSolid /
//! Gradient / Textured / Lcd / layer command.  A moderately complex scene
//! (atomartist's 3-D viewport + node canvas + HUD) emits ~200 commands per
//! frame, which translates to ~600 wgpu buffer allocations every single
//! frame.  Each `create_buffer_init` is a full GPU memory allocation under a
//! mutex in the wgpu driver, and the per-call overhead dominated the frame
//! budget — measurements on a release build showed ~9 ms in `prepare_all`
//! for ~213 commands (43 % of total frame time).
//!
//! ## How it works
//!
//! Three [`GpuArena`] instances live on [`crate::WgpuGfxCtx`] — one each for
//! vertex / index / uniform usage.  At the start of every flush, the host
//! calls [`GpuArena::begin_frame`], which resets the write cursor to chunk 0,
//! offset 0.  Each `alloc` advances the cursor: data is written via
//! `queue.write_buffer` into the *existing* chunk and the caller receives an
//! `Arc<Buffer>` handle plus the byte offset of the allocation.
//!
//! Chunks are **never resized in place** — when a chunk fills up the arena
//! moves to the next chunk, creating it lazily on first use.  Existing bind
//! groups and vertex / index slices that referenced the earlier chunk stay
//! valid because each chunk is owned by an `Arc` and the bind group / slice
//! holds an internal reference to that exact buffer.  Resizing in place
//! would invalidate every prior allocation in the same frame.
//!
//! ## Alignment
//!
//! `queue.write_buffer` requires offsets be multiples of
//! `wgpu::COPY_BUFFER_ALIGNMENT` (4 bytes).  Uniform bindings additionally
//! require offsets be multiples of
//! `Limits::min_uniform_buffer_offset_alignment` (256 bytes on D3D12 / many
//! Vulkan drivers).  Callers pass the alignment they need at construction
//! time and the arena rounds every allocation up accordingly.

use std::sync::Arc;

use wgpu::Buffer;

/// A growable, chunked GPU buffer pool.  See module docs.
pub(crate) struct GpuArena {
    /// Live chunks, in allocation order.  `Arc` so that allocations handed
    /// out earlier in the frame keep their chunk alive even after the arena
    /// has moved on to a newer one.
    chunks: Vec<Arc<Buffer>>,
    /// Capacity of each chunk in bytes.  Tracked separately because
    /// `wgpu::Buffer::size()` is available but cheaper to read from a Vec.
    chunk_caps: Vec<u64>,

    /// Which chunk we're currently writing into (index into `chunks`).
    cur_chunk: usize,
    /// Bytes already written into the current chunk.
    cur_used: u64,

    /// Default size for newly-allocated chunks.  Single allocations larger
    /// than this still succeed — the new chunk grows to whatever the
    /// request needs.
    chunk_size: u64,
    /// Per-allocation alignment.  Must be a power of two.
    alignment: u64,
    /// Buffer usage flags applied to every chunk.  `COPY_DST` is OR'ed in
    /// automatically (needed for `queue.write_buffer`).
    usage: wgpu::BufferUsages,
    /// Debug label applied to every chunk.
    label: &'static str,
}

impl GpuArena {
    /// Create a new arena with one pre-allocated chunk of `chunk_size` bytes.
    pub fn new(
        device: &wgpu::Device,
        chunk_size: u64,
        alignment: u64,
        usage: wgpu::BufferUsages,
        label: &'static str,
    ) -> Self {
        debug_assert!(
            alignment.is_power_of_two(),
            "GpuArena alignment must be a power of two"
        );
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: chunk_size,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            chunks: vec![Arc::new(buf)],
            chunk_caps: vec![chunk_size],
            cur_chunk: 0,
            cur_used: 0,
            chunk_size,
            alignment,
            usage,
            label,
        }
    }

    /// Reset the write cursor.  Call once at the start of every flush — the
    /// existing chunks are kept and immediately reused.
    pub fn begin_frame(&mut self) {
        self.cur_chunk = 0;
        self.cur_used = 0;
    }

    /// Allocate `data.len()` bytes (rounded up to alignment), upload `data`
    /// via `queue.write_buffer`, and return `(buffer, offset, size)`.
    ///
    /// `size` is the *original* byte count, not the aligned-up amount — use
    /// it when sizing bind-group / vertex-buffer slices.  `offset` is always
    /// aligned to `alignment`.
    pub fn alloc(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: &[u8],
    ) -> (Arc<Buffer>, u64, u64) {
        let size = data.len() as u64;
        let aligned = round_up(size, self.alignment);

        // If the request won't fit in the rest of the current chunk, move on
        // to the next one (creating / replacing it as necessary).  We never
        // resize the current chunk in place — see module docs for why.
        if self.cur_used + aligned > self.chunk_caps[self.cur_chunk] {
            self.cur_chunk += 1;
            self.cur_used = 0;
            let needed = aligned.max(self.chunk_size);
            if self.cur_chunk >= self.chunks.len() {
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(self.label),
                    size: needed,
                    usage: self.usage | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.chunks.push(Arc::new(buf));
                self.chunk_caps.push(needed);
            } else if self.chunk_caps[self.cur_chunk] < needed {
                // Existing chunk left over from a previous frame, but too
                // small for this allocation — replace it.  The prior Arc is
                // dropped now; any bind groups still holding it from last
                // frame keep their own internal reference alive.
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(self.label),
                    size: needed,
                    usage: self.usage | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.chunks[self.cur_chunk] = Arc::new(buf);
                self.chunk_caps[self.cur_chunk] = needed;
            }
        }

        let offset = self.cur_used;
        self.cur_used += aligned;
        let buf = Arc::clone(&self.chunks[self.cur_chunk]);
        queue.write_buffer(&buf, offset, data);
        (buf, offset, size)
    }
}

/// Per-frame bundle of arenas owned by [`crate::WgpuGfxCtx`].  Pulled out as
/// its own struct so `end_frame_prepare` can take a single `&mut FrameArenas`
/// reference (instead of three split borrows of `WgpuGfxCtx` fields) without
/// fighting the borrow checker.
pub(crate) struct FrameArenas {
    pub vertex: GpuArena,
    pub index: GpuArena,
    pub uniform: GpuArena,
}

impl FrameArenas {
    /// Construct with sensible per-arena defaults.  256 KB initial chunk
    /// covers a typical frame; the uniform alignment comes from the active
    /// device's `min_uniform_buffer_offset_alignment` limit (256 on D3D12,
    /// often the same on Vulkan).
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform_align = device.limits().min_uniform_buffer_offset_alignment as u64;
        let chunk = 256 * 1024;
        Self {
            vertex: GpuArena::new(
                device,
                chunk,
                wgpu::COPY_BUFFER_ALIGNMENT,
                wgpu::BufferUsages::VERTEX,
                "frame-vertex-arena",
            ),
            index: GpuArena::new(
                device,
                chunk,
                wgpu::COPY_BUFFER_ALIGNMENT,
                wgpu::BufferUsages::INDEX,
                "frame-index-arena",
            ),
            uniform: GpuArena::new(
                device,
                chunk,
                uniform_align,
                wgpu::BufferUsages::UNIFORM,
                "frame-uniform-arena",
            ),
        }
    }

    /// Reset all three arenas' write cursors.  Called from `flush_to_surface`.
    pub fn begin_frame(&mut self) {
        self.vertex.begin_frame();
        self.index.begin_frame();
        self.uniform.begin_frame();
    }
}

#[inline]
fn round_up(n: u64, align: u64) -> u64 {
    (n + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::round_up;

    #[test]
    fn round_up_basics() {
        assert_eq!(round_up(0, 4), 0);
        assert_eq!(round_up(1, 4), 4);
        assert_eq!(round_up(4, 4), 4);
        assert_eq!(round_up(5, 4), 8);
        assert_eq!(round_up(255, 256), 256);
        assert_eq!(round_up(256, 256), 256);
        assert_eq!(round_up(257, 256), 512);
    }
}
