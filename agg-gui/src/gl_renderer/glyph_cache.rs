//! Glyph vertex cache for the GL rendering path.
//!
//! # Problem
//!
//! `fill_text` called `shape_and_flatten_text_via_agg` every frame, running
//! rustybuzz shaping + AGG ConvCurve Bézier flattening + tess2 tessellation
//! for every visible text string.  On a frame with ~20 text strings this
//! dominated render time (~60 ms/frame).
//!
//! # Solution
//!
//! [`GlyphCache`] tessellates each (font, glyph_id, size) triple once and
//! stores the resulting triangle mesh in **glyph-local pixel coordinates**
//! (origin 0, 0; scaled by `size / units_per_em`).  On subsequent frames the
//! caller offsets those vertices by the glyph's screen position and uploads
//! directly to the GPU — no Bézier evaluation or tessellation.
//!
//! # Key design choices
//!
//! * **One entry per (font_ptr, glyph_id, size_bits)** — different sizes
//!   produce different tessellations; identical sizes share a single entry.
//! * **Glyph-local coordinates** — the CTM is applied at draw time
//!   (`transform_pt(pen_x + vx, y + vy)`), which is correct for any affine
//!   transform including rotation.
//! * **`None` entries are cached too** — so glyphs without outlines (space,
//!   tab) never re-enter the shaper.
//! * The cache is **never cleared between frames** (`reset()` must NOT call
//!   `glyph_cache.clear()`); it grows until the widget tree changes fonts or
//!   sizes, then entries for the old parameters simply become dead weight
//!   (acceptable for typical UI workloads).

use std::collections::HashMap;
use std::sync::Arc;

use crate::gl_renderer::tessellate_fill;
use crate::text::{flatten_glyph_at_origin, Font};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Pre-tessellated triangle mesh for one glyph at a specific pixel size.
///
/// All coordinates are in **glyph-local pixels** (origin 0, 0).
/// To place on screen: for each `[vx, vy]` in `verts` compute
/// `transform_pt(pen_x + vx as f64, baseline_y + vy as f64)`.
pub struct CachedGlyph {
    /// Flattened vertex list — each element is one screen-space `[x, y]`.
    pub verts: Vec<[f32; 2]>,
    /// Triangle index list — every three consecutive values index a triangle
    /// into `verts`.
    pub indices: Vec<u32>,
}

/// Per-frame glyph vertex cache shared by one [`GlGfxCtx`] instance.
///
/// Create once alongside `GlGfxCtx::new` and keep alive for the lifetime of
/// the rendering context.  Do **not** clear between frames.
pub struct GlyphCache {
    /// `None` entries represent glyphs with no visible outline (spaces, tabs).
    entries: HashMap<GlyphKey, Option<CachedGlyph>>,
}

impl GlyphCache {
    /// Create an empty cache.
    pub fn new() -> Self {
        GlyphCache {
            entries: HashMap::new(),
        }
    }

    /// Return the cached tessellation for `(font, glyph_id, size)`, tessellating
    /// on first access.
    ///
    /// Returns `None` for glyphs with no visible outline (space, tab, etc.).
    pub fn get_or_insert(&mut self, font: &Font, glyph_id: u16, size: f64) -> Option<&CachedGlyph> {
        let key = GlyphKey {
            font_ptr: Arc::as_ptr(&font.data) as usize,
            glyph_id,
            size_bits: size.to_bits(),
        };
        self.entries
            .entry(key)
            .or_insert_with(|| tessellate_glyph(font, glyph_id, size))
            .as_ref()
    }

    /// Number of entries (including None entries for non-outline glyphs).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no glyphs have been cached yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Discard all cached tessellations.  Only needed when the GL context is
    /// recreated (e.g., window resize on some platforms that destroy the
    /// surface).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

#[derive(Hash, Eq, PartialEq, Clone)]
struct GlyphKey {
    /// Pointer identity of the font's backing data (`Arc<Vec<u8>>`).
    font_ptr: usize,
    glyph_id: u16,
    /// Exact bit pattern of the `f64` size — guarantees identical sizes share
    /// a single entry even under floating-point representation.
    size_bits: u64,
}

/// Tessellate one glyph's outline at origin (0, 0) and return the mesh,
/// or `None` if the glyph has no outline.
fn tessellate_glyph(font: &Font, glyph_id: u16, size: f64) -> Option<CachedGlyph> {
    let contours = flatten_glyph_at_origin(font, glyph_id, size)?;

    let has_ccw = contours.iter().any(|c| contour_is_ccw(c));

    let (verts_flat, indices) = if has_ccw {
        // Mixed winding (e.g. 'O', 'D', 'B'): tessellate all contours together
        // so EvenOdd punches counter holes correctly.
        tessellate_fill(&contours)?
    } else {
        // All-CW strokes (e.g. 'T', 'E', 'N'): tessellate each contour
        // independently to avoid spurious EvenOdd holes at stroke overlaps.
        let mut all_vf: Vec<f32> = Vec::new();
        let mut all_idx: Vec<u32> = Vec::new();
        for contour in &contours {
            if let Some((vf, idx)) = tessellate_fill(&[contour.clone()]) {
                let base = (all_vf.len() / 2) as u32;
                all_vf.extend_from_slice(&vf);
                all_idx.extend(idx.iter().map(|&i| i + base));
            }
        }
        if all_vf.is_empty() {
            return None;
        }
        (all_vf, all_idx)
    };

    let verts: Vec<[f32; 2]> = verts_flat.chunks_exact(2).map(|c| [c[0], c[1]]).collect();

    Some(CachedGlyph { verts, indices })
}

/// Returns `true` if the contour winds counter-clockwise in Y-up space
/// (signed area > 0).  Inner counter contours (holes in O, D, B, R …)
/// wind opposite to the outer boundary.
fn contour_is_ccw(pts: &[[f32; 2]]) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut area = 0.0f32;
    for i in 0..n {
        let j = (i + 1) % n;
        area += pts[i][0] * pts[j][1] - pts[j][0] * pts[i][1];
    }
    area > 0.0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Font;
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

    fn test_font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font ok"))
    }

    /// First access populates the cache; second access reuses the same entry.
    #[test]
    fn test_cache_hit_on_second_access() {
        use crate::text::shape_glyphs;
        let font = test_font();
        let mut cache = GlyphCache::new();

        let glyphs = shape_glyphs(&font, "H", 14.0);
        let gid = glyphs[0].glyph_id;

        assert!(cache.is_empty(), "cache starts empty");

        let first = cache.get_or_insert(&font, gid, 14.0);
        assert!(first.is_some(), "'H' should have an outline");
        assert_eq!(cache.len(), 1, "one entry after first access");

        let second = cache.get_or_insert(&font, gid, 14.0);
        assert!(second.is_some());
        assert_eq!(cache.len(), 1, "cache still has one entry — no duplicate");
    }

    /// Space has no outline; the cache stores a None entry and returns None.
    #[test]
    fn test_cache_none_for_space() {
        use crate::text::shape_glyphs;
        let font = test_font();
        let mut cache = GlyphCache::new();

        let glyphs = shape_glyphs(&font, " ", 14.0);
        let gid = glyphs[0].glyph_id;

        let result = cache.get_or_insert(&font, gid, 14.0);
        assert!(result.is_none(), "space glyph has no outline");
        assert_eq!(
            cache.len(),
            1,
            "None is cached to avoid re-entering the shaper"
        );
    }

    /// Different sizes must produce separate cache entries.
    #[test]
    fn test_different_sizes_are_separate_entries() {
        use crate::text::shape_glyphs;
        let font = test_font();
        let mut cache = GlyphCache::new();

        let gid = shape_glyphs(&font, "H", 14.0)[0].glyph_id;

        cache.get_or_insert(&font, gid, 14.0);
        cache.get_or_insert(&font, gid, 16.0);
        assert_eq!(cache.len(), 2, "14px and 16px are separate entries");
    }

    /// Cached verts must be in glyph-local pixel range, not font units.
    #[test]
    fn test_cached_verts_are_in_pixel_range() {
        use crate::text::shape_glyphs;
        let font = test_font();
        let mut cache = GlyphCache::new();
        let size = 14.0_f64;

        let gid = shape_glyphs(&font, "H", size)[0].glyph_id;
        let cached = cache
            .get_or_insert(&font, gid, size)
            .expect("H has outline");

        for &[x, y] in &cached.verts {
            assert!(
                x >= -2.0 && x <= 20.0,
                "x={x} must be in glyph-local pixels, not font units"
            );
            assert!(
                y >= -4.0 && y <= 18.0,
                "y={y} must be in glyph-local pixels, not font units"
            );
        }
    }
}
