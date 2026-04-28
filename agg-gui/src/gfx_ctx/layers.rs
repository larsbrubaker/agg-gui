//! Offscreen compositing layer support for `GfxCtx`.
//!
//! Layers let widgets and SVG groups render into temporary transparent
//! framebuffers before compositing the result back into the active target.

use super::*;

impl GfxCtx<'_> {
    // -------------------------------------------------------------------------
    // Layer compositing
    // -------------------------------------------------------------------------

    /// Begin an offscreen compositing layer of `width × height` pixels.
    ///
    /// All draw calls until the matching `pop_layer` are redirected into a fresh
    /// transparent `Framebuffer`.  The current CTM's translation records the
    /// layer's screen-space origin; drawing inside uses a reset local transform.
    pub fn push_layer(&mut self, width: f64, height: f64) {
        self.push_layer_with_alpha(width, height, 1.0);
    }

    pub fn push_layer_with_alpha(&mut self, width: f64, height: f64, alpha: f64) {
        let origin_x = self.state.transform.tx;
        let origin_y = self.state.transform.ty;
        let saved_state = self.state.clone();
        let saved_stack = std::mem::take(&mut self.state_stack);
        let layer_fb = Framebuffer::new(width.ceil() as u32, height.ceil() as u32);
        self.layer_stack.push(LayerEntry {
            fb: layer_fb,
            saved_state,
            saved_stack,
            origin_x,
            origin_y,
            alpha: alpha.clamp(0.0, 1.0),
        });
        // Reset to local-space origin for the new layer.
        self.state.transform = TransAffine::new();
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
