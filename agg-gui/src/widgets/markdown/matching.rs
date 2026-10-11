//! Matching-text highlighting for `MarkdownView`.
//!
//! Ports agg-sharp MarkdigAgg's `MatchingTextParser` + `AggMatchingTextRenderer`
//! (`MarkdownWidget.MatchingText`): every occurrence of the matching text in the
//! document's inline text (paragraphs, headings, list items, quotes, link text
//! and table cells, never inline code or code blocks, which Markdig does not
//! inline-parse) is drawn in a box padded 4 px on each side and filled with the
//! theme's minimal accent overlay (MatterCAD's `AccentMimimalOverlay`, the
//! accent at alpha 90).
//!
//! Matching follows the C# parser: the whole string is one term (no splitting
//! into words), it matches as a substring (no word boundaries), left to right
//! without overlaps, and a null/whitespace-only string matches nothing. Two C#
//! defects are not carried over: Markdig triggered the parser only on the exact
//! first character (so "bolt" never matched "Bolt"), and the renderer drew the
//! lowercased search text in place of the document's own text. Here every
//! character compares case-insensitively and the document's text is drawn.
//!
//! `layout.rs` uses [`MarkdownView::word_segments`] to split a word into plain
//! and highlighted runs; `paint.rs` draws the boxes.

use std::ops::Range;

use crate::color::Color;

use super::MarkdownView;

/// Padding inside a highlight box on each side, as `CodeInlineX.Padding = 4`.
pub(super) const MATCH_PAD: f64 = 4.0;

/// The highlight fill: MatterCAD's `theme.AccentMimimalOverlay`
/// (`accentColor.WithAlpha(90)`, ThemeSet.cs).
pub(super) fn match_fill(accent: Color) -> Color {
    accent.with_alpha(90.0 / 255.0)
}

/// Byte ranges of every non-overlapping, case-insensitive occurrence of
/// `needle` in `text`, left to right.
pub(super) fn match_ranges(text: &str, needle: &str) -> Vec<Range<usize>> {
    let needle: Vec<char> = needle.chars().collect();
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        let hit = needle
            .iter()
            .zip(&chars[i..])
            .all(|(n, (_, c))| n.to_lowercase().eq(c.to_lowercase()));
        if hit {
            let start = chars[i].0;
            let end = chars
                .get(i + needle.len())
                .map(|(b, _)| *b)
                .unwrap_or(text.len());
            out.push(start..end);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// The words of `text` (split on whitespace, as the layout wraps) with their
/// byte offsets.
pub(super) fn words_with_offsets(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((s, &text[s..i]));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push((s, &text[s..]));
    }
    out
}

/// One piece of a laid-out word: plain text or part of a match.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Segment {
    pub(super) text: String,
    pub(super) highlight: bool,
    /// The segment begins a match (gets the left box padding).
    pub(super) starts_match: bool,
    /// The segment ends a match (gets the right box padding).
    pub(super) ends_match: bool,
}

impl MarkdownView {
    /// The active matching text, `None` when unset or whitespace-only (the C#
    /// `string.IsNullOrWhiteSpace` guard).
    pub(super) fn active_matching_text(&self) -> Option<&str> {
        self.matching_text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
    }

    /// Split the word at byte `offset` of an inline text (whose match ranges
    /// are `ranges`) into plain and highlighted segments. With `lead_space` the
    /// word is preceded by the single space the layout inserts between words;
    /// that space is highlighted when a match spans the whitespace before the
    /// word, so a multi-word match reads as one box.
    pub(super) fn word_segments(
        word: &str,
        offset: usize,
        ranges: &[Range<usize>],
        lead_space: bool,
    ) -> Vec<Segment> {
        let mut out: Vec<Segment> = Vec::new();
        let mut push = |text: &str, highlight: bool, starts: bool, ends: bool| {
            if let Some(last) = out.last_mut() {
                if last.highlight == highlight && !(highlight && (starts || last.ends_match)) {
                    last.text.push_str(text);
                    last.ends_match = ends;
                    return;
                }
            }
            out.push(Segment {
                text: text.to_string(),
                highlight,
                starts_match: starts,
                ends_match: ends,
            });
        };
        if lead_space {
            let spanning = ranges.iter().any(|r| r.start < offset && r.end > offset);
            push(" ", spanning, false, false);
        }
        let end = offset + word.len();
        let mut pos = offset;
        for r in ranges.iter().filter(|r| r.end > offset && r.start < end) {
            let s = r.start.max(offset);
            if s > pos {
                push(&word[pos - offset..s - offset], false, false, false);
            }
            let e = r.end.min(end);
            push(
                &word[s - offset..e - offset],
                true,
                r.start >= offset,
                r.end <= end,
            );
            pos = e;
        }
        if pos < end {
            push(&word[pos - offset..], false, false, false);
        }
        out
    }
}

#[cfg(test)]
#[path = "matching_tests.rs"]
mod tests;
