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

use agg_rust::basics::{
    is_end_poly, is_move_to, is_stop, VertexSource, PATH_CMD_LINE_TO, PATH_FLAGS_NONE,
};
use agg_rust::conv_contour::ConvContour;
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_transform::ConvTransform;
use agg_rust::path_storage::PathStorage;
use agg_rust::trans_affine::TransAffine;

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

impl TextMetrics {
    /// Baseline Y that visually centers this text run in a Y-up box.
    pub fn centered_baseline_y(&self, height: f64) -> f64 {
        (height - (self.ascent - self.descent)) * 0.5
    }
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

    /// Actual vertical extent of a single glyph in pixels (Y-up), relative
    /// to the baseline. Returns `(y_min, y_max)` where `y_min` is how far
    /// the glyph dips below the baseline (negative for descenders, near
    /// zero for upright glyphs) and `y_max` is how far it rises above.
    ///
    /// Use this for *visually* centring an icon glyph in a button —
    /// `ascender_px`/`descender_px` describe the FONT's worst-case
    /// extents and are too generous for most icon fonts (Font Awesome
    /// glyphs sit in a sub-rectangle of the design space, so centring
    /// by the font metric leaves them noticeably high). Returns `None`
    /// when the glyph has no outline (e.g. a space) or isn't in the
    /// font.
    pub fn glyph_visual_bounds(&self, glyph: char, size: f64) -> Option<(f64, f64)> {
        self.with_ttf_face(|face| {
            let gid = face.glyph_index(glyph)?;
            let bbox = face.glyph_bounding_box(gid)?;
            let scale = size / self.units_per_em as f64;
            // ttf_parser reports y_min / y_max in font units relative to
            // baseline, Y-up — convert directly.
            Some((bbox.y_min as f64 * scale, bbox.y_max as f64 * scale))
        })
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
///
/// The builder can optionally apply two of the `font_settings` typography
/// transforms directly at outline-construction time:
/// - `width_scale` — horizontal scale applied to every glyph vertex,
///   leaving advances untouched (matches AGG `truetype_lcd.cpp` "Width").
/// - `italic_shear` — horizontal shear as a fraction of Y: `x += y *
///   italic_shear`.  Matches the C++ "Faux Italic" which applies
///   `TransAffine::new_skewing(faux_italic/3, 0)`; the `/3` convention
///   keeps the slider range comparable.
pub(crate) struct GlyphPathBuilder {
    pub path: PathStorage,
    ox: f64,
    oy: f64,
    scale: f64,
    /// Horizontal-only outline scale.  Default `1.0`.
    width_scale: f64,
    /// Italic shear factor (x += y * italic_shear).  Default `0.0`.
    italic_shear: f64,
    pub has_outline: bool,
}

impl GlyphPathBuilder {
    pub fn new(ox: f64, oy: f64, scale: f64) -> Self {
        Self {
            path: PathStorage::new(),
            ox,
            oy,
            scale,
            width_scale: 1.0,
            italic_shear: 0.0,
            has_outline: false,
        }
    }

    /// Enable Width + Faux-Italic transforms for this glyph.  `width`
    /// multiplies every outline X after font-scaling; `italic` shears
    /// horizontally proportional to the vertex's Y above the baseline
    /// (positive italic slants top-right, matching the AGG reference).
    #[allow(dead_code)]
    pub fn with_style(mut self, width: f64, italic: f64) -> Self {
        self.width_scale = width;
        self.italic_shear = italic;
        self
    }

    /// Pixel-space X of a font-unit input vertex.
    ///
    /// `italic_shear` uses the **unsheared** Y (distance above baseline)
    /// so the shear stays consistent whether or not hinting has snapped
    /// the glyph origin — the shear depends on glyph geometry, not on
    /// where the baseline landed on screen.
    #[inline]
    fn x(&self, v: f32, y_raw: f32) -> f64 {
        let base_x = self.ox + v as f64 * self.scale * self.width_scale;
        let shear = y_raw as f64 * self.scale * self.italic_shear;
        base_x + shear
    }
    #[inline]
    fn y(&self, v: f32) -> f64 {
        self.oy + v as f64 * self.scale
    }
}

impl ttf_parser::OutlineBuilder for GlyphPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.x(x, y), self.y(y));
        self.has_outline = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.x(x, y), self.y(y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path
            .curve3(self.x(x1, y1), self.y(y1), self.x(x, y), self.y(y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path.curve4(
            self.x(x1, y1),
            self.y(y1),
            self.x(x2, y2),
            self.y(y2),
            self.x(x, y),
            self.y(y),
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
/// Apply the "faux weight" outline offset to a glyph path.
///
/// Port of the AGG C++ `truetype_lcd.cpp` technique:
/// ```text
///   curves -> scale(1, 100) -> ConvContour(width=w) -> scale(1, 1/100)
/// ```
/// The Y-zoom makes the contour offset act primarily horizontally —
/// vertical stems pick up the full `w` of extra thickness while
/// horizontal strokes stay thin, which is what you want for bold-like
/// weight.  Returns a fresh `PathStorage` containing the offset outline
/// flattened to straight segments (ConvCurve has already subdivided the
/// Béziers by the time ConvContour sees them).
///
/// `weight_px` is the raw contour width — matches the agg-rust
/// `contour.set_width(-faux_weight * height / 15.0)` convention; pass
/// the already-sign-flipped, already-scaled value.
fn apply_faux_weight(path: PathStorage, weight_px: f64) -> PathStorage {
    if weight_px.abs() < 1e-4 {
        return path;
    }
    let mut src = path;
    let mut curves = ConvCurve::new(&mut src);
    let zoom_in = TransAffine::new_scaling(1.0, 100.0);
    let mut zoomed_in = ConvTransform::new(&mut curves, zoom_in);
    let mut contour = ConvContour::new(&mut zoomed_in);
    contour.set_auto_detect_orientation(false);
    contour.set_width(weight_px);
    let zoom_out = TransAffine::new_scaling(1.0, 1.0 / 100.0);
    let mut out = ConvTransform::new(&mut contour, zoom_out);

    // Flatten the VertexSource chain into a fresh PathStorage.  ConvCurve
    // has converted all Béziers to line-segments by the time we get here,
    // so the output is only `move_to` / `line_to` / `end_poly` commands.
    let mut result = PathStorage::new();
    out.rewind(0);
    loop {
        let (mut vx, mut vy) = (0.0_f64, 0.0_f64);
        let cmd = out.vertex(&mut vx, &mut vy);
        if is_stop(cmd) {
            break;
        }
        if is_move_to(cmd) {
            result.move_to(vx, vy);
        } else if cmd == PATH_CMD_LINE_TO {
            result.line_to(vx, vy);
        } else if is_end_poly(cmd) {
            result.close_polygon(PATH_FLAGS_NONE);
        }
    }
    result
}

pub(crate) fn shape_text(
    font: &Font,
    text: &str,
    size: f64,
    x: f64,
    y: f64,
) -> (Vec<PathStorage>, f64) {
    let shaped = shape_glyphs(font, text, size);

    // Pull the current typography-style globals ONCE per call.  The
    // text render path consults them here so any widget (including the
    // LCD Subpixel demo's sliders) that writes through `font_settings`
    // affects the next paint.
    //
    // - `width_scale`  → horizontal outline scale per glyph
    // - `italic_shear` → faux-italic (0..1 range maps to /3 in the
    //   outline shear, matching the agg-rust reference)
    // - `hint_y`       → snap the glyph-origin Y to whole pixels
    //                    (Y-axis-only hinting, matches `(y+0.5).floor()`)
    // - `interval_px`  → extra pen advance in pixels per glyph,
    //                    proportional to em size
    let width_scale = crate::font_settings::current_width();
    let italic_shear = crate::font_settings::current_faux_italic() / 3.0;
    let hint_y = crate::font_settings::hinting_enabled();
    let interval_em = crate::font_settings::current_interval();
    let interval_px = interval_em * size;
    // Faux weight — negative sign matches agg-rust: +faux_weight
    // thickens (contour width negative expands outward for a CCW
    // outline), -faux_weight thins.  The `/15.0` denominator reproduces
    // the reference demo's slider-to-pixels conversion.
    let faux_weight = crate::font_settings::current_faux_weight();
    let weight_px = if faux_weight.abs() < 0.05 {
        0.0 // dead zone near 0, matches reference — avoids zero-width noise
    } else {
        -faux_weight * size / 15.0
    };

    let mut paths = Vec::new();
    let mut pen_x = x;
    let mut total_advance = 0.0;

    for g in &shaped {
        let gx = pen_x + g.x_offset;
        let gy_unsnapped = y + g.y_offset;
        // Hinting: snap the glyph origin's Y to the integer pixel
        // nearest the logical baseline.  Matches the AGG C++
        // `(y + 0.5).floor()` convention — simple, cheap, preserves
        // horizontal subpixel positioning.
        let gy = if hint_y {
            (gy_unsnapped + 0.5).floor()
        } else {
            gy_unsnapped
        };
        // glyph_id indexes into whichever font resolved the code point.
        let render_font = g.fallback_font.as_deref().unwrap_or(font);
        let scale = size / render_font.units_per_em() as f64;

        let mut builder =
            GlyphPathBuilder::new(gx, gy, scale).with_style(width_scale, italic_shear);
        let has_outline = render_font.with_ttf_face(|face| {
            face.outline_glyph(ttf_parser::GlyphId(g.glyph_id), &mut builder)
                .is_some()
        });
        if has_outline && builder.has_outline {
            // Apply faux weight (zero-cost pass-through at weight_px == 0).
            let path = apply_faux_weight(builder.path, weight_px);
            paths.push(path);
        }

        // Interval adds a fixed pen-advance delta per glyph, in pixels.
        // Applied after the font-native advance so kerning (already
        // baked into x_advance by rustybuzz) is preserved — the extra
        // spacing just piles on top.
        let advance = g.x_advance + interval_px;
        pen_x += advance;
        total_advance += advance;
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

        cache
            .borrow_mut()
            .insert((font_key, text.to_owned(), size_key), glyphs.clone());
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
pub fn flatten_glyph_at_origin(
    font: &Font,
    glyph_id: u16,
    size: f64,
) -> Option<Vec<Vec<[f32; 2]>>> {
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
        let mut current: Vec<[f32; 2]> = Vec::new();

        loop {
            let (mut cx, mut cy) = (0.0_f64, 0.0_f64);
            let cmd = curves.vertex(&mut cx, &mut cy);
            if is_stop(cmd) {
                break;
            }
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

        if contours.is_empty() {
            None
        } else {
            Some(contours)
        }
    })
}

/// Measure full text metrics (width, ascent, descent, line_height).
///
/// Useful for external rendering backends (e.g. `GlGfxCtx`) that need
/// text metrics without the `GfxCtx` wrapper.
pub fn measure_text_metrics(font: &Font, text: &str, size: f64) -> TextMetrics {
    TextMetrics {
        width: measure_advance(font, text, size),
        ascent: font.ascender_px(size),
        descent: font.descender_px(size),
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
///
/// The measurement matches what `shape_text` will actually pen at paint
/// time — so `interval` (extra letter-spacing) is added here too.  Width
/// and italic are ignored: width only affects per-glyph outline scale,
/// not advances, and italic shears the outline which doesn't change the
/// horizontal extent of the pen walk.
pub fn measure_advance(font: &Font, text: &str, size: f64) -> f64 {
    let shaped = shape_glyphs(font, text, size);
    let interval_px = crate::font_settings::current_interval() * size;
    shaped.iter().map(|g| g.x_advance + interval_px).sum()
}

#[cfg(test)]
mod tests;
