//! Compositing layer support for the wgpu backend.
//!
//! Mirrors `demo-gl/src/ctx_core/layers.rs`.  Each `push_layer` allocates a
//! `wgpu::Texture` (with `RENDER_ATTACHMENT | TEXTURE_BINDING`) that becomes
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
//!   `retained_layers`, keyed by `u64` widget handle.

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
    /// Allocate a fresh transient layer texture.  Used for `push_layer` (and
    /// transient retained-layer fallback).
    fn alloc_layer_texture(&self, w: u32, h: u32) -> (Arc<wgpu::Texture>, wgpu::TextureView) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (Arc::new(texture), view)
    }

    pub(crate) fn push_layer_with_alpha_impl(
        &mut self,
        width: f64,
        height: f64,
        alpha: f64,
        retained_key: Option<u64>,
    ) {
        let saved = self.capture_draw_state();
        let origin_x = self.ctm().tx;
        let origin_y = self.ctm().ty;
        let (scale_x, scale_y) = layer_scale_from_transform(self.ctm());
        let (w, h) = scaled_layer_size(width, height, scale_x, scale_y);
        let w = w as u32;
        let h = h as u32;

        let (texture, view) = if let Some(key) = retained_key {
            // Reuse retained texture if size matches; otherwise replace.
            let need_new = self
                .retained_layers
                .get(&key)
                .map(|l| l.width != w || l.height != h)
                .unwrap_or(true);
            if need_new {
                let (tex, vw) = self.alloc_layer_texture(w, h);
                self.retained_layers.insert(
                    key,
                    RetainedWgpuLayer {
                        texture: Arc::clone(&tex),
                        view: vw.clone(),
                        width: w,
                        height: h,
                        rounded_clip: None,
                    },
                );
                (tex, vw)
            } else {
                let entry = &self.retained_layers[&key];
                (Arc::clone(&entry.texture), entry.view.clone())
            }
        } else {
            self.alloc_layer_texture(w, h)
        };

        let rounded_clip = retained_key.and_then(|k| {
            self.retained_layers.get(&k).and_then(|l| l.rounded_clip)
        });

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
        });

        // Reset draw state for the layer's local coordinate system.
        self.viewport = (w as f32, h as f32);
        self.state_stack = vec![(TransAffine::new_scaling(scale_x, scale_y), None)];
        self.path = PathStorage::new();

        self.commands.push(DrawCommand::PushLayer {
            texture,
            view,
            width: w,
            height: h,
        });
    }

    pub(crate) fn pop_layer_impl(&mut self) {
        let Some(layer) = self.layer_stack.pop() else {
            return;
        };
        // Restore parent draw state BEFORE emitting the composite, so the
        // composite scissor is the parent's.
        self.restore_draw_state(layer.saved.clone());

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
        });
    }

    pub(crate) fn set_layer_rounded_clip_impl(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
    ) {
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
    /// such retained layer exists OR if its dimensions don't match the request
    /// (caller falls back to a fresh `push_retained_layer_with_alpha` rebuild).
    pub(crate) fn composite_retained_layer_impl(
        &mut self,
        key: u64,
        width: f64,
        height: f64,
        alpha: f64,
    ) -> bool {
        let (scale_x, scale_y) = layer_scale_from_transform(self.ctm());
        let (w, h) = scaled_layer_size(width, height, scale_x, scale_y);
        let (w, h) = (w as u32, h as u32);
        let Some(retained) = self.retained_layers.get(&key) else {
            return false;
        };
        if retained.width != w || retained.height != h {
            return false;
        }
        let texture = Arc::clone(&retained.texture);
        let view = retained.view.clone();
        let rounded_clip = retained.rounded_clip;

        self.commands.push(DrawCommand::CompositeLayer {
            texture,
            view,
            origin_x: self.ctm().tx as f32,
            origin_y: self.ctm().ty as f32,
            layer_w: w,
            layer_h: h,
            alpha: alpha.clamp(0.0, 1.0) as f32,
            rounded_clip,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{layer_scale_from_transform, scaled_layer_size, transformed_rounded_clip};
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
}
