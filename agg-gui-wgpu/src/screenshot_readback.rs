//! Frame screenshot read-back for [`WgpuGfxCtx`].
//!
//! Split out of `lib.rs` (800-line guardrail). Owns the surface-texture
//! stash and the GPU->CPU copy path used by
//! `agg_gui::screenshot::run_frame_with_capture`: the platform shell
//! stashes the frame texture before `begin_frame`, and after
//! `end_frame` the capture closure copies it into a top-down RGBA8
//! buffer via a padded staging buffer.
//!
//! The copy itself ([`read_texture_rgba`]) is shared with the offscreen
//! targets of `crate::headless`.
//!
//! Also owns the release half of that stash: [`WgpuGfxCtx::present`] /
//! [`WgpuGfxCtx::release_frame_texture`] drop the ctx's frame handles before
//! the swap chain gets the frame back (required on DX12 — see `present`).

use crate::WgpuGfxCtx;

impl WgpuGfxCtx {
    /// Stash a handle to the current frame's surface texture so a later
    /// [`Self::read_screenshot`] / `capture_screenshot` call can copy from
    /// it.  Called from the platform shell with `frame.texture.clone()`
    /// BEFORE [`begin_frame`](WgpuGfxCtx::begin_frame).  `wgpu::Texture` is
    /// internally ref-counted, so the clone is cheap — but it is a live
    /// reference to the swap-chain back buffer, so the frame MUST then be
    /// presented through [`Self::present`] (or the stash dropped with
    /// [`Self::release_frame_texture`]), never via a bare
    /// `SurfaceTexture::present()`.
    ///
    /// If the previous frame's texture is still stashed (the shell never
    /// released it), this logs a one-time `log::warn!` per ctx, since that
    /// is the pattern that breaks DX12 swap-chain resize.
    pub fn set_surface_texture(&mut self, tex: wgpu::Texture) {
        if self.surface_texture.is_some() && !self.warned_unreleased_frame {
            self.warned_unreleased_frame = true;
            log::warn!(
                "agg-gui-wgpu: set_surface_texture called while the previous frame's \
                 texture is still stashed. Present frames via WgpuGfxCtx::present (or call \
                 release_frame_texture) — holding a back-buffer handle past present makes \
                 DX12 swap-chain resizes fail (\"Invalid surface\")."
            );
        }
        self.surface_texture = Some(tex);
    }

    /// Release the ctx's per-frame handles: the [`Self::set_surface_texture`]
    /// stash and the [`begin_frame`](WgpuGfxCtx::begin_frame) view if
    /// `end_frame` did not already consume it.
    ///
    /// [`Self::present`] calls this for you — including when the stash is an
    /// SSAA / scene texture rather than the surface itself.  Call it directly
    /// only from shells that do not present a frame at all (e.g. a frame
    /// abandoned mid-way or an offscreen-only render).  Screenshot read-back
    /// must happen before this — afterwards [`Self::read_screenshot`]
    /// returns an empty buffer.
    pub fn release_frame_texture(&mut self) {
        self.surface_texture = None;
        self.surface_view = None;
    }

    /// Present `frame`, first releasing the ctx's per-frame handles
    /// (see [`Self::release_frame_texture`]).
    ///
    /// This is THE way to present whenever the ctx was given the frame's
    /// surface texture (via [`Self::set_surface_texture`] or
    /// [`begin_frame`](WgpuGfxCtx::begin_frame)).  On Windows/DX12 a clone of
    /// the back buffer that outlives `present()` keeps the swap chain
    /// referenced, so the next `Surface::configure` (DXGI `ResizeBuffers`)
    /// fails with "Invalid surface" and recreating the swap chain fails with
    /// `E_ACCESSDENIED`.  Do any [`Self::read_screenshot`] before calling.
    pub fn present(&mut self, frame: wgpu::SurfaceTexture) {
        self.release_frame_texture();
        frame.present();
    }

    /// Stash captured screenshot pixels for the read-back closure to pick
    /// up.  See [`Self::take_pending_screenshot`].
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
    /// BEFORE the platform shell calls [`Self::present`] (which releases the
    /// stashed texture).  Requires the platform shell to have called
    /// [`Self::set_surface_texture`] earlier in the frame so we hold a handle
    /// into the surface that's still valid post-render.
    ///
    /// Returns an empty buffer if no surface texture is currently stashed
    /// (including after [`Self::present`] / [`Self::release_frame_texture`]).
    pub fn read_screenshot(&self) -> (Vec<u8>, u32, u32) {
        let Some(texture) = self.surface_texture.as_ref() else {
            return (Vec::new(), 0, 0);
        };
        let size = texture.size();
        let (w, h) = (size.width, size.height);
        if w == 0 || h == 0 {
            return (Vec::new(), 0, 0);
        }
        match read_texture_rgba(&self.device, &self.queue, texture, self.surface_format) {
            Ok(pixels) => (pixels, w, h),
            Err(_) => (Vec::new(), 0, 0),
        }
    }
}

/// Copy mip 0 of `texture` back to CPU memory as tightly packed RGBA8, top
/// row first (Y-down image order, as surface textures are laid out).
///
/// `format` says how the texels are stored: a `Bgra8*` texture has R and B
/// swapped on the way out, so callers always get RGBA order; any other format
/// is copied as is, so it must be 4 bytes per texel. Blocks on the device
/// until the copy is mapped. Shared by [`WgpuGfxCtx::read_screenshot`] and
/// [`crate::headless::HeadlessTarget::read_rgba`].
pub(crate) fn read_texture_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
) -> Result<Vec<u8>, wgpu::BufferAsyncError> {
    let size = texture.size();
    let (w, h) = (size.width, size.height);

    // wgpu requires `bytes_per_row` to be a multiple of
    // COPY_BYTES_PER_ROW_ALIGNMENT (256).  We allocate a padded buffer
    // for the copy and strip the padding row-by-row when assembling the
    // returned `Vec<u8>`.
    const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let unpadded_bpr = w * 4;
    let padded_bpr = unpadded_bpr.div_ceil(ALIGN) * ALIGN;
    let buffer_size = (padded_bpr as u64) * (h as u64);

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot_staging"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
    queue.submit(std::iter::once(encoder.finish()));

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
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    receiver
        .recv()
        .expect("map_async sender dropped before resolving")?;

    // Surface format may be Bgra8Unorm; PNG / JS expects RGBA so swap
    // R↔B per pixel as we copy.  Surface textures are Y-down, which
    // matches the screenshot module's "TOP row first" convention, so
    // no row flip needed.
    let bgra = matches!(
        format,
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
    Ok(out)
}
