//! The global shaped-glyph cache: [`shape_glyphs`], [`with_shaped_glyphs`]
//! and the thread-local `SHAPE_CACHE` behind them.
//!
//! Split out of `text.rs` (which owns `Font`, `ShapedGlyph` and the outline
//! pipeline) so that file stays under the 800-line cap. `measure_advance` in
//! `text.rs` and the paint paths (`shape_text_snapped`, the wgpu outline
//! fill) read the same cache through [`with_shaped_glyphs`], which lends the
//! cached glyphs instead of cloning them. Fonts are identified by
//! [`Font::shape_id`] (`text/font_id.rs`), never by a pointer.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use super::{Font, ShapedGlyph};

// ---------------------------------------------------------------------------
// Global shape/measurement cache — survives across Label instance recreation
// ---------------------------------------------------------------------------
//
// TreeView and other widgets rebuild their Label children every layout() call,
// so a per-Label cache doesn't help: each new instance starts cold. This
// thread-local map caches rustybuzz::shape() results, keyed by (font shape id,
// size bits, tabular flag) and then by text. The shape id covers the face data
// and the fallback chain and is never reused, so neither two fonts sharing data
// with different fallbacks nor a font allocated where a freed one lived can be
// served another font's glyphs.
//
// The two levels let a lookup hash the caller's `&str` directly (a
// `HashMap<Box<str>, _>` is queried with `&str` through `Borrow`), so a hit
// allocates nothing; a flat `(font, String, size, tabular)` key would need an
// owned `String` built for every lookup.
//
// The cache is bounded: past `CAPACITY` entries it drops all but the `KEEP`
// most recently used (see `ShapeCache::evict`).

/// The cache's hasher: foldhash with a fixed seed. Lookups run on every
/// measurement of every frame, and SipHash (std's `RandomState`) made a
/// cache hit about twice as slow (`examples/text_measure_bench.rs`); the
/// cache holds the app's own text, so HashDoS resistance buys nothing here.
type FastHash = foldhash::fast::FixedState;

/// Outer `SHAPE_CACHE` key: font shaping identity, size bits and the
/// tabular-digits flag, which picks different digit glyphs from the same face.
type FaceKey = (u64, u64, bool);

/// Most shaped texts the cache holds before it evicts. A MatterCAD-like
/// column of 200 rows (`examples/text_measure_bench.rs`) keeps 200 entries
/// hot; a whole app's visible UI (menus, property panels, trees) is a few
/// times that. 4096 entries of typical UI strings are a few MB at most.
const CAPACITY: usize = 4096;

/// Entries an eviction keeps: the most recently used half, so the working
/// set survives and the next eviction is `CAPACITY - KEEP` insertions away
/// (eviction is O(n), so its cost per insertion stays constant).
const KEEP: usize = CAPACITY / 2;

/// One cached shaping and the tick it was last used at.
struct Entry {
    glyphs: Vec<ShapedGlyph>,
    /// Set on every hit through a shared borrow, so a hit needs no
    /// `borrow_mut` and stays allocation-free.
    last_used: Cell<u64>,
}

/// Shaped glyphs of every cached text for one [`FaceKey`].
type TextMap = HashMap<Box<str>, Entry, FastHash>;

#[derive(Default)]
struct ShapeCache {
    faces: HashMap<FaceKey, TextMap, FastHash>,
    /// Entries across all of `faces`.
    len: usize,
    /// Use counter: every lookup takes the next tick, so each entry's
    /// `last_used` is distinct and orders entries by recency.
    clock: Cell<u64>,
}

impl ShapeCache {
    fn tick(&self) -> u64 {
        let t = self.clock.get() + 1;
        self.clock.set(t);
        t
    }

    fn insert(&mut self, key: FaceKey, text: &str, glyphs: Vec<ShapedGlyph>) {
        let last_used = Cell::new(self.tick());
        let texts = self.faces.entry(key).or_default();
        if texts
            .insert(text.into(), Entry { glyphs, last_used })
            .is_none()
        {
            self.len += 1;
        }
        if self.len > CAPACITY {
            self.evict();
        }
    }

    /// Drop every entry but the `KEEP` most recently used.
    fn evict(&mut self) {
        let mut stamps: Vec<u64> = self
            .faces
            .values()
            .flat_map(|texts| texts.values().map(|e| e.last_used.get()))
            .collect();
        let Some(drop_count) = stamps.len().checked_sub(KEEP).filter(|&n| n > 0) else {
            return;
        };
        // Stamps are distinct, so the `drop_count`-th smallest is the
        // oldest one kept and exactly `KEEP` entries are at or after it.
        let (_, &mut oldest_kept, _) = stamps.select_nth_unstable(drop_count);
        for texts in self.faces.values_mut() {
            texts.retain(|_, e| e.last_used.get() >= oldest_kept);
        }
        self.faces.retain(|_, texts| !texts.is_empty());
        self.len = KEEP;
    }
}

thread_local! {
    /// Caches the full rustybuzz shaping output (per-glyph IDs + advances).
    /// Paint (`shape_text_snapped`, the wgpu outline fill) reads it so
    /// fill_text() avoids re-shaping every frame, and measure_advance()
    /// reads it as the measurement cache.
    static SHAPE_CACHE: RefCell<ShapeCache> = RefCell::new(ShapeCache::default());
}

/// How many shaped texts the calling thread's cache holds (diagnostics; it
/// never exceeds an internal bound of a few thousand).
pub fn shape_cache_len() -> usize {
    SHAPE_CACHE.with(|c| c.borrow().len)
}

/// Shape `text` and return per-glyph positioning info, with **no** outline
/// extraction or tessellation.
///
/// Results are cached in a bounded thread-local map keyed by
/// `(font.shape_id(), size_bits, tabular)` and then by text.  Prefer
/// [`with_shaped_glyphs`], which borrows the cached glyphs instead of
/// cloning them.
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
    let face_key = (font.shape_id(), size.to_bits(), font.tabular_digits);
    SHAPE_CACHE.with(|cell| {
        {
            let cache = cell.borrow();
            if let Some(entry) = cache.faces.get(&face_key).and_then(|t| t.get(text)) {
                entry.last_used.set(cache.tick());
                return f(&entry.glyphs);
            }
        }
        let glyphs = shape_uncached(font, text, size);
        let r = f(&glyphs);
        cell.borrow_mut().insert(face_key, text, glyphs);
        r
    })
}

#[cfg(test)]
pub(super) fn is_cached(font: &Font, text: &str, size: f64) -> bool {
    let face_key = (font.shape_id(), size.to_bits(), font.tabular_digits);
    SHAPE_CACHE.with(|c| {
        c.borrow()
            .faces
            .get(&face_key)
            .is_some_and(|t| t.contains_key(text))
    })
}

#[cfg(test)]
pub(super) const TEST_CAPACITY: usize = CAPACITY;
#[cfg(test)]
pub(super) const TEST_KEEP: usize = KEEP;

/// Shape `text` with rustybuzz, resolving missing glyphs through the font's
/// fallback chain. The uncached half of [`with_shaped_glyphs`].
pub(super) fn shape_uncached(font: &Font, text: &str, size: f64) -> Vec<ShapedGlyph> {
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
