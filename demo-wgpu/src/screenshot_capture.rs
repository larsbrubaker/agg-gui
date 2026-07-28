//! `DrawCtx` screenshot-capture texture path for the wgpu backend.
//!
//! Split out of `draw_ctx_impl.rs` (800-line guardrail).  Owns everything that
//! touches [`WgpuGfxCtx::capture_texture`]: the GPU-direct surface snapshot, the
//! textured re-draw of that snapshot into the scene, and both readback flavours
//! (blocking for native Save/Copy, non-blocking for the web screen-share sender).
//!
//! The `DrawCtx` trait methods in `draw_ctx_impl.rs` are thin delegates onto the
//! `*_impl` inherent methods here, following the same convention used by
//! `image_blit.rs` and `layers.rs`.  Sibling module `screenshot_readback.rs`
//! handles the *other* capture route — copying the frame's surface texture for
//! `agg_gui::screenshot::run_frame_with_capture`.

use crate::{DrawCommand, WgpuGfxCtx};

impl WgpuGfxCtx {
    /// Snapshot the surface texture into our internal capture texture
    /// via a single `copy_texture_to_texture` — pure GPU work, no CPU
    /// readback.  Re-allocate the capture texture when its size doesn't
    /// match the surface (resize, first capture).
    pub(crate) fn capture_screenshot_impl(&mut self) -> bool {
        let Some(src) = self.surface_texture.clone() else {
            return false;
        };
        let src_size = src.size();
        let (sw, sh) = (src_size.width, src_size.height);
        if sw == 0 || sh == 0 {
            return false;
        }

        let need_alloc = match &self.capture_texture {
            Some((_, _, w, h)) => *w != sw || *h != sh,
            None => true,
        };
        if need_alloc {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("screenshot_capture"),
                size: wgpu::Extent3d {
                    width: sw,
                    height: sh,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                // Sampled by the screenshot widget (TEXTURE_BINDING),
                // written by `copy_texture_to_texture` (COPY_DST), and
                // read back to CPU on Save / Copy (COPY_SRC).
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.capture_texture = Some((std::sync::Arc::new(tex), view, sw, sh));
        }

        let dst_tex = std::sync::Arc::clone(&self.capture_texture.as_ref().unwrap().0);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screenshot_capture_copy"),
            });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &src,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &dst_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: sw,
                height: sh,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        true
    }

    /// Draw the held capture texture as a textured quad in the current CTM.
    ///
    /// The capture texture is sample_count=1 with TEXTURE_BINDING, so it
    /// slots straight into the existing `Textured` deferred command —
    /// same pipeline / sampler as `draw_image_rgba_arc`.  We pick the
    /// linear sampler (trilinear when mipmaps are present, bilinear at
    /// base mip otherwise) so heavy downsampling in the preview pane
    /// doesn't point-alias.  No mipmap chain on the capture texture
    /// for now — sampling at typical preview-pane scales (2-4× shrink)
    /// is acceptable with bilinear; we can add a GPU mip-chain pass if
    /// visual review shows aliasing.
    pub(crate) fn draw_captured_screenshot_impl(
        &mut self,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) -> bool {
        let Some((texture, view, _, _)) = self.capture_texture.as_ref() else {
            return false;
        };
        let texture = std::sync::Arc::clone(texture);
        let view = view.clone();

        let bl = self.transform_pt(dst_x, dst_y);
        let br = self.transform_pt(dst_x + dst_w, dst_y);
        let tr = self.transform_pt(dst_x + dst_w, dst_y + dst_h);
        let tl = self.transform_pt(dst_x, dst_y + dst_h);
        // wgpu UV convention: v=0 is the top of the texture; bottom of
        // the destination quad samples v=1.  Same flip used by `image_blit`.
        let verts: [f32; 24] = [
            bl[0], bl[1], 0.0, 1.0, br[0], br[1], 1.0, 1.0, tr[0], tr[1], 1.0, 0.0, bl[0], bl[1],
            0.0, 1.0, tr[0], tr[1], 1.0, 0.0, tl[0], tl[1], 0.0, 0.0,
        ];
        let clip = self.current_clip();
        let alpha = self.global_alpha as f32;
        self.commands.push(DrawCommand::Textured {
            verts,
            texture,
            view,
            nearest: false,
            tint: [1.0, 1.0, 1.0, alpha],
            clip,
        });
        true
    }

    /// Single-shot synchronous readback for Save / Copy.  Issues a
    /// copy from the held capture texture to a `MAP_READ` staging
    /// buffer, polls the device until done, and unrolls the
    /// 256-byte-row-aligned blocks into a tight RGBA8 Y-down `Vec<u8>`.
    /// Format is normalized to RGBA8 (swapping R↔B for Bgra8 surface
    /// formats) so callers don't need to know the surface format.
    pub(crate) fn read_captured_screenshot_impl(&mut self) -> (Vec<u8>, u32, u32) {
        let Some((texture, _, w, h)) = self.capture_texture.as_ref() else {
            return (Vec::new(), 0, 0);
        };
        let (w, h) = (*w, *h);
        if w == 0 || h == 0 {
            return (Vec::new(), 0, 0);
        }
        let texture = std::sync::Arc::clone(texture);

        const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded_bpr = w * 4;
        let padded_bpr = unpadded_bpr.div_ceil(ALIGN) * ALIGN;
        let buffer_size = (padded_bpr as u64) * (h as u64);

        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture_readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("capture_readback_copy"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
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

        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = sender.send(res);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        if receiver.recv().ok().and_then(|r| r.ok()).is_none() {
            return (Vec::new(), 0, 0);
        }

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

// ── Non-blocking capture readback (web screen-share sender) ────────────────────
//
// The trait's `read_captured_screenshot` blocks the thread on `poll(Wait)`, which
// deadlocks on the single-threaded web build.  These inherent methods split that
// into start-now / harvest-later so `render()` never blocks.  See
// [`crate::PendingReadback`].
impl WgpuGfxCtx {
    /// Whether a capture readback is already in flight.
    pub fn has_pending_readback(&self) -> bool {
        self.pending_readback.is_some()
    }

    /// Kick off an async readback of the current capture texture.  Returns false
    /// if nothing is captured or a readback is already in flight.  Pure GPU
    /// submit + `map_async`; never blocks.
    pub fn begin_capture_readback(&mut self) -> bool {
        if self.pending_readback.is_some() {
            return false;
        }
        let Some((texture, _, w, h)) = self.capture_texture.as_ref() else {
            return false;
        };
        let (w, h) = (*w, *h);
        if w == 0 || h == 0 {
            return false;
        }
        let texture = std::sync::Arc::clone(texture);

        const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bpr = (w * 4).div_ceil(ALIGN) * ALIGN;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture_readback_async"),
            size: (padded_bpr as u64) * (h as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("capture_readback_async_copy"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
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

        let (tx, rx) = std::sync::mpsc::channel();
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |res| {
                let _ = tx.send(res);
            });
        self.pending_readback = Some(crate::PendingReadback {
            staging,
            w,
            h,
            padded_bpr,
            done: rx,
        });
        true
    }

    /// Harvest a completed readback's pixels, or `None` if still pending / none
    /// in flight.  Never blocks: on the web the map resolves on a later event
    /// loop turn; the cheap non-blocking `poll` keeps native progressing too.
    pub fn poll_capture_readback(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        let _ = self.device.poll(wgpu::PollType::Poll);
        match self.pending_readback.as_ref()?.done.try_recv() {
            Ok(Ok(())) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => return None, // not ready yet
            Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_readback = None; // failed map — drop it
                return None;
            }
        }

        let crate::PendingReadback {
            staging,
            w,
            h,
            padded_bpr,
            ..
        } = self.pending_readback.take().unwrap();

        let bgra = matches!(
            self.surface_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let unpadded_bpr = (w * 4) as usize;
        let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
        {
            let view = staging.slice(..).get_mapped_range();
            for row in 0..h as usize {
                let start = row * padded_bpr as usize;
                let src = &view[start..start + unpadded_bpr];
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
        Some((out, w, h))
    }
}
