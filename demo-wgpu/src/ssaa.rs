//! Re-usable off-screen **SSAA** framebuffer for widgets that need 3-D / GPU
//! rendering with antialiasing.
//!
//! # Why SSAA, not MSAA
//!
//! WebGPU's spec only guarantees `{1, 4}` for hardware MSAA sample counts on
//! arbitrary renderable formats; `8` / `16` need adapter-specific opt-in
//! features and can be silently absent on the WebGL2 backend.  Supersampling
//! sidesteps that: render into an oversized single-sample texture and
//! bilinear-downsample on composite.  Works on every adapter, and the
//! sample-count granularity is whatever linear-scale you can pay for in
//! pixels (`2² = 4`, `3² = 9`, `4² = 16`, ...).
//!
//! # Pattern
//!
//! A widget allocates an [`SsaaFramebuffer`] sized to **scale × on-screen**
//! pixels, renders into it through its own pipeline (configured for
//! `sample_count = 1`), then calls [`SsaaFramebuffer::blit_to`] which
//! composites the downsampled colour onto the active 2-D render target via
//! the shared `tex_pipeline`.
//!
//! Resolving into a private same-size texture and then alpha-blending it
//! into the target through a textured quad means pixels the widget didn't
//! render stay transparent and the underlying 2-D content shows through —
//! the same property hardware MSAA's automatic resolve would clobber if
//! aimed straight at the surface / layer view.
//!
//! # Minimal downstream usage
//!
//! ```ignore
//! use agg_gui_demo_wgpu::{ssaa_linear_scale, SsaaFramebuffer};
//!
//! // UI knob: 1 = off, 4 / 9 / 16 = 2× / 3× / 4× linear supersampling.
//! let samples = 9u32;
//! let scale = ssaa_linear_scale(samples);
//!
//! // One-time alloc (logical size × scale).
//! let mut fb = SsaaFramebuffer::new(
//!     device,
//!     widget_w * scale,
//!     widget_h * scale,
//!     surface_format,
//!     /* with_depth = */ true,
//! );
//!
//! // Each frame:
//! fb.ensure_size(device, widget_w * scale, widget_h * scale);
//! // ... begin_render_pass with fb.render_view() + fb.depth_view(), draw your 3-D scene ...
//! fb.blit_to(device, encoder, target_view, target_size, dst_rect, parent_clip, pipelines);
//! ```

use agg_gui::geometry::Rect;
use wgpu::util::DeviceExt;

use crate::pipelines::{TexUniforms, WgpuPipelines};

/// Map a UI-facing "samples" choice to the linear render-target scale factor
/// used when supersampling onto an oversized framebuffer.
///
/// Cell values are pixel multipliers (`1` / `4` / `9` / `16`); the linear
/// scale is `sqrt(samples)`, so a 16-sample request allocates a 4× linear
/// (= 16× pixel) backbuffer that gets bilinear-downsampled to the on-screen
/// rect.
///
/// Saved values from the old MSAA-semantics era (`0` for off) are coerced to
/// `1`.  Anything between supported steps rounds to the nearest supported
/// step so out-of-band saves don't fail loudly.
pub fn ssaa_linear_scale(requested_samples: u32) -> u32 {
    match requested_samples {
        0 | 1 => 1,
        2..=5 => 2,
        6..=12 => 3,
        _ => 4,
    }
}

/// Off-screen single-sample framebuffer for widgets that drive their own GPU
/// pipeline and composite the result onto the shared 2-D render target.
///
/// SSAA happens via *size*: the caller allocates this at `scale × {w, h}`
/// and bilinear-downsamples through [`Self::blit_to`].  This type holds no
/// concept of "samples" — it's just an offscreen colour (+ optional depth)
/// with a composite helper.
///
/// Allocated lazily; call [`Self::ensure_size`] each frame to keep the
/// attachments sized to the current widget rect × scale.
pub struct SsaaFramebuffer {
    /// Single-sample texture used as the render target and as the source of
    /// [`Self::blit_to`].
    color: (wgpu::Texture, wgpu::TextureView),
    /// Optional depth attachment, `sample_count = 1`.
    depth: Option<(wgpu::Texture, wgpu::TextureView)>,
    format: wgpu::TextureFormat,
    with_depth: bool,
    width: u32,
    height: u32,
    /// Linear sampler used by `blit_to`.  Linear minification is the
    /// downsample filter at < 4× linear; at exactly 4× linear (SSAA 16×)
    /// use [`Self::blit_downsample_4x_to`] instead — a single bilinear tap
    /// reads only 4 of the 16 source texels, losing half the AA benefit.
    blit_sampler: wgpu::Sampler,
}

impl SsaaFramebuffer {
    /// Build a fresh framebuffer at `(w, h)` with surface `format`.
    /// `with_depth = true` allocates a matching depth buffer; widgets that
    /// only need colour can pass `false`.
    ///
    /// `(w, h)` is the **physical** size of the texture, already multiplied
    /// by the desired linear SSAA scale.  Use [`ssaa_linear_scale`] to map
    /// a samples-style UI value to that multiplier.
    pub fn new(
        device: &wgpu::Device,
        w: u32,
        h: u32,
        format: wgpu::TextureFormat,
        with_depth: bool,
    ) -> Self {
        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ssaa_blit"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let mut fb = Self {
            color: alloc_color(device, w.max(1), h.max(1), format),
            depth: None,
            format,
            with_depth,
            width: w.max(1),
            height: h.max(1),
            blit_sampler,
        };
        if with_depth {
            fb.depth = Some(alloc_depth(device, fb.width, fb.height));
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
        self.color = alloc_color(device, w, h, self.format);
        if self.with_depth {
            self.depth = Some(alloc_depth(device, w, h));
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// View to use as the **colour attachment** of the widget's render pass.
    pub fn render_view(&self) -> &wgpu::TextureView {
        &self.color.1
    }

    /// Same view, exposed under the historic name used by post-render
    /// sampling sites (e.g. the screenshot path).
    pub fn resolve_view(&self) -> &wgpu::TextureView {
        &self.color.1
    }

    /// Colour texture handle.  Exposed so a platform shell (currently
    /// `demo-wasm`) can use this `SsaaFramebuffer` as the intermediate
    /// "scene" target — pass `resolve_texture().clone()` to
    /// [`crate::WgpuGfxCtx::set_surface_texture`] so the GPU-direct
    /// screenshot path copies from this scene texture instead of from the
    /// real swap-chain surface (which on WebGL2 cannot advertise
    /// `COPY_SRC` and so can't be the source of a `copy_texture_to_*`).
    pub fn resolve_texture(&self) -> &wgpu::Texture {
        &self.color.0
    }

    /// Depth attachment view, when one was requested at construction.
    pub fn depth_view(&self) -> Option<&wgpu::TextureView> {
        self.depth.as_ref().map(|(_, v)| v)
    }

    /// Composite the framebuffer's colour onto `target_view`'s `dst_rect`
    /// (Y-up screen-space pixels of the target).  Uses the shared 2-D
    /// textured-quad pipeline with `BLEND_STANDARD` so transparent pixels
    /// (where the widget didn't render) preserve the 2-D content
    /// underneath.
    ///
    /// At ≥4× linear minification (e.g. SSAA 16×) a single bilinear tap
    /// only reads 4 of the 16 source texels per output pixel; in that case
    /// use [`Self::blit_downsample_4x_to`] instead.  This base `blit_to`
    /// always runs the single-tap pipeline — fine for 1× / 2× / 3×
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
            label: Some("ssaa_blit_uniforms"),
            contents: bytemuck::bytes_of(&tex_uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let tex_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ssaa_blit_bg0"),
            layout: &pipelines.tex_bgl0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: tex_ub.as_entire_binding(),
            }],
        });
        let tex_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ssaa_blit_bg1"),
            layout: &pipelines.tex_bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.color.1),
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
            label: Some("ssaa_blit_vb"),
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
            label: Some("ssaa_blit_pass"),
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

fn alloc_color(
    device: &wgpu::Device,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ssaa_color"),
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
        // specifically when `demo-wasm` uses an `SsaaFramebuffer` as the
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

fn alloc_depth(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ssaa_depth"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
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

    #[test]
    fn ssaa_linear_scale_matches_segments() {
        assert_eq!(ssaa_linear_scale(0), 1);
        assert_eq!(ssaa_linear_scale(1), 1);
        assert_eq!(ssaa_linear_scale(4), 2);
        assert_eq!(ssaa_linear_scale(9), 3);
        assert_eq!(ssaa_linear_scale(16), 4);
        // Out-of-band rounds to nearest supported step.
        assert_eq!(ssaa_linear_scale(7), 3);
        assert_eq!(ssaa_linear_scale(100), 4);
    }
}
