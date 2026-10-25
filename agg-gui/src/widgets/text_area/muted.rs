//! A muted range for [`TextArea`]: bytes of the buffer painted in the hint
//! colour instead of the ambient text colour, for text that shows but is not
//! final yet, such as the words a dictation is still unsure of (an app shows
//! them grey until they are confirmed).
//!
//! The range lives in a shared cell the caller owns, so it can move it with
//! each edit it makes to the bound edit state ([`TextArea::with_edit_state`])
//! without reaching the widget. It is in buffer bytes and is clamped to the
//! text and to character boundaries when painted, so a stale range never
//! panics; the paint path (`widget_impl`) splits each wrapped line into
//! coloured runs through the same segmentation as the line highlighter
//! (`highlight.rs`), and the backbuffer signature includes the range, so a
//! change of colour alone re-rasters.
//! Split out of `text_area.rs` (800-line cap).

use super::*;
use crate::color::Color;

/// The shared cell holding a [`TextArea`]'s muted byte range.
pub type MutedRange = Rc<Cell<Option<(usize, usize)>>>;

impl TextArea {
    /// Paint bytes `[start, end)` of the buffer, while `range` holds them, in
    /// the muted hint colour. Setting the cell to `None` paints everything in
    /// the ambient colour again.
    pub fn with_muted_range(mut self, range: MutedRange) -> Self {
        self.muted = Some(range);
        self
    }

    /// The muted range now, if any and not empty.
    pub(super) fn muted_range(&self) -> Option<(usize, usize)> {
        self.muted
            .as_ref()
            .and_then(|cell| cell.get())
            .filter(|(start, end)| start < end)
    }

    /// The coloured runs for one wrapped line, relative to its text: the
    /// highlighter's (if any) with the muted part of the line, if any, in
    /// `muted_color` over them. `None` when the line is plain.
    pub(super) fn line_spans(
        &self,
        line: &WrappedLine,
        muted_color: Color,
    ) -> Option<Vec<(usize, usize, Color)>> {
        let muted = self.muted_range().and_then(|(start, end)| {
            let a = start.max(line.start).saturating_sub(line.start);
            let b = end
                .min(line.end)
                .saturating_sub(line.start)
                .min(line.text.len());
            (a < b).then_some((a, b))
        });
        let mut spans = match &self.highlighter {
            Some(highlighter) => highlighter(&line.text),
            None if muted.is_none() => return None,
            None => Vec::new(),
        };
        if let Some((a, b)) = muted {
            // The muted run wins: a highlighted run overlapping it keeps only
            // its parts outside, so every byte has exactly one colour.
            let mut kept = Vec::with_capacity(spans.len() + 2);
            for (s, e, color) in spans {
                if s < a {
                    kept.push((s, e.min(a), color));
                }
                if e > b {
                    kept.push((s.max(b), e, color));
                }
            }
            kept.push((a, b, muted_color));
            spans = kept;
        }
        Some(spans)
    }
}
