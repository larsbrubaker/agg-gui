//! Offscreen compositing layer support for `GfxCtx`.
//!
//! Layers let widgets and SVG groups render into temporary transparent
//! framebuffers before compositing the result back into the active target.

use super::*;

impl GfxCtx<'_> {
    // -------------------------------------------------------------------------
    // Layer compositing
    // -------------------------------------------------------------------------

    /// Begin an offscreen compositing layer of `width × height` **logical**
    /// pixels.
    ///
    /// All draw calls until the matching `pop_layer` are redirected into a fresh
    /// transparent `Framebuffer`.  The current CTM's translation records the
    /// layer's screen-space (physical) origin; the CTM's **scale is preserved**
    /// so the subtree rasterises at physical resolution inside the layer buffer.
    pub fn push_layer(&mut self, width: f64, height: f64) {
        self.push_layer_with_alpha(width, height, 1.0);
    }

    pub fn push_layer_with_alpha(&mut self, width: f64, height: f64, alpha: f64) {
        let origin_x = self.state.transform.tx;
        let origin_y = self.state.transform.ty;
        // Preserve the CTM's scale across the layer boundary.  Without this the
        // layer buffer would be sized in logical pixels and drawing would use an
        // identity transform, so on a HiDPI display (device scale > 1) a
        // composited subtree rendered at half size into the physical-pixel
        // target — wrong size and position.  We size the buffer in physical
        // pixels (logical × scale) and start the layer's local transform at that
        // pure scale (translation reset to 0, since `origin_x/origin_y` already
        // records where the layer lands in the parent).  This mirrors the wgpu
        // backend's `push_layer_with_alpha_impl`.  Rotation/shear in the CTM are
        // intentionally not carried into the layer — the compositing-layer
        // callers (opacity groups, widget backbuffers) are always axis-aligned.
        let (scale_x, scale_y) = self.state.transform.scaling_abs();
        let scale_x = scale_x.max(1e-6);
        let scale_y = scale_y.max(1e-6);
        let phys_w = (width * scale_x).ceil().max(1.0) as u32;
        let phys_h = (height * scale_y).ceil().max(1.0) as u32;
        let saved_state = self.state.clone();
        let saved_stack = std::mem::take(&mut self.state_stack);
        let layer_fb = Framebuffer::new(phys_w, phys_h);
        self.layer_stack.push(LayerEntry {
            fb: layer_fb,
            saved_state,
            saved_stack,
            origin_x,
            origin_y,
            alpha: alpha.clamp(0.0, 1.0),
            clip_mask: None,
            clip_saved_path: None,
        });
        // Reset to local-space origin, keeping the device/UX scale so content
        // rasterises 1:1 onto the physical-pixel layer buffer.
        self.state.transform = TransAffine::new_scaling(scale_x, scale_y);
        self.state.clip = None;
    }

    /// Intersect the clip with the current path (canvas-2D `clip()`).
    ///
    /// The path is rasterised — with the current CTM and fill rule — into an
    /// 8-bit anti-aliased coverage mask, and a **clip layer** of the path's
    /// (clipped) device-space bounding box is pushed.  Subsequent drawing goes
    /// into that layer in the *same* coordinate space (the CTM is only shifted
    /// by the layer origin, rotation and scale are preserved), and the matching
    /// `restore()` multiplies the layer by the mask before compositing it back.
    ///
    /// `reset_clip()` inside a clip layer only clears the rectangular scissor;
    /// the path mask stays in force until the matching `restore()`.
    ///
    /// Like canvas `clip()` this does **not** clear the current path.  Because
    /// drawing inside the clip layer may replace it, the path as of this call
    /// is snapshotted and put back when the layer pops, so the canvas-port
    /// idiom `begin_path(); <outline>; fill(); save(); clip_path();
    /// <interior>; restore(); stroke();` strokes the outline.  (Canvas leaves
    /// whatever the interior built; restoring the outline is the more useful
    /// behaviour here and is what the wgpu backend does too.)
    pub fn clip_path(&mut self) {
        let ctm = self.state.transform;
        let fill_rule = self.state.fill_rule;

        // Device-space bounds of the path under the current CTM.
        let bounds = {
            let mut curves = ConvCurve::new(&mut self.path);
            let mut transformed = ConvTransform::new(&mut curves, ctm);
            bounding_rect_single(&mut transformed, 0)
        };
        let target_w = active_fb(self.base_fb, &mut self.layer_stack).width() as f64;
        let target_h = active_fb(self.base_fb, &mut self.layer_stack).height() as f64;

        let mut empty = true;
        let (mut x1, mut y1, mut x2, mut y2) = (0.0, 0.0, 0.0, 0.0);
        if let Some(r) = bounds {
            x1 = r.x1;
            y1 = r.y1;
            x2 = r.x2;
            y2 = r.y2;
            // Intersect with the active rectangular scissor and the target.
            if let Some((cx, cy, cw, ch)) = self.state.clip {
                x1 = x1.max(cx);
                y1 = y1.max(cy);
                x2 = x2.min(cx + cw);
                y2 = y2.min(cy + ch);
            }
            x1 = x1.max(0.0);
            y1 = y1.max(0.0);
            x2 = x2.min(target_w);
            y2 = y2.min(target_h);
            empty = !(x2 > x1 && y2 > y1);
        }

        let origin_x = if empty { 0.0 } else { x1.floor() };
        let origin_y = if empty { 0.0 } else { y1.floor() };
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

        // The layer-local CTM is the parent CTM shifted by the layer origin —
        // rotation and scale are deliberately preserved so drawing continues in
        // exactly the same space.
        let mut local = ctm;
        local.tx -= origin_x;
        local.ty -= origin_y;

        // Rasterise the mask: coverage lands in the alpha channel.
        let mask = if empty {
            vec![0u8; (w * h) as usize]
        } else {
            let mut mask_fb = Framebuffer::new(w, h);
            let white = agg_rust::color::Rgba8::new(255, 255, 255, 255);
            rasterize_fill(
                &mut mask_fb,
                &mut self.path,
                &white,
                CompOp::SrcOver,
                None,
                fill_rule,
                &local,
            );
            mask_fb.pixels().chunks_exact(4).map(|p| p[3]).collect()
        };

        let saved_state = self.state.clone();
        let saved_stack = std::mem::take(&mut self.state_stack);
        self.layer_stack.push(LayerEntry {
            fb: Framebuffer::new(w, h),
            saved_state,
            saved_stack,
            origin_x,
            origin_y,
            alpha: 1.0,
            clip_mask: Some(mask),
            // Canvas `clip()` leaves the current path alone; drawing inside the
            // clip layer may replace it, so the snapshot is put back on pop and
            // `save(); clip_path(); …; restore(); stroke();` strokes the
            // outline that was clipped with.
            clip_saved_path: Some(self.path.clone()),
        });
        self.state.transform = local;
        self.state.clip = None;
    }

    /// SrcOver-composite the current layer into the previous render target, then
    /// restore the graphics state that was active at the matching `push_layer`.
    pub fn pop_layer(&mut self) {
        let Some(mut layer) = self.layer_stack.pop() else {
            return;
        };
        // Clip layer: fold the path coverage into the layer's premultiplied
        // alpha before compositing, giving anti-aliased clip edges.
        if let Some(mask) = layer.clip_mask.take() {
            if let Some(path) = layer.clip_saved_path.take() {
                self.path = path;
            }
            let px = layer.fb.pixels_mut();
            // The mask is rasterised at exactly the layer framebuffer's size in
            // `clip_path`; a mismatch means the layer was resized behind our
            // back and the clip would silently mask the wrong pixels.
            debug_assert_eq!(mask.len(), px.len() / 4);
            for (i, cov) in mask.iter().take(px.len() / 4).enumerate() {
                let di = i * 4;
                if *cov == 255 {
                    continue;
                }
                let m = *cov as u32;
                for k in 0..4 {
                    px[di + k] = ((px[di + k] as u32 * m + 127) / 255) as u8;
                }
            }
        }
        let ox = layer.origin_x as i32;
        let oy = layer.origin_y as i32;
        self.state = layer.saved_state;
        self.state_stack = layer.saved_stack;
        // Clip the composite to the parent scissor restored above — a layer
        // sized larger than its clipped content must not overpaint siblings
        // (e.g. a window's opacity group spilling over the title bar).
        let parent_clip = self.state.clip;
        // Composite: src = layer.fb, dst = now-active framebuffer.
        if let Some(top) = self.layer_stack.last_mut() {
            composite_framebuffers(&mut top.fb, &layer.fb, ox, oy, layer.alpha, parent_clip);
        } else {
            composite_framebuffers(self.base_fb, &layer.fb, ox, oy, layer.alpha, parent_clip);
        }
    }
}
