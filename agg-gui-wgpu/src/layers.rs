//! Compositing layer support for the wgpu backend.
//!
//! Mirrors `demo-gl/src/ctx_core/layers.rs`.  Each `push_layer` takes a
//! `wgpu::Texture` (with `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC` —
//! the copy is what lets a blend-mode draw read it, see `comp_op.rs`) that becomes
//! the active draw target until the matching `pop_layer`.  On pop the layer
//! is composited back into its parent via the `LayerPipeline` (textured quad
//! + optional SDF rounded-corner mask in the fragment shader).
//!
//! Differences from the GL version:
//! - No FBOs/renderbuffers: the layer is just a texture, the render pass
//!   binds it as a colour attachment when active.
//! - No stencil for rounded clip.  Drawing into the layer is unconstrained;
//!   the SDF mask in `LAYER_FRAG` clips at composite time, with anti-aliased
//!   edges built in (vs. the GL stencil path's binary edges + 1px feather).
//! - Retained layers persist the `wgpu::Texture` across frames in
//!   `retained_layers`, keyed by `u64` widget handle; one rebuilt at a new
//!   size retires its old texture to the context's ledger (`texture_life.rs`).
//! - Transient layers come from the context's size-keyed `LayerPool`.
//! - A layer composites at whole parent pixels: the sub-pixel part of the
//!   push-time translation is drawn into the layer's content instead
//!   (`LayerPlacement`), so buffered content lands where direct drawing
//!   would and the composite never resamples.

use std::sync::Arc;

use agg_gui::TransAffine;
use agg_rust::path_storage::PathStorage;

use crate::{
    DrawCommand, LayerRoundedClip, RetainedWgpuLayer, SavedWgpuDrawState, WgpuGfxCtx,
    WgpuLayerEntry,
};

// ---------------------------------------------------------------------------
// Geometry helpers (matched to demo-gl/src/ctx_core/layers.rs)
// ---------------------------------------------------------------------------

/// Extract isotropic-ish scale factors from a transform's linear part.  Used
/// to compute physical-pixel layer extents under hi-DPI / runtime scaling.
fn layer_scale_from_transform(t: &TransAffine) -> (f64, f64) {
    let sx = (t.sx * t.sx + t.shy * t.shy).sqrt().max(1e-6);
    let sy = (t.shx * t.shx + t.sy * t.sy).sqrt().max(1e-6);
    (sx, sy)
}

fn scaled_layer_size(width: f64, height: f64, sx: f64, sy: f64) -> (i32, i32) {
    (
        (width * sx).ceil().max(1.0) as i32,
        (height * sy).ceil().max(1.0) as i32,
    )
}

/// Where a layer pushed under transform `t` sits on its parent target: the
/// composite origin on whole parent pixels, the sub-pixel remainder of the
/// transform's translation that the layer's own content carries instead
/// (so drawing into the layer lands on the same device pixels as drawing
/// straight onto the parent, and the composite is a 1:1 blit that never
/// resamples), and the texture size in physical pixels, one pixel wider or
/// taller when the remainder pushes content over an edge.
struct LayerPlacement {
    origin: (f64, f64),
    frac: (f64, f64),
    scale: (f64, f64),
    size: (u32, u32),
}

/// The placement of a layer `width` x `height` pushed under `t`. Neither
/// side of the texture is over `max_dim`, the device's
/// `max_texture_dimension_2d` (a larger request is a validation error):
/// content of a layer larger than that, past `max_dim` pixels from its
/// origin, is cut off rather than taking the frame down.
fn layer_placement(t: &TransAffine, width: f64, height: f64, max_dim: u32) -> LayerPlacement {
    let (sx, sy) = layer_scale_from_transform(t);
    let origin = (t.tx.floor(), t.ty.floor());
    let frac = (t.tx - origin.0, t.ty - origin.1);
    let (w, h) = scaled_layer_size(width, height, sx, sy);
    // Grow only when the remainder carries content past the unshifted size.
    let w = ((width * sx + frac.0).ceil().max(1.0) as i32).max(w) as u32;
    let h = ((height * sy + frac.1).ceil().max(1.0) as i32).max(h) as u32;
    let (w, h) = (w.min(max_dim.max(1)), h.min(max_dim.max(1)));
    LayerPlacement {
        origin,
        frac,
        scale: (sx, sy),
        size: (w, h),
    }
}

fn transformed_rounded_clip(
    t: &TransAffine,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
) -> LayerRoundedClip {
    let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (mut px, mut py) in corners {
        t.transform(&mut px, &mut py);
        min_x = min_x.min(px);
        min_y = min_y.min(py);
        max_x = max_x.max(px);
        max_y = max_y.max(py);
    }
    let (sx, sy) = layer_scale_from_transform(t);
    LayerRoundedClip {
        x: min_x as f32,
        y: min_y as f32,
        w: (max_x - min_x).abs() as f32,
        h: (max_y - min_y).abs() as f32,
        r: (r * sx.min(sy)) as f32,
    }
}

// ---------------------------------------------------------------------------
// State save/restore for layer push/pop
// ---------------------------------------------------------------------------

impl WgpuGfxCtx {
    pub(crate) fn capture_draw_state(&self) -> SavedWgpuDrawState {
        SavedWgpuDrawState {
            viewport: self.viewport,
            fill_color: self.fill_color,
            fill_linear_gradient: self.fill_linear_gradient.clone(),
            fill_radial_gradient: self.fill_radial_gradient.clone(),
            stroke_color: self.stroke_color,
            stroke_linear_gradient: self.stroke_linear_gradient.clone(),
            stroke_radial_gradient: self.stroke_radial_gradient.clone(),
            line_width: self.line_width,
            line_join: self.line_join,
            line_cap: self.line_cap,
            fill_rule: self.fill_rule,
            miter_limit: self.miter_limit,
            line_dash: self.line_dash.clone(),
            dash_offset: self.dash_offset,
            global_alpha: self.global_alpha,
            blend_mode: self.blend_mode,
            paint_state_stack: self.paint_state_stack.clone(),
            state_stack: self.state_stack.clone(),
            font: self.font.clone(),
            font_size: self.font_size,
            lcd_mode: self.lcd_mode,
        }
    }

    pub(crate) fn restore_draw_state(&mut self, s: SavedWgpuDrawState) {
        self.viewport = s.viewport;
        self.fill_color = s.fill_color;
        self.fill_linear_gradient = s.fill_linear_gradient;
        self.fill_radial_gradient = s.fill_radial_gradient;
        self.stroke_color = s.stroke_color;
        self.stroke_linear_gradient = s.stroke_linear_gradient;
        self.stroke_radial_gradient = s.stroke_radial_gradient;
        self.line_width = s.line_width;
        self.line_join = s.line_join;
        self.line_cap = s.line_cap;
        self.fill_rule = s.fill_rule;
        self.miter_limit = s.miter_limit;
        self.line_dash = s.line_dash;
        self.dash_offset = s.dash_offset;
        self.global_alpha = s.global_alpha;
        self.blend_mode = s.blend_mode;
        self.paint_state_stack = s.paint_state_stack;
        self.state_stack = s.state_stack;
        self.font = s.font;
        self.font_size = s.font_size;
        self.lcd_mode = s.lcd_mode;
        self.path = PathStorage::new();
    }
}

// ---------------------------------------------------------------------------
// Layer push / pop
// ---------------------------------------------------------------------------

impl WgpuGfxCtx {
    /// Allocate a fresh layer texture, counted in the context's texture
    /// ledger. Retained layers own theirs; transient ones come from
    /// [`Self::transient_layer_texture`].
    pub(crate) fn alloc_layer_texture(
        &mut self,
        w: u32,
        h: u32,
    ) -> (Arc<wgpu::Texture>, wgpu::TextureView) {
        new_layer_texture(&mut self.textures, &self.device, self.surface_format, w, h)
    }

    /// A layer texture for this frame only (`push_layer`, a clip layer, a
    /// blend-mode coverage layer): reused from the previous frame's when one
    /// of the same size is free, so steady frames allocate none.
    pub(crate) fn transient_layer_texture(
        &mut self,
        w: u32,
        h: u32,
    ) -> (Arc<wgpu::Texture>, wgpu::TextureView) {
        let (textures, device, format) = (&mut self.textures, &self.device, self.surface_format);
        self.layer_pool
            .take(w, h, || new_layer_texture(textures, device, format, w, h))
    }

    /// Mark the innermost layer as fully covered by opaque content.
    ///
    /// Backs `DrawCtx::set_layer_opaque_backdrop`. No-op at the top level,
    /// where the backbuffer is already opaque.
    pub(crate) fn set_layer_opaque_backdrop_impl(&mut self, opaque: bool) {
        if let Some(layer) = self.layer_stack.last_mut() {
            layer.opaque_backdrop = opaque;
        }
    }

    /// Whether text drawn right now lands on opaque destination pixels —
    /// the real precondition for LCD subpixel rendering.
    ///
    /// True at the top level (the backbuffer's alpha is pinned at 1) and
    /// inside a layer whose caller has declared an opaque backdrop. Note
    /// this asks about the *innermost* layer only: that is the surface
    /// being drawn into, and an opaque region inside it stays opaque
    /// regardless of what the enclosing layers look like. A partially
    /// transparent ancestor fades the whole composite uniformly, which
    /// preserves the subpixel ratios within the glyph.
    pub(crate) fn text_dst_is_opaque(&self) -> bool {
        // A blend-mode draw's coverage layer starts transparent.
        if self.coverage_pass {
            return false;
        }
        match self.layer_stack.last() {
            None => true,
            Some(layer) => layer.opaque_backdrop,
        }
    }

    pub(crate) fn push_layer_with_alpha_impl(
        &mut self,
        width: f64,
        height: f64,
        alpha: f64,
        retained_key: Option<u64>,
    ) {
        let saved = self.capture_draw_state();
        // Snapshot the parent's active scissor BEFORE the layer resets clip
        // state — the composite blit on pop must be clipped to it.
        let parent_clip = self.current_clip();
        let placement = layer_placement(self.ctm(), width, height, self.max_texture_dim);
        let (origin_x, origin_y) = placement.origin;
        let (frac_x, frac_y) = placement.frac;
        let (scale_x, scale_y) = placement.scale;
        let (w, h) = placement.size;

        let (texture, view) = if let Some(key) = retained_key {
            // Reuse retained texture if size matches; otherwise replace.
            let need_new = self
                .retained_layers
                .get(&key)
                .map(|l| l.width != w || l.height != h || l.frac != placement.frac)
                .unwrap_or(true);
            if need_new {
                let (tex, vw) = self.alloc_layer_texture(w, h);
                // The old texture (another size or sub-pixel phase) may still
                // be drawn by this frame's earlier commands; it is destroyed
                // after the frame's submit.
                let old = self.retained_layers.insert(
                    key,
                    RetainedWgpuLayer {
                        texture: Arc::clone(&tex),
                        view: vw.clone(),
                        width: w,
                        height: h,
                        frac: placement.frac,
                        rounded_clip: None,
                    },
                );
                if let Some(old) = old {
                    self.textures.retire(&old.texture);
                }
                (tex, vw)
            } else {
                let entry = &self.retained_layers[&key];
                (Arc::clone(&entry.texture), entry.view.clone())
            }
        } else {
            self.transient_layer_texture(w, h)
        };

        let rounded_clip =
            retained_key.and_then(|k| self.retained_layers.get(&k).and_then(|l| l.rounded_clip));

        self.layer_stack.push(WgpuLayerEntry {
            texture: Arc::clone(&texture),
            view: view.clone(),
            width: w,
            height: h,
            origin_x,
            origin_y,
            alpha: alpha.clamp(0.0, 1.0),
            saved,
            retained_key,
            rounded_clip,
            parent_clip,
            clip_mask: None,
            clip_saved_path: None,
            // Starts false: nothing has been drawn into the fresh texture
            // yet, so it is entirely transparent. Callers opt in after
            // painting their opaque body.
            opaque_backdrop: false,
        });

        // Reset draw state for the layer's local coordinate system.
        self.viewport = (w as f32, h as f32);
        // The translation's sub-pixel remainder stays with the content (see
        // `LayerPlacement`); the composite lands on whole pixels.
        let mut layer_ctm = TransAffine::new_scaling(scale_x, scale_y);
        layer_ctm.multiply(&TransAffine::new_translation(frac_x, frac_y));
        self.state_stack = vec![(layer_ctm, None)];
        // The paint state carries into the layer (as in the software
        // `GfxCtx`); its save/restore stack starts afresh with `state_stack`.
        self.paint_state_stack.clear();
        self.path = PathStorage::new();

        self.commands.push(DrawCommand::PushLayer {
            texture,
            view,
            width: w,
            height: h,
        });
    }

    pub(crate) fn pop_layer_impl(&mut self) {
        let Some(mut layer) = self.layer_stack.pop() else {
            return;
        };
        // Restore parent draw state BEFORE emitting the composite, so the
        // composite scissor is the parent's.
        self.restore_draw_state(layer.saved.clone());

        // Clip layer: composite through the clip path's mesh (see layer_mask.rs).
        if let Some(mask) = layer.clip_mask.take() {
            // `restore_draw_state` cleared the path; canvas `clip()` keeps it,
            // so put back the path that was current at `clip_path()` time.
            if let Some(path) = layer.clip_saved_path.take() {
                self.path = path;
            }
            self.push_masked_pop_command(layer, mask);
            return;
        }

        // Persist rounded clip back into retained store, if applicable.
        if let Some(key) = layer.retained_key {
            if let Some(retained) = self.retained_layers.get_mut(&key) {
                retained.rounded_clip = layer.rounded_clip;
            }
        }

        self.commands.push(DrawCommand::PopLayer {
            texture: layer.texture,
            view: layer.view,
            origin_x: layer.origin_x as f32,
            origin_y: layer.origin_y as f32,
            layer_w: layer.width,
            layer_h: layer.height,
            alpha: layer.alpha as f32,
            rounded_clip: layer.rounded_clip,
            parent_clip: layer.parent_clip,
        });
    }

    pub(crate) fn set_layer_rounded_clip_impl(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        if self.layer_stack.is_empty() {
            return;
        }
        let exact_clip = transformed_rounded_clip(self.ctm(), x, y, w, h, r);
        if let Some(layer) = self.layer_stack.last_mut() {
            layer.rounded_clip = Some(exact_clip);
        }
    }

    /// Composite a previously-retained layer directly onto the current target,
    /// without first pushing into it as a draw target.  Returns `false` if no
    /// such retained layer exists OR if its dimensions or sub-pixel phase
    /// don't match the request (caller falls back to a
    /// fresh `push_retained_layer_with_alpha` rebuild).
    pub(crate) fn composite_retained_layer_impl(
        &mut self,
        key: u64,
        width: f64,
        height: f64,
        alpha: f64,
    ) -> bool {
        let placement = layer_placement(self.ctm(), width, height, self.max_texture_dim);
        let (w, h) = placement.size;
        let Some(retained) = self.retained_layers.get(&key) else {
            return false;
        };
        // Content rastered at another sub-pixel phase would land up to a
        // pixel off, so that rebuilds too.
        if retained.width != w || retained.height != h || retained.frac != placement.frac {
            return false;
        }
        let texture = Arc::clone(&retained.texture);
        let view = retained.view.clone();
        let rounded_clip = retained.rounded_clip;

        let parent_clip = self.current_clip();
        self.commands.push(DrawCommand::CompositeLayer {
            texture,
            view,
            origin_x: placement.origin.0 as f32,
            origin_y: placement.origin.1 as f32,
            layer_w: w,
            layer_h: h,
            alpha: alpha.clamp(0.0, 1.0) as f32,
            rounded_clip,
            parent_clip,
        });
        true
    }
}

/// A layer texture: a colour attachment the composite samples, copyable so
/// a blend-mode draw can read it (`comp_op.rs`).
fn new_layer_texture(
    textures: &mut crate::texture_life::TextureLedger,
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
) -> (Arc<wgpu::Texture>, wgpu::TextureView) {
    let texture = textures.create(
        device,
        &wgpu::TextureDescriptor {
            label: Some("layer"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
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
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (Arc::new(texture), view)
}

#[cfg(test)]
mod tests {
    use super::{
        layer_placement, layer_scale_from_transform, scaled_layer_size, transformed_rounded_clip,
    };
    use agg_gui::TransAffine;

    #[test]
    fn scaled_layer_size_uses_physical_pixels_for_hidpi() {
        let t = TransAffine::new_scaling(2.0, 2.0);
        let (sx, sy) = layer_scale_from_transform(&t);
        assert_eq!(scaled_layer_size(100.0, 50.0, sx, sy), (200, 100));
    }

    #[test]
    fn scaled_layer_size_ceilings_fractional_physical_extent() {
        let t = TransAffine::new_scaling(1.5, 1.5);
        let (sx, sy) = layer_scale_from_transform(&t);
        assert_eq!(scaled_layer_size(11.0, 7.0, sx, sy), (17, 11));
    }

    #[test]
    fn rounded_clip_uses_physical_layer_coordinates() {
        let mut t = TransAffine::new_scaling(2.0, 2.0);
        t.premultiply(&TransAffine::new_translation(8.0, 6.0));
        let clip = transformed_rounded_clip(&t, 4.0, 3.0, 20.0, 10.0, 5.0);
        assert_eq!(clip.x, 24.0);
        assert_eq!(clip.y, 18.0);
        assert_eq!(clip.w, 40.0);
        assert_eq!(clip.h, 20.0);
        assert_eq!(clip.r, 10.0);
    }

    #[test]
    fn rust_only_layer_placement_keeps_the_sub_pixel_translation_in_the_content() {
        let mut t = TransAffine::new_scaling(1.5, 1.5);
        t.multiply(&TransAffine::new_translation(4.5, 3.25));
        let p = layer_placement(&t, 190.0, 80.0, 16384);
        assert_eq!(p.origin, (4.0, 3.0));
        assert_eq!(p.frac, (0.5, 0.25));
        // 285 + 0.5 and 120 + 0.25 physical pixels, rounded out.
        assert_eq!(p.size, (286, 121));
        // A whole-pixel translation keeps the plain size.
        let p = layer_placement(&TransAffine::new_translation(7.0, 2.0), 11.0, 7.0, 16384);
        assert_eq!(
            (p.origin, p.frac, p.size),
            ((7.0, 2.0), (0.0, 0.0), (11, 7))
        );
    }

    #[test]
    fn a_layer_larger_than_the_texture_limit_is_held_to_it() {
        let p = layer_placement(
            &TransAffine::new_translation(3.0, 1.0),
            40000.0,
            80.0,
            16384,
        );
        assert_eq!(p.size, (16384, 80));
        let p = layer_placement(&TransAffine::new_scaling(2.0, 2.0), 30.0, 9000.0, 16384);
        assert_eq!(p.size, (60, 16384));
    }
}
