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
//! render_app_frame(gl_ctx, app, w, h, frame_ms,
//!                  show_inspector, inspector_nodes, hovered_bounds)
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use glow::HasContext;

use crate::{draw_hover_overlay, GlGfxCtx};
use agg_gui::{App, InspectorNode, Rect, Size};

thread_local! {
    static INSPECTOR_SNAPSHOT_EPOCH: Cell<Option<u64>> = const { Cell::new(None) };
    static LAYOUT_FRAME_KEY: Cell<Option<(u32, u32, u64)>> = const { Cell::new(None) };
}

/// Clear the GL framebuffer and configure blend state for a new frame.
///
/// Sets the viewport to `(0, 0, width, height)`, clears colour + depth,
/// enables standard alpha blending, and disables depth testing and scissor.
///
/// Uses `blend_func_separate` so that the RGB channels blend with
/// `SRC_ALPHA / ONE_MINUS_SRC_ALPHA` (normal Porter-Duff over) while the
/// **alpha channel** of the framebuffer is always kept at 1.0 (`ZERO / ONE`).
/// This prevents the WebGL canvas from becoming semi-transparent when widgets
/// with alpha < 1 are drawn — if the framebuffer alpha dropped below 1 the
/// browser would composite the semi-transparent canvas over the white webpage
/// background, making semi-transparent colours (e.g. the text-selection
/// highlight) appear washed out or invisible.  On native OpenGL the alpha
/// channel of the default framebuffer is unused, so this setting is harmless.
///
/// Call once per frame before any draw calls on both native and WASM paths.
pub fn begin_frame(gl: &glow::Context, width: u32, height: u32) {
    unsafe {
        gl.viewport(0, 0, width as i32, height as i32);
        // Clear to the active theme's `bg_color` so any area the widget tree
        // doesn't paint over shows the theme background (important for
        // translucent separators / edges that composite over this colour).
        let bg = agg_gui::current_visuals().bg_color;
        gl.clear_color(bg.r, bg.g, bg.b, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        gl.enable(glow::BLEND);
        // RGB: standard alpha compositing.
        // Alpha: keep framebuffer alpha at 1.0 (no change from destination).
        gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA, // RGB factors
            glow::ZERO,
            glow::ONE, // alpha factors
        );
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::SCISSOR_TEST);
    }
}

/// Reset `ctx`, sync the inspector snapshot, lay out and paint `app`, then
/// draw the inspector hover overlay.
///
/// The inspector-snapshot sync was previously a separate helper each shell
/// called before render; folding it in keeps both platforms from drifting
/// on *when* the snapshot is refreshed relative to paint, and removes
/// demo-specific coordination from the shells.
///
/// The caller must draw any platform-specific content (e.g. the rotating 3D
/// cube) *after* this function returns so it appears on top.
///
/// `frame_ms` is the render time of the **previous** frame, available to the
/// backend panel display.
pub fn render_app_frame(
    ctx: &mut GlGfxCtx,
    app: &mut App,
    width: u32,
    height: u32,
    _frame_ms: f64,
    show_inspector: bool,
    inspector_nodes: &Rc<RefCell<Vec<InspectorNode>>>,
    hovered_bounds: &Rc<RefCell<Option<Rect>>>,
) {
    // Inspector snapshot sync: refresh the tree snapshot when the
    // inspector is shown, or clear the hover highlight when it's hidden
    // so the overlay vanishes without waiting for the next mouse event.
    if show_inspector {
        let epoch = agg_gui::animation::invalidation_epoch();
        let nodes_empty = inspector_nodes.borrow().is_empty();
        let should_refresh =
            nodes_empty || INSPECTOR_SNAPSHOT_EPOCH.with(|last| last.get() != Some(epoch));
        if should_refresh {
            *inspector_nodes.borrow_mut() = app.collect_inspector_nodes();
            INSPECTOR_SNAPSHOT_EPOCH.with(|last| last.set(Some(epoch)));
        }
    } else {
        *hovered_bounds.borrow_mut() = None;
        INSPECTOR_SNAPSHOT_EPOCH.with(|last| last.set(None));
    }

    ctx.reset(width as f32, height as f32);
    ctx.set_lcd_mode(agg_gui::font_settings::lcd_enabled());

    let layout_key = (width, height, agg_gui::animation::invalidation_epoch());
    let needs_layout = LAYOUT_FRAME_KEY.with(|last| last.get() != Some(layout_key));
    if needs_layout {
        app.layout(Size::new(width as f64, height as f64));
        LAYOUT_FRAME_KEY.with(|last| last.set(Some(layout_key)));
    }
    app.paint(ctx);

    let hovered = *hovered_bounds.borrow();
    if let Some(rect) = hovered {
        draw_hover_overlay(ctx, rect);
    }
}
