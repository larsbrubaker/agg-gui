//! Text rendering — font loading, shaping, and glyph rasterization.
//!
//! # Pipeline
//!
//! ```text
//! Font bytes (TTF/OTF)
//!   │  ttf-parser  →  glyph outline curves
//!   │  rustybuzz   →  shaped glyph positions & advances
//!   │
//! GlyphPathBuilder  →  AGG PathStorage (Bézier curves)
//!   │
//! rasterize_fill_path  →  Framebuffer pixels
//! ```
//!
//! # Coordinate system
//!
//! TrueType fonts use Y-up coordinates (positive Y = above baseline).
//! This matches GfxCtx's first-quadrant convention exactly — no Y-flip
//! is needed at the glyph boundary.
//!
//! The baseline is placed at the Y coordinate passed to `GfxCtx::fill_text`.
//! Ascenders go to higher Y values (up), descenders to lower Y values (down),
//! which is correct for Y-up rendering.

mod bezier_flat;
pub use bezier_flat::{shape_and_flatten_text, shape_and_flatten_text_via_agg};

use std::sync::Arc;

use agg_rust::basics::{is_end_poly, is_move_to, is_stop, PATH_CMD_LINE_TO, PATH_FLAGS_NONE, VertexSource};
use agg_rust::conv_curve::ConvCurve;
use agg_rust::path_storage::PathStorage;

/// Metrics describing a single line of shaped text.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextMetrics {
    /// Advance width of the text run in pixels.
    pub width: f64,
    /// Distance from baseline to top of tallest ascender, in pixels (positive).
    pub ascent: f64,
    /// Distance from baseline to bottom of deepest descender, in pixels (positive).
    pub descent: f64,
    /// Recommended line height (ascender + descender + line gap), in pixels.
    pub line_height: f64,
}

/// A loaded font, ready for shaping and rasterization.
///
/// Constructed from raw TTF/OTF bytes via [`Font::from_bytes`]. The data is
/// reference-counted so fonts can be cheaply shared and saved across frames.
///
/// An optional fallback font can be chained via [`Font::with_fallback`]; when
/// a glyph is missing from the primary font (glyph_id == 0 after shaping),
/// the fallback is consulted for both the glyph outline and advance width.
pub struct Font {
    pub(crate) data: Arc<Vec<u8>>,
    index: u32,
    /// Cached at construction to avoid repeated parsing.
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    line_gap: i16,
    /// Optional fallback used when the primary font lacks a glyph.
    pub(crate) fallback: Option<Arc<Font>>,
}

impl Font {
    /// Parse a font from raw TTF/OTF bytes.
    ///
    /// Returns `Err` if the data is not a valid font.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, &'static str> {
        let face = ttf_parser::Face::parse(&data, 0).map_err(|_| "failed to parse font")?;
        Ok(Self {
            units_per_em: face.units_per_em(),
            ascender: face.ascender(),
            descender: face.descender(),
            line_gap: face.line_gap(),
            data: Arc::new(data),
            index: 0,
            fallback: None,
        })
    }

    /// Parse a font from a borrowed byte slice (data is copied).
    pub fn from_slice(data: &[u8]) -> Result<Self, &'static str> {
        Self::from_bytes(data.to_vec())
    }

    /// Chain a fallback font consulted when this font lacks a glyph.
    ///
    /// Returns `self` so it can be used as a builder method:
    /// ```ignore
    /// let font = Font::from_slice(MAIN_BYTES)?.with_fallback(Arc::new(emoji_font));
    /// ```
    pub fn with_fallback(mut self, fallback: Arc<Font>) -> Self {
        self.fallback = Some(fallback);
        self
    }

    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// Ascender height in pixels at the given font size.
    pub fn ascender_px(&self, size: f64) -> f64 {
        self.ascender as f64 * size / self.units_per_em as f64
    }

    /// Descender depth in pixels at the given font size (positive value).
    pub fn descender_px(&self, size: f64) -> f64 {
        self.descender.unsigned_abs() as f64 * size / self.units_per_em as f64
    }

    /// Recommended line height in pixels at the given font size.
    pub fn line_height_px(&self, size: f64) -> f64 {
        let total = (self.ascender - self.descender + self.line_gap) as f64;
        total * size / self.units_per_em as f64
    }

    /// Run `f` with a `rustybuzz::Face` borrowed from the internal data.
    ///
    /// The face has the same lifetime as the closure invocation, so it cannot
    /// outlive this call. Use this for shaping + outline extraction.
    pub(crate) fn with_rb_face<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&rustybuzz::Face<'_>) -> R,
    {
        let face = rustybuzz::Face::from_slice(&self.data, self.index)
            .expect("font was validated at construction");
        f(&face)
    }

    /// Run `f` with a `ttf_parser::Face` borrowed from the internal data.
    ///
    /// Used for glyph index lookups (fallback resolution) without full shaping.
    pub(crate) fn with_ttf_face<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ttf_parser::Face<'_>) -> R,
    {
        let face = ttf_parser::Face::parse(&self.data, self.index)
            .expect("font was validated at construction");
        f(&face)
    }
}

// ---------------------------------------------------------------------------
// Glyph outline → AGG PathStorage
// ---------------------------------------------------------------------------

/// Converts ttf-parser outline callbacks into an AGG `PathStorage`.
///
/// TTF fonts are Y-up; GfxCtx is Y-up — no axis flip is needed. Each glyph
/// is translated to its screen position `(ox, oy)` and scaled by `scale`.
pub(crate) struct GlyphPathBuilder {
    pub path: PathStorage,
    ox: f64,
    oy: f64,
    scale: f64,
    pub has_outline: bool,
}

impl GlyphPathBuilder {
    pub fn new(ox: f64, oy: f64, scale: f64) -> Self {
        Self {
            path: PathStorage::new(),
            ox,
            oy,
            scale,
            has_outline: false,
        }
    }

    #[inline]
    fn x(&self, v: f32) -> f64 { self.ox + v as f64 * self.scale }
    #[inline]
    fn y(&self, v: f32) -> f64 { self.oy + v as f64 * self.scale }
}

impl ttf_parser::OutlineBuilder for GlyphPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.x(x), self.y(y));
        self.has_outline = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.x(x), self.y(y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.curve3(self.x(x1), self.y(y1), self.x(x), self.y(y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path.curve4(
            self.x(x1), self.y(y1),
            self.x(x2), self.y(y2),
            self.x(x),  self.y(y),
        );
    }
    fn close(&mut self) {
        self.path.close_polygon(PATH_FLAGS_NONE);
    }
}

// ---------------------------------------------------------------------------
// Shaping helper — shapes text and returns per-glyph paths
// ---------------------------------------------------------------------------

/// Shape `text` with `font` at `size` pixels, starting at screen position
/// `(x, y)` (baseline-left, Y-up). Returns one `PathStorage` per glyph that
/// has an outline (spaces and control chars yield no path).
///
/// Walks the fallback font chain via [`shape_glyphs`], so Font Awesome /
/// emoji glyphs not present in the primary font are still resolved and
/// rasterized using the font they live in.
pub(crate) fn shape_text(
    font: &Font,
    text: &str,
    size: f64,
    x: f64,
    y: f64,
) -> (Vec<PathStorage>, f64) {
    let shaped = shape_glyphs(font, text, size);

    let mut paths = Vec::new();
    let mut pen_x = x;
    let mut total_advance = 0.0;

    for g in &shaped {
        let gx = pen_x + g.x_offset;
        let gy = y + g.y_offset;
        // glyph_id indexes into whichever font resolved the code point.
        let render_font = g.fallback_font.as_deref().unwrap_or(font);
        let scale = size / render_font.units_per_em() as f64;

        let mut builder = GlyphPathBuilder::new(gx, gy, scale);
        let has_outline = render_font.with_ttf_face(|face| {
            face.outline_glyph(ttf_parser::GlyphId(g.glyph_id), &mut builder)
                .is_some()
        });
        if has_outline && builder.has_outline {
            paths.push(builder.path);
        }

        pen_x += g.x_advance;
        total_advance += g.x_advance;
    }
    (paths, total_advance)
}

// ---------------------------------------------------------------------------
// Glyph cache support — shaped glyph info + single-glyph outline extraction
// ---------------------------------------------------------------------------

/// Position and identity of one shaped glyph, without any rendering.
///
/// Returned by [`shape_glyphs`].  All distances are in **pixels** at the
/// requested font size.
///
/// When `fallback_font` is `Some`, the glyph was resolved from the fallback
/// font rather than the primary.  Callers must use that font for outline
/// extraction and glyph cache lookups, since `glyph_id` is an index into
/// the fallback's glyph table, not the primary's.
#[derive(Clone)]
pub struct ShapedGlyph {
    /// Index into the font's glyph table (or fallback's if `fallback_font` is Some).
    pub glyph_id: u16,
    /// How far to advance the pen after this glyph.
    pub x_advance: f64,
    /// Horizontal offset from the pen position to this glyph's origin.
    pub x_offset: f64,
    /// Vertical offset from the baseline to this glyph's origin.
    pub y_offset: f64,
    /// Set when this glyph was resolved via the fallback font.
    /// Use this font instead of the primary for cache lookups and rendering.
    pub fallback_font: Option<Arc<Font>>,
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

    SHAPE_CACHE.with(|cache| {
        {
            let c = cache.borrow();
            if let Some(cached) = c.get(&(font_key, text.to_owned(), size_key)) {
                return cached.clone();
            }
        }

        // Cache miss — shape the text.
        let scale = size / font.units_per_em() as f64;
        let glyphs = font.with_rb_face(|face| {
            let mut buffer = rustybuzz::UnicodeBuffer::new();
            buffer.push_str(text);
            let output = rustybuzz::shape(face, &[], buffer);
            output
                .glyph_infos()
                .iter()
                .zip(output.glyph_positions().iter())
                .map(|(info, pos)| {
                    let glyph_id  = info.glyph_id as u16;
                    let x_advance = pos.x_advance as f64 * scale;
                    let x_offset  = pos.x_offset  as f64 * scale;
                    let y_offset  = pos.y_offset  as f64 * scale;

                    // glyph_id == 0 means the primary font has no glyph for
                    // this code point.  Walk the fallback chain until a font
                    // with a matching glyph is found.
                    if glyph_id == 0 {
                        let byte_off = info.cluster as usize;
                        if let Some(ch) = text.get(byte_off..).and_then(|s| s.chars().next()) {
                            let mut cur_fb = font.fallback.as_ref();
                            while let Some(fb) = cur_fb {
                                let fb_id = fb.with_ttf_face(|f| {
                                    f.glyph_index(ch).map(|g| g.0).unwrap_or(0)
                                });
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

                    ShapedGlyph { glyph_id, x_advance, x_offset, y_offset,
                                  fallback_font: None }
                })
                .collect::<Vec<_>>()
        });

        cache.borrow_mut().insert((font_key, text.to_owned(), size_key), glyphs.clone());
        glyphs
    })
}

/// Flatten a single glyph's outline using AGG `ConvCurve`, with the glyph
/// origin at **(0, 0)** in pixel space.
///
/// Returns one `Vec<[f32;2]>` per closed contour, ready to pass to
/// `tessellate_fill`.  Returns `None` for glyphs without an outline (space,
/// tab, or glyph IDs that reference nothing).
///
/// The vertices are in **glyph-local pixels**: the glyph baseline is y=0 and
/// the leftmost bearing is x=0 (approximately).  To place the glyph on screen
/// at `(gx, gy)`, translate every vertex by that amount before tessellating or
/// uploading to the GPU.
pub fn flatten_glyph_at_origin(font: &Font, glyph_id: u16, size: f64)
    -> Option<Vec<Vec<[f32; 2]>>>
{
    let scale = size / font.units_per_em() as f64;
    font.with_rb_face(|face| {
        let gid = ttf_parser::GlyphId(glyph_id);
        let mut builder = GlyphPathBuilder::new(0.0, 0.0, scale);
        let has_outline = face.outline_glyph(gid, &mut builder).is_some();
        if !has_outline || !builder.has_outline {
            return None;
        }

        let mut curves = ConvCurve::new(builder.path);
        curves.rewind(0);

        let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut current: Vec<[f32; 2]>       = Vec::new();

        loop {
            let (mut cx, mut cy) = (0.0_f64, 0.0_f64);
            let cmd = curves.vertex(&mut cx, &mut cy);
            if is_stop(cmd) { break; }
            if is_move_to(cmd) {
                if current.len() >= 3 {
                    contours.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                current.push([cx as f32, cy as f32]);
            } else if cmd == PATH_CMD_LINE_TO {
                current.push([cx as f32, cy as f32]);
            } else if is_end_poly(cmd) {
                if current.len() >= 3 {
                    contours.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
            }
        }
        if current.len() >= 3 {
            contours.push(current);
        }

        if contours.is_empty() { None } else { Some(contours) }
    })
}

/// Measure full text metrics (width, ascent, descent, line_height).
///
/// Useful for external rendering backends (e.g. `GlGfxCtx`) that need
/// text metrics without the `GfxCtx` wrapper.
pub fn measure_text_metrics(font: &Font, text: &str, size: f64) -> TextMetrics {
    TextMetrics {
        width:       measure_advance(font, text, size),
        ascent:      font.ascender_px(size),
        descent:     font.descender_px(size),
        line_height: font.line_height_px(size),
    }
}

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

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Caches the full rustybuzz shaping output (per-glyph IDs + advances).
    /// Used by shape_glyphs() so fill_text() avoids re-shaping every frame.
    /// Also serves as the measurement cache — measure_advance() reads it too.
    static SHAPE_CACHE: RefCell<HashMap<(usize, String, u64), Vec<ShapedGlyph>>> =
        RefCell::new(HashMap::new());
}

/// Measure text advance width without rasterizing.
///
/// Delegates to [`shape_glyphs`] so that fallback-font advances are included
/// in the measurement.  Results are cached via the shared shape cache.
pub fn measure_advance(font: &Font, text: &str, size: f64) -> f64 {
    shape_glyphs(font, text, size)
        .iter()
        .map(|g| g.x_advance)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT_BYTES: &[u8] =
        include_bytes!("../../demo/assets/CascadiaCode.ttf");
    const FA_BYTES: &[u8] =
        include_bytes!("../../demo/assets/fa.ttf");

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
            Font::from_slice(FONT_BYTES).expect("cc")
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
        use agg_rust::conv_curve::ConvCurve;
        use agg_rust::basics::{is_stop, VertexSource};

        let fa_arc = Arc::new(Font::from_slice(FA_BYTES).expect("fa"));
        let font = Arc::new(
            Font::from_slice(FONT_BYTES).expect("cc")
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
            if is_stop(cmd) { break; }
            if cx < xmin { xmin = cx; }
            if cx > xmax { xmax = cx; }
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
                let contours =
                    shape_and_flatten_text(&font, text, size, 0.0, 0.0, 0.5);

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
                if c.is_empty() { continue; }
                let xs: Vec<f32> = c.iter().map(|p| p[0]).collect();
                let ys: Vec<f32> = c.iter().map(|p| p[1]).collect();
                let xmin = xs.iter().cloned().fold(f32::INFINITY, f32::min);
                let xmax = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let ymin = ys.iter().cloned().fold(f32::INFINITY, f32::min);
                let ymax = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                eprintln!("  contour {ci}: {}/{} pts  x:[{xmin:.1},{xmax:.1}] y:[{ymin:.1},{ymax:.1}]",
                    c.len(), c.len());
            }
            let result = tessellate_fill(&contours);
            eprintln!("  tess: {:?}", result.as_ref().map(|(v,i)| (v.len()/2, i.len()/3)));
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
            ("Basics",   13.0),
            ("Widgets",  13.0),
            ("Text",     13.0),
            ("Layout",   13.0),
            ("Tree",     13.0),
            // floating window
            ("3D Demo",                  16.0),
            ("WebGL2 — rotating cube",   11.0),
            // Basics tab buttons
            ("Primary Action",  14.0),
            ("Secondary",       14.0),
            ("Destructive",     14.0),
            // text field placeholders
            ("Type something\u{2026}",  14.0),
            ("Another field",           14.0),
        ];

        let mut total_pts  = 0usize;
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
        assert!(total_pts  > 0,  "no contour points produced");
        assert!(total_tris > 0,  "no triangles tessellated");

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
        let size  = 16.0_f64;
        let glyphs = shape_glyphs(&font, "H", size);
        assert!(!glyphs.is_empty());
        let gid = glyphs[0].glyph_id;

        let contours = flatten_glyph_at_origin(&font, gid, size)
            .expect("'H' must have an outline");
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
        let font   = test_font();
        let glyphs = shape_glyphs(&font, " ", 14.0);
        assert_eq!(glyphs.len(), 1);
        let result = flatten_glyph_at_origin(&font, glyphs[0].glyph_id, 14.0);
        assert!(
            result.is_none(),
            "space glyph should have no outline, got {:?}",
            result.as_ref().map(|c| c.len())
        );
    }

    /// Verify that all contour points are in screen-pixel range for the
    /// given font size (not left in raw font units).
    #[test]
    fn test_flatten_output_is_in_screen_space() {
        let font = test_font();
        // Place text at (100, 200) at size 16.
        let contours =
            shape_and_flatten_text(&font, "Hello", 16.0, 100.0, 200.0, 0.5);

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
}
