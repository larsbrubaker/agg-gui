//! Width-fitting text elision in three modes: at the end (agg-sharp's
//! `EllipsisIfClipped`, see [`super::ellipsis`]), in the middle, and in the
//! middle at path separators (`/Users/alex/…/node_modules/esm`).
//!
//! [`elide_text`] is pure: it takes a `measure` closure, so the same code
//! serves [`crate::widgets::Label::with_ellipsis_mode`] (which measures with
//! the paint context's font) and custom-painted widgets such as status bars or
//! breadcrumbs that draw their own text.  Tests live in `elide_tests.rs`.

use super::ellipsis::ellipsize_with;
use super::{measure_text_metrics, Font};

/// The one-character ellipsis "…" (U+2026) the middle modes insert.
pub const ELLIPSIS: &str = "\u{2026}";

/// Where a too-wide single line is shortened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EllipsisMode {
    /// Cut the end and append "..." — agg-sharp's `EllipsisIfClipped`
    /// ([`ellipsize_with`]), what
    /// [`Label::with_ellipsis_if_clipped`](crate::widgets::Label::with_ellipsis_if_clipped)
    /// turns on.  Like agg-sharp it stops at four characters, so a very
    /// narrow box can still be overrun (the label's clip trims the rest).
    #[default]
    End,
    /// Keep as much of the start and end as fits around a "…", split evenly
    /// (the head gets the odd character).
    Middle,
    /// Path-aware middle: cut whole segments at `/` or `\` separators, always
    /// keeping the first segment, then as many trailing segments as fit, then
    /// as many more leading segments as fit.  Falls back to [`Self::Middle`]
    /// when not even `first/…/last` fits, or the text has no separators.
    PathMiddle,
}

/// Shorten `text` to fit `max_width` in the given `mode`.  `measure` returns
/// a string's advance width in the same units as `max_width`.
///
/// Text that already fits is returned unchanged.  The middle modes always
/// return a string that fits, except that a width too small for anything
/// yields just [`ELLIPSIS`].  Cuts land only on grapheme-cluster boundaries
/// (see [`cluster_bounds`]), never inside a character, combining sequence,
/// emoji ZWJ sequence or flag.  Widths are found by binary search, so a call
/// measures O(log n) candidate strings.
pub fn elide_text(
    text: &str,
    max_width: f64,
    mode: EllipsisMode,
    measure: impl Fn(&str) -> f64,
) -> String {
    if measure(text) <= max_width {
        return text.to_string();
    }
    match mode {
        EllipsisMode::End => ellipsize_with(text, max_width, &measure),
        EllipsisMode::Middle => elide_middle(text, max_width, &measure),
        EllipsisMode::PathMiddle => elide_path(text, max_width, &measure)
            .unwrap_or_else(|| elide_middle(text, max_width, &measure)),
    }
}

/// [`elide_text`] measuring with `font` at `size` pixels.
pub fn elide_to_width(
    font: &Font,
    text: &str,
    size: f64,
    max_width: f64,
    mode: EllipsisMode,
) -> String {
    elide_text(text, max_width, mode, |s| {
        measure_text_metrics(font, s, size).width
    })
}

/// Largest `t` in `lo..=hi` for which `fits(t)` holds, assuming `fits` is
/// true up to some point and false after it.  `None` when `fits(lo)` fails.
fn largest_fitting(lo: usize, hi: usize, fits: impl Fn(usize) -> bool) -> Option<usize> {
    if lo > hi || !fits(lo) {
        return None;
    }
    let (mut good, mut bad) = (lo, hi + 1);
    while bad - good > 1 {
        let mid = good + (bad - good) / 2;
        if fits(mid) {
            good = mid;
        } else {
            bad = mid;
        }
    }
    Some(good)
}

fn elide_middle(text: &str, max_width: f64, measure: &impl Fn(&str) -> f64) -> String {
    let bounds = cluster_bounds(text);
    let n = bounds.len() - 1;
    // Keep `keep` of the `n` clusters, head taking the odd one.
    let build = |keep: usize| {
        let head = bounds[keep - keep / 2];
        let tail = bounds[n - keep / 2];
        format!("{}{ELLIPSIS}{}", &text[..head], &text[tail..])
    };
    let keep = largest_fitting(0, n.saturating_sub(1), |k| measure(&build(k)) <= max_width);
    build(keep.unwrap_or(0))
}

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Segment-level cut for [`EllipsisMode::PathMiddle`]: `head` ends just after
/// a separator, `tail` starts at one.  `None` when the text has no usable
/// separators or even the shortest `first/…/last` is too wide.
fn elide_path(text: &str, max_width: f64, measure: &impl Fn(&str) -> f64) -> Option<String> {
    let first_content = text.find(|c| !is_separator(c))?;
    let seps: Vec<usize> = text
        .char_indices()
        .filter(|&(_, c)| is_separator(c))
        .map(|(i, _)| i)
        .collect();
    // Separators are one byte, so `s + 1` is the next char boundary.
    let head_ends: Vec<usize> = seps
        .iter()
        .filter(|&&s| s > first_content)
        .map(|&s| s + 1)
        .collect();
    let min_head = *head_ends.first()?;
    // A tail must leave something to elide and keep a non-separator char.
    let tail_starts: Vec<usize> = seps
        .iter()
        .copied()
        .filter(|&s| s > min_head && !text[s..].trim_start_matches(is_separator).is_empty())
        .collect();
    let build = |head: usize, tail: usize| format!("{}{ELLIPSIS}{}", &text[..head], &text[tail..]);
    // Most trailing segments first, with only the first segment in front.
    let count = tail_starts.len();
    let tails = largest_fitting(1, count, |t| {
        measure(&build(min_head, tail_starts[count - t])) <= max_width
    })?;
    let tail = tail_starts[count - tails];
    // Then as many leading segments as still fit before that tail.
    let heads: Vec<usize> = head_ends.into_iter().filter(|&h| h < tail).collect();
    let head = largest_fitting(0, heads.len() - 1, |j| {
        measure(&build(heads[j], tail)) <= max_width
    })
    .map_or(min_head, |j| heads[j]);
    Some(build(head, tail))
}

/// Byte offsets where each grapheme cluster of `text` starts, followed by
/// `text.len()`.  A lightweight approximation of Unicode UAX #29 (the crate
/// has no segmentation dependency): a char joins the previous cluster when it
/// is a combining mark, variation selector, joiner, emoji skin-tone modifier,
/// tag character or Hangul medial/final jamo; when it follows a ZWJ; when it
/// is the second regional indicator of a flag; or for `\r\n`.
pub(crate) fn cluster_bounds(text: &str) -> Vec<usize> {
    let mut bounds = Vec::with_capacity(text.len() + 1);
    let mut prev: Option<char> = None;
    let mut regional_run = 0usize;
    for (i, c) in text.char_indices() {
        let regional = is_regional_indicator(c);
        let joins = prev.is_some_and(|p| {
            extends_cluster(c)
                || p == '\u{200D}'
                || (p == '\r' && c == '\n')
                || (regional && regional_run % 2 == 1)
        });
        regional_run = if regional { regional_run + 1 } else { 0 };
        if !joins {
            bounds.push(i);
        }
        prev = Some(c);
    }
    bounds.push(text.len());
    bounds
}

fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// Chars that never start a cluster of their own (see [`cluster_bounds`]).
fn extends_cluster(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F   // combining diacritical marks
        | 0x0483..=0x0489 // Cyrillic combining marks
        | 0x0591..=0x05BD // Hebrew points
        | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4 // Arabic marks
        | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0951..=0x0957 | 0x0962..=0x0963 // Devanagari signs
        | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E // Thai vowels and tones
        | 0x1160..=0x11FF | 0xD7B0..=0xD7FF // Hangul medial / final jamo
        | 0x1AB0..=0x1AFF // combining diacritical marks extended
        | 0x1DC0..=0x1DFF // combining diacritical marks supplement
        | 0x200C..=0x200D // ZWNJ, ZWJ
        | 0x20D0..=0x20FF // combining marks for symbols
        | 0xFE00..=0xFE0F // variation selectors
        | 0xFE20..=0xFE2F // combining half marks
        | 0x1F3FB..=0x1F3FF // emoji skin-tone modifiers
        | 0xE0020..=0xE007F // tag characters (subdivision flags)
        | 0xE0100..=0xE01EF // variation selectors supplement
    )
}
