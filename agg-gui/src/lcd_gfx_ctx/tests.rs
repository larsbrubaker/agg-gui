use super::*;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Smoke test: an `LcdGfxCtx` constructed over a fresh `LcdBuffer`
/// can `clear` + `set_fill_color` + `set_font` + `fill_text` without
/// panicking, and produces non-zero coverage somewhere.  Catches
/// any state-plumbing typo that would silently no-op the path.
#[test]
fn test_lcd_gfx_ctx_basic_fill_text_smoke() {
    let mut buf = LcdBuffer::new(80, 24);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.set_font(font());
        ctx.set_font_size(16.0);
        ctx.fill_text("ABC", 4.0, 14.0);
    }
    // Some pixels should be darker than white (where text was painted).
    let any_dark = buf
        .color_plane()
        .chunks_exact(3)
        .any(|p| p[0] < 250 || p[1] < 250 || p[2] < 250);
    assert!(any_dark, "fill_text via LcdGfxCtx left buffer fully white");
}

/// **End-to-end equivalence (Step 2 contract).**
///
/// Painting the SAME text two ways must produce byte-identical RGB:
///
///   A. Legacy: `GfxCtx` over an RGBA `Framebuffer` with `lcd_mode=true`.
///   B. New:    `LcdGfxCtx` over an `LcdBuffer`.
///
/// Both routes go through `rasterize_text_lcd_cached` (same mask) and
/// per-channel src-over compositing (same math); the only difference
/// is destination format (4 bytes vs 3 bytes per pixel).  If the RGB
/// triplets diverge, the new ctx is producing a different mask
/// placement or compositor than the existing one, and any widget
/// rewired to paint into an `LcdGfxCtx` would visibly disagree with
/// today's text rendering.  This is the contract Step 3 (wiring the
/// ctx into `paint_subtree_backbuffered`) builds on.
#[test]
fn test_lcd_gfx_ctx_text_matches_legacy_lcd_mode() {
    let f = font();
    let w = 120u32;
    let h = 28u32;

    // Way A — legacy `GfxCtx + lcd_mode=true` onto RGBA `Framebuffer`.
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_lcd_mode(true);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.set_font(Arc::clone(&f));
        ctx.set_font_size(18.0);
        <GfxCtx as DrawCtx>::fill_text(&mut ctx, "Hello!", 4.0, 18.0);
    }

    // Way B — new `LcdGfxCtx` onto `LcdBuffer`.
    let mut buf = LcdBuffer::new(w, h);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.set_font(Arc::clone(&f));
        ctx.set_font_size(18.0);
        ctx.fill_text("Hello!", 4.0, 18.0);
    }

    // Compare RGB triplets at every pixel — alpha column in `fb`
    // is not part of the contract (LcdBuffer has no alpha to match
    // against).
    for y in 0..h as usize {
        for x in 0..w as usize {
            let ai = (y * w as usize + x) * 4;
            let bi = (y * w as usize + x) * 3;
            let a_rgb = (fb.pixels()[ai], fb.pixels()[ai + 1], fb.pixels()[ai + 2]);
            let b_rgb = (
                buf.color_plane()[bi],
                buf.color_plane()[bi + 1],
                buf.color_plane()[bi + 2],
            );
            assert_eq!(
                a_rgb, b_rgb,
                "pixel mismatch at ({x},{y}): legacy={a_rgb:?} LcdGfxCtx={b_rgb:?}"
            );
        }
    }
}

// ── Step 2c: stroke / arc / circle / rounded_rect / image blit ──────────

/// `stroke` of a horizontal line must deposit dark pixels along the
/// line's path.  Uses width=1, so we expect the line's row to read
/// noticeably darker than the surrounding rows.
#[test]
fn test_lcd_gfx_ctx_stroke_horizontal_line() {
    let mut buf = LcdBuffer::new(20, 11);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.set_stroke_color(Color::black());
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(2.0, 5.0);
        ctx.line_to(18.0, 5.0);
        ctx.stroke();
    }
    let row_brightness = |y: usize| -> u32 {
        (4..16)
            .map(|x| {
                let i = (y * 20 + x) * 3;
                buf.color_plane()[i] as u32
                    + buf.color_plane()[i + 1] as u32
                    + buf.color_plane()[i + 2] as u32
            })
            .sum()
    };
    let line = row_brightness(5); // line row in Y-up
    let above = row_brightness(8);
    let below = row_brightness(2);
    assert!(
        line < above,
        "stroke row should be darker than row above (line={line}, above={above})"
    );
    assert!(
        line < below,
        "stroke row should be darker than row below (line={line}, below={below})"
    );
}

/// `circle` then `fill` must darken the centre but leave a corner
/// well outside the disc untouched — proves arc emission + concat
/// produce a closed region rather than degenerating to nothing.
#[test]
fn test_lcd_gfx_ctx_circle_darkens_center_not_corner() {
    let mut buf = LcdBuffer::new(20, 20);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.circle(10.0, 10.0, 5.0);
        ctx.fill();
    }
    let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 20 + x) * 3;
        (
            buf.color_plane()[i],
            buf.color_plane()[i + 1],
            buf.color_plane()[i + 2],
        )
    };
    let (cr, cg, cb) = pixel(10, 10);
    assert!(
        cr < 60 && cg < 60 && cb < 60,
        "circle centre should be dark; got ({cr}, {cg}, {cb})"
    );
    let (xr, xg, xb) = pixel(1, 1);
    assert!(
        xr > 240 && xg > 240 && xb > 240,
        "outside-circle corner should stay white; got ({xr}, {xg}, {xb})"
    );
}

/// `rounded_rect` — corner pixels must remain background (rounded
/// off), while the centre is filled.  Catches a missing
/// `concat_path` or a bogus radius normalize that would degenerate
/// the rounded rect to a sharp rect or to nothing.
///
/// Rect (0,0)–(20,20) with r=8: the BL corner arc has centre (8,8)
/// and radius 8, so any pixel outside that arc (distance from (8,8)
/// > 8) but inside the bbox is in the "rounded-off" region.  We
/// >    pick (1,1) which is ~9.9 px from (8,8) — well past the arc edge,
/// >    so AA leak from the LCD filter (which has ±2 subpixel = ~0.67
/// >    pixel reach) cannot reach it.
#[test]
fn test_lcd_gfx_ctx_rounded_rect_clips_corners() {
    let mut buf = LcdBuffer::new(20, 20);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, 20.0, 20.0, 8.0);
        ctx.fill();
    }
    let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 20 + x) * 3;
        (
            buf.color_plane()[i],
            buf.color_plane()[i + 1],
            buf.color_plane()[i + 2],
        )
    };
    // Centre fully inside the rounded rect → dark.
    let (cr, cg, cb) = pixel(10, 10);
    assert!(
        cr < 50 && cg < 50 && cb < 50,
        "rounded rect centre should be dark; got ({cr}, {cg}, {cb})"
    );
    // Far corner of the bbox (1, 1) — beyond the corner arc, inside
    // the rounded-off region.  Must remain white.
    let (xr, xg, xb) = pixel(1, 1);
    assert!(
        xr > 240 && xg > 240 && xb > 240,
        "rounded rect corner area should stay white; got ({xr}, {xg}, {xb})"
    );
    // Mid-edge (10, 1) — inside the rect on its straight bottom edge,
    // far from any corner arc.  Must be dark.
    let (er, eg, eb) = pixel(10, 1);
    assert!(
        er < 50 && eg < 50 && eb < 50,
        "rounded rect mid-edge should be dark; got ({er}, {eg}, {eb})"
    );
}

/// Image blit with Y-flip: a 2×2 source image with distinct colours
/// per cell (top-left=red, top-right=green, bottom-left=blue,
/// bottom-right=opaque-grey).  After blit into a Y-up LcdBuffer at
/// (1,1), the source's top row must land at the buffer's TOP-of-rect
/// row (Y-up = higher Y), the bottom row at the BOTTOM-of-rect row.
/// Catches any Y-flip arithmetic mistake.
#[test]
fn test_lcd_gfx_ctx_image_blit_y_flips_correctly() {
    // RGBA, top-row first.
    let img: Vec<u8> = vec![
        // Row 0 (top): red, green
        255, 0, 0, 255, 0, 255, 0, 255, // Row 1 (bottom): blue, grey
        0, 0, 255, 255, 128, 128, 128, 255,
    ];
    let mut buf = LcdBuffer::new(8, 8);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::black());
        ctx.draw_image_rgba(&img, 2, 2, 1.0, 1.0, 2.0, 2.0);
    }
    let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 8 + x) * 3;
        (
            buf.color_plane()[i],
            buf.color_plane()[i + 1],
            buf.color_plane()[i + 2],
        )
    };
    // Y-up: y=1 is bottom row of dst rect, y=2 is top.  Source's top
    // row (row 0 in storage) is the visually-top row, which lands at
    // buffer y=2.
    assert_eq!(
        pixel(1, 2),
        (255, 0, 0),
        "top-left source must land at top-left of dst rect (Y-up high)"
    );
    assert_eq!(
        pixel(2, 2),
        (0, 255, 0),
        "top-right source must land at top-right of dst rect"
    );
    assert_eq!(
        pixel(1, 1),
        (0, 0, 255),
        "bottom-left source must land at bottom-left of dst rect (Y-up low)"
    );
    assert_eq!(
        pixel(2, 1),
        (128, 128, 128),
        "bottom-right source must land at bottom-right of dst rect"
    );
    // Outside the blit rect — untouched.
    assert_eq!(
        pixel(0, 0),
        (0, 0, 0),
        "pixel outside blit rect should be untouched"
    );
}

/// Image blit alpha — a half-transparent source over a known bg
/// must produce per-channel src-over output (alpha is the same on
/// all three subpixels for image data, by design).
#[test]
fn test_lcd_gfx_ctx_image_blit_alpha_blends_with_destination() {
    // Single pixel: red at 50% alpha (straight-alpha encoding).
    let img: Vec<u8> = vec![255, 0, 0, 128];
    let mut buf = LcdBuffer::new(4, 4);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.draw_image_rgba(&img, 1, 1, 1.0, 1.0, 1.0, 1.0);
    }
    let i = (4 + 1) * 3;
    let (r, g, b) = (
        buf.color_plane()[i],
        buf.color_plane()[i + 1],
        buf.color_plane()[i + 2],
    );
    // Expected: src(255,0,0) * 0.502 + dst(255,255,255) * 0.498
    //         = (255, ~127, ~127)  (slightly biased by quantization)
    assert!(r > 250, "R should be near 255 (bg + src red); got {r}");
    assert!(
        g > 120 && g < 140,
        "G should be near 127 (white minus alpha-attenuated red); got {g}"
    );
    assert!(b > 120 && b < 140, "B should be near 127; got {b}");
}

// ── Step 2d.1: clip enforcement ─────────────────────────────────────────

/// `fill` of a rect that crosses the clip boundary must darken
/// only the pixels inside the clip; the half outside the clip
/// stays untouched.  Catches a missing clip plumb-through to
/// either the AGG raster step or the composite step.
#[test]
fn test_lcd_gfx_ctx_clip_rect_constrains_fill() {
    let mut buf = LcdBuffer::new(20, 10);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.clip_rect(0.0, 0.0, 10.0, 10.0); // clip to LEFT half
        ctx.begin_path();
        ctx.rect(2.0, 2.0, 16.0, 6.0); // straddles the clip edge
        ctx.fill();
    }
    let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 20 + x) * 3;
        (
            buf.color_plane()[i],
            buf.color_plane()[i + 1],
            buf.color_plane()[i + 2],
        )
    };
    // Inside clip + inside rect → dark.
    let (lr, lg, lb) = pixel(5, 5);
    assert!(
        lr < 50 && lg < 50 && lb < 50,
        "pixel inside clip + rect should be dark; got ({lr}, {lg}, {lb})"
    );
    // Outside clip but inside rect → must stay white.
    let (rr, rg, rb) = pixel(15, 5);
    assert!(
        rr > 240 && rg > 240 && rb > 240,
        "pixel outside clip should stay white; got ({rr}, {rg}, {rb})"
    );
}

/// `fill_text` honours the clip — text that runs past the clip
/// edge should leave the post-clip region untouched.  Set up a
/// long string and a short clip; sample beyond the clip edge.
#[test]
fn test_lcd_gfx_ctx_clip_rect_constrains_fill_text() {
    let mut buf = LcdBuffer::new(120, 24);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.set_font(font());
        ctx.set_font_size(18.0);
        ctx.clip_rect(0.0, 0.0, 40.0, 24.0); // clip to first ~40 px
        ctx.fill_text("MMMMMMMMMMMM", 2.0, 18.0);
    }
    // Inside clip, on glyph stroke → expect some dark pixel in the
    // first 40 px columns.
    let mut saw_dark_inside = false;
    for x in 0..40 {
        for y in 0..24 {
            let i = (y * 120 + x) * 3;
            if buf.color_plane()[i] < 100 {
                saw_dark_inside = true;
                break;
            }
        }
        if saw_dark_inside {
            break;
        }
    }
    assert!(
        saw_dark_inside,
        "expected some dark text pixel inside the clip"
    );

    // Outside clip — every pixel beyond x=42 (a small margin past
    // the clip edge to absorb the 5-tap filter's ±2 subpixel reach)
    // must remain white.
    for x in 42..120 {
        for y in 0..24 {
            let i = (y * 120 + x) * 3;
            let (r, g, b) = (
                buf.color_plane()[i],
                buf.color_plane()[i + 1],
                buf.color_plane()[i + 2],
            );
            assert!(
                r > 240 && g > 240 && b > 240,
                "pixel at ({x},{y}) outside clip should stay white; got ({r}, {g}, {b})"
            );
        }
    }
}

/// `draw_image_rgba` honours the clip — pixels outside the clip
/// rect stay untouched even though the source image's destination
/// rect overlaps them.
#[test]
fn test_lcd_gfx_ctx_clip_rect_constrains_image_blit() {
    // Solid red 10×10 RGBA.
    let img: Vec<u8> = (0..10 * 10).flat_map(|_| [255u8, 0, 0, 255]).collect();
    let mut buf = LcdBuffer::new(20, 10);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.clip_rect(0.0, 0.0, 5.0, 10.0); // clip to leftmost 5 columns
        ctx.draw_image_rgba(&img, 10, 10, 0.0, 0.0, 10.0, 10.0);
    }
    let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 20 + x) * 3;
        (
            buf.color_plane()[i],
            buf.color_plane()[i + 1],
            buf.color_plane()[i + 2],
        )
    };
    // Inside clip → red.
    assert_eq!(
        pixel(2, 5),
        (255, 0, 0),
        "inside clip should show source red"
    );
    // Outside clip → white (image suppressed there).
    assert_eq!(
        pixel(7, 5),
        (255, 255, 255),
        "outside clip should stay white"
    );
}

/// `reset_clip` removes a previously-set clip — paint after the
/// reset should reach the full buffer again.
#[test]
fn test_lcd_gfx_ctx_reset_clip_restores_full_buffer() {
    let mut buf = LcdBuffer::new(20, 10);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.clip_rect(0.0, 0.0, 5.0, 10.0);
        ctx.reset_clip();
        ctx.begin_path();
        ctx.rect(2.0, 2.0, 16.0, 6.0); // would be clipped at x=5 if clip remained
        ctx.fill();
    }
    // Pixel at x=15 should now be dark (no clip blocking it).
    let i = (5 * 20 + 15) * 3;
    let (r, g, b) = (
        buf.color_plane()[i],
        buf.color_plane()[i + 1],
        buf.color_plane()[i + 2],
    );
    assert!(
        r < 50 && g < 50 && b < 50,
        "after reset_clip, fill at x=15 should be dark; got ({r}, {g}, {b})"
    );
}

/// Nested `clip_rect` calls intersect — the second call narrows
/// the active clip, doesn't replace it.  Mirrors `GfxCtx::clip_rect`
/// semantics so widget code that nests clips behaves identically.
#[test]
fn test_lcd_gfx_ctx_clip_rect_nests_via_intersection() {
    let mut buf = LcdBuffer::new(20, 20);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        // Outer clip: left half.
        ctx.clip_rect(0.0, 0.0, 10.0, 20.0);
        // Inner clip: top half.  Intersection = top-left quadrant.
        ctx.clip_rect(0.0, 10.0, 20.0, 10.0);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 20.0, 20.0); // would fill everything if no clip
        ctx.fill();
    }
    let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 20 + x) * 3;
        (
            buf.color_plane()[i],
            buf.color_plane()[i + 1],
            buf.color_plane()[i + 2],
        )
    };
    // Top-left (inside intersection) — dark.
    let (tlr, tlg, tlb) = pixel(2, 17);
    assert!(
        tlr < 50 && tlg < 50 && tlb < 50,
        "top-left should be dark; got ({tlr}, {tlg}, {tlb})"
    );
    // Top-right (outside outer clip) — white.
    let (trr, trg, trb) = pixel(17, 17);
    assert!(
        trr > 240 && trg > 240 && trb > 240,
        "top-right should stay white; got ({trr}, {trg}, {trb})"
    );
    // Bottom-left (outside inner clip) — white.
    let (blr, blg, blb) = pixel(2, 2);
    assert!(
        blr > 240 && blg > 240 && blb > 240,
        "bottom-left should stay white; got ({blr}, {blg}, {blb})"
    );
}

// ── Step 2d.2: push_layer / pop_layer ───────────────────────────────────

/// Sanity: paint inside a `push_layer`/`pop_layer` block lands in
/// the parent buffer at the recorded origin.  Catches a missing
/// composite-on-pop or a wrong-origin bug.
#[test]
fn test_lcd_gfx_ctx_push_pop_layer_flushes_into_parent() {
    let mut buf = LcdBuffer::new(20, 20);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        // Translate the parent so the layer lands at (5, 5) in the
        // base buffer's coords — exercises the origin pickup from
        // the CTM at push time.
        ctx.translate(5.0, 5.0);
        ctx.push_layer(8.0, 8.0);
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 8.0, 8.0); // fills the whole layer
        ctx.fill();
        ctx.pop_layer();
    }
    let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 20 + x) * 3;
        (
            buf.color_plane()[i],
            buf.color_plane()[i + 1],
            buf.color_plane()[i + 2],
        )
    };
    // Inside the layer's destination region in the parent → dark.
    assert_eq!(
        pixel(8, 8),
        (0, 0, 0),
        "interior of flushed layer should be dark"
    );
    // Just outside the layer's region → still white.
    assert_eq!(
        pixel(2, 2),
        (255, 255, 255),
        "outside layer region should stay white"
    );
    assert_eq!(
        pixel(15, 15),
        (255, 255, 255),
        "outside layer region should stay white"
    );
}

/// State must be restored after `pop_layer`: the fill colour, font
/// size, transform, and clip rect set inside the layer must NOT
/// leak out into the parent's subsequent paint.  Also: the layer's
/// transform starts at identity (matches `GfxCtx::push_layer`).
#[test]
fn test_lcd_gfx_ctx_push_pop_layer_restores_state() {
    let mut buf = LcdBuffer::new(20, 20);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());

        ctx.set_fill_color(Color::white()); // pre-layer fill colour
        ctx.translate(3.0, 4.0);
        assert_eq!((ctx.transform().tx, ctx.transform().ty), (3.0, 4.0));

        ctx.push_layer(10.0, 10.0);
        // Inside the layer transform must reset to identity.
        assert_eq!(
            (ctx.transform().tx, ctx.transform().ty),
            (0.0, 0.0),
            "push_layer must reset transform inside the layer"
        );
        // Mutate state inside the layer.
        ctx.set_fill_color(Color::rgba(0.1, 0.2, 0.3, 1.0));
        ctx.translate(1.0, 1.0);
        ctx.pop_layer();

        // After pop: transform restored to (3, 4); fill colour restored
        // to white.
        assert_eq!(
            (ctx.transform().tx, ctx.transform().ty),
            (3.0, 4.0),
            "pop_layer must restore transform to its push-time value"
        );

        // Verify fill colour by painting and inspecting bg-untouched
        // pixels.  We fill a small rect into the parent — if the
        // fill colour were the leaked dark teal, those pixels would
        // be that, not white.
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 4.0, 4.0);
        ctx.fill();
    }
    // The post-pop fill happens at translate(3,4), filling rect (3..7, 4..8).
    // Fill colour is white (restored) → those pixels must be white.
    let i = (5 * 20 + 5) * 3;
    let (r, g, b) = (
        buf.color_plane()[i],
        buf.color_plane()[i + 1],
        buf.color_plane()[i + 2],
    );
    assert_eq!(
        (r, g, b),
        (255, 255, 255),
        "post-pop fill must use restored white colour"
    );
}

/// Paint inside a layer must NOT touch the parent buffer until pop.
/// Inspect the parent buffer mid-layer and verify the painted pixels
/// haven't appeared yet.
#[test]
fn test_lcd_gfx_ctx_push_layer_isolates_paint_until_pop() {
    let mut buf = LcdBuffer::new(20, 20);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.push_layer(10.0, 10.0);
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 10.0, 10.0);
        ctx.fill();
        // Mid-layer: parent buffer's pixels must still be all white.
        let base = ctx.buffer();
        assert!(
            base.color_plane()
                .chunks_exact(3)
                .all(|p| p[0] == 255 && p[1] == 255 && p[2] == 255),
            "base buffer must not see layer paint until pop_layer"
        );
        ctx.pop_layer();
    }
    // After pop: pixels (0..10, 0..10) should be dark.
    let i = (5 * 20 + 5) * 3;
    let (r, g, b) = (
        buf.color_plane()[i],
        buf.color_plane()[i + 1],
        buf.color_plane()[i + 2],
    );
    assert_eq!(
        (r, g, b),
        (0, 0, 0),
        "after pop_layer, painted pixels should appear in base"
    );
}

/// Nested layers compose correctly: outer layer flushes the inner
/// layer's contribution as part of its own flush.  Catches stack
/// management bugs where a pop misroutes which buffer becomes
/// "active" after.
#[test]
fn test_lcd_gfx_ctx_push_layer_nests() {
    let mut buf = LcdBuffer::new(30, 30);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.translate(2.0, 2.0);
        ctx.push_layer(20.0, 20.0); // outer layer at (2,2)
        ctx.set_fill_color(Color::black());

        ctx.translate(4.0, 4.0);
        ctx.push_layer(8.0, 8.0); // inner layer at (4,4) within outer
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 8.0, 8.0);
        ctx.fill();
        ctx.pop_layer(); // flush inner → outer at (4,4)

        ctx.pop_layer(); // flush outer → base at (2,2)
    }
    // Inner layer fills (0..8, 0..8) of itself.  Outer composites it
    // at (4,4) → outer pixels (4..12, 4..12) = inner content.  Base
    // composites outer at (2,2) → base pixels (6..14, 6..14) = inner
    // black region.
    let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 30 + x) * 3;
        (
            buf.color_plane()[i],
            buf.color_plane()[i + 1],
            buf.color_plane()[i + 2],
        )
    };
    assert_eq!(
        pixel(10, 10),
        (0, 0, 0),
        "centre of nested layer region should be dark"
    );
    assert_eq!(
        pixel(2, 2),
        (255, 255, 255),
        "well outside nested region should stay white"
    );
    assert_eq!(
        pixel(20, 20),
        (255, 255, 255),
        "well outside nested region should stay white"
    );
}

/// Unmatched `pop_layer` (no preceding `push_layer`) must be a
/// silent no-op — same contract as `GfxCtx::pop_layer`.
#[test]
fn test_lcd_gfx_ctx_unmatched_pop_layer_is_noop() {
    let mut buf = LcdBuffer::new(8, 8);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.pop_layer(); // must not panic
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 8.0, 8.0);
        ctx.fill();
    }
    // Subsequent paint still works — sample an INTERIOR pixel; the
    // 5-tap LCD filter naturally produces partial coverage at the
    // buffer edges (subpixel samples beyond the buffer read as 0)
    // which is a known + correct property of the pipeline.
    let i = (4 * 8 + 4) * 3;
    let (r, g, b) = (
        buf.color_plane()[i],
        buf.color_plane()[i + 1],
        buf.color_plane()[i + 2],
    );
    assert_eq!(
        (r, g, b),
        (0, 0, 0),
        "subsequent paint after unmatched pop should still work"
    );
}

/// CTM must be honoured by `fill_text` — translating the ctx by
/// `(dx, dy)` then drawing at `(x, y)` should land at the same pixel
/// as drawing at `(x+dx, y+dy)` with no translation.  Guards against
/// "forgot to apply CTM in the LCD path" bugs (we hit one of those
/// in the legacy path two iterations ago).
#[test]
fn test_lcd_gfx_ctx_fill_text_honours_translation() {
    let f = font();
    let w = 100u32;
    let h = 24u32;

    let mut buf_a = LcdBuffer::new(w, h);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf_a);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.set_font(Arc::clone(&f));
        ctx.set_font_size(16.0);
        ctx.translate(10.0, 4.0);
        ctx.fill_text("Hi", 0.0, 12.0);
    }

    let mut buf_b = LcdBuffer::new(w, h);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf_b);
        ctx.clear(Color::white());
        ctx.set_fill_color(Color::black());
        ctx.set_font(f);
        ctx.set_font_size(16.0);
        ctx.fill_text("Hi", 10.0, 16.0);
    }

    assert_eq!(
        buf_a.color_plane(),
        buf_b.color_plane(),
        "translate(10,4) + fill_text(0,12) must equal fill_text(10,16)"
    );
}
