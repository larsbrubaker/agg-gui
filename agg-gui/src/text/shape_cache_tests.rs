//! Unit tests for the shape cache (`shape_cache.rs`): fonts are told apart by
//! their ids (`font_id.rs`), never by a pointer, and the cache stays bounded
//! while keeping the entries in use.

use std::collections::HashSet;
use std::sync::Arc;

use super::shape_cache::{is_cached, shape_uncached, TEST_CAPACITY, TEST_KEEP};
use super::*;

const CASCADIA: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const FA: &[u8] = include_bytes!("../../../demo/assets/fa.ttf");
const NOTO: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");

/// Font Awesome's laptop icon: not in Cascadia Code, only in its fallback.
const FA_LAPTOP: &str = "\u{F109}";

/// Two fonts on the very same data `Arc` but with different fallback chains
/// must not share shaped glyphs: the one without a fallback shapes the icon
/// to `.notdef`, the one with Font Awesome behind it to the real glyph, in
/// whichever order the cache sees them.
#[test]
fn fonts_sharing_data_with_different_fallbacks_shape_apart() {
    let base = Font::from_slice(CASCADIA).expect("cascadia");
    let plain = base
        .variant_with_tabular_digits()
        .with_tabular_digits(false);
    let fa = Arc::new(Font::from_slice(FA).expect("fa"));
    let with_fa = base
        .variant_with_tabular_digits()
        .with_tabular_digits(false)
        .with_fallback(fa);
    assert!(Arc::ptr_eq(&plain.data, &with_fa.data), "same data Arc");
    assert_ne!(plain.shape_id(), with_fa.shape_id());

    let without = shape_glyphs(&plain, FA_LAPTOP, 16.0);
    assert_eq!(without.len(), 1);
    assert!(without[0].fallback_font.is_none());
    assert_eq!(without[0].glyph_id, 0, "Cascadia has no laptop icon");

    let through_fallback = shape_glyphs(&with_fa, FA_LAPTOP, 16.0);
    assert_eq!(through_fallback.len(), 1);
    assert!(
        through_fallback[0].fallback_font.is_some(),
        "the fallback chain must resolve the icon, not the plain font's entry"
    );
    assert_ne!(through_fallback[0].glyph_id, 0);

    // And the other way round: a cached fallback shaping never leaks back.
    assert!(shape_glyphs(&plain, FA_LAPTOP, 16.0)[0]
        .fallback_font
        .is_none());
}

/// Variants that only restyle a face share its shaping identity (and so its
/// cache entries); everything else gets an id no font had before.
#[test]
fn font_ids_are_fresh_and_variants_keep_the_shape_id() {
    let base = Font::from_slice(NOTO).expect("noto");
    let italic = base.variant_with_faux_style(true, false);
    let tabular = base.variant_with_tabular_digits();
    assert_eq!(italic.shape_id(), base.shape_id());
    assert_eq!(tabular.shape_id(), base.shape_id());
    let ids: HashSet<u64> = [base.id(), italic.id(), tabular.id()].into();
    assert_eq!(ids.len(), 3, "every font value has its own id");
    assert!(!ids.contains(&0), "0 is reserved for 'no font yet'");
}

/// A font created after another was dropped never sees the dropped font's
/// shapes, even when the allocator hands it the same data address. Rather
/// than relying on the allocator reusing an address, this checks the
/// property the cache keys on: every font ever created has a distinct shape
/// id, whatever address its data lands at, and the cache answers each font
/// with its own shaping.
#[test]
fn a_new_font_never_serves_a_dropped_fonts_shapes() {
    let mut seen = HashSet::new();
    for i in 0..32 {
        // Alternate faces so a stale entry would show as different glyphs.
        let bytes = if i % 2 == 0 { CASCADIA } else { NOTO };
        let font = Font::from_slice(bytes).expect("font");
        assert!(seen.insert(font.shape_id()), "shape id reused");
        let fresh = shape_uncached(&font, "Hello 0123", 14.0);
        let cached = shape_glyphs(&font, "Hello 0123", 14.0);
        assert_eq!(cached.len(), fresh.len());
        for (c, f) in cached.iter().zip(&fresh) {
            assert_eq!(c.glyph_id, f.glyph_id);
            assert_eq!(c.x_advance.to_bits(), f.x_advance.to_bits());
        }
        // `font` (and its data) is dropped here; the next one may reuse it.
    }
}

/// Shaping many distinct strings never grows the cache past its bound, and
/// strings used every "frame" in between survive every eviction.
#[test]
fn cache_stays_bounded_and_keeps_hot_entries() {
    let font = Font::from_slice(NOTO).expect("noto");
    let hot = ["File", "Edit", "View", "Width 15.00 mm"];
    let cold_first = "distinct 0";
    for i in 0..TEST_CAPACITY * 2 {
        if i % 256 == 0 {
            for h in hot {
                measure_advance(&font, h, 14.0);
            }
        }
        measure_advance(&font, &format!("distinct {i}"), 14.0);
        assert!(shape_cache_len() <= TEST_CAPACITY);
    }
    for h in hot {
        assert!(is_cached(&font, h, 14.0), "hot entry {h:?} was evicted");
    }
    assert!(!is_cached(&font, cold_first, 14.0), "cold entry kept");
    assert!(shape_cache_len() >= TEST_KEEP);
}
