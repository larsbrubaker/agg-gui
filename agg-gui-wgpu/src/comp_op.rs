//! Blend modes (`DrawCtx::set_blend_mode`) on the wgpu backend — every SVG
//! compositing operator the software `GfxCtx` applies, matched to it.
//!
//! The software renderer blends a solid shape's span pixel by pixel through
//! `agg_rust::comp_op` at each pixel's cover.  A GPU blend state cannot do
//! that: most operators need the destination in the maths, and the ones that
//! lerp the destination by the cover (clear, src, src-in, ...) or carry AGG's
//! quirks (src-atop's blue) have no fixed-function form.  So a draw under any
//! mode other than source-over (which keeps the ordinary pipelines) and dst
//! (which draws nothing, as software leaves the pixel) runs as:
//!
//! 1. `DrawCommand::CompOpBegin` — a transparent **coverage layer** the size
//!    of the current target becomes the render target, and the shape is drawn
//!    into it in opaque white with the ordinary pipelines, so its alpha is the
//!    shape's cover.
//! 2. `DrawCommand::CompOpEnd` — the target is copied (within the clip) to a
//!    destination texture, and `comp_op.wgsl` writes every clipped pixel once
//!    from that copy, the cover, and the source colour with the software
//!    formulas.  One composite per draw call, as one software span pass.
//!
//! The copy needs the target's texture with `COPY_SRC`: layers are allocated
//! with it, and the window surface has it when the shell configured it so
//! (`CopySrc`) and handed it over with `set_surface_texture`.  When it does
//! not, `end_frame` renders the frame into a proxy texture that has it and
//! blits that onto the surface at the end ([`blit_proxy`]).
//!
//! The software renderer applies blend modes to solid fills, solid strokes
//! and grayscale text; gradient fills, images and LCD text ignore the mode
//! there, and do here too.
//!
//! Port of agg-sharp `RenderGl/Renderer/GpuCompOp.cs` (`ICompOpGraphics` on
//! `Graphics2DGpu`), per draw call instead of per `DrawWithCompOp` block.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::CompOp;
use agg_rust::color::Rgba8;
use wgpu::util::DeviceExt;

use crate::end_frame::{Prepared, PreparedSlice};
use crate::{DrawCommand, WgpuGfxCtx};

/// The composite pass's uniform block — `CompOpUniforms` in `comp_op.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CompOpUniforms {
    source: [f32; 4],
    op: u32,
    mode: u32,
    srgb: u32,
    pad: u32,
}

/// The composite pipeline and its bind-group layout, built once per context.
pub(crate) struct CompOpGpu {
    pub(crate) bgl: wgpu::BindGroupLayout,
    pub(crate) pipeline: wgpu::RenderPipeline,
}

impl CompOpGpu {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("comp_op_bgl"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("comp_op_layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("comp_op"),
            source: wgpu::ShaderSource::Wgsl(include_str!("comp_op.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("comp_op"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                // No blending: the shader's result replaces the pixel.
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { bgl, pipeline }
    }

    fn bind_group(
        &self,
        device: &wgpu::Device,
        coverage: &wgpu::TextureView,
        dest: &wgpu::TextureView,
        uniform: wgpu::BindingResource<'_>,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("comp_op_bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(coverage),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(dest),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform,
                },
            ],
        })
    }
}

/// The premultiplied source bytes software blends with: the colour with the
/// global alpha folded in, converted as `GfxCtx` converts it
/// (`Color::to_rgba8`), then premultiplied with `Rgba8::multiply` as
/// `comp_op_adaptor_rgba` does.
pub(crate) fn premultiplied_source(color: Color, global_alpha: f64) -> [u8; 4] {
    let mut c = color;
    c.a *= global_alpha as f32;
    let s = c.to_rgba8();
    [
        Rgba8::multiply(s.r, s.a),
        Rgba8::multiply(s.g, s.a),
        Rgba8::multiply(s.b, s.a),
        s.a,
    ]
}

impl WgpuGfxCtx {
    /// Run `draw` — which pushes the draw command(s) of one solid shape in
    /// the colour it is given — under the current blend mode.
    ///
    /// Source-over draws `color` directly; dst draws nothing; every other
    /// mode draws white into a coverage layer (with the global alpha at 1)
    /// and composites `color` through the mode.
    pub(crate) fn draw_with_blend_mode(
        &mut self,
        color: Color,
        draw: impl FnOnce(&mut Self, Color),
    ) {
        let op = self.blend_mode;
        match op {
            CompOp::SrcOver => draw(self, color),
            CompOp::Dst => {}
            _ => {
                let source = premultiplied_source(color, self.global_alpha);
                let (w, h) = (
                    self.viewport.0.max(1.0) as u32,
                    self.viewport.1.max(1.0) as u32,
                );
                let (texture, view) = self.alloc_layer_texture(w, h);
                self.commands.push(DrawCommand::CompOpBegin {
                    texture: Arc::clone(&texture),
                    view: view.clone(),
                    width: w,
                    height: h,
                });
                let global_alpha = std::mem::replace(&mut self.global_alpha, 1.0);
                self.coverage_pass = true;
                draw(self, Color::white());
                self.coverage_pass = false;
                self.global_alpha = global_alpha;
                self.commands.push(DrawCommand::CompOpEnd {
                    texture,
                    view,
                    op,
                    source,
                    clip: self.current_clip(),
                });
            }
        }
    }
}

/// Prepare a `CompOpEnd`: the destination-copy texture (the parent target's
/// size, its format without sRGB so the shader reads raw bytes) and the
/// composite's bind group.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_end(
    device: &wgpu::Device,
    comp: &CompOpGpu,
    format: wgpu::TextureFormat,
    texture: &Arc<wgpu::Texture>,
    view: &wgpu::TextureView,
    parent_size: (u32, u32),
    uniform: &PreparedSlice,
    clip: Option<[i32; 4]>,
) -> Prepared {
    let dest = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("comp_op_dest"),
        size: wgpu::Extent3d {
            width: parent_size.0.max(1),
            height: parent_size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: format.remove_srgb_suffix(),
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let dest_view = dest.create_view(&wgpu::TextureViewDescriptor::default());
    let bg = comp.bind_group(
        device,
        view,
        &dest_view,
        wgpu::BindingResource::Buffer(uniform.uniform_binding()),
    );
    Prepared::CompOpEnd {
        _layer: Arc::clone(texture),
        dest,
        bg,
        clip,
    }
}

/// The uniform bytes of a composite through `op` with `source`.
pub(crate) fn composite_uniforms(
    op: CompOp,
    source: [u8; 4],
    format: wgpu::TextureFormat,
) -> CompOpUniforms {
    CompOpUniforms {
        source: source.map(f32::from),
        op: op as u32,
        mode: 0,
        srgb: format.is_srgb() as u32,
        pad: 0,
    }
}

/// The composite's rect in the target's Y-down texels — `clip` (Y-up,
/// `[x, y_bottom, w, h]`) intersected with the target — or `None` when it
/// covers nothing.
pub(crate) fn device_rect(clip: Option<[i32; 4]>, vp: (f32, f32)) -> Option<(u32, u32, u32, u32)> {
    let (vp_w, vp_h) = (vp.0 as u32, vp.1 as u32);
    let (x, y, w, h) = match clip {
        Some(scissor) => {
            // The scissor's Y-down conversion clamps a negative top to 0
            // without shortening the height; do the clamp here as an
            // intersection so the rect never reaches past the clip.
            let [cx, cy, cw, ch] = scissor;
            let top = vp_h as i32 - (cy + ch);
            let (x0, y0) = (cx.max(0), top.max(0));
            let x1 = (cx + cw).min(vp_w as i32);
            let y1 = (top + ch).min(vp_h as i32);
            (x0, y0, x1 - x0, y1 - y0)
        }
        None => (0, 0, vp_w as i32, vp_h as i32),
    };
    (w > 0 && h > 0).then_some((x as u32, y as u32, w as u32, h as u32))
}

/// Copy `rect` of the parent target into the composite's destination texture.
pub(crate) fn copy_destination(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::Texture,
    dest: &wgpu::Texture,
    rect: (u32, u32, u32, u32),
) {
    let (x, y, w, h) = rect;
    let origin = wgpu::Origin3d { x, y, z: 0 };
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: dest,
            mip_level: 0,
            origin,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}

/// Draw a prepared composite into the open pass on its target.
pub(crate) fn draw_composite(
    pass: &mut wgpu::RenderPass,
    comp: &CompOpGpu,
    bg: &wgpu::BindGroup,
    rect: (u32, u32, u32, u32),
) {
    pass.set_pipeline(&comp.pipeline);
    pass.set_bind_group(0, bg, &[]);
    pass.set_scissor_rect(rect.0, rect.1, rect.2, rect.3);
    pass.draw(0..3, 0..1);
}

/// Whether the frame's root target can be read back for a composite: the
/// surface texture the shell handed over has `COPY_SRC` and is the frame's
/// size and format.
pub(crate) fn root_is_readable(
    texture: &wgpu::Texture,
    viewport: (f32, f32),
    format: wgpu::TextureFormat,
) -> bool {
    texture.usage().contains(wgpu::TextureUsages::COPY_SRC)
        && texture.width() == viewport.0 as u32
        && texture.height() == viewport.1 as u32
        && texture.format() == format
}

/// The texture a frame renders into when its surface cannot be read back:
/// the surface's size and format, readable and sampleable.
pub(crate) fn alloc_proxy(
    device: &wgpu::Device,
    viewport: (f32, f32),
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("comp_op_proxy"),
        size: wgpu::Extent3d {
            width: (viewport.0 as u32).max(1),
            height: (viewport.1 as u32).max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Copy the proxy onto the surface, texel for texel (the composite shader's
/// copy mode, no blending).
pub(crate) fn blit_proxy(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    comp: &CompOpGpu,
    proxy: &wgpu::TextureView,
    surface: &wgpu::TextureView,
) {
    let uniforms = CompOpUniforms {
        source: [0.0; 4],
        op: CompOp::Dst as u32,
        mode: 1,
        srgb: 0,
        pad: 0,
    };
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("comp_op_blit"),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bg = comp.bind_group(device, proxy, proxy, buffer.as_entire_binding());
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("comp_op_blit"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: surface,
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
    pass.set_pipeline(&comp.pipeline);
    pass.set_bind_group(0, &bg, &[]);
    pass.draw(0..3, 0..1);
}
