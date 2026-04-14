//! Shared per-frame rendering helpers used by both the native and WASM harnesses.
//!
//! Keeping this code in one place guarantees that both targets render identically.
//! Each harness is responsible only for:
//!   - platform-specific GL context setup (winit/glutin vs. WebGL2)
//!   - cube drawing (platform renderers differ in API surface)
//!   - unpacking thread-locals (WASM) / stack variables (native)
//!
//! # Typical frame sequence
//!
//! ```text
//! begin_frame(gl, w, h)
//! sync_inspector(app, show, nodes, hovered_bounds)
//! render_app_frame(gl_ctx, app, font, w, h, frame_ms, hovered)
//! // ── platform-specific ──
//! cube.draw_gl(...)
//! ```

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use glow::HasContext;

use agg_gui::{App, Font, InspectorNode, Rect, Size};
use crate::{GlGfxCtx, draw_hover_overlay, draw_status_overlay};

/// Clear the GL framebuffer and configure blend state for a new frame.
///
/// Sets the viewport to `(0, 0, width, height)`, clears colour + depth,
/// enables `SRC_ALPHA / ONE_MINUS_SRC_ALPHA` blending, and disables depth
/// testing and scissor — the standard 2-D UI render state.
///
/// Call once per frame before any draw calls on both native and WASM paths.
pub fn begin_frame(gl: &glow::Context, width: u32, height: u32) {
    unsafe {
        gl.viewport(0, 0, width as i32, height as i32);
        gl.clear_color(0.1, 0.1, 0.1, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::SCISSOR_TEST);
    }
}

/// Synchronise the inspector node snapshot and hover-bounds state.
///
/// When `show_inspector` is `true`, the app's current widget tree is collected
/// into `inspector_nodes` so the inspector panel can render an up-to-date list.
///
/// When `show_inspector` is `false`, `hovered_bounds` is cleared immediately
/// so the teal hover overlay disappears without waiting for the next mouse event.
///
/// Call before [`render_app_frame`] so the snapshot is ready when the inspector
/// panel paints itself.
pub fn sync_inspector(
    app:             &App,
    show_inspector:  bool,
    inspector_nodes: &Rc<RefCell<Vec<InspectorNode>>>,
    hovered_bounds:  &Rc<RefCell<Option<Rect>>>,
) {
    if show_inspector {
        *inspector_nodes.borrow_mut() = app.collect_inspector_nodes();
    } else {
        *hovered_bounds.borrow_mut() = None;
    }
}

/// Reset `ctx`, lay out and paint `app`, then draw the inspector hover overlay
/// and the status bar.
///
/// The caller must draw any platform-specific content (e.g. the rotating 3D
/// cube) *after* this function returns so it appears on top.
///
/// `frame_ms` is the render time of the **previous** frame; it is displayed in
/// the status overlay so the readout does not include its own drawing cost.
/// Pass `hovered_bounds = None` when the inspector is hidden.
pub fn render_app_frame(
    ctx:            &mut GlGfxCtx,
    app:            &mut App,
    font:           Arc<Font>,
    width:          u32,
    height:         u32,
    frame_ms:       f64,
    hovered_bounds: Option<Rect>,
) {
    ctx.reset(width as f32, height as f32);
    app.layout(Size::new(width as f64, height as f64));
    app.paint(ctx);

    if let Some(rect) = hovered_bounds {
        draw_hover_overlay(ctx, rect);
    }
    draw_status_overlay(ctx, font, width, height, frame_ms);
}
