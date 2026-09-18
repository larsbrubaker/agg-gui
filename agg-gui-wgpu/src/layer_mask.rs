//! Path-clip ("clip layer") support for the wgpu backend.
//!
//! `DrawCtx::clip_path` is implemented as a compositing layer whose composite
//! is a *mesh* instead of a quad: the clipping path is tessellated on the CPU
//! (same AA tessellator the gradient fill path uses), stored on the layer
//! entry, and at pop time drawn as `pos2 + uv2 + coverage` triangles sampling
//! the layer texture.  Fragments outside the path simply are not rasterised,
//! and the halo strip's per-vertex coverage gives anti-aliased clip edges.
//!
//! Relationship to the other modules:
//! - `layers.rs` owns the normal (quad-composited) layer push/pop; this module
//!   adds [`WgpuGfxCtx::clip_path_impl`] and the masked-pop command emission.
//! - `end_frame_prepare.rs` turns `DrawCommand::PopLayerMasked` into
//!   `Prepared::PopLayerMasked`; `end_frame.rs` executes it as the pending
//!   composite of the resumed parent pass.
//! - `pipelines.rs` stores the pipeline built by
//!   [`build_layer_mesh_pipeline`] here.

use std::sync::Arc;

use agg_gui::TransAffine;
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_transform::ConvTransform;

use crate::{DrawCommand, WgpuGfxCtx, WgpuLayerEntry};

/// CPU-side clip mask carried by a clip layer.
///
/// `verts` are `(x, y, coverage)` in the **parent** layer's physical pixel
/// coordinates — the space the composite draw runs in.
pub(crate) struct ClipMaskMesh {
    pub(crate) verts: Vec<[f32; 3]>,
    pub(crate) indices: Vec<u32>,
}

impl ClipMaskMesh {
    /// Build the composite vertex buffer: `(x, y, u, v, coverage)`.
    ///
    /// `v` is flipped (`y0` → `v = 1`) to match `composite_quad_verts`' UV
    /// convention for the layer texture.
    pub(crate) fn composite_verts(&self, origin_x: f32, origin_y: f32, w: u32, h: u32) -> Vec<f32> {
        let fw = (w as f32).max(1.0);
        let fh = (h as f32).max(1.0);
        let mut out = Vec::with_capacity(self.verts.len() * 5);
        for v in &self.verts {
            let u = (v[0] - origin_x) / fw;
            let vv = 1.0 - (v[1] - origin_y) / fh;
            out.extend_from_slice(&[v[0], v[1], u, vv, v[2]]);
        }
        out
    }
}

impl WgpuGfxCtx {
    /// Intersect the clip with the current path (canvas-2D `clip()`).
    ///
    /// Pushes a clip layer sized to the path's device-space bounding box
    /// (intersected with the active scissor and the current target), keeping
    /// the CTM — rotation included — and only shifting it by the layer origin,
    /// so drawing continues in exactly the same coordinate space.  The layer is
    /// popped by the `restore()` matching the `save()` that preceded this call
    /// (see `DrawCtx::restore` in `draw_ctx_impl.rs`).
    ///
    /// `reset_clip()` inside a clip layer only clears the rectangular scissor;
    /// the path mask stays in force until that `restore()`.
    pub(crate) fn clip_path_impl(&mut self) {
        let ctm = *self.ctm();
        let fill_rule = self.fill_rule;
        // Legacy per-vertex-alpha tessellation (as `do_fill` uses for
        // gradients): the halo strip's alpha is exactly the clip coverage.
        let tess = {
            let mut curves = ConvCurve::new(&mut self.path);
            let mut transformed = ConvTransform::new(&mut curves, ctm);
            agg_gui::gl_renderer::tessellate_path_aa(&mut transformed, 1.0, fill_rule)
        };
        let (verts, indices) = match tess {
            Some((v, i)) if !v.is_empty() && !i.is_empty() => (v, i),
            _ => (Vec::new(), Vec::new()),
        };

        // Mesh bounds in physical pixels, intersected with the active scissor
        // and clamped to the current render target.
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for v in &verts {
            min_x = min_x.min(v[0]);
            min_y = min_y.min(v[1]);
            max_x = max_x.max(v[0]);
            max_y = max_y.max(v[1]);
        }
        let (mut x1, mut y1, mut x2, mut y2) =
            (min_x as f64, min_y as f64, max_x as f64, max_y as f64);
        if let Some([cx, cy, cw, ch]) = self.current_clip() {
            x1 = x1.max(cx as f64);
            y1 = y1.max(cy as f64);
            x2 = x2.min((cx + cw) as f64);
            y2 = y2.min((cy + ch) as f64);
        }
        x1 = x1.max(0.0);
        y1 = y1.max(0.0);
        x2 = x2.min(self.viewport.0 as f64);
        y2 = y2.min(self.viewport.1 as f64);

        let empty = verts.is_empty() || !(x2 > x1 && y2 > y1);
        let (origin_x, origin_y) = if empty {
            (0.0, 0.0)
        } else {
            (x1.floor(), y1.floor())
        };
        let w = if empty {
            1
        } else {
            (x2.ceil() - origin_x).max(1.0) as u32
        };
        let h = if empty {
            1
        } else {
            (y2.ceil() - origin_y).max(1.0) as u32
        };
        // An empty clip draws nothing: keep a 1×1 layer whose composite mesh
        // has no triangles.
        let mask = if empty {
            ClipMaskMesh {
                verts: Vec::new(),
                indices: Vec::new(),
            }
        } else {
            ClipMaskMesh { verts, indices }
        };

        let saved = self.capture_draw_state();
        let parent_clip = self.current_clip();
        let (texture, view) = self.alloc_layer_texture(w, h);

        // Layer-local CTM: the parent CTM shifted by the layer origin.  Unlike
        // `push_layer_with_alpha_impl` we do NOT collapse it to a pure scale —
        // the caller's rotation/shear must survive the clip.
        let mut local: TransAffine = ctm;
        local.tx -= origin_x;
        local.ty -= origin_y;

        self.layer_stack.push(WgpuLayerEntry {
            texture: Arc::clone(&texture),
            view: view.clone(),
            width: w,
            height: h,
            origin_x,
            origin_y,
            alpha: 1.0,
            saved,
            retained_key: None,
            rounded_clip: None,
            parent_clip,
            opaque_backdrop: false,
            clip_mask: Some(mask),
            clip_saved_path: Some(self.path.clone()),
        });

        self.viewport = (w as f32, h as f32);
        self.state_stack = vec![(local, None)];
        // `self.path` is deliberately left intact: canvas-2D `clip()` does not
        // reset the current path, so drawing inside the clip layer starts from
        // the same path, and `pop_layer_impl` puts the snapshot back after
        // `restore_draw_state` (which clears it) so the caller can still
        // `stroke()` the outline it clipped with.

        self.commands.push(DrawCommand::PushLayer {
            texture,
            view,
            width: w,
            height: h,
        });
    }

    /// Emit the masked composite for a popped clip layer.  Called from
    /// `pop_layer_impl` once the parent draw state has been restored.
    ///
    /// Caveat inherited from the gradient fill path: the AA halo strip
    /// `tessellate_path_aa` emits can self-overlap at convex corners, so the
    /// composite mesh covers those seam pixels twice.  Opaque layer content is
    /// unaffected (the second draw writes the same colour at coverage 1), but
    /// semi-transparent content is double-composited there and reads slightly
    /// too strong.  If that ever becomes visible, the fix is to rasterise the
    /// mask into an R8 texture once and composite the layer as a single quad
    /// sampling it, instead of drawing the tessellated strip.
    pub(crate) fn push_masked_pop_command(&mut self, layer: WgpuLayerEntry, mask: ClipMaskMesh) {
        let verts = mask.composite_verts(
            layer.origin_x as f32,
            layer.origin_y as f32,
            layer.width,
            layer.height,
        );
        self.commands.push(DrawCommand::PopLayerMasked {
            texture: layer.texture,
            view: layer.view,
            layer_w: layer.width,
            layer_h: layer.height,
            alpha: layer.alpha as f32,
            verts,
            indices: mask.indices,
            parent_clip: layer.parent_clip,
        });
    }
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Masked-layer composite shader: same as `LAYER_WGSL` but the geometry is an
/// arbitrary mesh carrying a per-vertex coverage that multiplies the alpha.
pub(crate) const LAYER_MESH_WGSL: &str = "
struct LayerUniforms {
    resolution: vec2<f32>,
    alpha: f32,
    mask_enabled: u32,
    layer_size: vec2<f32>,
    mask_radius: f32,
    pad0: f32,
    mask_rect: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: LayerUniforms;
@group(1) @binding(0) var u_tex: texture_2d<f32>;
@group(1) @binding(1) var u_sampler: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) cov: f32,
}
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) v_uv: vec2<f32>,
    @location(1) v_cov: f32,
}

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = (in.pos / u.resolution) * 2.0 - 1.0;
    return VOut(vec4<f32>(ndc, 0.0, 1.0), in.uv, in.cov);
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(u_tex, u_sampler, in.v_uv);
    let a = u.alpha * clamp(in.v_cov, 0.0, 1.0);
    return vec4<f32>(c.rgb * a, c.a * a);
}
";

/// Build the masked-layer composite pipeline (`pos2 + uv2 + coverage`,
/// stride 20, premultiplied src-over — matching `layer_pipeline`).
pub(crate) fn build_layer_mesh_pipeline(
    device: &wgpu::Device,
    bgl0: &wgpu::BindGroupLayout,
    bgl1: &wgpu::BindGroupLayout,
    surface_format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer_mesh"),
        source: wgpu::ShaderSource::Wgsl(LAYER_MESH_WGSL.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layer_mesh"),
        bind_group_layouts: &[Some(bgl0), Some(bgl1)],
        immediate_size: 0,
    });
    let attrs = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 8,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32,
            offset: 16,
            shader_location: 2,
        },
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("layer_mesh"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 20,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attrs,
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::ClipMaskMesh;

    #[test]
    fn composite_verts_map_uv_with_flipped_v() {
        let mesh = ClipMaskMesh {
            verts: vec![[10.0, 20.0, 1.0], [30.0, 40.0, 0.5]],
            indices: vec![0, 1, 0],
        };
        let out = mesh.composite_verts(10.0, 20.0, 20, 20);
        // First vertex sits at the layer origin: u = 0, v = 1 (bottom row).
        assert_eq!(&out[0..5], &[10.0, 20.0, 0.0, 1.0, 1.0]);
        // Second is the opposite corner: u = 1, v = 0.
        assert_eq!(&out[5..10], &[30.0, 40.0, 1.0, 0.0, 0.5]);
    }
}
