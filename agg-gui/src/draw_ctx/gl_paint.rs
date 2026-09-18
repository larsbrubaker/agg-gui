//! The [`GlPaint`] hook trait — inline GPU rendering from a widget's paint.
//!
//! Split out of `draw_ctx.rs` (which re-exports it) to keep that file inside
//! the project's file-length budget.  `DrawCtx::gl_paint` takes a
//! `&mut dyn GlPaint` and calls it at the right point in painter order.

use crate::geometry::Rect;

/// Trait for widgets that want to render 3-D (or other GPU) content inline
/// during the widget paint pass.
///
/// `DrawCtx::gl_paint` calls this with an opaque `gl` handle — implementations
/// downcast it to `glow::Context` (or whatever GL type the platform provides).
/// The software `GfxCtx` never calls `paint`; see [`DrawCtx::gl_paint`].
pub trait GlPaint {
    /// Execute GPU draw calls for the widget's 3-D content.
    ///
    /// `gl` — opaque platform GL context; downcast via `std::any::Any`.
    /// `screen_rect` — Y-up screen-space rect for this widget (for viewport/scissor).
    /// `full_w`, `full_h` — full viewport dimensions (for restoring after).
    /// `parent_clip` — current framework scissor rect `[x, y, w, h]` in GL/Y-up
    ///   pixels, or `None` if no clip is active.  Implementations **must intersect**
    ///   any scissor they set with this rect so that parent widget clips (e.g. a
    ///   collapsed window) correctly hide GPU-rendered content.
    fn gl_paint(
        &mut self,
        gl: &dyn std::any::Any,
        screen_rect: Rect,
        full_w: i32,
        full_h: i32,
        parent_clip: Option<[i32; 4]>,
    );
}
