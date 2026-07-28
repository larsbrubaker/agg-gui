//! Paint orchestration for the widget tree.
//!
//! Owns the painting traversal: the thread-local `PAINT_CLIP_STACK` that
//! lets descendants query the active clip, [`paint_subtree`] dispatch
//! between direct paint and backbuffer-cached paint, and the direct
//! (non-cached) traversal itself. The offscreen variants it dispatches to —
//! GL FBO layers, compositing layers and the CPU backbuffer — live in the
//! [`offscreen`] submodule and are used by widgets that opt in via
//! [`Widget::backbuffer_spec`](crate::widget::Widget::backbuffer_spec).
//!
//! # Coordinate system
//!
//! All paint coordinates are **logical Y-up**, origin at the bottom-left.
//! Each subtree paints with its `DrawCtx` translated so that (0,0) maps to
//! the widget's own bottom-left corner; child traversal applies further
//! per-child translations. Platform input coordinates are Y-down and are
//! converted at the App event boundary (see `App::flip_y`), not here.

use super::*;

mod offscreen;

use offscreen::{
    paint_subtree_backbuffered, paint_subtree_layer, paint_subtree_unified_backbuffer,
};

std::thread_local! {
    static PAINT_CLIP_STACK: std::cell::RefCell<Vec<Rect>> =
        std::cell::RefCell::new(Vec::new());
}

/// Current visible paint clip in root coordinates, if painting is inside a
/// clipped subtree. Widgets can use this to avoid starting expensive work for
/// content that traversal visits but the active clip will discard.
pub fn current_paint_clip() -> Option<Rect> {
    PAINT_CLIP_STACK.with(|stack| stack.borrow().last().copied())
}

// ---------------------------------------------------------------------------
// Tree traversal helpers (free functions operating on &mut dyn Widget)
// ---------------------------------------------------------------------------

/// Paint `widget` and all its descendants. The caller must ensure `ctx` is
/// already translated so that (0,0) maps to `widget`'s bottom-left corner.
///
/// If the widget returns `Some` from [`Widget::backbuffer_cache_mut`], the
/// whole subtree (widget + children + overlay) is rendered once into a CPU
/// [`Framebuffer`](crate::framebuffer::Framebuffer) via a software
/// [`GfxCtx`](crate::gfx_ctx::GfxCtx), cached as an
/// `Arc<Vec<u8>>` on the widget, and blitted through
/// [`DrawCtx::draw_image_rgba_arc`].  Subsequent frames that find
/// `cache.dirty == false` skip the re-raster entirely and just blit the
/// existing bitmap — identical fast path to MatterCAD's `DoubleBuffer`.
pub fn paint_subtree(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    // Widgets that defer to the global-overlay pass (modal dialogs) paint
    // nothing here — neither their own body nor their children — so an
    // ancestor's clip can't truncate them. Their `paint_global_overlay`
    // re-enters via `paint_subtree_forced` during the clip-free overlay walk.
    if widget.is_visible() && widget.defer_paint_to_overlay() {
        return;
    }
    paint_subtree_forced(widget, ctx);
}

/// Paint `widget` and its descendants ignoring [`Widget::defer_paint_to_overlay`]
/// for `widget` itself. This is the entry point a deferred widget calls from its
/// `paint_global_overlay` to render its subtree during the clip-escaping global
/// overlay pass. Deferred *descendants* (rare) still route through
/// [`paint_subtree`] and thus keep deferring.
pub(crate) fn paint_subtree_forced(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    if !widget.is_visible() {
        if paint_subtree_unified_backbuffer(widget, ctx, true) {
            return;
        }
        if ctx.supports_compositing_layers() {
            if let Some(layer) = widget.compositing_layer() {
                paint_subtree_layer(widget, ctx, true, layer);
            }
        }
        return;
    }

    // Snap CTM at paint_subtree ENTRY — see the commentary preserved
    // below inside `paint_subtree_direct` for the full rationale.  The
    // backbuffer path bypasses this because the bitmap is already at
    // integer texel positions by construction.
    if paint_subtree_unified_backbuffer(widget, ctx, true) {
        return;
    } else if widget.backbuffer_cache_mut().is_some() {
        paint_subtree_backbuffered(widget, ctx);
    } else {
        paint_subtree_direct(widget, ctx);
    }
}

/// Paint app-level overlays after the whole tree has rendered.
///
/// Traverses in paint order while preserving each widget's normal local
/// transform. Implementors can use `ctx.root_transform()` to submit app-level
/// overlay geometry without forcing retained parents to repaint.
pub fn paint_global_overlays(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    if !widget.is_visible() {
        return;
    }
    let n = widget.children().len();
    for i in 0..n {
        let child = &mut widget.children_mut()[i];
        let b = child.bounds();
        ctx.save();
        ctx.translate(b.x, b.y);
        paint_global_overlays(child.as_mut(), ctx);
        ctx.restore();
    }
    widget.paint_global_overlay(ctx);
}

/// Direct (non-cached) paint: widget and its children paint onto `ctx`
/// at the current CTM.  This is the default path for widgets that don't
/// opt into backbuffer caching via `Widget::backbuffer_cache_mut`.
fn paint_subtree_direct(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    paint_subtree_direct_inner(widget, ctx, true, true);
}

/// Cache-building variant: paints body + children into the given ctx
/// WITHOUT calling `paint_overlay`.  The overlay is what `TextField` uses
/// for its blinking cursor — if we baked the overlay into the cache bitmap,
/// the drawn cursor would stay visible forever on blit while a second
/// (blinking) overlay was being drawn on top of it every frame, producing
/// two cursors.  Overlay runs only on the outer ctx in
/// `paint_subtree_backbuffered` after the cache blit.
fn paint_subtree_direct_no_overlay(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    paint_subtree_direct_inner(widget, ctx, false, true);
}

fn paint_subtree_direct_inner(
    widget: &mut dyn Widget,
    ctx: &mut dyn DrawCtx,
    include_overlay: bool,
    allow_compositing_layer: bool,
) {
    if allow_compositing_layer && ctx.supports_compositing_layers() {
        if let Some(layer) = widget.compositing_layer() {
            paint_subtree_layer(widget, ctx, include_overlay, layer);
            return;
        }
    }

    let snap_this = widget.enforce_integer_bounds();
    if snap_this {
        ctx.save();
        ctx.snap_to_pixel();
    }

    widget.paint(ctx);

    let b = widget.bounds();
    let (cx, cy, cw, ch) = widget
        .clip_children_rect()
        .unwrap_or((0.0, 0.0, b.width, b.height));
    ctx.save();
    ctx.clip_rect(cx, cy, cw, ch);
    let clip = root_rect_from_local(ctx, cx, cy, cw, ch);
    PAINT_CLIP_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let clipped = if let Some(prev) = stack.last().copied() {
            intersect_rects(prev, clip).unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0))
        } else {
            clip
        };
        stack.push(clipped);
    });

    // Apply the widget's optional child transform (pan/zoom for a Scene) to
    // the whole child group.  It goes on AFTER the children clip — the clip
    // stays axis-aligned in this widget's screen-local space while the
    // children paint under the scaled/translated frame.  The transform is
    // popped by the `ctx.restore()` that lifts the children clip below, so
    // per-child `bounds()` offsets are interpreted inside the transform.
    if let Some(t) = widget.child_transform() {
        let mut m = ctx.transform();
        m.premultiply(&t);
        ctx.set_transform(m);
    }

    let n = widget.children().len();
    for i in 0..n {
        let child_bounds = widget.children()[i].bounds();
        let snap_to_pixel = widget.children()[i].enforce_integer_bounds();
        ctx.save();
        if snap_to_pixel {
            ctx.translate(child_bounds.x.round(), child_bounds.y.round());
        } else {
            ctx.translate(child_bounds.x, child_bounds.y);
        }
        let child = &mut widget.children_mut()[i];
        paint_subtree(child.as_mut(), ctx);
        ctx.restore();
    }

    PAINT_CLIP_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
    ctx.restore(); // lifts the children clip before paint_overlay
    if include_overlay {
        widget.paint_overlay(ctx);
    }
    widget.finish_paint(ctx);

    if snap_this {
        ctx.restore();
    }
}

fn root_rect_from_local(ctx: &dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) -> Rect {
    let mut points = [(x, y), (x + w, y), (x, y + h), (x + w, y + h)];
    let transform = ctx.root_transform();
    for (px, py) in &mut points {
        transform.transform(px, py);
    }
    let min_x = points.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
    let max_x = points
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = points.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let max_y = points
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    Rect::new(
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    )
}

fn intersect_rects(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    (x1 >= x0 && y1 >= y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}
