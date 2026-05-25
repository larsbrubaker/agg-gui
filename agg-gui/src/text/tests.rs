use super::*;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const FA_BYTES: &[u8] = include_bytes!("../../../demo/assets/fa.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font ok"))
}

/// Font-Awesome codepoint U+F109 ("fa-laptop") — used by the demo's
/// backend-panel button label.  The primary font (CascadiaCode) does not
/// cover the FA range, so the fallback chain must carry it.
const FA_LAPTOP: &str = "\u{F109}";

/// A `shape_text` call for a codepoint absent from the primary font must
/// walk the fallback chain and produce the real glyph outline — not the
/// primary font's `.notdef` (the tofu box the top screenshot shows).
#[test]
fn test_shape_text_renders_fa_icon_via_fallback() {
    let fa = Font::from_slice(FA_BYTES).expect("parse fa.ttf");
    let font = Arc::new(
        Font::from_slice(FONT_BYTES)
            .expect("cc")
            .with_fallback(Arc::new(fa)),
    );

    // shape_glyphs must agree the glyph was resolved via fallback.
    let shaped = shape_glyphs(&font, FA_LAPTOP, 16.0);
    assert_eq!(shaped.len(), 1);
    assert!(
        shaped[0].fallback_font.is_some(),
        "FA codepoint must resolve via fallback font"
    );

    // shape_text must return a non-empty path for that glyph.
    let (paths, _adv) = shape_text(&font, FA_LAPTOP, 16.0, 0.0, 0.0);
    assert_eq!(
        paths.len(),
        1,
        "fallback outline must yield exactly one PathStorage for FA_LAPTOP"
    );
}

/// The outline returned by `shape_text` for a codepoint missing from the
/// primary font must match the fallback font's outline — not the primary
/// font's `.notdef`.  Compare flattened bounding boxes.
#[test]
fn test_shape_text_fa_outline_matches_fallback_font() {
    use agg_rust::basics::{is_stop, VertexSource};
    use agg_rust::conv_curve::ConvCurve;

    let fa_arc = Arc::new(Font::from_slice(FA_BYTES).expect("fa"));
    let font = Arc::new(
        Font::from_slice(FONT_BYTES)
            .expect("cc")
            .with_fallback(Arc::clone(&fa_arc)),
    );

    // Outline via the fallback-aware shape_text.
    let (mut paths, _) = shape_text(&font, FA_LAPTOP, 48.0, 0.0, 0.0);
    assert_eq!(paths.len(), 1);
    let mut curves = ConvCurve::new(&mut paths[0]);
    curves.rewind(0);

    let (mut xmin, mut xmax) = (f64::INFINITY, f64::NEG_INFINITY);
    loop {
        let (mut cx, mut cy) = (0.0, 0.0);
        let cmd = curves.vertex(&mut cx, &mut cy);
        if is_stop(cmd) {
            break;
        }
        if cx < xmin {
            xmin = cx;
        }
        if cx > xmax {
            xmax = cx;
        }
        let _ = cy;
    }
    let width = xmax - xmin;

    // FA's "laptop" glyph is full-width at 48 px; the CascadiaCode .notdef
    // (tofu) is closer to advance-width (~24 px).  A width over 32 px at
    // size 48 proves we took the fallback outline, not .notdef.
    assert!(
        width > 32.0,
        "FA glyph outline width at 48 px was {width:.1} — too narrow, \
         likely still rendering CascadiaCode .notdef instead of FA fallback"
    );
}

/// Verify that shape_and_flatten_text produces a sane number of
/// contour points at typical UI font sizes.
///
/// Before the fix, subdivide_quad tested flatness in font units
/// (~2048 upm), producing ~1000 sub-divisions per Bézier segment
/// instead of ~4 — this test would time-out or produce millions of
/// points under the broken implementation.
#[test]
fn test_flatten_point_count_is_sane() {
    let font = test_font();
    let sizes: &[f64] = &[10.0, 13.0, 14.0, 24.0, 34.0];
    let texts: &[&str] = &[
        "Hello",
        "The quick brown fox",
        "Caption — 10px  The quick brown fox",
        "agg-gui",
        "Aa",
    ];

    for &size in sizes {
        for &text in texts {
            let contours = shape_and_flatten_text(&font, text, size, 0.0, 0.0, 0.5);

            let total_pts: usize = contours.iter().map(|c| c.len()).sum();
            let char_count = text.chars().count().max(1);
            let pts_per_char = total_pts / char_count;

            // A well-formed glyph at any typical size should produce
            // between 4 and 300 points per character.  Anything above
            // ~500 means over-subdivision is happening again.
            assert!(
                pts_per_char <= 500,
                "size={size} text={text:?}: {pts_per_char} pts/char \
                 (total {total_pts}) — too many, subdivision loop likely"
            );
            assert!(
                total_pts > 0 || text.trim().is_empty(),
                "size={size} text={text:?}: zero points produced"
            );
        }
    }
}

/// Print raw contour coordinates for a single character.
#[test]
fn test_dump_single_char_coords() {
    use crate::gl_renderer::tessellate_fill;
    let font = test_font();
    for ch in ['W', 'i', 'd', 'g', 'e', 't', 's'] {
        let s = ch.to_string();
        let contours = shape_and_flatten_text(&font, &s, 13.0, 10.0, 50.0, 0.5);
        let total: usize = contours.iter().map(|c| c.len()).sum();
        eprintln!("{:?}: {} contours, {} pts", ch, contours.len(), total);
        // Print bounding box of each contour
        for (ci, c) in contours.iter().enumerate() {
            if c.is_empty() {
                continue;
            }
            let xs: Vec<f32> = c.iter().map(|p| p[0]).collect();
            let ys: Vec<f32> = c.iter().map(|p| p[1]).collect();
            let xmin = xs.iter().cloned().fold(f32::INFINITY, f32::min);
            let xmax = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let ymin = ys.iter().cloned().fold(f32::INFINITY, f32::min);
            let ymax = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            eprintln!(
                "  contour {ci}: {}/{} pts  x:[{xmin:.1},{xmax:.1}] y:[{ymin:.1},{ymax:.1}]",
                c.len(),
                c.len()
            );
        }
        let result = tessellate_fill(&contours);
        eprintln!(
            "  tess: {:?}",
            result.as_ref().map(|(v, i)| (v.len() / 2, i.len() / 3))
        );
    }
}

/// Simulate the text draw calls that happen on the very first WASM
/// render frame (Basics tab + window visible) and assert the full
/// pipeline (shape → flatten → tessellate) completes in < 200 ms.
///
/// This test catches both infinite-subdivision loops and algorithmic
/// slowness that would cause a tab-kill dialog in the browser.
/// WASM is ~5× slower than native, so 200 ms native ≈ 1 s WASM — fine.
#[test]
fn test_first_frame_text_pipeline_is_fast() {
    use crate::gl_renderer::tessellate_fill;
    use std::time::Instant;

    let font = test_font();
    let t0 = Instant::now();

    // All fill_text calls expected on the first rendered frame:
    //   tab bar (TabView), window title + label (Window),
    //   button labels (Button), text field placeholders (TextField).
    let calls: &[(&str, f64)] = &[
        // tab bar labels (13 pt)
        ("Basics", 13.0),
        ("Widgets", 13.0),
        ("Text", 13.0),
        ("Layout", 13.0),
        ("Tree", 13.0),
        // floating window
        ("3D Demo", 16.0),
        ("WebGL2 — rotating cube", 11.0),
        // Basics tab buttons
        ("Primary Action", 14.0),
        ("Secondary", 14.0),
        ("Destructive", 14.0),
        // text field placeholders
        ("Type something\u{2026}", 14.0),
        ("Another field", 14.0),
    ];

    let mut total_pts = 0usize;
    let mut total_tris = 0usize;

    for &(text, size) in calls {
        let contours = shape_and_flatten_text(&font, text, size, 10.0, 50.0, 0.5);
        total_pts += contours.iter().map(|c| c.len()).sum::<usize>();

        if let Some((verts, idx)) = tessellate_fill(&contours) {
            total_tris += idx.len() / 3;
            let _ = verts;
        }
    }

    let elapsed = t0.elapsed();

    // Sanity: we should have produced some geometry.
    assert!(total_pts > 0, "no contour points produced");
    assert!(total_tris > 0, "no triangles tessellated");

    // Performance gate: must finish in under 200 ms natively.
    assert!(
        elapsed.as_millis() < 200,
        "first-frame text pipeline took {}ms (pts={total_pts} tris={total_tris}) — \
         too slow, would hang browser (WASM is ~5× slower)",
        elapsed.as_millis()
    );

    eprintln!(
        "first-frame text: {total_pts} pts, {total_tris} tris in {}ms",
        elapsed.as_millis()
    );
}

/// Verify shape_glyphs returns the right number of glyphs with positive advances.
#[test]
fn test_shape_glyphs_basic() {
    let font = test_font();
    let glyphs = shape_glyphs(&font, "Hi", 14.0);
    assert_eq!(glyphs.len(), 2, "two glyphs for 'Hi'");
    assert!(glyphs[0].x_advance > 0.0, "H has positive advance");
    assert!(glyphs[1].x_advance > 0.0, "i has positive advance");
}

/// flatten_glyph_at_origin must produce coords in glyph-local pixel space
/// (roughly 0..size range), not in font units (hundreds–thousands).
#[test]
fn test_flatten_glyph_at_origin_local_coords() {
    let font = test_font();
    let size = 16.0_f64;
    let glyphs = shape_glyphs(&font, "H", size);
    assert!(!glyphs.is_empty());
    let gid = glyphs[0].glyph_id;

    let contours = flatten_glyph_at_origin(&font, gid, size).expect("'H' must have an outline");
    assert!(!contours.is_empty(), "should produce at least one contour");

    for contour in &contours {
        for &[x, y] in contour {
            assert!(
                x >= -2.0 && x <= size as f32 + 4.0,
                "x={x} should be in glyph-local pixels for size={size}"
            );
            assert!(
                y >= -size as f32 * 0.3 && y <= size as f32 * 1.2,
                "y={y} should be in glyph-local pixels for size={size}"
            );
        }
    }
}

/// Space has no outline; flatten_glyph_at_origin should return None.
#[test]
fn test_flatten_glyph_at_origin_space_returns_none() {
    let font = test_font();
    let glyphs = shape_glyphs(&font, " ", 14.0);
    assert_eq!(glyphs.len(), 1);
    let result = flatten_glyph_at_origin(&font, glyphs[0].glyph_id, 14.0);
    assert!(
        result.is_none(),
        "space glyph should have no outline, got {:?}",
        result.as_ref().map(|c| c.len())
    );
}

/// `glyph_visual_bounds` must return the actual glyph extent, not the
/// font's worst-case ascender/descender. For Font Awesome's "crosshairs"
/// glyph at 14 pt the real height should be noticeably less than the
/// font ascender — that's what makes the per-glyph bbox useful for
/// vertically-centring icons in buttons. If this test stops finding a
/// gap, the centring fix in `Button::paint_icon` will silently regress
/// back to "icon floats to the top".
#[test]
fn glyph_visual_bounds_is_tighter_than_font_metric_for_fa_icon() {
    let fa = Font::from_slice(FA_BYTES).expect("parse fa.ttf");
    let size = 14.0;
    let (y_min, y_max) = fa
        .glyph_visual_bounds('\u{F05B}', size)
        .expect("FA crosshairs glyph must have an outline");
    let glyph_height = y_max - y_min;
    let font_height = fa.ascender_px(size) + fa.descender_px(size);
    assert!(glyph_height > 0.0, "glyph height should be positive");
    assert!(
        glyph_height < font_height,
        "glyph height ({glyph_height}) must be tighter than font ascender+descender ({font_height})"
    );
    // Glyph extent shouldn't exceed the em-size by more than a hair
    // (FA glyphs live inside the design space).
    assert!(
        glyph_height <= size * 1.05,
        "glyph height should fit inside the em-box, got {glyph_height} at size {size}"
    );
}

/// Glyphs absent from the font (or with no outline, like a space) must
/// return `None` so callers can fall back to a font-metric estimate
/// without panicking.
#[test]
fn glyph_visual_bounds_returns_none_for_outlineless_glyph() {
    let font = test_font();
    // ASCII space — has an advance but no outline.
    assert!(font.glyph_visual_bounds(' ', 14.0).is_none());
}

/// Verify that all contour points are in screen-pixel range for the
/// given font size (not left in raw font units).
#[test]
fn test_flatten_output_is_in_screen_space() {
    let font = test_font();
    // Place text at (100, 200) at size 16.
    let contours = shape_and_flatten_text(&font, "Hello", 16.0, 100.0, 200.0, 0.5);

    assert!(!contours.is_empty(), "should produce contours for 'Hello'");

    for (ci, contour) in contours.iter().enumerate() {
        for &[x, y] in contour {
            // Screen-space points should be near (100±50, 200±30) at 16pt.
            // Font-unit coordinates would be in the hundreds–thousands.
            assert!(
                x > 50.0 && x < 300.0,
                "contour {ci}: x={x} looks like font units, not screen px"
            );
            assert!(
                y > 150.0 && y < 280.0,
                "contour {ci}: y={y} looks like font units, not screen px"
            );
        }
    }
}
