//! The global shaped-glyph cache: [`shape_glyphs`], [`with_shaped_glyphs`]
//! and the thread-local `SHAPE_CACHE` behind them.
//!
//! Split out of `text.rs` (which owns `Font`, `ShapedGlyph` and the outline
//! pipeline) so that file stays under the 800-line cap. `measure_advance` in
//! `text.rs` reads the same cache through [`with_shaped_glyphs`], which lends
//! the cached glyphs instead of cloning them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use super::{Font, ShapedGlyph};

// ---------------------------------------------------------------------------
// Global shape/measurement cache — survives across Label instance recreation
// ---------------------------------------------------------------------------
//
// TreeView and other widgets rebuild their Label children every layout() call,
// so a per-Label cache doesn't help: each new instance starts cold. This
// thread-local map caches rustybuzz::shape() results for the lifetime of
// the process, keyed by (font data pointer, size bits, tabular flag) and then
// by text. The pointer is stable as long as any Arc<Vec<u8>> clone exists
// (which is always true while the Font is alive).
//
// The two levels let a lookup hash the caller's `&str` directly (a
// `HashMap<Box<str>, _>` is queried with `&str` through `Borrow`), so a hit
// allocates nothing; a flat `(font, String, size, tabular)` key would need an
// owned `String` built for every lookup.

/// The cache's hasher: foldhash with a fixed seed. Lookups run on every
/// measurement of every frame, and SipHash (std's `RandomState`) made a
/// cache hit about twice as slow (`examples/text_measure_bench.rs`); the
/// cache holds the app's own text, so HashDoS resistance buys nothing here.
type FastHash = foldhash::fast::FixedState;

/// Outer `SHAPE_CACHE` key: font identity, size bits and the tabular-digits
/// flag, which picks different digit glyphs from the same face.
type FaceKey = (usize, u64, bool);

/// Shaped glyphs of every cached text for one [`FaceKey`].
type TextMap = HashMap<Box<str>, Vec<ShapedGlyph>, FastHash>;

thread_local! {
    /// Caches the full rustybuzz shaping output (per-glyph IDs + advances).
    /// Used by shape_glyphs() so fill_text() avoids re-shaping every frame.
    /// Also serves as the measurement cache — measure_advance() reads it too.
    static SHAPE_CACHE: RefCell<HashMap<FaceKey, TextMap, FastHash>> =
        RefCell::new(HashMap::default());
}

/// Shape `text` and return per-glyph positioning info, with **no** outline
/// extraction or tessellation.
///
/// Results are cached in a thread-local map keyed by
/// `(font_data_ptr, size_bits, tabular)` and then by text.  The GL
/// `fill_text()` path calls this on every paint; caching it eliminates the
/// per-frame `rustybuzz::shape()` cost for static labels and sidebar items.
///
/// Use the result together with [`flatten_glyph_at_origin`] and a
/// [`GlyphCache`] to avoid re-tessellating glyphs every frame.
pub fn shape_glyphs(font: &Font, text: &str, size: f64) -> Vec<ShapedGlyph> {
    with_shaped_glyphs(font, text, size, <[ShapedGlyph]>::to_vec)
}

/// Lend the shaped glyphs of `text` (see [`shape_glyphs`]) to `f` without
/// cloning them: on a cache hit nothing is allocated.
///
/// `f` runs while the cache is borrowed, so it must not shape text itself
/// (that would panic on the nested `RefCell` borrow); copy out what it needs.
pub fn with_shaped_glyphs<R>(
    font: &Font,
    text: &str,
    size: f64,
    f: impl FnOnce(&[ShapedGlyph]) -> R,
) -> R {
    let face_key = (
        Arc::as_ptr(&font.data) as usize,
        size.to_bits(),
        font.tabular_digits,
    );
    SHAPE_CACHE.with(|cache| {
        if let Some(glyphs) = cache
            .borrow()
            .get(&face_key)
            .and_then(|texts| texts.get(text))
        {
            return f(glyphs);
        }
        let glyphs = shape_uncached(font, text, size);
        let r = f(&glyphs);
        cache
            .borrow_mut()
            .entry(face_key)
            .or_default()
            .insert(text.into(), glyphs);
        r
    })
}

/// Shape `text` with rustybuzz, resolving missing glyphs through the font's
/// fallback chain. The uncached half of [`with_shaped_glyphs`].
fn shape_uncached(font: &Font, text: &str, size: f64) -> Vec<ShapedGlyph> {
    let tabular = font.tabular_digits;
    let scale = size / font.units_per_em() as f64;
    font.with_rb_face(|face| {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        // Tabular figures are a real GSUB substitution (Inter and the
        // other UI faces swap in `.tnum` digit glyphs), so the feature
        // has to reach the shaper — it cannot be faked with advances.
        let features: &[rustybuzz::Feature] = if tabular {
            &[rustybuzz::Feature::new(
                ttf_parser::Tag::from_bytes(b"tnum"),
                1,
                ..,
            )]
        } else {
            &[]
        };
        let output = rustybuzz::shape(face, features, buffer);
        output
            .glyph_infos()
            .iter()
            .zip(output.glyph_positions().iter())
            .map(|(info, pos)| {
                let glyph_id = info.glyph_id as u16;
                let x_advance = pos.x_advance as f64 * scale;
                let x_offset = pos.x_offset as f64 * scale;
                let y_offset = pos.y_offset as f64 * scale;

                // glyph_id == 0 means the primary font has no glyph for
                // this code point.  Walk the fallback chain until a font
                // with a matching glyph is found.
                if glyph_id == 0 {
                    let byte_off = info.cluster as usize;
                    if let Some(ch) = text.get(byte_off..).and_then(|s| s.chars().next()) {
                        let mut cur_fb = font.fallback.as_ref();
                        while let Some(fb) = cur_fb {
                            let fb_id =
                                fb.with_ttf_face(|f| f.glyph_index(ch).map(|g| g.0).unwrap_or(0));
                            if fb_id != 0 {
                                let fb_scale = size / fb.units_per_em() as f64;
                                let fb_adv = fb.with_ttf_face(|f| {
                                    f.glyph_hor_advance(ttf_parser::GlyphId(fb_id))
                                        .map(|a| a as f64 * fb_scale)
                                        .unwrap_or(0.0)
                                });
                                return ShapedGlyph {
                                    glyph_id: fb_id,
                                    x_advance: fb_adv,
                                    x_offset,
                                    y_offset,
                                    fallback_font: Some(Arc::clone(fb)),
                                };
                            }
                            cur_fb = fb.fallback.as_ref();
                        }
                    }
                }

                ShapedGlyph {
                    glyph_id,
                    x_advance,
                    x_offset,
                    y_offset,
                    fallback_font: None,
                }
            })
            .collect::<Vec<_>>()
    })
}
