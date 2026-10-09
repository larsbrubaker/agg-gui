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
//! offset 0.  Each `alloc` advances the cursor: data is copied into a
//! CPU-side staging `Vec` that mirrors the *existing* chunk, and the caller
//! receives an `Arc<Buffer>` handle plus the byte offset of the allocation.
//! After the prepare walk, [`GpuArena::flush`] uploads each chunk's staged
//! bytes with a single `queue.write_buffer` — before the frame's
//! `queue.submit`, so the render passes see the data.
//!
//! ## Why the uploads are batched
//!
//! wgpu implements every `queue.write_buffer` call with a freshly created
//! staging buffer that is destroyed once the GPU is done with it.  Uploading
//! per allocation (~3 per draw command) meant ~660 Metal buffer creations
//! and destructions per frame for the demo's ~220 commands.  Profiling an
//! idle-scene repaint on Metal put about half of the busy main-thread time
//! in `write_buffer` and another third in destroying those staging buffers
//! inside `device.poll`.  One write per chunk per frame keeps the arena's
//! offsets unchanged while cutting that to a handful.  Each staging `Vec`
//! is reserved to exactly its chunk's capacity when the chunk is created
//! or replaced and keeps that capacity across frames, so steady-state
//! frames never reallocate them and CPU retention matches the GPU chunks
//! (no doubling-growth slack left behind by one large frame).
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
    /// CPU-side copy of this frame's bytes for each chunk, parallel to
    /// `chunks`.  `staging[i].len()` is the number of bytes allocated from
    /// chunk `i` this frame (always a multiple of `alignment`, alignment
    /// padding zero-filled).  `staging[i].capacity()` equals
    /// `chunk_caps[i]`, and since a chunk never takes more than its
    /// capacity, appending never reallocates.  Cleared — capacity kept — by
    /// `begin_frame`; uploaded by `flush`.
    staging: Vec<Vec<u8>>,

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
        // `flush` uploads whole staged chunks, so their lengths (multiples
        // of `alignment`) must satisfy `write_buffer`'s size rule.
        debug_assert!(
            alignment >= wgpu::COPY_BUFFER_ALIGNMENT,
            "GpuArena alignment must be at least COPY_BUFFER_ALIGNMENT"
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
            staging: vec![Vec::with_capacity(chunk_size as usize)],
            cur_chunk: 0,
            cur_used: 0,
            chunk_size,
            alignment,
            usage,
            label,
        }
    }

    /// Reset the write cursor.  Call once at the start of every flush — the
    /// existing chunks are kept and immediately reused, and the staging
    /// `Vec`s are emptied without releasing their capacity.
    pub fn begin_frame(&mut self) {
        self.cur_chunk = 0;
        self.cur_used = 0;
        for staged in &mut self.staging {
            staged.clear();
        }
    }

    /// Allocate `data.len()` bytes (rounded up to alignment), stage `data`
    /// for upload, and return `(buffer, offset, size)`.  The bytes reach the
    /// GPU buffer on the next [`Self::flush`], which must run before the
    /// frame's `queue.submit`.
    ///
    /// `size` is the *original* byte count, not the aligned-up amount — use
    /// it when sizing bind-group / vertex-buffer slices.  `offset` is always
    /// aligned to `alignment`.
    pub fn alloc(&mut self, device: &wgpu::Device, data: &[u8]) -> (Arc<Buffer>, u64, u64) {
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
                self.staging.push(Vec::with_capacity(needed as usize));
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
                // The cursor only moves forward within a frame, so nothing
                // has been staged in this chunk since `begin_frame`; grow
                // its staging to exactly the new capacity.
                let staged = &mut self.staging[self.cur_chunk];
                debug_assert!(staged.is_empty());
                staged.reserve_exact(needed as usize);
            }
        }

        let offset = self.cur_used;
        self.cur_used += aligned;
        let staged = &mut self.staging[self.cur_chunk];
        debug_assert_eq!(staged.len() as u64, offset);
        staged.extend_from_slice(data);
        staged.resize(self.cur_used as usize, 0);
        let buf = Arc::clone(&self.chunks[self.cur_chunk]);
        (buf, offset, size)
    }

    /// Upload everything staged since [`Self::begin_frame`]: one
    /// `queue.write_buffer` per chunk used this frame, each starting at
    /// offset 0.  Call after the last `alloc` and before the `queue.submit`
    /// whose render passes read the data.  Staged bytes stay in place until
    /// the next `begin_frame`, so a later `alloc` + `flush` in the same frame
    /// still uploads correct (if partly redundant) contents.
    pub fn flush(&self, queue: &wgpu::Queue) {
        // `alloc` creates a chunk as soon as the cursor reaches it, so
        // `cur_chunk` always indexes an existing chunk.
        let used = self.cur_chunk + 1;
        for (buf, staged) in self.chunks[..used].iter().zip(&self.staging) {
            if !staged.is_empty() {
                queue.write_buffer(buf, 0, staged);
            }
        }
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

    /// Upload all three arenas' staged bytes.  Called from
    /// `flush_to_surface` after `prepare_all` and before `queue.submit`.
    pub fn flush(&self, queue: &wgpu::Queue) {
        self.vertex.flush(queue);
        self.index.flush(queue);
        self.uniform.flush(queue);
    }
}

#[inline]
fn round_up(n: u64, align: u64) -> u64 {
    (n + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::{round_up, GpuArena};
    use crate::layer_text_readback_tests::try_device;

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

    /// Copy `size` bytes at `offset` of `src` back to the CPU.
    fn read_back(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        src: &wgpu::Buffer,
        offset: u64,
        size: u64,
    ) -> Vec<u8> {
        let dst = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("arena-test-readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        enc.copy_buffer_to_buffer(src, offset, &dst, 0, size);
        queue.submit(std::iter::once(enc.finish()));
        let slice = dst.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().unwrap().unwrap();
        let data = slice.get_mapped_range().to_vec();
        dst.unmap();
        data
    }

    /// Allocations are staged and only reach the GPU on `flush`; offsets,
    /// chunk advances (including an oversized request that needs its own,
    /// larger chunk, and one that must replace a smaller chunk left over
    /// from an earlier frame) and per-frame reuse must all land the right
    /// bytes at the offsets `alloc` handed out.
    #[test]
    fn flush_uploads_every_staged_allocation_at_its_offset() {
        let Some((device, queue)) = try_device() else {
            return;
        };
        // Tiny chunks force chunk advances; COPY_SRC lets the test read back.
        let mut arena = GpuArena::new(&device, 64, 16, wgpu::BufferUsages::COPY_SRC, "test");

        let a = [1u8; 20];
        let b = [2u8; 40];
        let c = [3u8; 100];
        let d = [4u8; 8];
        let e = [5u8; 4];
        arena.begin_frame();
        let (buf_a, off_a, size_a) = arena.alloc(&device, &a);
        let (buf_b, off_b, _) = arena.alloc(&device, &b);
        let (buf_c, off_c, _) = arena.alloc(&device, &c);
        let (buf_d, off_d, _) = arena.alloc(&device, &d);
        let (buf_e, off_e, _) = arena.alloc(&device, &e);
        // Offsets are what the per-allocation-upload arena handed out.
        assert_eq!((off_a, size_a), (0, 20));
        assert_eq!(
            off_b, 0,
            "40 bytes at aligned offset 32 overflow a 64-byte chunk"
        );
        assert!(!std::sync::Arc::ptr_eq(&buf_a, &buf_b));
        assert_eq!(off_c, 0, "oversized request gets its own chunk");
        assert_eq!(buf_c.size(), 112, "chunk grown to the aligned request");
        assert_eq!(off_d, 0);
        assert_eq!(off_e, 16, "8 bytes pad to 16 before the next allocation");
        assert!(std::sync::Arc::ptr_eq(&buf_d, &buf_e));
        arena.flush(&queue);

        let cases: [(&wgpu::Buffer, u64, &[u8]); 5] = [
            (&buf_a, off_a, &a),
            (&buf_b, off_b, &b),
            (&buf_c, off_c, &c),
            (&buf_d, off_d, &d),
            (&buf_e, off_e, &e),
        ];
        for (i, (buf, off, want)) in cases.into_iter().enumerate() {
            let got = read_back(&device, &queue, buf, off, want.len() as u64);
            assert_eq!(got, want, "allocation {i} bytes");
        }

        // Next frame reuses chunk 0 and must upload only the new bytes.
        // Then an oversized request overflows chunk 0 onto chunk 1 — still
        // the 64-byte chunk from the first frame — which must be replaced
        // by one large enough, with its staged bytes uploaded to the
        // replacement.
        arena.begin_frame();
        let f = [6u8; 12];
        let g = [7u8; 200];
        let (buf_f, off_f, _) = arena.alloc(&device, &f);
        assert!(std::sync::Arc::ptr_eq(&buf_a, &buf_f));
        assert_eq!(off_f, 0);
        let (buf_g, off_g, _) = arena.alloc(&device, &g);
        assert!(
            !std::sync::Arc::ptr_eq(&buf_b, &buf_g),
            "chunk 1 is replaced, not reused"
        );
        assert_eq!(off_g, 0);
        assert_eq!(
            buf_g.size(),
            208,
            "replacement grown to the aligned request"
        );
        arena.flush(&queue);
        assert_eq!(read_back(&device, &queue, &buf_f, 0, 12), f);
        assert_eq!(read_back(&device, &queue, &buf_g, off_g, 200), g);
    }

    /// Each chunk's staging `Vec` holds exactly the chunk's capacity: when
    /// the chunk is created, when an oversized request replaces it, and
    /// across steady-state frames, which must not reallocate it.  Doubling
    /// growth would leave up to 2x the chunk pinned after one large frame.
    /// (Relies on std's `with_capacity` / `reserve_exact` allocating the
    /// exact `u8` count requested.)
    #[test]
    fn staging_capacity_matches_chunk_capacity_and_is_kept() {
        let Some((device, _queue)) = try_device() else {
            return;
        };
        let mut arena = GpuArena::new(&device, 64, 16, wgpu::BufferUsages::COPY_SRC, "test");
        let assert_caps = |arena: &GpuArena, when: &str| {
            assert_eq!(arena.staging.len(), arena.chunks.len());
            for (i, (staged, &cap)) in arena.staging.iter().zip(&arena.chunk_caps).enumerate() {
                assert_eq!(staged.capacity() as u64, cap, "{when}: chunk {i} staging");
            }
        };

        // Frame 1: two 64-byte chunks, the second filled by two allocations.
        arena.begin_frame();
        arena.alloc(&device, &[1u8; 40]);
        arena.alloc(&device, &[2u8; 40]);
        arena.alloc(&device, &[3u8; 8]);
        assert_eq!(arena.chunk_caps, [64, 64]);
        assert_caps(&arena, "created");

        // Frame 2: an oversized request replaces chunk 1.
        let large_frame = |arena: &mut GpuArena| {
            arena.begin_frame();
            arena.alloc(&device, &[4u8; 12]);
            arena.alloc(&device, &[5u8; 200]);
        };
        large_frame(&mut arena);
        assert_eq!(arena.chunk_caps, [64, 208]);
        assert_caps(&arena, "replaced");

        // Frame 3 repeats frame 2: no staging reallocation.
        let ptrs: Vec<*const u8> = arena.staging.iter().map(|s| s.as_ptr()).collect();
        large_frame(&mut arena);
        assert_eq!(arena.chunk_caps, [64, 208]);
        assert_caps(&arena, "steady state");
        let after: Vec<*const u8> = arena.staging.iter().map(|s| s.as_ptr()).collect();
        assert_eq!(ptrs, after, "steady-state frame reallocated staging");
    }
}
