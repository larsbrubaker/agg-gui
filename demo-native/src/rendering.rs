//! Native GL frame rendering for the demo shell.
//!
//! `main.rs` owns the event loop and window lifecycle.  This module owns the
//! small per-frame bridge from the native OpenGL context into the shared
//! `demo-gl`/`demo-ui` rendering path, keeping the platform entry point under
//! the project line limit.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{App, InspectorOverlay, Rect};
use demo_gl::{begin_frame, render_app_frame, GlGfxCtx, CUBE_SCREEN_RECT};

pub fn render_frame(
    app: &mut App,
    gl_ctx: &mut GlGfxCtx,
    gl: &glow::Context,
    w: u32,
    h: u32,
    frame_ms: f64,
    show_inspector: bool,
    inspector_nodes: &Rc<RefCell<Vec<agg_gui::InspectorNode>>>,
    hovered_bounds: &Rc<RefCell<Option<InspectorOverlay>>>,
    base_edits: &Rc<RefCell<Vec<agg_gui::WidgetBaseEdit>>>,
    #[cfg(feature = "reflect")] inspector_edits: &Rc<
        RefCell<Vec<agg_gui::InspectorEdit>>,
    >,
) {
    begin_frame(gl, w, h);
    CUBE_SCREEN_RECT.with(|r| r.set(Rect::default()));
    render_app_frame(
        gl_ctx,
        app,
        w,
        h,
        frame_ms,
        show_inspector,
        inspector_nodes,
        hovered_bounds,
        base_edits,
        #[cfg(feature = "reflect")]
        inspector_edits,
    );
}
