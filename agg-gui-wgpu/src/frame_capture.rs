//! The GPU side of `agg_gui::frame_capture`: the one frame an app asked to
//! capture is rendered where it can be read back, and its pixels reach the
//! app's callback on a later frame, without stalling any frame.
//!
//! A window's surface cannot be copied from: the browser's canvas never
//! advertises `COPY_SRC`, and a native surface configured with it loses the
//! framebuffer-only fast path on Metal. So [`FrameCaptureService`] renders
//! only the asked-for frame into an offscreen [`SsaaFramebuffer`] at the
//! surface's size, blits it onto the surface (the same factor-1 blit the web
//! shell's `offscreen_scene` uses every frame), and copies each requested
//! region of it into a `MAP_READ` buffer. The frame after, the offscreen
//! texture is destroyed; frames nobody asked to capture never touch any of
//! this. A host already rendering offscreen hands its own texture to
//! [`FrameCaptureService::read_from`] instead.
//!
//! Reads are non-blocking: `map_async` now, harvested by
//! [`FrameCaptureService::poll`] on later frames (the same start-now /
//! harvest-later split as `screenshot_capture.rs`'s
//! `begin_capture_readback`). Each is checked against the context's loss
//! record right before its mapped range is read (`crate::device_lost_flag`),
//! and a lost device delivers [`FrameCaptureError::DeviceLost`].
//!
//! Shell order per frame: [`FrameCaptureService::begin_frame`] →
//! (armed: [`FrameCaptureService::target_view`] as the frame's colour target)
//! → paint, `end_frame`, the host's after-paint → (armed:
//! [`FrameCaptureService::finish_frame`]) → present →
//! [`FrameCaptureService::poll`]. `agg-gui-shell` and `agg-gui-web-shell`
//! both follow it.

use agg_gui::frame_capture::{
    region_in_pixels, take_frame_capture_requests, CapturedFrame, FrameCaptureError,
    FrameCaptureRequest, PixelRegion,
};

use crate::texture_life::DestroyOnDrop;
use crate::{SsaaFramebuffer, WgpuGfxCtx};

type MapDone = std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>;

/// One region's read in flight.
struct PendingRead {
    request: FrameCaptureRequest,
    staging: DestroyOnDrop,
    region: PixelRegion,
    padded_bpr: u32,
    bgra: bool,
    done: MapDone,
}

/// Serves `agg_gui::frame_capture` requests for one window's render context.
/// Kept beside the context for the window's life; see the module docs for
/// the per-frame calls.
#[derive(Default)]
pub struct FrameCaptureService {
    /// Requests taken by this frame, not yet read.
    armed: Vec<FrameCaptureRequest>,
    /// The armed frame's offscreen colour target.
    target: Option<SsaaFramebuffer>,
    /// Last capture's target, destroyed at the next frame's start, when the
    /// frame that drew and copied it has been submitted.
    spent: Option<SsaaFramebuffer>,
    reads: Vec<PendingRead>,
}

impl FrameCaptureService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Frame start: frees the last capture's target and takes the queued
    /// requests. `true` when this frame is to be captured (render it into
    /// [`Self::target_view`] and call [`Self::finish_frame`]); `false` when
    /// the frame takes its ordinary path.
    pub fn begin_frame(&mut self) -> bool {
        if let Some(fb) = self.spent.take() {
            fb.resolve_texture().destroy();
        }
        self.armed.extend(take_frame_capture_requests());
        !self.armed.is_empty()
    }

    /// Whether this frame is being captured.
    pub fn is_armed(&self) -> bool {
        !self.armed.is_empty()
    }

    /// Whether a read is still waiting for its pixels.
    pub fn has_reads_in_flight(&self) -> bool {
        !self.reads.is_empty()
    }

    /// The colour target for an armed `w` x `h` frame. Also makes it the
    /// context's surface texture, so `DrawCtx::capture_screenshot` during the
    /// frame copies the same pixels the window shows.
    pub fn target_view(&mut self, ctx: &mut WgpuGfxCtx, w: u32, h: u32) -> wgpu::TextureView {
        let device = std::sync::Arc::clone(ctx.device());
        let format = ctx.surface_format();
        let fb = match self.target.as_mut() {
            Some(fb) => {
                fb.ensure_size(&device, w, h);
                fb
            }
            None => self
                .target
                .insert(SsaaFramebuffer::new(&device, w, h, format, false)),
        };
        ctx.set_surface_texture(fb.resolve_texture().clone());
        fb.render_view().clone()
    }

    /// After an armed frame's render was submitted: blit the offscreen frame
    /// onto `surface_view` (`surface_size` pixels) so the window shows it,
    /// and start the reads of the requested regions.
    pub fn finish_frame(
        &mut self,
        ctx: &WgpuGfxCtx,
        surface_view: &wgpu::TextureView,
        surface_size: (u32, u32),
    ) {
        let Some(fb) = self.target.take() else {
            self.fail_armed(|| {
                FrameCaptureError::Unavailable("the frame had no capture target".to_string())
            });
            return;
        };
        let (device, queue) = (ctx.device(), ctx.queue());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame_capture_blit"),
        });
        let mut blit_buffers = Vec::new();
        fb.blit_downsample_retiring(
            device,
            queue,
            &mut encoder,
            surface_view,
            surface_size,
            agg_gui::Rect::new(
                0.0,
                0.0,
                f64::from(surface_size.0),
                f64::from(surface_size.1),
            ),
            None,
            ctx.pipelines(),
            1,
            &mut |buffer| blit_buffers.push(buffer),
        );
        queue.submit(std::iter::once(encoder.finish()));
        // Destroyed once submitted: dropped, the browser frees them only when
        // its garbage collector runs.
        for buffer in blit_buffers {
            buffer.destroy();
        }
        self.read_from(ctx, fb.resolve_texture());
        self.spent = Some(fb);
    }

    /// Start the armed requests' reads from `source`, a `COPY_SRC` texture in
    /// the context's surface format holding the finished frame — for a host
    /// that already renders its frames offscreen.
    pub fn read_from(&mut self, ctx: &WgpuGfxCtx, source: &wgpu::Texture) {
        let (fw, fh) = (source.width(), source.height());
        let scale = agg_gui::ux_scale::effective_scale();
        let bgra = matches!(
            source.format(),
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut encoder = ctx
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame_capture_copy"),
            });
        let mut started = Vec::new();
        for request in std::mem::take(&mut self.armed) {
            let region = match region_in_pixels(request.region(), scale, fw, fh) {
                Ok(r) => r,
                Err(e) => {
                    request.deliver(Err(e));
                    continue;
                }
            };
            let padded_bpr = (region.width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let staging = DestroyOnDrop(ctx.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame_capture_readback"),
                size: u64::from(padded_bpr) * u64::from(region.height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: source,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: region.x,
                        y: region.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_bpr),
                        rows_per_image: Some(region.height),
                    },
                },
                wgpu::Extent3d {
                    width: region.width,
                    height: region.height,
                    depth_or_array_layers: 1,
                },
            );
            started.push((request, staging, region, padded_bpr));
        }
        if started.is_empty() {
            return;
        }
        ctx.queue().submit(std::iter::once(encoder.finish()));
        for (request, staging, region, padded_bpr) in started {
            let (tx, rx) = std::sync::mpsc::channel();
            staging
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |res| {
                    let _ = tx.send(res);
                });
            self.reads.push(PendingRead {
                request,
                staging,
                region,
                padded_bpr,
                bgra,
                done: rx,
            });
        }
    }

    /// Deliver every read whose pixels have arrived (each callback runs here,
    /// on this thread). Never blocks. While reads are still in flight it asks
    /// for another frame, so an idle app keeps polling until they land; call
    /// it after the frame's paint (`App::paint` clears draw requests).
    /// Returns whether reads are still in flight.
    pub fn poll(&mut self, ctx: &WgpuGfxCtx) -> bool {
        if self.reads.is_empty() {
            return false;
        }
        let loss = ctx.device_lost().loss();
        if !crate::device_lost_flag::poll_unless_lost(ctx.device(), loss) {
            let reason = ctx
                .device_lost()
                .lost_reason()
                .unwrap_or_else(|| "the device stopped responding".to_string());
            self.device_lost(&reason);
            return false;
        }
        let mut still = Vec::new();
        for read in std::mem::take(&mut self.reads) {
            match read.done.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => still.push(read),
                Ok(Ok(())) => {
                    // The map may have completed before the poll above;
                    // checked again right before the read.
                    match crate::device_lost_flag::mapped_read_allowed(Some(loss)) {
                        Ok(()) => {
                            let frame = harvest(&read);
                            read.request.deliver(Ok(frame));
                        }
                        Err(reason) => read
                            .request
                            .deliver(Err(FrameCaptureError::DeviceLost(reason))),
                    }
                }
                Ok(Err(e)) => read
                    .request
                    .deliver(Err(FrameCaptureError::Unavailable(format!(
                        "the frame's read-back failed: {e}"
                    )))),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    read.request.deliver(Err(FrameCaptureError::Unavailable(
                        "the frame's read-back was abandoned".to_string(),
                    )))
                }
            }
        }
        self.reads = still;
        if self.reads.is_empty() {
            return false;
        }
        agg_gui::animation::request_draw_without_invalidation();
        true
    }

    /// The device was lost: every capture this service holds is delivered
    /// [`FrameCaptureError::DeviceLost`]. Requests not yet taken stay queued
    /// for the rebuilt device's next frame.
    pub fn device_lost(&mut self, reason: &str) {
        self.target = None;
        self.spent = None;
        for read in std::mem::take(&mut self.reads) {
            read.request
                .deliver(Err(FrameCaptureError::DeviceLost(reason.to_string())));
        }
        self.fail_armed(|| FrameCaptureError::DeviceLost(reason.to_string()));
    }

    fn fail_armed(&mut self, err: impl Fn() -> FrameCaptureError) {
        for request in std::mem::take(&mut self.armed) {
            request.deliver(Err(err()));
        }
    }
}

/// Copy a mapped read's rows out tight, RGBA order, then unmap.
fn harvest(read: &PendingRead) -> CapturedFrame {
    let PixelRegion { width, height, .. } = read.region;
    let row_bytes = width as usize * 4;
    let mut rgba = Vec::with_capacity(row_bytes * height as usize);
    {
        let view = read.staging.slice(..).get_mapped_range();
        for row in 0..height as usize {
            let start = row * read.padded_bpr as usize;
            let src = &view[start..start + row_bytes];
            if read.bgra {
                for px in src.chunks_exact(4) {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            } else {
                rgba.extend_from_slice(src);
            }
        }
    }
    read.staging.unmap();
    CapturedFrame {
        rgba,
        width,
        height,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "frame_capture_tests.rs"]
mod tests;
