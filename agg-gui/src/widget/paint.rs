//! Paint orchestration for the widget tree.
//!
//! Owns the painting traversal: the thread-local `PAINT_CLIP_STACK` that
//! lets descendants query the active clip, [`paint_subtree`] dispatch
//! between direct paint and backbuffer-cached paint, and the GL/software
//! backbuffer variants used by widgets that opt in via
//! [`Widget::backbuffer_spec`](crate::widget::Widget::backbuffer_spec).
//!
//! # Coordinate system
//!
//! All paint coordinates are **logical Y-up**, origin at the bottom-left.
//! Each subtree paints with its `DrawCtx` translated so that (0,0) maps to
//! the widget's own bottom-left corner; child traversal applies further
//! per-child translations. Platform input coordinates are Y-down and are
//! converted at the App event boundary (see `App::flip_y`), not here.

use std::sync::Arc;

use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::lcd_coverage::LcdBuffer;

use super::*;

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
/// [`Framebuffer`] via a software [`GfxCtx`], cached as an
/// `Arc<Vec<u8>>` on the widget, and blitted through
/// [`DrawCtx::draw_image_rgba_arc`].  Subsequent frames that find
/// `cache.dirty == false` skip the re-raster entirely and just blit the
/// existing bitmap — identical fast path to MatterCAD's `DoubleBuffer`.
pub fn paint_subtree(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
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

fn paint_subtree_unified_backbuffer(
    widget: &mut dyn Widget,
    ctx: &mut dyn DrawCtx,
    include_overlay: bool,
) -> bool {
    let spec = widget.backbuffer_spec();
    if spec.kind == BackbufferKind::None {
        return false;
    }

    match spec.kind {
        BackbufferKind::GlFbo if ctx.supports_retained_layers() => {
            paint_subtree_gl_backbuffer(widget, ctx, include_overlay, spec);
            true
        }
        BackbufferKind::SoftwareRgba | BackbufferKind::SoftwareLcd => {
            // Existing CPU widgets still use `backbuffer_cache_mut`; the
            // unified spec provides the migration point without changing their
            // current behavior.
            if widget.backbuffer_cache_mut().is_some() {
                paint_subtree_backbuffered(widget, ctx);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn paint_subtree_gl_backbuffer(
    widget: &mut dyn Widget,
    ctx: &mut dyn DrawCtx,
    include_overlay: bool,
    spec: BackbufferSpec,
) {
    let b = widget.bounds();
    let layer_w = (b.width + spec.outsets.left + spec.outsets.right).max(1.0);
    let layer_h = (b.height + spec.outsets.bottom + spec.outsets.top).max(1.0);
    let subtree_needs_draw = widget.needs_draw();
    let theme_epoch = crate::theme::current_visuals_epoch();
    let typography_epoch = crate::font_settings::current_typography_epoch();
    let async_state_epoch = crate::animation::async_state_epoch();
    let (key, needs_draw) = {
        let Some(state) = widget.backbuffer_state_mut() else {
            paint_subtree_direct(widget, ctx);
            return;
        };
        let w = layer_w.ceil().max(1.0) as u32;
        let h = layer_h.ceil().max(1.0) as u32;
        let changed = state.width != w || state.height != h || state.spec_kind != spec.kind;
        let style_changed = state.theme_epoch != theme_epoch
            || state.typography_epoch != typography_epoch
            || state.async_state_epoch != async_state_epoch;
        let needs = !spec.cached || state.dirty || changed || style_changed || subtree_needs_draw;
        if changed {
            state.width = w;
            state.height = h;
            state.spec_kind = spec.kind;
        }
        (state.id(), needs)
    };

    if spec.cached && !needs_draw {
        ctx.save();
        ctx.translate(-spec.outsets.left, -spec.outsets.bottom);
        let composited = ctx.composite_retained_layer(key, layer_w, layer_h, spec.alpha);
        ctx.restore();
        if composited {
            if let Some(state) = widget.backbuffer_state_mut() {
                state.composite_count = state.composite_count.saturating_add(1);
            }
            return;
        }
    }

    ctx.save();
    ctx.translate(-spec.outsets.left, -spec.outsets.bottom);
    if spec.cached {
        ctx.push_retained_layer_with_alpha(key, layer_w, layer_h, spec.alpha);
    } else {
        ctx.push_layer_with_alpha(layer_w, layer_h, spec.alpha);
    }
    ctx.translate(spec.outsets.left, spec.outsets.bottom);
    paint_subtree_direct_inner(widget, ctx, include_overlay, false);
    ctx.pop_layer();
    ctx.restore();

    if let Some(state) = widget.backbuffer_state_mut() {
        state.dirty = false;
        state.theme_epoch = theme_epoch;
        state.typography_epoch = typography_epoch;
        state.async_state_epoch = async_state_epoch;
        state.repaint_count = state.repaint_count.saturating_add(1);
        state.composite_count = state.composite_count.saturating_add(1);
    }
}

fn paint_subtree_layer(
    widget: &mut dyn Widget,
    ctx: &mut dyn DrawCtx,
    include_overlay: bool,
    layer: crate::widget::CompositingLayer,
) {
    let b = widget.bounds();
    let layer_w = (b.width + layer.outset_left + layer.outset_right).max(1.0);
    let layer_h = (b.height + layer.outset_bottom + layer.outset_top).max(1.0);

    ctx.save();
    ctx.translate(-layer.outset_left, -layer.outset_bottom);
    ctx.push_layer_with_alpha(layer_w, layer_h, layer.alpha);
    ctx.translate(layer.outset_left, layer.outset_bottom);
    paint_subtree_direct_inner(widget, ctx, include_overlay, false);
    ctx.pop_layer();
    ctx.restore();
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

/// Backbuffered paint: re-raster through AGG if dirty, blit the cached
/// bitmap via `draw_image_rgba_arc` regardless.
///
/// # HiDPI
///
/// The backing bitmap is allocated at **physical pixel** dimensions
/// (`bounds × device_scale`) and the sub-ctx running the widget's paint has
/// a matching `scale(dps, dps)` applied.  This means glyph outlines are
/// rasterised at the physical grid — "true" HiDPI rendering, not pixel
/// doubling — and the outer blit then draws the physical-sized image at the
/// widget's logical rect, which the outer CTM (also scaled by dps) maps 1:1
/// back to physical pixels.  Net: logical layout, physical rasterisation,
/// zero upscale blur.
fn paint_subtree_backbuffered(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    // Snap the outer CTM to the pixel grid BEFORE blitting the cached
    // bitmap.  `draw_image_rgba_arc` uses a NEAREST filter for Arc-keyed
    // textures (1:1 blit lane), so a fractional CTM translation shifts
    // every screen pixel by a sub-texel amount — reading back interpolated
    // near-black/near-white instead of the crisp AGG output.  Snapping
    // here restores the "AGG rasterised it, show it at the pixel grid"
    // contract the old pre-refactor code preserved.
    ctx.save();
    ctx.snap_to_pixel();

    let b = widget.bounds();
    // Rasterise at the CURRENT CTM scale, not the bare device-pixel ratio.
    // The on-screen footprint of this widget is `bounds × ctm_scale`, where
    // `ctm_scale = device_scale × ux_scale` at the top level (and may be just
    // `device_scale` inside an offscreen layer that reset its transform).
    // Sizing the offscreen bitmap to `device_scale` only — as this code used
    // to — left the cached bitmap at `1/ux_scale` of its destination quad, so
    // on mobile (ux_scale ≈ 1.7) every CPU-backbuffered widget (the menu bar,
    // Labels) rendered shrunken inside its layout slot while sibling
    // GL-FBO widgets (Windows), which allocate their layer via
    // `layer_scale_from_transform`, scaled correctly.  Matching the CTM scale
    // here puts both paths on the same footing and gives a true 1:1 blit.
    let (sx, sy) = ctx.transform().scaling_abs();
    let dps_x = sx.max(1e-6);
    let dps_y = sy.max(1e-6);
    // Physical pixel dimensions of the offscreen render target.
    let w_phys = (b.width * dps_x).ceil().max(1.0) as u32;
    let h_phys = (b.height * dps_y).ceil().max(1.0) as u32;
    // Logical dimensions used as the blit destination rect.  **Must** be
    // derived from `w_phys / dps` rather than `b.width` so the quad the
    // bitmap is drawn into matches the bitmap's actual pixel extent.  If
    // `b.width` is non-integer (e.g. 19.5 for a sidebar Label), using
    // it as `dst_w` stretches a 20-pixel bitmap into a 19.5-pixel quad —
    // sub-pixel shrink that drops partial-coverage rows at the edges,
    // which reads as a faint fade along the top / bottom of the glyph.
    // Pre-HiDPI the blit used the bitmap's integer pixel size directly;
    // this restores that contract for the logical-units pipeline.
    let w_logical = w_phys as f64 / dps_x;
    let h_logical = h_phys as f64 / dps_y;

    // Decide whether to re-raster.  Size change invalidates; so does a
    // mode swap — if the cache holds `Rgba` bytes but the widget now
    // wants `LcdCoverage` (or vice versa) we must re-raster through the
    // correct pipeline.  Mode membership is recorded implicitly by
    // `cache.lcd_alpha`: `Some` means LCD cache, `None` means Rgba.
    let mode = widget.backbuffer_mode();
    let mode_is_lcd = matches!(mode, BackbufferMode::LcdCoverage);
    let theme_epoch = crate::theme::current_visuals_epoch();
    let typography_epoch = crate::font_settings::current_typography_epoch();
    let async_state_epoch = crate::animation::async_state_epoch();
    let (needs_raster, has_bitmap) = {
        let cache = widget
            .backbuffer_cache_mut()
            .expect("backbuffered widget must return Some from backbuffer_cache_mut");
        let cache_is_lcd = cache.lcd_alpha.is_some();
        let needs = cache.dirty
            || cache.pixels.is_none()
            || cache.width != w_phys
            || cache.height != h_phys
            || cache_is_lcd != mode_is_lcd
            || cache.theme_epoch != theme_epoch
            || cache.typography_epoch != typography_epoch
            || cache.async_state_epoch != async_state_epoch;
        (needs, cache.pixels.is_some())
    };

    if needs_raster {
        // Allocate a fresh render target whose format matches the
        // widget's chosen backbuffer mode, paint the subtree into it,
        // then convert to top-down RGBA for the cache (the blit lane
        // expects `(R, G, B, A)` rows top-first).
        //
        // `LcdCoverage` mode now uses an `LcdGfxCtx` over an `LcdBuffer`
        // — every primitive (fill, stroke, text, image) flows through
        // the per-channel LCD pipeline, so child widgets that paint
        // into this widget's backbuffer compose correctly with
        // LCD-treated text instead of breaking the per-channel
        // coverage at the first non-text fill (the alpha bug the
        // search-box screenshot showed before this change).
        // Each branch produces `(pixels, lcd_alpha)` top-down:
        //   - `Rgba`: `pixels` = straight-alpha RGBA8; `lcd_alpha` = None.
        //   - `LcdCoverage`: `pixels` = premultiplied colour plane (3 B/px);
        //     `lcd_alpha` = per-channel alpha plane (3 B/px).  The blit
        //     step below picks a compositor based on which is present.
        let (pixels_bytes, lcd_alpha_bytes): (Vec<u8>, Option<Vec<u8>>) = match mode {
            BackbufferMode::Rgba => {
                let mut fb = Framebuffer::new(w_phys, h_phys);
                {
                    let mut sub = GfxCtx::new(&mut fb);
                    sub.set_lcd_mode(false); // RGBA mode never uses LCD text
                    if (dps_x - 1.0).abs() > 1e-6 || (dps_y - 1.0).abs() > 1e-6 {
                        // Widgets paint in logical coords — scale the sub ctx
                        // so their drawing lands on the physical pixel grid.
                        sub.scale(dps_x, dps_y);
                    }
                    paint_subtree_direct_no_overlay(widget, &mut sub);
                }
                // Two conversions to make the bitmap directly blittable:
                //   1. Row order — Framebuffer is Y-up, blit lane is top-down.
                //   2. Alpha format — AGG writes premultiplied; the blend
                //      function expects straight alpha so that half-coverage
                //      AA edges composite without the dark-fringe artifact.
                let mut pixels = fb.pixels_flipped();
                crate::framebuffer::unpremultiply_rgba_inplace(&mut pixels);
                (pixels, None)
            }
            BackbufferMode::LcdCoverage => {
                // The LCD pipeline is strictly WRITE-only.  The buffer
                // starts at zero coverage everywhere; the widget paints
                // opaque content covering its full bounds (the contract
                // for this mode) into it via an `LcdGfxCtx`; then the
                // two planes (premultiplied colour + per-channel alpha)
                // are cached and composited onto the destination at
                // blit time via `draw_lcd_backbuffer_arc` — which
                // preserves LCD per-channel chroma through the cache.
                //
                // We deliberately do NOT read from any destination —
                // seeding the buffer from the parent's pixels would
                // tie the cache's validity to the widget's current
                // screen position (stale on scroll / reparent), stall
                // the GPU pipeline on GL (glReadPixels is sync), and
                // break on backends that can't read their own target.
                // Widgets that can't paint their own opaque bg should
                // use `Rgba` mode or paint through the parent's ctx
                // directly instead.
                let mut buf = LcdBuffer::new(w_phys, h_phys);
                {
                    let mut sub = crate::lcd_gfx_ctx::LcdGfxCtx::new(&mut buf);
                    if (dps_x - 1.0).abs() > 1e-6 || (dps_y - 1.0).abs() > 1e-6 {
                        // Match the RGBA branch: widgets paint in logical
                        // coords; the sub ctx's scale transforms them into
                        // the physical-pixel LCD buffer.
                        sub.scale(dps_x, dps_y);
                    }
                    paint_subtree_direct_no_overlay(widget, &mut sub);
                }
                (buf.color_plane_flipped(), Some(buf.alpha_plane_flipped()))
            }
        };
        let pixels = Arc::new(pixels_bytes);
        let lcd_alpha = lcd_alpha_bytes.map(Arc::new);

        let cache = widget.backbuffer_cache_mut().unwrap();
        cache.pixels = Some(Arc::clone(&pixels));
        cache.lcd_alpha = lcd_alpha.as_ref().map(Arc::clone);
        cache.width = w_phys;
        cache.height = h_phys;
        cache.dirty = false;
        cache.theme_epoch = theme_epoch;
        cache.typography_epoch = typography_epoch;
        cache.async_state_epoch = async_state_epoch;
    }

    // Blit the cached bitmap onto the outer ctx.  Two paths:
    //
    //   - `Rgba` cache (no `lcd_alpha`): a single RGBA8 texture via the
    //     standard image-blit lane.  Alpha-aware SrcOver at the blend
    //     stage handles transparency.
    //
    //   - `LcdCoverage` cache (`lcd_alpha` is `Some`): two 3-byte/pixel
    //     planes — premultiplied colour + per-channel alpha.  The
    //     backend's `draw_lcd_backbuffer_arc` composites them with
    //     per-channel src-over, preserving LCD chroma through the
    //     cache round-trip (grayscale AA on backends that fall back
    //     to the default trait impl).
    let cache = widget.backbuffer_cache_mut().unwrap();
    // Image is physical-sized; dst is logical.  The bitmap was rasterised at
    // the current CTM scale (`dps_x`/`dps_y`), and the outer CTM applies that
    // same scale to the logical dst rect, so logical dst × ctm_scale ==
    // physical dst == bitmap size, giving a 1:1 texel-to-pixel blit (no
    // up/downscale blur).
    let img_w = cache.width;
    let img_h = cache.height;
    match (cache.pixels.as_ref(), cache.lcd_alpha.as_ref()) {
        (Some(color), Some(alpha)) => {
            ctx.draw_lcd_backbuffer_arc(color, alpha, img_w, img_h, 0.0, 0.0, w_logical, h_logical);
        }
        (Some(bmp), None) => {
            ctx.draw_image_rgba_arc(bmp, img_w, img_h, 0.0, 0.0, w_logical, h_logical);
        }
        _ => {}
    }
    let _ = has_bitmap;

    // Overlay paint runs AFTER the cache blit and paints directly onto
    // the outer ctx.  Widgets use this for content that changes too
    // often to be worth caching — the canonical case is `TextField`'s
    // blinking cursor, which flips twice per second and would otherwise
    // invalidate the cache 2×/s.  With overlay, cursor is drawn fresh
    // each frame onto the already-blitted bg+text; the cache only
    // invalidates when the text/focus/selection actually changes.
    //
    // `paint_subtree_direct` has the same overlay call after children
    // (see its own body); this keeps the two paint paths consistent.
    widget.paint_overlay(ctx);

    ctx.restore(); // pops the snap_to_pixel save above.
}
