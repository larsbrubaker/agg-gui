use super::*;
use std::sync::Arc;
use std::time::Instant;

use crate::text::Font;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// The rasteriser must produce some non-zero coverage for ordinary
/// text — sanity check that the pipeline wires up at all.
#[test]
fn test_lcd_mask_has_coverage() {
    let mask = rasterize_lcd_mask(
        &font(),
        "Hello",
        16.0,
        4.0,
        12.0,
        200,
        40,
        &TransAffine::new(),
    );
    let total: u64 = mask.data.iter().map(|&b| b as u64).sum();
    assert!(total > 0, "rasterize_lcd_mask produced all-zero coverage");
}

/// Edge pixels must exhibit **per-channel variation** — the
/// defining property of LCD subpixel rendering.  Without the 5-tap
/// filter's phase shift between R/G/B, the three channels would be
/// identical at every pixel.
#[test]
fn test_lcd_mask_has_channel_variation() {
    let mask = rasterize_lcd_mask(
        &font(),
        "Wing",
        24.0,
        4.0,
        16.0,
        400,
        40,
        &TransAffine::new(),
    );
    let mut saw = false;
    for px in mask.data.chunks_exact(3) {
        let r = px[0];
        let g = px[1];
        let b = px[2];
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        if mx > 20 && (mx - mn) > 10 {
            saw = true;
            break;
        }
    }
    assert!(saw, "no per-channel variation at edges");
}

/// Compositing the mask must mix text into any destination bg and
/// produce plausibly darker pixels for dark-on-light, and plausibly
/// lighter pixels for light-on-dark, regardless of which bg the mask
/// was rastered against (it wasn't rastered against any).
#[test]
fn test_composite_dark_on_light_and_light_on_dark() {
    let mask = rasterize_lcd_mask(&font(), "Hi", 20.0, 2.0, 14.0, 80, 24, &TransAffine::new());

    // Dark text on white.
    let mut fb_white = vec![255u8; 80 * 24 * 4];
    composite_lcd_mask(&mut fb_white, 80, 24, &mask, Color::black(), 0, 0);
    let sum_white: u64 = fb_white
        .chunks_exact(4)
        .map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64)
        .sum();
    assert!(
        sum_white < 80 * 24 * 3 * 255,
        "dark-on-white composite left every pixel white"
    );

    // Light text on black.
    let mut fb_black = vec![0u8; 80 * 24 * 4];
    for chunk in fb_black.chunks_exact_mut(4) {
        chunk[3] = 255;
    }
    composite_lcd_mask(&mut fb_black, 80, 24, &mask, Color::white(), 0, 0);
    let sum_black: u64 = fb_black
        .chunks_exact(4)
        .map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64)
        .sum();
    assert!(
        sum_black > 0,
        "light-on-black composite left every pixel black"
    );
}

/// `composite_lcd_mask` must honour `src.a` — multiply each channel's
/// coverage by the source alpha.  Without this, partial-alpha text
/// (e.g. a placeholder drawn in a half-opacity "dim" colour) blits
/// at full opacity, looking solid instead of faded.
///
/// Regression test for the bug visible in the search-box placeholder
/// where "Search..." rendered at full intensity in LCD mode.
#[test]
fn test_composite_lcd_mask_honours_src_alpha() {
    // Single pixel, full coverage on all three channels.
    let mask = LcdMask {
        data: vec![255, 255, 255],
        width: 1,
        height: 1,
    };

    // Opaque black on white → full black.
    let mut fb_full = vec![255u8, 255, 255, 255];
    composite_lcd_mask(
        &mut fb_full,
        1,
        1,
        &mask,
        Color::rgba(0.0, 0.0, 0.0, 1.0),
        0,
        0,
    );
    assert_eq!(
        fb_full[0], 0,
        "alpha=1 black-on-white should fully cover → R=0"
    );

    // Half-alpha black on white → ~50% grey.
    let mut fb_half = vec![255u8, 255, 255, 255];
    composite_lcd_mask(
        &mut fb_half,
        1,
        1,
        &mask,
        Color::rgba(0.0, 0.0, 0.0, 0.5),
        0,
        0,
    );
    // Expected: cov = 1.0 × 0.5 = 0.5; dst = 0×0.5 + 255×0.5 ≈ 128.
    assert!(
        fb_half[0] >= 120 && fb_half[0] <= 135,
        "alpha=0.5 black-on-white should land near R=128, got {}",
        fb_half[0]
    );

    // Zero-alpha: dst unchanged.
    let mut fb_zero = vec![255u8, 255, 255, 255];
    composite_lcd_mask(
        &mut fb_zero,
        1,
        1,
        &mask,
        Color::rgba(0.0, 0.0, 0.0, 0.0),
        0,
        0,
    );
    assert_eq!(fb_zero[0], 255, "alpha=0 must leave destination untouched");
}

// ── LcdBuffer paint primitives ──────────────────────────────────────────

/// `LcdBuffer::clear` writes the requested colour into every pixel.
/// Premultiplied alpha applies uniformly across all three channels —
/// the buffer has no alpha store, so partial-alpha is realised by
/// darkening the colour, not by storing transparency.
#[test]
fn test_lcd_buffer_clear_writes_solid_color() {
    let mut buf = LcdBuffer::new(4, 3);
    buf.clear(Color::rgba(1.0, 0.5, 0.25, 1.0));
    for px in buf.color_plane().chunks_exact(3) {
        assert_eq!(px[0], 255);
        assert_eq!(px[1], 128);
        assert_eq!(px[2], 64);
    }

    // Half-alpha → premultiplied colour at half intensity.
    let mut buf2 = LcdBuffer::new(2, 2);
    buf2.clear(Color::rgba(1.0, 1.0, 1.0, 0.5));
    for px in buf2.color_plane().chunks_exact(3) {
        assert_eq!(px[0], 128);
        assert_eq!(px[1], 128);
        assert_eq!(px[2], 128);
    }
}

// ── Per-channel alpha: the new capability ────────────────────────────────

/// Fresh buffer is fully transparent (both planes zero).  This is
/// the defining change from the old 3-byte LcdBuffer: unpainted
/// regions no longer read as "intentional black" on composite.
#[test]
fn test_lcd_buffer_fresh_is_fully_transparent() {
    let buf = LcdBuffer::new(8, 4);
    assert!(
        buf.color_plane().iter().all(|&b| b == 0),
        "fresh buffer's color plane must be zero"
    );
    assert!(
        buf.alpha_plane().iter().all(|&b| b == 0),
        "fresh buffer's alpha plane must be zero (= fully transparent)"
    );
}

/// Paint black text onto a transparent buffer.  The premultiplied
/// colour is black × alpha = 0, so `color_plane` stays all zeros —
/// but `alpha_plane` picks up coverage at text pixels and stays
/// zero elsewhere.  That zero-alpha outside-text region is exactly
/// the property that lets a cached LcdBuffer blit onto any parent
/// without the "black rect where unpainted" failure mode.
#[test]
fn test_lcd_buffer_transparent_plus_black_text_leaves_alpha_only() {
    let f = font();
    let mask = rasterize_lcd_mask(&f, "Hi", 20.0, 2.0, 14.0, 80, 24, &TransAffine::new());
    let mut buf = LcdBuffer::new(80, 24);
    buf.composite_mask(&mask, Color::black(), 0, 0, None);

    assert!(
        buf.color_plane().iter().all(|&b| b == 0),
        "black-text-on-transparent: premult colour is 0, so color_plane stays zero"
    );
    let alpha_nonzero = buf.alpha_plane().iter().filter(|&&b| b > 0).count();
    assert!(
        alpha_nonzero > 0,
        "alpha_plane must show coverage where text was rasterized"
    );

    // Corners of the buffer (far from text) must stay fully transparent.
    let bottom_left_i = 0;
    let bottom_right_i = (80 - 1) * 3;
    let top_left_i = (23 * 80) * 3;
    let top_right_i = (23 * 80 + 79) * 3;
    for i in [bottom_left_i, bottom_right_i, top_left_i, top_right_i] {
        assert_eq!(
            &buf.alpha_plane()[i..i + 3],
            &[0u8, 0, 0],
            "corner at byte offset {i} should be transparent"
        );
    }
}

/// Opaque red text deposits premultiplied red into the colour plane
/// AND full alpha into the alpha plane at fully-covered subpixels.
/// This is the crisp case where per-channel alpha == per-channel
/// coverage, no divergence.
#[test]
fn test_lcd_buffer_red_text_writes_premultiplied_color() {
    let f = font();
    let w = 80u32;
    let h = 24u32;
    let mask = rasterize_lcd_mask(&f, "I", 24.0, 4.0, 18.0, w, h, &TransAffine::new());
    let mut buf = LcdBuffer::new(w, h);
    buf.composite_mask(&mask, Color::rgba(1.0, 0.0, 0.0, 1.0), 0, 0, None);

    // Look for at least one pixel where the R channel is fully
    // covered: R_alpha = 255, R_color = 255 (premult red × 1),
    // and G/B colour stay zero (red source has no G or B).
    let mut saw_full_red = false;
    for i in (0..(w * h) as usize).map(|p| p * 3) {
        if buf.alpha_plane()[i] == 255
            && buf.color_plane()[i] == 255
            && buf.color_plane()[i + 1] == 0
            && buf.color_plane()[i + 2] == 0
        {
            saw_full_red = true;
            break;
        }
    }
    assert!(
        saw_full_red,
        "expected at least one fully-covered pure-red pixel"
    );
}

#[test]
fn test_opaque_lcd_composite_matches_float_formula() {
    let mask = LcdMask {
        data: vec![0, 64, 255, 128, 192, 32],
        width: 2,
        height: 1,
    };
    let src = Color::rgba(0.25, 0.5, 0.75, 1.0);
    let mut buf = LcdBuffer::new(2, 1);
    buf.clear(Color::rgba(0.1, 0.2, 0.3, 0.4));

    let before_color = buf.color_plane().to_vec();
    let before_alpha = buf.alpha_plane().to_vec();
    buf.composite_mask(&mask, src, 0, 0, None);

    for i in 0..6 {
        let channel = i % 3;
        let src_channel = [src.r, src.g, src.b][channel];
        let mask_channel = mask.data[i];
        assert_eq!(
            buf.color_plane()[i],
            reference_float_blend(src_channel, before_color[i], mask_channel)
        );
        assert_eq!(
            buf.alpha_plane()[i],
            reference_float_blend(1.0, before_alpha[i], mask_channel)
        );
    }
}

#[test]
#[ignore = "diagnostic timing report; run manually with --ignored --nocapture"]
fn lcd_pipeline_timing_report() {
    const W: u32 = 480;
    const H: u32 = 360;
    const ITERS: usize = 100;

    let mut path = PathStorage::new();
    path.move_to(40.0, 40.0);
    path.line_to(440.0, 40.0);
    path.line_to(440.0, 320.0);
    path.line_to(40.0, 320.0);
    path.close_polygon(agg_rust::basics::PATH_FLAGS_NONE);

    let mut masks = Vec::with_capacity(ITERS);
    let start = Instant::now();
    for _ in 0..ITERS {
        let _ = LcdMaskBuilder::new(W, H);
    }
    let builder_new = elapsed_ms(start);

    let start = Instant::now();
    for _ in 0..ITERS {
        let mut builder = LcdMaskBuilder::new(W, H);
        builder.with_paths(&TransAffine::new(), |add| add(&mut path));
        masks.push(builder.finalize());
    }
    let rasterize_and_filter = elapsed_ms(start);

    let mut buffer = LcdBuffer::new(W, H);
    let start = Instant::now();
    for mask in &masks {
        buffer.composite_mask(mask, Color::rgba(0.2, 0.4, 0.8, 1.0), 0, 0, None);
    }
    let composite_solid = elapsed_ms(start);

    let start = Instant::now();
    for _ in 0..ITERS {
        let _ = LcdBuffer::new(W, H);
    }
    let buffer_new = elapsed_ms(start);

    eprintln!("LCD pipeline timing over {ITERS} iterations at {W}x{H}");
    eprintln!("LcdMaskBuilder::new:       {:>8.2} ms", builder_new);
    eprintln!(
        "rasterize + 5-tap filter:  {:>8.2} ms",
        rasterize_and_filter
    );
    eprintln!("LcdBuffer::composite_mask: {:>8.2} ms", composite_solid);
    eprintln!("LcdBuffer::new:            {:>8.2} ms", buffer_new);
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn reference_float_blend(src: f32, dst_byte: u8, coverage_byte: u8) -> u8 {
    let coverage = coverage_byte as f32 / 255.0;
    let dst = dst_byte as f32 / 255.0;
    ((src * coverage + dst * (1.0 - coverage)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// `composite_buffer` leaves dst untouched wherever src has alpha=0.
/// The defining behavioural property of the two-plane design: a
/// sub-layer with painted content plus unpainted margins flushes
/// back onto its parent without clobbering the margins.
#[test]
fn test_lcd_buffer_composite_buffer_leaves_dst_untouched_where_src_is_transparent() {
    // src: all transparent (no paint).
    let src = LcdBuffer::new(4, 4);

    // dst: solid white.
    let mut dst = LcdBuffer::new(4, 4);
    dst.clear(Color::white());

    // Snapshot expected values: white everywhere, full alpha.
    for px in dst.color_plane().chunks_exact(3) {
        assert_eq!(px, [255, 255, 255]);
    }
    for px in dst.alpha_plane().chunks_exact(3) {
        assert_eq!(px, [255, 255, 255]);
    }

    // Composite transparent src onto white dst.  Must leave dst unchanged.
    dst.composite_buffer(&src, 0, 0, None);
    for px in dst.color_plane().chunks_exact(3) {
        assert_eq!(
            px,
            [255, 255, 255],
            "dst colour must survive transparent src composite"
        );
    }
    for px in dst.alpha_plane().chunks_exact(3) {
        assert_eq!(
            px,
            [255, 255, 255],
            "dst alpha must survive transparent src composite"
        );
    }
}

/// `composite_buffer`: a fully-opaque src pixel fully replaces the
/// corresponding dst pixel; a fully-transparent src pixel leaves
/// dst alone.  This is exactly the Porter-Duff src-over you'd want
/// for any layer-flush operation, just expressed per-channel.
#[test]
fn test_lcd_buffer_composite_buffer_opaque_pixel_replaces_dst() {
    // src: pixel (1,1) painted opaque red, rest transparent.
    let mut src = LcdBuffer::new(3, 3);
    // Manually set pixel (1,1) premultiplied red + full alpha on all three channels.
    let i = (1 * 3 + 1) * 3;
    src.color_plane_mut()[i] = 255; // R premult = 1.0 * 1.0 = 1.0 → 255
    src.color_plane_mut()[i + 1] = 0;
    src.color_plane_mut()[i + 2] = 0;
    src.alpha_plane_mut()[i] = 255;
    src.alpha_plane_mut()[i + 1] = 255;
    src.alpha_plane_mut()[i + 2] = 255;

    // dst: solid white.
    let mut dst = LcdBuffer::new(3, 3);
    dst.clear(Color::white());

    dst.composite_buffer(&src, 0, 0, None);

    // Pixel (1,1) should now be red (fully replaced).
    assert_eq!(
        &dst.color_plane()[i..i + 3],
        &[255, 0, 0],
        "opaque src pixel must fully replace dst pixel's colour"
    );
    assert_eq!(
        &dst.alpha_plane()[i..i + 3],
        &[255, 255, 255],
        "alpha stays full opacity after opaque-src overwrite"
    );

    // Corner (0,0) — src transparent → dst white unchanged.
    assert_eq!(
        &dst.color_plane()[0..3],
        &[255, 255, 255],
        "corner should retain dst white (src was transparent there)"
    );
}

// ── Legacy tests (opaque content — still valid under new semantics) ──────

/// Compositing a non-empty mask onto a cleared buffer must leave at
/// least some pixels modified — proves the path connects.
#[test]
fn test_lcd_buffer_composite_mask_deposits_coverage() {
    let mask = rasterize_lcd_mask(&font(), "Hi", 20.0, 2.0, 14.0, 80, 24, &TransAffine::new());
    let mut buf = LcdBuffer::new(80, 24);
    buf.clear(Color::white()); // white bg
    let before: u64 = buf.color_plane().iter().map(|&b| b as u64).sum();
    buf.composite_mask(&mask, Color::black(), 0, 0, None); // black text
    let after: u64 = buf.color_plane().iter().map(|&b| b as u64).sum();
    assert!(
        after < before,
        "compositing dark text onto white bg should reduce summed brightness"
    );
}

// ── LcdMaskBuilder + LcdBuffer::fill_path ───────────────────────────────

/// **Refactor regression** — the legacy `rasterize_lcd_mask_multi`
/// must produce byte-identical output after being rewritten as a
/// thin wrapper over `LcdMaskBuilder`.  If the bytes drift, every
/// cached glyph mask in the existing text path subtly changes and
/// the equivalence chain to all the prior tests breaks.
#[test]
fn test_lcd_mask_builder_matches_legacy_text_path() {
    let f = font();
    let w: u32 = 120;
    let h: u32 = 30;
    let xform = TransAffine::new();

    // Legacy path.
    let legacy = rasterize_lcd_mask_multi(&f, &[("Equiv", 4.0, 18.0)], 22.0, w, h, &xform);

    // Builder path — same setup spelt out by hand.
    let mut builder = LcdMaskBuilder::new(w, h);
    builder.with_paths(&xform, |add| {
        let (mut paths, _) = crate::text::shape_text(&f, "Equiv", 22.0, 4.0, 18.0);
        for p in paths.iter_mut() {
            add(p);
        }
    });
    let built = builder.finalize();

    assert_eq!(legacy.width, built.width);
    assert_eq!(legacy.height, built.height);
    assert_eq!(
        legacy.data, built.data,
        "LcdMaskBuilder must reproduce rasterize_lcd_mask_multi byte-for-byte"
    );
}

/// Non-text smoke test for the path entry point — fill a small
/// rectangular AGG path through the LCD pipeline and verify pixels
/// inside the rect are dark, outside are untouched.  Exercises the
/// builder + composite_mask seam without any text shaping involved.
#[test]
fn test_lcd_buffer_fill_path_solid_rect() {
    use agg_rust::basics::PATH_FLAGS_NONE;
    let mut buf = LcdBuffer::new(20, 10);
    buf.clear(Color::white());

    // Rectangle from (5, 3) to (15, 7) in Y-up pixel space.
    let mut path = PathStorage::new();
    path.move_to(5.0, 3.0);
    path.line_to(15.0, 3.0);
    path.line_to(15.0, 7.0);
    path.line_to(5.0, 7.0);
    path.close_polygon(PATH_FLAGS_NONE);

    buf.fill_path(
        &mut path,
        Color::black(),
        &TransAffine::new(),
        None,
        FillRule::NonZero,
    );

    let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = (y * 20 + x) * 3;
        (
            buf.color_plane()[i],
            buf.color_plane()[i + 1],
            buf.color_plane()[i + 2],
        )
    };

    // Centre of rect — fully covered, must be black on every channel.
    assert_eq!(
        pixel(10, 5),
        (0, 0, 0),
        "interior pixel of solid rect should be fully covered black"
    );
    // Outside rect — untouched, must stay white.
    assert_eq!(
        pixel(1, 1),
        (255, 255, 255),
        "pixel outside rect should be untouched"
    );
    assert_eq!(
        pixel(18, 8),
        (255, 255, 255),
        "pixel outside rect should be untouched"
    );
}

/// **End-to-end equivalence** — proves the new path-driven LcdBuffer
/// pipeline matches the existing text-driven one for the same glyph
/// outlines, when both are composited onto the same starting bg.
/// This is the contract the LcdGfxCtx (Step 2) relies on.
#[test]
fn test_lcd_buffer_fill_path_matches_text_pipeline_for_glyphs() {
    let f = font();
    let w: u32 = 80;
    let h: u32 = 24;
    let size = 18.0;
    let baseline = (4.0_f64, 14.0_f64);

    // Way A — legacy: rasterize text mask, composite_mask onto white buffer.
    let legacy_mask = rasterize_lcd_mask_multi(
        &f,
        &[("ag", baseline.0, baseline.1)],
        size,
        w,
        h,
        &TransAffine::new(),
    );
    let mut buf_a = LcdBuffer::new(w, h);
    buf_a.clear(Color::white());
    buf_a.composite_mask(&legacy_mask, Color::black(), 0, 0, None);

    // Way B — builder + fill_path: shape glyphs to paths, fill each onto a
    // freshly cleared buffer.  The end result must be pixel-identical.
    let (mut paths, _) = crate::text::shape_text(&f, "ag", size, baseline.0, baseline.1);
    let mut buf_b = LcdBuffer::new(w, h);
    buf_b.clear(Color::white());
    // Each glyph is its own path; compose them in one mask via the builder
    // so adjacent glyphs share the same gray buffer (matches the legacy
    // batching — separate fill_path calls would also work but each would
    // re-run the filter independently and two adjacent glyphs near a
    // pixel boundary could disagree on the filter input by one subpixel).
    let mut builder = LcdMaskBuilder::new(w, h);
    builder.with_paths(&TransAffine::new(), |add| {
        for p in paths.iter_mut() {
            add(p);
        }
    });
    let mask_b = builder.finalize();
    buf_b.composite_mask(&mask_b, Color::black(), 0, 0, None);

    assert_eq!(
        buf_a.color_plane(),
        buf_b.color_plane(),
        "fill_path-via-builder must match legacy text mask pipeline byte-for-byte"
    );
}

/// **Equivalence test** — the load-bearing one for this step.
///
/// Painting `text` two ways must produce identical RGB:
///
///   A. Existing `composite_lcd_mask` writing into a white RGBA frame.
///   B. New `LcdBuffer::clear(white) + composite_mask(black)` route.
///
/// If these diverge, the new buffer-side compositor doesn't match the
/// existing one and any LcdGfxCtx built on top of it will subtly
/// disagree with the legacy text path.  This is the contract that
/// future widget-level migrations rely on.
#[test]
fn test_lcd_buffer_composite_matches_composite_lcd_mask() {
    let w: u32 = 100;
    let h: u32 = 28;
    let mask = rasterize_lcd_mask(&font(), "Equiv", 22.0, 4.0, 18.0, w, h, &TransAffine::new());

    // Way A — straight RGBA composite.
    let mut rgba = vec![255u8; (w * h * 4) as usize];
    composite_lcd_mask(&mut rgba, w, h, &mask, Color::black(), 0, 0);

    // Way B — paint into LcdBuffer, then read RGB out directly.
    let mut buf = LcdBuffer::new(w, h);
    buf.clear(Color::white());
    buf.composite_mask(&mask, Color::black(), 0, 0, None);

    for y in 0..h as usize {
        for x in 0..w as usize {
            let ai = (y * w as usize + x) * 4;
            let bi = (y * w as usize + x) * 3;
            let a_rgb = (rgba[ai], rgba[ai + 1], rgba[ai + 2]);
            let b_rgb = (
                buf.color_plane()[bi],
                buf.color_plane()[bi + 1],
                buf.color_plane()[bi + 2],
            );
            assert_eq!(
                a_rgb, b_rgb,
                "RGB mismatch at ({x},{y}): RGBA-path={a_rgb:?} LcdBuffer-path={b_rgb:?}"
            );
        }
    }
}
