//! Per-font faux italic and faux bold — agg-sharp's `StyledTypeFace.FauxItalic`.
//!
//! The fonts agg-sharp ships have no italic face, so its `TextWidget.Italic`
//! shears the current face by `StyledTypeFace.FauxItalicShear` (0.2: x moves
//! right by a fifth of the height above the baseline). A sheared glyph keeps
//! its advance, so italic never changes measurement. Here the slant is a flag
//! on a [`Font`] handle, applied wherever glyph outlines are built
//! (`shape_text` in `text.rs`, and agg-gui-wgpu's outline path), stacking on
//! the global Faux Italic of `font_settings` as the C# shear does.
//!
//! Faux bold is the fallback for a host that has no bold face to hand
//! `MarkdownView`: the glyph outlines grow by [`FAUX_BOLD_WEIGHT`] on the
//! global Faux Weight scale (agg's `truetype_lcd.cpp` contour offset), which
//! leaves advances alone just as the shear does.
//!
//! A variant shares the font data `Arc`, so it costs no extra bytes; it must
//! be kept in an `Arc` of its own across frames, because the text mask cache
//! keys on the `Arc` pointer (one fresh `Arc` per paint would miss every time).

use std::sync::Arc;

use super::Font;

/// The horizontal shift per unit of height of a faux-italic font,
/// agg-sharp's `StyledTypeFace.FauxItalicShear`.
pub const FAUX_ITALIC_SHEAR: f64 = 0.2;

/// The outline growth of a faux-bold font on the `font_settings` Faux Weight
/// scale (-1..1), half the slider's heaviest setting.
pub const FAUX_BOLD_WEIGHT: f64 = 0.5;

impl Font {
    /// A second handle on the same face whose glyphs are slanted
    /// (`italic`, by [`FAUX_ITALIC_SHEAR`]) and/or thickened (`bold`, by
    /// [`FAUX_BOLD_WEIGHT`]). Advances, metrics, fallback chain and the
    /// tabular-digits flag are those of `self`.
    pub fn variant_with_faux_style(&self, italic: bool, bold: bool) -> Font {
        Font {
            id: super::font_id::next_font_id(),
            shape_id: self.shape_id,
            data: Arc::clone(&self.data),
            index: self.index,
            units_per_em: self.units_per_em,
            ascender: self.ascender,
            descender: self.descender,
            line_gap: self.line_gap,
            cap_height: self.cap_height,
            face_ascender: self.face_ascender,
            face_descender: self.face_descender,
            fallback: self.fallback.clone(),
            tabular_digits: self.tabular_digits,
            faux_italic: italic,
            faux_bold: bold,
        }
    }

    /// Whether this font slants its glyphs (agg-sharp `StyledTypeFace.FauxItalic`).
    pub fn faux_italic(&self) -> bool {
        self.faux_italic
    }

    /// Whether this font thickens its glyphs.
    pub fn faux_bold(&self) -> bool {
        self.faux_bold
    }

    /// The shear this font adds to every glyph outline: [`FAUX_ITALIC_SHEAR`]
    /// when faux italic, else 0.
    pub fn faux_italic_shear(&self) -> f64 {
        if self.faux_italic {
            FAUX_ITALIC_SHEAR
        } else {
            0.0
        }
    }

    /// The Faux Weight this font adds to every glyph outline:
    /// [`FAUX_BOLD_WEIGHT`] when faux bold, else 0.
    pub fn faux_bold_weight(&self) -> f64 {
        if self.faux_bold {
            FAUX_BOLD_WEIGHT
        } else {
            0.0
        }
    }
}
