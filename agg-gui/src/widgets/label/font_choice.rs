//! Which face and size a `Label` lays out and paints with.
//!
//! `layout`, `paint`, `ink_overhang` and `measure_min_height` (in
//! `label.rs`) all resolve the font through [`Label::active_font`] and the
//! size through [`Label::active_font_size`] on every call, so a live change
//! of the system font, its bold companion or the text-size scale
//! (`font_settings`) reaches every label without a rebuild: the resolved
//! font's id and size are compared against the last layout's, which drops
//! the label's cached bitmap.
//!
//! A bold label ([`Label::with_bold`]) draws with the bold companion of the
//! system font (`font_settings::set_system_bold_font`). Without one it draws
//! the face a regular label would use, thickened
//! (`Font::variant_with_faux_style`), so it is still bold and still follows
//! the system font. The face passed to `Label::new` is the regular one: a
//! bold face passed there would be thickened again.

use std::sync::Arc;

use super::Label;
use crate::text::Font;

impl Label {
    /// Draw this label bold: with the bold companion of the system font
    /// ([`font_settings::set_system_bold_font`](crate::font_settings::set_system_bold_font))
    /// when the app set one, otherwise with the face a regular label would
    /// use (the system font, or the one passed to [`Label::new`]) thickened.
    /// Either way the text keeps following live system-font and text-size
    /// changes. Pass the regular face to `Label::new`.
    pub fn with_bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }

    /// Setter form of [`with_bold`](Self::with_bold).
    pub fn set_bold(&mut self, bold: bool) {
        if self.bold != bold {
            self.bold = bold;
            self.cache.invalidate();
        }
    }

    /// Whether this label draws bold ([`with_bold`](Self::with_bold)).
    pub fn is_bold(&self) -> bool {
        self.bold
    }

    /// The face this label draws with on this layout/paint.
    ///
    /// Regular: the system-wide font override (the System window /
    /// `font_settings::set_system_font`) so swapping the system font live
    /// flows through every widget, else the per-instance font; with
    /// `ignore_system_font`, always the per-instance font.
    ///
    /// Bold: the system bold companion when set (unless the label ignores
    /// the system font), else the regular choice thickened.
    pub(super) fn active_font(&self) -> Arc<Font> {
        if self.bold && !self.ignore_system_font {
            if let Some(bold) = crate::font_settings::current_system_bold_font() {
                return bold;
            }
        }
        let regular = if self.ignore_system_font {
            Arc::clone(&self.font)
        } else {
            crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
        };
        if self.bold {
            self.faux_bold_of(&regular)
        } else {
            regular
        }
    }

    /// The thickened variant of `regular`, built once per face: a fresh
    /// variant each call would carry a new font id, so every layout would
    /// see a "new" font and throw the cached bitmap away.
    fn faux_bold_of(&self, regular: &Arc<Font>) -> Arc<Font> {
        let mut cache = self.faux_bold.borrow_mut();
        match cache.as_ref() {
            Some((base_id, variant)) if *base_id == regular.id() => Arc::clone(variant),
            _ => {
                let variant = Arc::new(regular.variant_with_faux_style(false, true));
                *cache = Some((regular.id(), Arc::clone(&variant)));
                variant
            }
        }
    }

    /// Per-instance font size multiplied by the system-wide
    /// [`font_settings::current_font_size_scale`](crate::font_settings::current_font_size_scale),
    /// bold or not.  Label's font-preview UI (combo-box items flagged
    /// `ignore_system_font`) ALSO ignores the scale — a font picker must
    /// show every entry at the same reference size or comparing faces
    /// becomes useless.
    pub(super) fn active_font_size(&self) -> f64 {
        if self.ignore_system_font {
            self.font_size
        } else {
            self.font_size * crate::font_settings::current_font_size_scale()
        }
    }
}
