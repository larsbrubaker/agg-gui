//! Shared per-frame rendering helpers used by both the native and WASM harnesses.
//!
//! Mirrors `demo-gl/src/frame.rs`.  Both platform shells call:
//!
//! ```text
//! begin_frame(&mut ctx, surface_view)
//! render_app_frame(&mut ctx, &mut app, width, height, ...)
//! ctx.end_frame()
//! surface_texture.present()
//! ```
//!
//! `begin_frame` issues the clear; `render_app_frame` does layout + paint;
//! `end_frame` flushes the deferred draw-command list to the GPU.
//!
//! `begin_frame` / `end_frame` belong to the renderer (`agg-gui-wgpu`); what
//! stays here is `render_app_frame`, which is inspector plumbing — draining the
//! live-edit queues and refreshing the widget-tree snapshot around layout and
//! paint — and therefore demo/shell code, not renderer code.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::{App, InspectorNode, InspectorOverlay, Size};

use crate::WgpuGfxCtx;

thread_local! {
    static INSPECTOR_SNAPSHOT_EPOCH: Cell<Option<u64>> = const { Cell::new(None) };
    static LAYOUT_FRAME_KEY: Cell<Option<(u32, u32, u64)>> = const { Cell::new(None) };
}

/// Free-function form of [`WgpuGfxCtx::begin_frame`], kept because every
/// existing shell calls `begin_frame(&mut ctx, view)`.  New code should call
/// the method on the context directly.
pub fn begin_frame(ctx: &mut WgpuGfxCtx, view: wgpu::TextureView) {
    ctx.begin_frame(view);
}

/// Reset `ctx`, sync the inspector snapshot, lay out and paint `app`.
///
/// Identical logic to `demo-gl/src/frame.rs::render_app_frame`; only the
/// context type differs.  The caller must call `ctx.end_frame()` after
/// this function returns, then `surface_texture.present()`.
#[allow(clippy::too_many_arguments)]
pub fn render_app_frame(
    ctx: &mut WgpuGfxCtx,
    app: &mut App,
    width: u32,
    height: u32,
    _frame_ms: f64,
    show_inspector: bool,
    inspector_nodes: &Rc<RefCell<Vec<InspectorNode>>>,
    hovered_bounds: &Rc<RefCell<Option<InspectorOverlay>>>,
    base_edits: &Rc<RefCell<Vec<agg_gui::WidgetBaseEdit>>>,
    #[cfg(feature = "reflect")] inspector_edits: &Rc<RefCell<Vec<agg_gui::InspectorEdit>>>,
) {
    // Drain WidgetBase live-edits first.
    {
        let mut q = base_edits.borrow_mut();
        if !q.is_empty() {
            for edit in q.drain(..) {
                let _ = agg_gui::apply_widget_base_edit(app.root_mut(), &edit);
            }
            INSPECTOR_SNAPSHOT_EPOCH.with(|last| last.set(None));
        }
    }

    #[cfg(feature = "reflect")]
    {
        let mut q = inspector_edits.borrow_mut();
        if !q.is_empty() {
            for edit in q.drain(..) {
                let _ = agg_gui::apply_inspector_edit(app.root_mut(), &edit);
            }
            INSPECTOR_SNAPSHOT_EPOCH.with(|last| last.set(None));
        }
    }

    if show_inspector {
        let epoch = agg_gui::animation::invalidation_epoch();
        let nodes_empty = inspector_nodes.borrow().is_empty();
        let captured = app.has_captured_pointer();
        let should_refresh = nodes_empty
            || (!captured && INSPECTOR_SNAPSHOT_EPOCH.with(|last| last.get() != Some(epoch)));
        if should_refresh {
            let t = web_time::Instant::now();
            *inspector_nodes.borrow_mut() = app.collect_inspector_nodes();
            INSPECTOR_SNAPSHOT_EPOCH.with(|last| last.set(Some(epoch)));
            let elapsed = t.elapsed();
            if elapsed.as_millis() >= 10 {
                let n = inspector_nodes.borrow().len();
                eprintln!(
                    "[inspector] collect_inspector_nodes {n} widgets in {:.1}ms",
                    elapsed.as_secs_f64() * 1000.0,
                );
            }
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
    if let Some(_overlay) = hovered {
        // draw_hover_overlay — added in Phase 4 once drawing works.
        // draw_hover_overlay(ctx, overlay);
    }
}
