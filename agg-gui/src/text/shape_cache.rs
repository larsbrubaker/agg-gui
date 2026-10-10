//! The global shaped-glyph cache: [`shape_glyphs`] and the thread-local
//! `SHAPE_CACHE` behind it.
//!
//! Split out of `text.rs` (which owns `Font`, `ShapedGlyph` and the outline
//! pipeline) so that file stays under the 800-line cap. `measure_advance` in
//! `text.rs` reads the same cache through [`shape_glyphs`].

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
// thread-local HashMap caches rustybuzz::shape() results for the lifetime of
// the process, keyed by (font data pointer, text, size bits). The pointer is
// stable as long as any Arc<Vec<u8>> clone exists (which is always true while
// the Font is alive).

/// `SHAPE_CACHE` key: font identity, text, size bits and the tabular-digits flag.
type ShapeKey = (usize, String, u64, bool);

thread_local! {
    /// Caches the full rustybuzz shaping output (per-glyph IDs + advances).
    /// Used by shape_glyphs() so fill_text() avoids re-shaping every frame.
    /// Also serves as the measurement cache — measure_advance() reads it too.
    /// The trailing flag is the font's tabular-digits setting, which picks
    /// different digit glyphs from the same face.
    static SHAPE_CACHE: RefCell<HashMap<ShapeKey, Vec<ShapedGlyph>>> =
        RefCell::new(HashMap::new());
}

/// Shape `text` and return per-glyph positioning info, with **no** outline
/// extraction or tessellation.
///
/// Results are cached in a thread-local `HashMap` keyed by
/// `(font_data_ptr, text, size_bits)`.  The GL `fill_text()` path calls this
/// on every paint; caching it eliminates the per-frame `rustybuzz::shape()`
/// cost for static labels and sidebar items.
///
/// Use the result together with [`flatten_glyph_at_origin`] and a
/// [`GlyphCache`] to avoid re-tessellating glyphs every frame.
pub fn shape_glyphs(font: &Font, text: &str, size: f64) -> Vec<ShapedGlyph> {
    let font_key = Arc::as_ptr(&font.data) as usize;
    let size_key = size.to_bits();
    let tabular = font.tabular_digits;

    SHAPE_CACHE.with(|cache| {
        {
            let c = cache.borrow();
            if let Some(cached) = c.get(&(font_key, text.to_owned(), size_key, tabular)) {
                return cached.clone();
            }
        }

        // Cache miss — shape the text.
        let scale = size / font.units_per_em() as f64;
        let glyphs = font.with_rb_face(|face| {
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
                                let fb_id = fb
                                    .with_ttf_face(|f| f.glyph_index(ch).map(|g| g.0).unwrap_or(0));
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
        });

        cache.borrow_mut().insert(
            (font_key, text.to_owned(), size_key, tabular),
            glyphs.clone(),
        );
        glyphs
    })
}
