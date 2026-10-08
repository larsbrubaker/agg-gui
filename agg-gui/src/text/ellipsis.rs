//! Ellipsis-if-clipped text shortening, agg-sharp's `TextWidget.EllipsisIfClipped`
//! (`Gui/TextWidgets/TextWidget.cs`, `OnDraw`): a single line too wide for the
//! space it is given is cut back and ends in "..." so it never paints past its
//! box. Used by [`crate::widgets::Label::with_ellipsis_if_clipped`] and by
//! custom widgets that draw their own text (section headings, list rows).
//! Lives beside `text.rs`, whose measurement it builds on.

use super::{measure_text_metrics, Font};

/// The text agg-sharp's `TextWidget` draws for `text` in `max_width` when
/// `EllipsisIfClipped` is on: `text` itself when it fits, otherwise, while it
/// is still too wide and longer than four characters, drop the last four
/// characters, trim trailing spaces and append "..." (so after the first cut
/// each step removes one more character of the original). `measure` returns a
/// string's advance width in the same units as `max_width`.
///
/// agg-sharp stops at four characters even when the result is still too wide;
/// callers clip as well, so the last few characters are cut by the clip, as
/// they are there.
pub fn ellipsize_with(text: &str, max_width: f64, measure: impl Fn(&str) -> f64) -> String {
    if measure(text) <= max_width {
        return text.to_string();
    }
    let mut short = text.to_string();
    // C# `Text.Length > 4` and `Substring(0, Length - 4)` count UTF-16 units;
    // counting chars keeps every cut on a character boundary.
    while measure(&short) > max_width && short.chars().count() > 4 {
        let keep = short.chars().count() - 4;
        let cut: String = short.chars().take(keep).collect();
        short = format!("{}...", cut.trim_end_matches(' '));
    }
    short
}

/// [`ellipsize_with`] measuring with `font` at `size` pixels.
pub fn ellipsize_to_width(font: &Font, text: &str, size: f64, max_width: f64) -> String {
    ellipsize_with(text, max_width, |s| {
        measure_text_metrics(font, s, size).width
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seven pixels per character, as the test `PaintRecorder` measures.
    fn seven(s: &str) -> f64 {
        s.chars().count() as f64 * 7.0
    }

    #[test]
    fn text_that_fits_is_unchanged() {
        assert_eq!(ellipsize_with("Cube", 28.0, seven), "Cube");
    }

    #[test]
    fn too_wide_text_is_cut_back_and_ends_in_dots() {
        // 26 chars = 182 px; 70 px holds 10 chars. "Simple P..." (11) is still too
        // wide; the next cut leaves "Simple " whose space is trimmed.
        let shown = ellipsize_with("Simple Proof of Bevel.mcx!", 70.0, seven);
        assert_eq!(shown, "Simple...");
        assert!(seven(&shown) <= 70.0);
    }

    #[test]
    fn trailing_spaces_before_the_dots_are_trimmed() {
        // "abc defgh" (63 px) in 49 px: cut 4 -> "abc d" + "..." = 8 chars (56 px),
        // then "abc " trimmed to "abc" + "..." = 6 chars (42 px).
        assert_eq!(ellipsize_with("abc defgh", 49.0, seven), "abc...");
    }

    #[test]
    fn stops_at_four_characters_as_agg_sharp_does() {
        assert_eq!(ellipsize_with("abcdefgh", 1.0, seven), "a...");
    }

    #[test]
    fn multibyte_text_is_cut_on_character_boundaries() {
        let shown = ellipsize_with("ééééééééé", 42.0, seven);
        assert_eq!(shown, "ééé...");
    }
}
