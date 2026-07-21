use super::*;

/// Draw the same LCD mask at a fractional dst (0.4, 0.4) and at the
/// integer (0, 0).  Rounding snaps 0.4 -> 0, so both outputs must be
/// identical.  If someone removes the `.round()` in `draw_lcd_mask`,
/// the fractional call would either miss the mask entirely (casting
/// 0.4 as i32 -> 0 by truncation, accidentally still works) or shift
/// by one pixel, and the assertion fails.
#[test]
fn test_lcd_mask_rounds_fractional_dst_to_pixel_grid() {
    use crate::DrawCtx;

    // 3×3 mask with the middle subpixel triplet fully covered.  Chosen
    // small so the test is trivial to reason about; positioning bugs
    // show up as one-pixel shifts in the composited output.
    let mask: Vec<u8> = vec![
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let draw = |dst_x: f64, dst_y: f64| -> Framebuffer {
        let mut fb = Framebuffer::new(8, 8);
        // Fill white so the black mask is visible on composite.
        for p in fb.pixels_mut().chunks_exact_mut(4) {
            p[0] = 255;
            p[1] = 255;
            p[2] = 255;
            p[3] = 255;
        }
        {
            let mut ctx = GfxCtx::new(&mut fb);
            ctx.draw_lcd_mask(&mask, 3, 3, Color::black(), dst_x, dst_y);
        }
        fb
    };

    let integer = draw(2.0, 2.0);
    let fractional = draw(2.4, 2.4); // rounds to 2
    let fractional2 = draw(1.6, 1.6); // rounds to 2
    assert_eq!(
        integer.pixels(),
        fractional.pixels(),
        "LCD mask at fractional dst (2.4, 2.4) must round to integer grid"
    );
    assert_eq!(
        integer.pixels(),
        fractional2.pixels(),
        "LCD mask at fractional dst (1.6, 1.6) must round to integer grid"
    );

    // Cross-check the assertion is meaningful: shifting by a full pixel
    // (not just fractional noise) produces different output.
    let shifted = draw(3.0, 2.0);
    assert_ne!(
        integer.pixels(),
        shifted.pixels(),
        "integer-pixel shift should change output — otherwise the rounding test is vacuous"
    );
}

// ---------------------------------------------------------------------------
// Step 3 — paint_subtree_backbuffered routing for LcdCoverage mode
// ---------------------------------------------------------------------------
//
// A widget that returns `BackbufferMode::LcdCoverage` from
// `backbuffer_mode()` should now have its subtree painted via an
// `LcdGfxCtx` over an `LcdBuffer`, with the resulting RGB converted to
// RGBA (alpha=255, top-row-first) for the cache.  The defining
// observable property of LCD output is **per-channel coverage variation
// at glyph edges** — the same pixel reads different R/G/B values, which
// is what produces the subpixel-aware sharpness.  An RGBA-grayscale
// path would give R==G==B at every pixel.

/// End-to-end: a widget that opts into `LcdCoverage` and paints an
/// opaque white bg + black text routes through the new LcdGfxCtx path,
/// and the cached bitmap exhibits the per-channel chroma signature of
/// LCD subpixel rendering.
#[test]
fn test_paint_subtree_backbuffered_lcd_coverage_routes_through_lcd_pipeline() {
    use crate::draw_ctx::DrawCtx;
    use crate::event::{Event, EventResult};
    use crate::framebuffer::Framebuffer;
    use crate::geometry::{Rect, Size};
    use crate::gfx_ctx::GfxCtx;
    use crate::text::Font;
    use crate::widget::{paint_subtree, BackbufferCache, BackbufferMode, Widget};
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    /// Minimal widget: paints opaque white bg + black "abc" text.
    /// Opts into `LcdCoverage` backbuffer mode + provides a cache so
    /// `paint_subtree` routes through `paint_subtree_backbuffered`.
    struct LcdTestWidget {
        bounds: Rect,
        cache: BackbufferCache,
        font: Arc<Font>,
        children: Vec<Box<dyn Widget>>,
    }

    impl Widget for LcdTestWidget {
        fn type_name(&self) -> &'static str {
            "LcdTestWidget"
        }
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            // Opaque bg covering full bounds — the LcdCoverage contract.
            ctx.set_fill_color(Color::white());
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
            // Then black text on top.
            ctx.set_fill_color(Color::black());
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(18.0);
            ctx.fill_text("abc", 4.0, 16.0);
        }
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }

        fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
            Some(&mut self.cache)
        }
        fn backbuffer_mode(&self) -> BackbufferMode {
            BackbufferMode::LcdCoverage
        }
    }

    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut widget = LcdTestWidget {
        bounds: Rect::new(0.0, 0.0, 60.0, 24.0),
        cache: BackbufferCache::default(),
        font,
        children: Vec::new(),
    };
    widget.cache.invalidate();

    // Paint via the public entry point — exercises the real
    // `paint_subtree` → `paint_subtree_backbuffered` plumbing.
    let mut fb = Framebuffer::new(60, 24);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        paint_subtree(&mut widget, &mut ctx);
    }

    // Cache must be populated.  `LcdCoverage` mode stores TWO planes:
    // `pixels` = premultiplied colour (3 B/px), `lcd_alpha` = per-channel
    // alpha (3 B/px).  Both must be present and correctly sized.
    let cache = widget.backbuffer_cache_mut().unwrap();
    let color = cache
        .pixels
        .as_ref()
        .expect("colour plane must be populated");
    let alpha = cache
        .lcd_alpha
        .as_ref()
        .expect("LcdCoverage mode must populate lcd_alpha");
    assert_eq!(cache.width, 60);
    assert_eq!(cache.height, 24);
    assert_eq!(color.len(), 60 * 24 * 3, "colour plane is 3 bytes/pixel");
    assert_eq!(alpha.len(), 60 * 24 * 3, "alpha plane is 3 bytes/pixel");

    // Defining property of LCD output: at least one pixel along glyph
    // edges has noticeably different per-channel alphas (R_alpha, G_alpha,
    // B_alpha vary due to the 5-tap filter's phase shift between channels).
    // A grayscale AA path would have R_alpha == G_alpha == B_alpha at every
    // pixel — if THIS check fails, the wiring fell back to the Rgba branch.
    let mut saw_chroma = false;
    for px in alpha.chunks_exact(3) {
        let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        if mx > 30 && (mx - mn) > 10 {
            saw_chroma = true;
            break;
        }
    }
    assert!(saw_chroma,
        "cached alpha plane must show per-channel variation — proves LcdGfxCtx, not GfxCtx, painted");

    // The widget paints an opaque white bg covering its full bounds (the
    // `LcdCoverage` contract), so every subpixel's alpha should be 255.
    // Interior pixels satisfy this cleanly; buffer edges have the 5-tap
    // filter's reach issue and land a little less than 255, so we check
    // for "most pixels fully covered" rather than "every pixel".
    let fully_covered = alpha
        .chunks_exact(3)
        .filter(|px| px[0] == 255 && px[1] == 255 && px[2] == 255)
        .count();
    assert!(
        fully_covered > 60 * 24 / 2,
        "more than half of cached pixels should have full per-channel alpha \
         (opaque-bg widget); got {fully_covered} of {}",
        60 * 24
    );
}

/// `BackbufferMode::Rgba` (default) must keep using the existing
/// `Framebuffer + GfxCtx` path — no behavioural change for the
/// majority of widgets.  Sample a non-text pixel and verify R==G==B
/// (no LCD chroma in the Rgba branch).
#[test]
fn test_paint_subtree_backbuffered_rgba_mode_unchanged() {
    use crate::draw_ctx::DrawCtx;
    use crate::event::{Event, EventResult};
    use crate::framebuffer::Framebuffer;
    use crate::geometry::{Rect, Size};
    use crate::gfx_ctx::GfxCtx;
    use crate::text::Font;
    use crate::widget::{paint_subtree, BackbufferCache, BackbufferMode, Widget};
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    struct RgbaTestWidget {
        bounds: Rect,
        cache: BackbufferCache,
        font: Arc<Font>,
        children: Vec<Box<dyn Widget>>,
    }
    impl Widget for RgbaTestWidget {
        fn type_name(&self) -> &'static str {
            "RgbaTestWidget"
        }
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            ctx.set_fill_color(Color::black());
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(18.0);
            ctx.fill_text("abc", 4.0, 16.0);
        }
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
            Some(&mut self.cache)
        }
        fn backbuffer_mode(&self) -> BackbufferMode {
            BackbufferMode::Rgba
        }
    }

    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut widget = RgbaTestWidget {
        bounds: Rect::new(0.0, 0.0, 60.0, 24.0),
        cache: BackbufferCache::default(),
        font,
        children: Vec::new(),
    };
    widget.cache.invalidate();

    let mut fb = Framebuffer::new(60, 24);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        paint_subtree(&mut widget, &mut ctx);
    }

    let cache = widget.backbuffer_cache_mut().unwrap();
    let bmp = cache
        .pixels
        .as_ref()
        .expect("backbuffer cache must be populated");
    // Rgba path → text on transparent bg, no chroma signature.  Every
    // pixel must satisfy R == G == B (grayscale AA in straight alpha).
    for (i, px) in bmp.chunks_exact(4).enumerate() {
        let (r, g, b) = (px[0], px[1], px[2]);
        assert!(
            r == g && g == b,
            "Rgba mode must produce grayscale pixels (R==G==B); pixel {i} = ({r}, {g}, {b})"
        );
    }
}

// ---------------------------------------------------------------------------
// Phase 5.2 — `draw_lcd_backbuffer_arc` preserves LCD chroma through cache
// ---------------------------------------------------------------------------

/// Direct primitive test: feed `GfxCtx::draw_lcd_backbuffer_arc` a
/// synthetic backbuffer with distinct per-channel alphas and all-zero
/// premultiplied colour (the canonical "black text edge" shape), onto
/// a white framebuffer.  The output must show clear per-channel
/// variation — chroma visibly different per subpixel — proving the
/// per-channel src-over preserves the subpixel data rather than
/// collapsing to grayscale.
#[test]
fn test_gfx_ctx_draw_lcd_backbuffer_arc_preserves_per_channel_chroma() {
    use crate::draw_ctx::DrawCtx;
    use std::sync::Arc;

    // 1×1 backbuffer: black premult colour (0 on all channels) with
    // distinct per-channel alphas.  Each subpixel "fades" the dst's
    // white by a different amount → R/G/B diverge noticeably.
    let color = Arc::new(vec![0u8, 0, 0]);
    let alpha = Arc::new(vec![50u8, 100, 200]);

    // Destination: single pixel, opaque white.
    let mut fb = Framebuffer::new(1, 1);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_fill_color(Color::rgba(1.0, 1.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 1.0, 1.0);
        ctx.fill();
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, 1, 1, 0.0, 0.0, 1.0, 1.0);
    }
    // Per-channel premult src-over: dst.ch = 0 + white_ch * (1 - alpha_ch)
    //   R: 255 * (1 - 50/255)  ≈ 205
    //   G: 255 * (1 - 100/255) ≈ 155
    //   B: 255 * (1 - 200/255) ≈ 55
    // fb alpha ends at 255 (max-alpha accumulation onto already-opaque dst),
    // so fb RGB equals straight-alpha RGB.
    let r = fb.pixels()[0];
    let g = fb.pixels()[1];
    let b = fb.pixels()[2];
    assert!(
        (r as i32 - 205).abs() <= 1,
        "R should be ~205 (255-50), got {r}"
    );
    assert!(
        (g as i32 - 155).abs() <= 1,
        "G should be ~155 (255-100), got {g}"
    );
    assert!(
        (b as i32 - 55).abs() <= 1,
        "B should be ~55 (255-200), got {b}"
    );
    // Explicit chroma check — the three channels must differ by a lot
    // (the whole point of per-channel subpixel rendering).
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    assert!(
        (mx - mn) > 100,
        "per-channel blit must preserve chroma spread; got R={r} G={g} B={b}"
    );
}

/// **Full round-trip:** paint a widget that opts into `LcdCoverage`
/// through `paint_subtree_backbuffered` onto a fresh framebuffer.
/// After paint+cache+blit, the destination must show per-channel RGB
/// variation at glyph edges — LCD chroma survived the cache.
///
/// If the blit path had fallen through to the default-trait collapse
/// + `draw_image_rgba`, channels would be indistinguishable (grayscale
/// AA) and this test would fail.
#[test]
fn test_paint_subtree_backbuffered_lcd_cache_preserves_chroma_at_destination() {
    use crate::draw_ctx::DrawCtx;
    use crate::event::{Event, EventResult};
    use crate::geometry::{Rect, Size};
    use crate::text::Font;
    use crate::widget::{paint_subtree, BackbufferCache, BackbufferMode, Widget};
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    /// Same shape as the Step-3 widget: opaque white bg + black text,
    /// opts into LcdCoverage.
    struct LcdW {
        bounds: Rect,
        cache: BackbufferCache,
        font: Arc<Font>,
        children: Vec<Box<dyn Widget>>,
    }
    impl Widget for LcdW {
        fn type_name(&self) -> &'static str {
            "LcdW"
        }
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            ctx.set_fill_color(Color::white());
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
            ctx.set_fill_color(Color::black());
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(22.0);
            ctx.fill_text("Wing", 4.0, 20.0);
        }
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
            Some(&mut self.cache)
        }
        fn backbuffer_mode(&self) -> BackbufferMode {
            BackbufferMode::LcdCoverage
        }
    }

    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut widget = LcdW {
        bounds: Rect::new(0.0, 0.0, 100.0, 30.0),
        cache: BackbufferCache::default(),
        font,
        children: Vec::new(),
    };
    widget.cache.invalidate();

    // Paint the subtree — this goes all the way through the new
    // LcdCoverage cache pipeline AND the per-channel blit to fb.
    let mut fb = Framebuffer::new(100, 30);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        paint_subtree(&mut widget, &mut ctx);
    }

    // fb now holds premultiplied RGBA with per-channel chroma at glyph
    // edges.  For an opaque-bg widget, the dst alpha stays 255, so
    // the RGB values are effectively the straight-alpha colour.
    // Search for chroma: any pixel with noticeable R/G/B divergence.
    let w = 100usize;
    let h = 30usize;
    let mut saw_chroma = false;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let r = fb.pixels()[i] as i32;
            let g = fb.pixels()[i + 1] as i32;
            let b = fb.pixels()[i + 2] as i32;
            let mx = r.max(g).max(b);
            let mn = r.min(g).min(b);
            if mx > 30 && mn < 230 && (mx - mn) > 15 {
                saw_chroma = true;
                break;
            }
        }
        if saw_chroma {
            break;
        }
    }
    assert!(
        saw_chroma,
        "LcdCoverage cache + draw_lcd_backbuffer_arc blit must land per-channel \
         chroma in the destination framebuffer — proves chroma survived the cache"
    );
}
