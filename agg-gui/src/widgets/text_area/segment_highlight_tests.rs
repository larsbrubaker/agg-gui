//! Unit tests for [`segment_highlight`](super::widget_impl::segment_highlight),
//! the helper the [`TextArea`](super::TextArea) paint path uses to turn caller
//! supplied highlight spans into draw-ready colour runs.
//!
//! Split out of the sibling `tests.rs` to keep both files under the project's
//! 800-line limit. The highlighter paint path must split a line into gap-free,
//! non-overlapping colour segments so every glyph is filled exactly once (no
//! double-paint on AA fringes). These exercise the production function directly.

use super::widget_impl::segment_highlight;
use crate::color::Color;

const BASE: Color = Color::rgb(1.0, 1.0, 1.0);
const RUN: Color = Color::rgb(1.0, 0.0, 0.0);

/// Every byte of the text is covered by exactly one segment, in order, with
/// no overlaps and no gaps.
fn assert_covers_once(text: &str, segs: &[(usize, usize, Color)]) {
    let mut pos = 0usize;
    for &(s, e, _) in segs {
        assert_eq!(s, pos, "segment start must abut the previous end: {segs:?}");
        assert!(e > s, "segment must be non-empty: {segs:?}");
        pos = e;
    }
    assert_eq!(
        pos,
        text.len(),
        "segments must cover the whole line: {segs:?}"
    );
}

#[test]
fn segment_highlight_fills_gaps_and_runs_once() {
    let text = "let x = 1;";
    // Colour "let" and "1" only; the rest are gaps in the base colour.
    let spans = [(0usize, 3usize, RUN), (8usize, 9usize, RUN)];
    let segs = segment_highlight(text, &spans, BASE);
    assert_covers_once(text, &segs);
    assert_eq!(
        segs,
        vec![
            (0, 3, RUN),   // "let"
            (3, 8, BASE),  // " x = "
            (8, 9, RUN),   // "1"
            (9, 10, BASE), // ";"
        ]
    );
}

#[test]
fn segment_highlight_no_spans_is_single_base_run() {
    let text = "plain";
    let segs = segment_highlight(text, &[], BASE);
    assert_eq!(segs, vec![(0, 5, BASE)]);
}

#[test]
fn segment_highlight_drops_invalid_and_resolves_overlap() {
    let text = "abcdef";
    // Reversed, out-of-range, and non-char-boundary-safe-but-overlapping spans.
    let spans = [
        (2usize, 2usize, RUN),  // empty → dropped
        (4usize, 3usize, RUN),  // reversed → dropped
        (0usize, 10usize, RUN), // out of range → dropped
        (0usize, 3usize, RUN),  // valid
        (2usize, 5usize, BASE), // overlaps the previous → clamped to [3,5)
    ];
    let segs = segment_highlight(text, &spans, BASE);
    assert_covers_once(text, &segs);
    // First span wins bytes 0..3; the overlapper contributes only 3..5.
    assert_eq!(segs, vec![(0, 3, RUN), (3, 5, BASE), (5, 6, BASE)]);
}

#[test]
fn segment_highlight_empty_text_is_empty() {
    assert!(segment_highlight("", &[(0, 0, RUN)], BASE).is_empty());
}
