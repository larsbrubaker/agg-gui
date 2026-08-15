//! Scaled / region screenshot readback for [`WgpuGfxCtx`].
//!
//! Sibling of `screenshot_capture.rs`, which owns the full-surface capture
//! texture and the two full-surface readback flavours (blocking for native
//! Save/Copy, non-blocking `begin_capture_readback` / `poll_capture_readback`
//! for the web screen-share sender).  Both of those pull the ENTIRE surface
//! back to system memory and swizzle it on the calling thread — measured at
//! ~5.6 ms for a 1280x720 window, scaling with window area.
//!
//! This module adds the small-output path: a GPU render-pass blit from the
//! capture texture (optionally from a sub-rect of it) into a
//! `dst_w` x `dst_h` `Rgba8Unorm` texture, and a non-blocking readback of
//! only THAT texture.  A 256x192 thumbnail is ~200 KB regardless of window
//! size, so the per-capture CPU cost stops tracking resolution.
//!
//! Everything here is additive: [`WgpuGfxCtx::begin_capture_readback`] and
//! friends are untouched and keep their own in-flight slot, so a consumer can
//! run the `--screenshot` flow and periodic thumbnails independently.
//!
//! # Output format
//!
//! The scaled path delivers exactly what [`WgpuGfxCtx::poll_capture_readback`]
//! delivers today: tight, top-row-first, 4-bytes-per-pixel RGBA8 whose bytes
//! are sRGB-encoded when the surface format is sRGB.  Two deliberate choices
//! get us there:
//!
//! * The blit target is `Rgba8Unorm`, **not** `Rgba8UnormSrgb`, and the
//!   fragment shader re-encodes the (sampler-decoded) linear value back to
//!   sRGB when the source format is sRGB.  So the stored bytes are the raw
//!   surface bytes, not their linearization — an sRGB target would store the
//!   linear value and darken every thumbnail.
//! * Channel order needs no work: `textureSample` yields RGBA regardless of
//!   the source's BGRA memory layout, and the target is RGBA8, so the R↔B
//!   swap `poll_capture_readback` does per pixel on the CPU happens for free.
//!
//! Net effect: a PNG encoder fed by this path needs no changes relative to the
//! full-surface path — `screenshot_scaled_tests` pins that byte-for-byte.

use bytemuck::{Pod, Zeroable};

use crate::shaders::SCALED_BLIT_WGSL;
use crate::WgpuGfxCtx;

/// Format of the small blit target, and therefore of the bytes handed back by
/// [`WgpuGfxCtx::poll_capture_readback_scaled`].  See the module docs for why
/// this is the non-sRGB variant.
const DST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A rectangle in **framebuffer pixels**: origin at the TOP-left of the
/// surface, +Y downward — the same convention the readback buffers use
/// ("first row is the top row"), and deliberately NOT agg-gui's Y-up
/// drawing space.  Callers holding a Y-up widget rect must flip it:
/// `y = surface_height - (rect.top)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectInPixels {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl RectInPixels {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Intersect `region` with the `src_w` x `src_h` surface.
///
/// `None` in means "the whole surface".  `None` out means "nothing to
/// capture" — an empty request, or one that starts past the surface edge;
/// callers report that the same way the existing capture API reports
/// "nothing captured" (a `false` return, no readback queued).
pub(crate) fn clamp_region(
    region: Option<RectInPixels>,
    src_w: u32,
    src_h: u32,
) -> Option<RectInPixels> {
    if src_w == 0 || src_h == 0 {
        return None;
    }
    let Some(r) = region else {
        return Some(RectInPixels::new(0, 0, src_w, src_h));
    };
    if r.width == 0 || r.height == 0 || r.x >= src_w || r.y >= src_h {
        return None;
    }
    let width = r.width.min(src_w - r.x);
    let height = r.height.min(src_h - r.y);
    Some(RectInPixels::new(r.x, r.y, width, height))
}

/// `bytes_per_row` for a `w`-pixel-wide RGBA8 texture-to-buffer copy, rounded
/// up to `COPY_BYTES_PER_ROW_ALIGNMENT` as wgpu requires.  The alignment
/// applies to the small blit target exactly as it does to a full-surface
/// copy, so the map still hands back padded rows we strip on the CPU.
pub(crate) fn padded_bytes_per_row(w: u32) -> u32 {
    const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    (w * 4).div_ceil(ALIGN) * ALIGN
}

/// 32-byte uniform block for `SCALED_BLIT_WGSL` (layout must match the WGSL
/// struct).  `flags.x` = re-encode linear→sRGB; `flags.yzw` are padding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ScaledBlitUniforms {
    uv_offset: [f32; 2],
    uv_scale: [f32; 2],
    flags: [f32; 4],
}
const _: () = assert!(std::mem::size_of::<ScaledBlitUniforms>() == 32);

/// Lazily-created GPU resources for the scaled blit: one pipeline, its two
/// bind-group layouts, a linear sampler (the filtering IS the downscale), and
/// a size-cached destination texture.  Built on first use rather than in
/// `WgpuGfxCtx::new` so contexts that never take a scaled screenshot pay
/// nothing.
pub(crate) struct ScaledBlit {
    pipeline: wgpu::RenderPipeline,
    bgl0: wgpu::BindGroupLayout,
    bgl1: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    dst: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
}

impl ScaledBlit {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scaled_blit"),
            source: wgpu::ShaderSource::Wgsl(SCALED_BLIT_WGSL.into()),
        });
        let bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scaled_blit_bgl0"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<ScaledBlitUniforms>() as u64,
                    ),
                },
                count: None,
            }],
        });
        let bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scaled_blit_bgl1"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scaled_blit_layout"),
            bind_group_layouts: &[Some(&bgl0), Some(&bgl1)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scaled_blit"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: DST_FORMAT,
                    // Straight overwrite: the pass clears nothing meaningful
                    // and every dst pixel is covered by the triangle.
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scaled_blit_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            bgl0,
            bgl1,
            sampler,
            dst: None,
        }
    }

    /// (Re-)allocate the destination texture when the requested output size
    /// changes, then hand back its view.  Thumbnail sizes are stable in
    /// practice, so this allocates once.
    fn ensure_dst(&mut self, device: &wgpu::Device, w: u32, h: u32) -> &wgpu::TextureView {
        let need_alloc = match &self.dst {
            Some((_, _, dw, dh)) => *dw != w || *dh != h,
            None => true,
        };
        if need_alloc {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scaled_capture_dst"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DST_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.dst = Some((texture, view, w, h));
        }
        // `dst` was just populated when it was missing.
        &self.dst.as_ref().expect("dst texture just allocated").1
    }
}

impl WgpuGfxCtx {
    /// Whether a *scaled* capture readback is already in flight.  Independent
    /// of [`Self::has_pending_readback`], which tracks the full-surface one.
    pub fn has_pending_scaled_readback(&self) -> bool {
        self.pending_scaled_readback.is_some()
    }

    /// Kick off an async readback of a scaled (and optionally cropped) copy of
    /// the current capture texture.
    ///
    /// `src_region` is in framebuffer pixels (top-down, see [`RectInPixels`])
    /// and is clamped to the surface; `None` means the whole surface.  The
    /// result is resampled to `dst_w` x `dst_h` with linear filtering, so the
    /// caller controls the readback cost directly: only
    /// `dst_w * dst_h * 4` bytes ever cross back to the CPU.
    ///
    /// Returns `false` — queueing nothing — when there is nothing to capture:
    /// no capture texture (call `DrawCtx::capture_screenshot` first), a
    /// zero-sized output, an empty or entirely off-surface region, or a scaled
    /// readback already in flight.  Same "nothing captured" signal as
    /// [`Self::begin_capture_readback`].
    ///
    /// Pure GPU submit + `map_async`; never blocks.  Harvest with
    /// [`Self::poll_capture_readback_scaled`].
    pub fn begin_capture_readback_scaled(
        &mut self,
        src_region: Option<RectInPixels>,
        dst_w: u32,
        dst_h: u32,
    ) -> bool {
        if self.pending_scaled_readback.is_some() || dst_w == 0 || dst_h == 0 {
            return false;
        }
        let Some((_, src_view, src_w, src_h)) = self.capture_texture.as_ref() else {
            return false;
        };
        let (src_view, src_w, src_h) = (src_view.clone(), *src_w, *src_h);
        let Some(region) = clamp_region(src_region, src_w, src_h) else {
            return false;
        };

        if self.scaled_blit.is_none() {
            self.scaled_blit = Some(ScaledBlit::new(&self.device));
        }
        let device = std::sync::Arc::clone(&self.device);
        let srgb_src = matches!(
            self.surface_format,
            wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb
        );
        let uniforms = ScaledBlitUniforms {
            uv_offset: [
                region.x as f32 / src_w as f32,
                region.y as f32 / src_h as f32,
            ],
            uv_scale: [
                region.width as f32 / src_w as f32,
                region.height as f32 / src_h as f32,
            ],
            flags: [if srgb_src { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        };

        let blit = self
            .scaled_blit
            .as_mut()
            .expect("scaled blit resources just created");
        let dst_view = blit.ensure_dst(&device, dst_w, dst_h).clone();
        let ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scaled_blit_uniforms"),
            size: std::mem::size_of::<ScaledBlitUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue
            .write_buffer(&ub, 0, bytemuck::bytes_of(&uniforms));
        let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scaled_blit_bg0"),
            layout: &blit.bgl0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ub.as_entire_binding(),
            }],
        });
        let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scaled_blit_bg1"),
            layout: &blit.bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&blit.sampler),
                },
            ],
        });

        let padded_bpr = padded_bytes_per_row(dst_w);
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scaled_capture_readback"),
            size: (padded_bpr as u64) * (dst_h as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scaled_capture_blit"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scaled_capture_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&blit.pipeline);
            pass.set_bind_group(0, &bg0, &[]);
            pass.set_bind_group(1, &bg1, &[]);
            // Fullscreen triangle — positions come from `vertex_index`.
            pass.draw(0..3, 0..1);
        }
        let dst_texture = &blit.dst.as_ref().expect("dst texture ensured above").0;
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: dst_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(dst_h),
                },
            },
            wgpu::Extent3d {
                width: dst_w,
                height: dst_h,
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
        self.pending_scaled_readback = Some(crate::PendingReadback {
            staging,
            w: dst_w,
            h: dst_h,
            padded_bpr,
            done: rx,
        });
        true
    }

    /// Harvest a completed scaled readback's pixels as tight top-row-first
    /// RGBA8, or `None` if still pending / none in flight.  Never blocks.
    ///
    /// Unlike [`Self::poll_capture_readback`] there is no per-pixel swizzle
    /// here — the blit already produced RGBA in the surface's own byte
    /// encoding — so the only CPU work is stripping the row padding.
    pub fn poll_capture_readback_scaled(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        let _ = self.device.poll(wgpu::PollType::Poll);
        match self.pending_scaled_readback.as_ref()?.done.try_recv() {
            Ok(Ok(())) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => return None, // not ready yet
            Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_scaled_readback = None; // failed map — drop it
                return None;
            }
        }

        let crate::PendingReadback {
            staging,
            w,
            h,
            padded_bpr,
            ..
        } = self.pending_scaled_readback.take()?;

        let unpadded_bpr = (w * 4) as usize;
        let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
        {
            let view = staging.slice(..).get_mapped_range();
            for row in 0..h as usize {
                let start = row * padded_bpr as usize;
                out.extend_from_slice(&view[start..start + unpadded_bpr]);
            }
        }
        staging.unmap();
        Some((out, w, h))
    }
}
