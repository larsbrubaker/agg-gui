// Whole-window opacity for `Window` — a port of agg-sharp's
// `GuiWidget.BackbufferOpacity` as a double-buffered `WindowWidget` uses it.
//
// The window (chrome, shadow and content) renders into its own buffer at
// full strength, and that buffer is composited onto the parent at
// `backbuffer_opacity`, so what is behind the window shows through evenly
// instead of each overlapping shape fading on its own. Two paths carry it:
//
// - Retained-layer backends (wgpu/GL): `backbuffer_spec` (in
//   `widget_impl.rs`) multiplies the retained layer's composite alpha by the
//   opacity. Changing it needs no re-raster.
// - Everything else (the software `GfxCtx`, or a window built with
//   `with_gl_backbuffer(false)`): `opacity_layer` returns a transient
//   compositing layer at the opacity, consulted by `compositing_layer`.
//
// At opacity 1 neither path changes anything, so existing windows paint
// exactly as before.

use super::*;
use crate::widget::CompositingLayer;

impl Window {
    /// Whole-window opacity, `0.0` (invisible) to `1.0` (opaque, the default).
    pub fn backbuffer_opacity(&self) -> f64 {
        self.backbuffer_opacity
    }

    /// Set the whole-window opacity. Clamped to `0.0..=1.0` as agg-sharp's
    /// `BackbufferOpacity` setter clamps; NaN reads as opaque.
    pub fn set_backbuffer_opacity(&mut self, opacity: f64) {
        let clamped = if opacity.is_nan() {
            1.0
        } else {
            opacity.clamp(0.0, 1.0)
        };
        if clamped != self.backbuffer_opacity {
            self.backbuffer_opacity = clamped;
            crate::animation::request_draw();
        }
    }

    /// Builder form of [`Window::set_backbuffer_opacity`].
    pub fn with_backbuffer_opacity(mut self, opacity: f64) -> Self {
        self.set_backbuffer_opacity(opacity);
        self
    }

    /// The transient compositing layer that applies the opacity when the
    /// retained backbuffer path is not in use, or `None` when opaque.
    pub(super) fn opacity_layer(&self) -> Option<CompositingLayer> {
        if self.backbuffer_opacity >= 1.0 {
            return None;
        }
        let (left, bottom, right, top) = if self.chrome {
            Self::layer_outsets()
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        Some(CompositingLayer::new(
            left,
            bottom,
            right,
            top,
            self.backbuffer_opacity,
        ))
    }
}
