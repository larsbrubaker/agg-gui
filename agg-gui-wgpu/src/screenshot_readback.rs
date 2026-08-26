//! Frame screenshot read-back for [`WgpuGfxCtx`].
//!
//! Split out of `lib.rs` (800-line guardrail). Owns the surface-texture
//! stash and the GPU->CPU copy path used by
//! `agg_gui::screenshot::run_frame_with_capture`: the platform shell
//! stashes the frame texture before `begin_frame`, and after
//! `end_frame` the capture closure copies it into a top-down RGBA8
//! buffer via a padded staging buffer.

use crate::WgpuGfxCtx;

impl WgpuGfxCtx {
    /// Stash a handle to the current frame's surface texture so a later
    /// [`Self::read_screenshot`] call can copy from it.  Called from the
    /// platform shell with `frame.texture.clone()` BEFORE [`begin_frame`].
    /// `wgpu::Texture` is internally ref-counted, so the clone is cheap.
    pub fn set_surface_texture(&mut self, tex: wgpu::Texture) {
        self.surface_texture = Some(tex);
    }

    /// Stash captured screenshot pixels for the read-back closure to pick
    /// up.  See [`Self::pending_screenshot`] / [`Self::take_pending_screenshot`].
    pub fn set_pending_screenshot(&mut self, captured: (Vec<u8>, u32, u32)) {
        self.pending_screenshot = Some(captured);
    }

    /// Consume the pending screenshot pixels — returns `(Vec::new(), 0, 0)`
    /// when none are stashed (typical non-capture frames).  Called by the
    /// `agg_gui::screenshot::run_frame_with_capture` read-back closure.
    pub fn take_pending_screenshot(&mut self) -> (Vec<u8>, u32, u32) {
        self.pending_screenshot.take().unwrap_or((Vec::new(), 0, 0))
    }

    /// Read the current frame's rendered pixels back to CPU memory as a
    /// top-down RGBA8 buffer.  Returns `(pixels, width, height)`.
    /// The first `width * 4` bytes are the TOP row (Y-down image order).
    ///
    /// Must be called AFTER [`Self::end_frame`] has submitted the render and
    /// BEFORE the platform shell calls `frame.present()`.  Requires the
    /// platform shell to have called [`Self::set_surface_texture`] earlier
    /// in the frame so we hold a handle into the surface that's still
    /// valid post-render.
    ///
    /// Returns an empty buffer if no surface texture is currently stashed.
    pub fn read_screenshot(&self) -> (Vec<u8>, u32, u32) {
        let Some(texture) = self.surface_texture.as_ref() else {
            return (Vec::new(), 0, 0);
        };
        let size = texture.size();
        let w = size.width;
        let h = size.height;
        if w == 0 || h == 0 {
            return (Vec::new(), 0, 0);
        }

        // wgpu requires `bytes_per_row` to be a multiple of
        // COPY_BYTES_PER_ROW_ALIGNMENT (256).  We allocate a padded buffer
        // for the copy and strip the padding row-by-row when assembling the
        // returned `Vec<u8>`.
        const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded_bpr = w * 4;
        let padded_bpr = unpadded_bpr.div_ceil(ALIGN) * ALIGN;
        let buffer_size = (padded_bpr as u64) * (h as u64);

        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot_staging"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screenshot_copy"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        // Map the staging buffer.  `map_async` is async via callback; we
        // poll the device until the map completes.  On native this is fine
        // (synchronous from the caller's POV); on WASM the wgpu webgl
        // backend resolves the future on the JS event-loop tick that the
        // surrounding render loop is running on, so this still works
        // because the JS harness drives `render()` from a microtask.
        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = sender.send(res);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let map_result = receiver
            .recv()
            .expect("map_async sender dropped before resolving");
        if map_result.is_err() {
            return (Vec::new(), 0, 0);
        }

        // Surface format may be Bgra8Unorm; PNG / JS expects RGBA so swap
        // R↔B per pixel as we copy.  Surface textures are Y-down, which
        // matches the screenshot module's "TOP row first" convention, so
        // no row flip needed.
        let bgra = matches!(
            self.surface_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
        {
            let view = slice.get_mapped_range();
            for row in 0..h as usize {
                let start = row * padded_bpr as usize;
                let end = start + unpadded_bpr as usize;
                let src = &view[start..end];
                if bgra {
                    for px in src.chunks_exact(4) {
                        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                    }
                } else {
                    out.extend_from_slice(src);
                }
            }
        }
        staging.unmap();
        (out, w, h)
    }
}
