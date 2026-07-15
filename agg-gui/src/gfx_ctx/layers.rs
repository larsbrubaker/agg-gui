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
        });
        // Reset to local-space origin, keeping the device/UX scale so content
        // rasterises 1:1 onto the physical-pixel layer buffer.
        self.state.transform = TransAffine::new_scaling(scale_x, scale_y);
        self.state.clip = None;
    }

    /// SrcOver-composite the current layer into the previous render target, then
    /// restore the graphics state that was active at the matching `push_layer`.
    pub fn pop_layer(&mut self) {
        let Some(layer) = self.layer_stack.pop() else {
            return;
        };
        let ox = layer.origin_x as i32;
        let oy = layer.origin_y as i32;
        self.state = layer.saved_state;
        self.state_stack = layer.saved_stack;
        // Composite: src = layer.fb, dst = now-active framebuffer.
        if let Some(top) = self.layer_stack.last_mut() {
            composite_framebuffers(&mut top.fb, &layer.fb, ox, oy, layer.alpha);
        } else {
            composite_framebuffers(self.base_fb, &layer.fb, ox, oy, layer.alpha);
        }
    }
}
