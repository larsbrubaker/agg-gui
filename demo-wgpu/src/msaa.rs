//! Re-usable off-screen MSAA framebuffer for widgets that need 3-D / GPU
//! rendering with hardware antialiasing.
//!
//! Pattern: a widget allocates an [`MsaaFramebuffer`] sized to its on-screen
//! rect, renders into it via its own pipeline (configured for the same
//! `sample_count`), then calls [`MsaaFramebuffer::blit_to`] which composites
//! the resolved colour onto the active 2-D render target through the shared
//! `tex_pipeline`.
//!
//! This avoids the natural pitfall of using wgpu's automatic resolve onto
//! the surface / layer view directly: the resolve covers the *full*
//! attachment area, which would clobber any 2-D content outside the widget
//! rect.  Instead we resolve into a private same-size texture and then
//! alpha-blend that into the target through a textured quad — pixels the
//! widget didn't render stay transparent and the underlying 2-D content
//! shows through.
//!
//! `sample_count == 1` is supported as a no-MSAA fast path: there's no
//! separate multisample buffer, the widget renders directly into the
//! single-sample resolve texture, and `blit_to` works the same way.

use agg_gui::geometry::Rect;
use wgpu::util::DeviceExt;

use crate::pipelines::{TexUniforms, WgpuPipelines};

/// Clamp a requested MSAA sample count to a value the WebGPU specification
/// guarantees works on any device without opting into
/// `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`.
///
/// The spec mandates support for `1` (no MSAA) and `4` for every renderable
/// color format.  Higher values like `8` and `16` are commonly available on
/// desktop adapters but rejected by validation on others (and on most
/// WebGL2 backends) — pipeline creation panics rather than silently
/// degrading.  Callers building MSAA-aware widgets pass their saved /
/// requested setting through this and rebuild on change.
pub fn safe_sample_count(requested: u32) -> u32 {
    match requested {
        0 | 1 => 1,
        _ => 4,
    }
}

/// Map a UI-facing SSAA "samples" choice to the linear render-target scale
/// factor used when supersampling onto an oversized framebuffer.
///
/// Cell values are pixel multipliers (`1` / `4` / `16`); the linear scale is
/// `sqrt(samples)`, so a 16-sample request allocates a 4× linear (= 16×
/// pixel) backbuffer that gets bilinear-downsampled to the on-screen rect.
/// Unlike hardware MSAA this works on any adapter — the framebuffer stays
/// `sample_count = 1` and only the *size* changes, so the WebGPU
/// `{1, 4}`-only guarantee for multisampled formats is irrelevant.
///
/// Saved values from the old MSAA-semantics era (`0` for off) are coerced
/// to `1`.  Anything between supported steps is rounded to the nearest
/// supported step so out-of-band saves don't fail loudly.
pub fn ssaa_linear_scale(requested_samples: u32) -> u32 {
    match requested_samples {
        0 | 1 => 1,
        2..=8 => 2,
        _ => 4,
    }
}

/// Off-screen framebuffer for widgets that drive their own GPU pipeline and
/// composite the result onto the shared 2-D render target.
///
/// Allocated lazily; call [`Self::ensure_size`] each frame to keep the
/// attachments sized to the current widget rect.
pub struct MsaaFramebuffer {
    /// Multisample colour (`sample_count = N`), only allocated when `N > 1`.
    /// When present, this is the colour attachment for the widget's render
    /// pass and `resolve` is its resolve target.
    msaa_color: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Single-sample texture used as the **resolve target** (when MSAA is
    /// on) or as the **direct render target** (when off), and as the source
    /// of [`Self::blit_to`] in either case.
    resolve: (wgpu::Texture, wgpu::TextureView),
    /// Optional depth attachment, matched to `sample_count`.
    depth: Option<(wgpu::Texture, wgpu::TextureView)>,
    sample_count: u32,
    format: wgpu::TextureFormat,
    with_depth: bool,
    width: u32,
    height: u32,
    /// Linear sampler used by `blit_to`.  1:1 pixel mapping makes linear vs.
    /// nearest indistinguishable; linear is more forgiving on fractional
    /// alignment if the caller's coordinates aren't pixel-perfect.
    blit_sampler: wgpu::Sampler,
}

impl MsaaFramebuffer {
    /// Build a fresh framebuffer at `(w, h)` with the given `sample_count`
    /// and surface `format`.  `with_depth = true` allocates a matching depth
    /// buffer; widgets that only need colour can pass `false`.
    ///
    /// `sample_count` is clamped to the values the WebGPU spec guarantees
    /// for any color format without opt-in features — `1` (no MSAA) and
    /// `4`.  Higher values like `8` and `16` require the
    /// `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` feature and can be
    /// per-format unsupported on otherwise-MSAA-capable hardware; rather
    /// than panic on pipeline creation, callers can request `8` and we
    /// silently round to `4`.  Pass [`safe_sample_count`] up front if you
    /// want a UI to display the actual count we'll use.
    pub fn new(
        device: &wgpu::Device,
        w: u32,
        h: u32,
        sample_count: u32,
        format: wgpu::TextureFormat,
        with_depth: bool,
    ) -> Self {
        let sample_count = safe_sample_count(sample_count);
        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("msaa_blit"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let mut fb = Self {
            msaa_color: None,
            resolve: alloc_resolve(device, w.max(1), h.max(1), format),
            depth: None,
            sample_count,
            format,
            with_depth,
            width: w.max(1),
            height: h.max(1),
            blit_sampler,
        };
        if sample_count > 1 {
            fb.msaa_color = Some(alloc_msaa(
                device,
                fb.width,
                fb.height,
                format,
                sample_count,
            ));
        }
        if with_depth {
            fb.depth = Some(alloc_depth(device, fb.width, fb.height, sample_count));
        }
        fb
    }

    /// Reallocate the attachments if `(w, h)` has changed since the last call.
    /// Cheap when the size is stable.
    pub fn ensure_size(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        let w = w.max(1);
        let h = h.max(1);
        if w == self.width && h == self.height {
            return;
        }
        self.width = w;
        self.height = h;
        self.resolve = alloc_resolve(device, w, h, self.format);
        if self.sample_count > 1 {
            self.msaa_color = Some(alloc_msaa(device, w, h, self.format, self.sample_count));
        }
        if self.with_depth {
            self.depth = Some(alloc_depth(device, w, h, self.sample_count));
        }
    }

    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// View to use as the **colour attachment** of the widget's render pass.
    pub fn render_view(&self) -> &wgpu::TextureView {
        match &self.msaa_color {
            Some((_, v)) => v,
            None => &self.resolve.1,
        }
    }

    /// `resolve_target` to set on the colour attachment.  `Some` when MSAA
    /// is on, `None` when off (writing directly to the resolve texture, no
    /// resolve step needed).
    pub fn resolve_target(&self) -> Option<&wgpu::TextureView> {
        if self.sample_count > 1 {
            Some(&self.resolve.1)
        } else {
            None
        }
    }

    /// Single-sample resolved view — sample this in `blit_to` or any other
    /// post-render pass.
    pub fn resolve_view(&self) -> &wgpu::TextureView {
        &self.resolve.1
    }

    /// Single-sample resolved texture handle.  Exposed so a platform shell
    /// (currently `demo-wasm`) can use this `MsaaFramebuffer` as the
    /// intermediate "scene" target — pass `resolve_texture().clone()` to
    /// [`crate::WgpuGfxCtx::set_surface_texture`] so the GPU-direct
    /// screenshot path copies from this scene texture instead of from the
    /// real swap-chain surface (which on WebGL2 cannot advertise
    /// `COPY_SRC` and so can't be the source of a `copy_texture_to_*`).
    pub fn resolve_texture(&self) -> &wgpu::Texture {
        &self.resolve.0
    }

    /// Depth attachment view, when one was requested at construction.
    pub fn depth_view(&self) -> Option<&wgpu::TextureView> {
        self.depth.as_ref().map(|(_, v)| v)
    }

    /// Composite the framebuffer's resolved colour onto `target_view`'s
    /// `dst_rect` (Y-up screen-space pixels of the target).  Uses the shared
    /// 2-D textured-quad pipeline with `BLEND_STANDARD` so transparent
    /// pixels (where the widget didn't render) preserve the 2-D content
    /// underneath.
    ///
    /// At ≥4× linear minification (e.g. SSAA 16×) a single bilinear tap
    /// only reads 4 of the 16 source texels per output pixel; in that case
    /// the caller can dispatch through `tex_downsample_4x_pipeline` instead
    /// by routing through this method's higher-level helper.  This base
    /// `blit_to` always runs the single-tap pipeline — fine for 1×/2×
    /// minification (perfect 2×2 box at 2×).
    ///
    /// `parent_clip` is intersected with `dst_rect` to set the pass scissor —
    /// pass the framework scissor that was active when the widget called
    /// `gl_paint` / pushed its draw command.
    pub fn blit_to(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        target_size: (u32, u32),
        dst_rect: Rect,
        parent_clip: Option<[i32; 4]>,
        pipelines: &WgpuPipelines,
    ) {
        self.blit_to_inner(
            device,
            encoder,
            target_view,
            target_size,
            dst_rect,
            parent_clip,
            &pipelines.tex_pipeline,
            pipelines,
        );
    }

    /// 4×4-box downsample variant of [`Self::blit_to`].  Use when the
    /// framebuffer is exactly 4× the linear size of `dst_rect` (SSAA 16×):
    /// runs `tex_downsample_4x_pipeline` so all 16 source texels under each
    /// output pixel contribute equally, instead of the 4-of-16 you'd get
    /// from a single bilinear tap.
    pub fn blit_downsample_4x_to(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        target_size: (u32, u32),
        dst_rect: Rect,
        parent_clip: Option<[i32; 4]>,
        pipelines: &WgpuPipelines,
    ) {
        self.blit_to_inner(
            device,
            encoder,
            target_view,
            target_size,
            dst_rect,
            parent_clip,
            &pipelines.tex_downsample_4x_pipeline,
            pipelines,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn blit_to_inner(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        target_size: (u32, u32),
        dst_rect: Rect,
        parent_clip: Option<[i32; 4]>,
        pipeline: &wgpu::RenderPipeline,
        pipelines: &WgpuPipelines,
    ) {
        let [gl_x, gl_y, gl_w, gl_h] = pixel_rect(dst_rect);
        if gl_w <= 0 || gl_h <= 0 {
            return;
        }

        let tex_uniforms = TexUniforms {
            resolution: [target_size.0 as f32, target_size.1 as f32],
            _pad: [0.0; 2],
            tint: [1.0, 1.0, 1.0, 1.0],
        };
        let tex_ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msaa_blit_uniforms"),
            contents: bytemuck::bytes_of(&tex_uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let tex_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msaa_blit_bg0"),
            layout: &pipelines.tex_bgl0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: tex_ub.as_entire_binding(),
            }],
        });
        let tex_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("msaa_blit_bg1"),
            layout: &pipelines.tex_bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.resolve.1),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
                },
            ],
        });

        // Quad in target_view's Y-up local coords.  bl=v=1, tr=v=0 mirrors
        // the wgpu UV convention used elsewhere (e.g. `image_blit.rs`).
        let x0 = gl_x as f32;
        let y0 = gl_y as f32;
        let x1 = x0 + gl_w as f32;
        let y1 = y0 + gl_h as f32;
        let verts: [f32; 24] = [
            x0, y0, 0.0, 1.0, x1, y0, 1.0, 1.0, x1, y1, 1.0, 0.0, x0, y0, 0.0, 1.0, x1, y1, 1.0,
            0.0, x0, y1, 0.0, 0.0,
        ];
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("msaa_blit_vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Convert parent_clip (Y-up) ∩ dst_rect to wgpu Y-down for set_scissor.
        let scissor = parent_clip
            .map(|[px, py, pw, ph]| {
                let x1 = (gl_x + gl_w).min(px + pw);
                let y1 = (gl_y + gl_h).min(py + ph);
                let x0c = gl_x.max(px);
                let y0c = gl_y.max(py);
                let w = (x1 - x0c).max(0);
                let h = (y1 - y0c).max(0);
                let y_down = (target_size.1 as i32 - (y0c + h)).max(0);
                (x0c.max(0) as u32, y_down as u32, w as u32, h as u32)
            })
            .unwrap_or_else(|| {
                let y_down = (target_size.1 as i32 - (gl_y + gl_h)).max(0);
                (gl_x.max(0) as u32, y_down as u32, gl_w as u32, gl_h as u32)
            });
        if scissor.2 == 0 || scissor.3 == 0 {
            return;
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("msaa_blit_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(
            0.0,
            0.0,
            target_size.0 as f32,
            target_size.1 as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &tex_bg0, &[]);
        pass.set_bind_group(1, &tex_bg1, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.draw(0..6, 0..1);
        // pass dropped here; vb / bind groups kept alive by `_` below.
        drop(pass);
        let _ = (tex_ub, tex_bg0, tex_bg1, vb);
    }
}

fn alloc_resolve(
    device: &wgpu::Device,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("msaa_resolve"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        // `COPY_SRC` so this texture can serve as the source of a
        // `copy_texture_to_texture` / `copy_texture_to_buffer` —
        // specifically when `demo-wasm` uses an `MsaaFramebuffer` as the
        // intermediate scene buffer behind the screenshot path.  Cost is
        // a flag bit; backends that don't physically support COPY_SRC on
        // a non-swap-chain texture are extinct in practice (wgpu's
        // WebGL2 backend does support it on regular textures).
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

fn alloc_msaa(
    device: &wgpu::Device,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
    sample_count: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("msaa_color"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

fn alloc_depth(
    device: &wgpu::Device,
    w: u32,
    h: u32,
    sample_count: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("msaa_depth"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

fn pixel_rect(rect: Rect) -> [i32; 4] {
    let x0 = rect.x.floor() as i32;
    let y0 = rect.y.floor() as i32;
    let x1 = (rect.x + rect.width).ceil() as i32;
    let y1 = (rect.y + rect.height).ceil() as i32;
    [x0, y0, (x1 - x0).max(0), (y1 - y0).max(0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_rect_covers_fractional_physical_extent() {
        let rect = Rect::new(10.25, 20.5, 16.5, 10.25);
        assert_eq!(pixel_rect(rect), [10, 20, 17, 11]);
    }
}
