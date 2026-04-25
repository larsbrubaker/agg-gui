use super::*;

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
        return;
    }

    // Snap CTM at paint_subtree ENTRY — see the commentary preserved
    // below inside `paint_subtree_direct` for the full rationale.  The
    // backbuffer path bypasses this because the bitmap is already at
    // integer texel positions by construction.
    if widget.backbuffer_cache_mut().is_some() {
        paint_subtree_backbuffered(widget, ctx);
    } else {
        paint_subtree_direct(widget, ctx);
    }
}

/// Paint app-level overlays after the whole tree has rendered.
///
/// Traverses children first so deeper/modal content wins, then lets each widget
/// draw any global overlay it owns. No parent-local translation is applied:
/// implementors paint in app-level logical coordinates.
pub fn paint_global_overlays(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    if !widget.is_visible() {
        return;
    }
    let n = widget.children().len();
    for i in 0..n {
        let child = &mut widget.children_mut()[i];
        paint_global_overlays(child.as_mut(), ctx);
    }
    widget.paint_global_overlay(ctx);
}

/// Direct (non-cached) paint: widget and its children paint onto `ctx`
/// at the current CTM.  This is the default path for widgets that don't
/// opt into backbuffer caching via `Widget::backbuffer_cache_mut`.
fn paint_subtree_direct(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    paint_subtree_direct_inner(widget, ctx, true);
}

/// Cache-building variant: paints body + children into the given ctx
/// WITHOUT calling `paint_overlay`.  The overlay is what `TextField` uses
/// for its blinking cursor — if we baked the overlay into the cache bitmap,
/// the drawn cursor would stay visible forever on blit while a second
/// (blinking) overlay was being drawn on top of it every frame, producing
/// two cursors.  Overlay runs only on the outer ctx in
/// `paint_subtree_backbuffered` after the cache blit.
fn paint_subtree_direct_no_overlay(widget: &mut dyn Widget, ctx: &mut dyn DrawCtx) {
    paint_subtree_direct_inner(widget, ctx, false);
}

fn paint_subtree_direct_inner(
    widget: &mut dyn Widget,
    ctx: &mut dyn DrawCtx,
    include_overlay: bool,
) {
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

    ctx.restore(); // lifts the children clip before paint_overlay
    if include_overlay {
        widget.paint_overlay(ctx);
    }

    if snap_this {
        ctx.restore();
    }
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
    let dps = crate::device_scale::device_scale().max(1e-6);
    // Physical pixel dimensions of the offscreen render target.
    let w_phys = (b.width * dps).ceil().max(1.0) as u32;
    let h_phys = (b.height * dps).ceil().max(1.0) as u32;
    // Logical dimensions used as the blit destination rect.  **Must** be
    // derived from `w_phys / dps` rather than `b.width` so the quad the
    // bitmap is drawn into matches the bitmap's actual pixel extent.  If
    // `b.width` is non-integer (e.g. 19.5 for a sidebar Label), using
    // it as `dst_w` stretches a 20-pixel bitmap into a 19.5-pixel quad —
    // sub-pixel shrink that drops partial-coverage rows at the edges,
    // which reads as a faint fade along the top / bottom of the glyph.
    // Pre-HiDPI the blit used the bitmap's integer pixel size directly;
    // this restores that contract for the logical-units pipeline.
    let w_logical = w_phys as f64 / dps;
    let h_logical = h_phys as f64 / dps;

    // Decide whether to re-raster.  Size change invalidates; so does a
    // mode swap — if the cache holds `Rgba` bytes but the widget now
    // wants `LcdCoverage` (or vice versa) we must re-raster through the
    // correct pipeline.  Mode membership is recorded implicitly by
    // `cache.lcd_alpha`: `Some` means LCD cache, `None` means Rgba.
    let mode = widget.backbuffer_mode();
    let mode_is_lcd = matches!(mode, BackbufferMode::LcdCoverage);
    let theme_epoch = crate::theme::current_visuals_epoch();
    let typography_epoch = crate::font_settings::current_typography_epoch();
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
            || cache.typography_epoch != typography_epoch;
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
                    if (dps - 1.0).abs() > 1e-6 {
                        // Widgets paint in logical coords — scale the sub ctx
                        // so their drawing lands on the physical pixel grid.
                        sub.scale(dps, dps);
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
                    if (dps - 1.0).abs() > 1e-6 {
                        // Match the RGBA branch: widgets paint in logical
                        // coords; the sub ctx's scale transforms them into
                        // the physical-pixel LCD buffer.
                        sub.scale(dps, dps);
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
    // Image is physical-sized; dst is logical.  The outer CTM already has
    // `scale(dps, dps)` active, so logical dst × dps == physical dst ==
    // bitmap size, giving a 1:1 texel-to-pixel blit (no up/downscale blur).
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
