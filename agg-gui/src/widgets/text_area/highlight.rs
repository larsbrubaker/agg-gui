//! Syntax-highlight segmentation and painting for [`TextArea`].
//!
//! A caller-supplied highlighter (see `TextArea::with_highlighter`) hands back
//! coloured byte spans for a line of text. Those spans are advisory: they may
//! overlap, arrive out of order, run past the end of the line, or land mid-UTF-8.
//! Painting them naively — the ambient text first, then each span on top — draws
//! highlighted glyphs twice, and the second alpha pass thickens their fringes.
//!
//! This module turns the advisory spans into a *partition* of the line
//! ([`segment_highlight`]) and paints that partition ([`TextArea::paint_highlighted_line`]),
//! so the AA text path in `widget_impl`'s `paint` fills every glyph exactly once.
//! The segmentation is pure and side-effect free so it can be unit-tested in
//! isolation (`segment_highlight_tests`); `widget_impl` re-exports it so the
//! historic `widget_impl::segment_highlight` path keeps resolving.

use super::*;
use crate::color::Color;

/// Split a highlighted line into gap-free, non-overlapping colour segments
/// covering `[0, text.len())` exactly once.
///
/// Bytes covered by a valid span take that span's colour; every uncovered
/// gap takes `base_color`. This is the segmentation the AA text path needs:
/// each glyph is emitted by exactly one segment, so highlighted tokens never
/// accumulate a second alpha pass on their fringes (which made them look
/// subtly bolder) and no fill work is duplicated.
///
/// Spans are validated defensively — reversed, out-of-range, or
/// non-char-boundary spans are dropped. Spans are processed in start order
/// and the first span to cover a byte wins any overlap, so the output stays
/// strictly non-overlapping even if a highlighter hands back sloppy ranges.
pub(crate) fn segment_highlight(
    text: &str,
    spans: &[(usize, usize, Color)],
    base_color: Color,
) -> Vec<(usize, usize, Color)> {
    let len = text.len();
    let mut valid: Vec<(usize, usize, Color)> = spans
        .iter()
        .copied()
        .filter(|&(s, e, _)| {
            s < e && e <= len && text.is_char_boundary(s) && text.is_char_boundary(e)
        })
        .collect();
    valid.sort_by_key(|&(s, _, _)| s);

    let mut out: Vec<(usize, usize, Color)> = Vec::new();
    let mut pos = 0usize;
    for (s, e, color) in valid {
        if e <= pos {
            // Fully behind already-emitted output — first span won this byte.
            continue;
        }
        // Clamp a partially overlapping start up to the emitted frontier.
        let s = s.max(pos);
        if s > pos {
            out.push((pos, s, base_color)); // uncovered gap
        }
        out.push((s, e, color));
        pos = e;
    }
    if pos < len {
        out.push((pos, len, base_color));
    }
    out
}

impl TextArea {
    /// Paint one wrapped line as gap-free, non-overlapping colour segments so
    /// every glyph is filled exactly once (see [`segment_highlight`]). Byte
    /// offsets in `spans` are relative to `text`.
    pub(super) fn paint_highlighted_line(
        &self,
        ctx: &mut dyn DrawCtx,
        text: &str,
        spans: &[(usize, usize, Color)],
        x0: f64,
        baseline_y: f64,
        base_color: Color,
    ) {
        for (s, e, color) in segment_highlight(text, spans, base_color) {
            let x = x0 + measure_advance(&self.font, &text[..s], self.font_size);
            ctx.set_fill_color(color);
            ctx.fill_text(&text[s..e], x, baseline_y);
        }
    }
}
