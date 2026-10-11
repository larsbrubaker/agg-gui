//! One laid-out line of a rich block, its fragments and its caret stops — a
//! port of agg-sharp's `RichLayoutLine.cs` (`RichLayoutFragment`,
//! `RichCaret`, `RichCaretStop`, `RichLayoutLine`).
//!
//! [`super::block_layout`] builds these; coordinates are its: y up from the
//! bottom of the block.  Offsets count chars (see the module docs).

use super::inline::InlineAtom;
use super::layout_style::RichFace;

/// A piece of one line drawn in one face: part of a run, a whole atom, or part
/// of a code block's text.
#[derive(Clone, Debug, PartialEq)]
pub struct RichLayoutFragment {
    /// The inline this fragment draws, or `None` for a code block's text and
    /// a Raw block's box (C# -1).
    pub inline_index: Option<usize>,
    /// Where the fragment starts within its inline's text, in chars (within
    /// the code text for a code block).
    pub start_in_inline: usize,
    /// The block offset of the fragment's first caret position.
    pub start: usize,
    /// How many block offsets the fragment covers (1 for an atom).
    pub length: usize,
    /// What to draw: the run's characters, an atom's raw markdown, or "" for
    /// an image box.
    pub text: String,
    /// The face to draw `text` in; `None` for a box (an image or a Raw block)
    /// the widget fills.
    pub face: Option<RichFace>,
    /// The atom this fragment shows, or `None` for text.
    pub atom: Option<InlineAtom>,
    /// Left edge.
    pub x: f64,
    /// Width.
    pub width: f64,
    /// The baseline the fragment sits on.
    pub baseline: f64,
    /// Height above the baseline (a box's whole height: boxes sit on the
    /// baseline).
    pub ascent: f64,
    /// Depth below the baseline, positive.
    pub descent: f64,
}

impl RichLayoutFragment {
    /// Ascent plus descent.
    pub fn height(&self) -> f64 {
        self.ascent + self.descent
    }
}

/// A caret position in a block.  `at_line_end` picks between the two places
/// an offset can show where a line wraps with no space (a word broken between
/// characters, or a box pushed to the next line): false is the start of the
/// next line, true the end of the line before — where a click right of that
/// line or End puts it.  In a table, `row` and `column` name the cell `offset`
/// counts through, as in `DocPosition`; they are 0 in every other block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RichCaret {
    /// Caret offset in chars.
    pub offset: usize,
    /// Shown at the end of the line before a no-space wrap.
    pub at_line_end: bool,
    /// Table row (0 outside a table).
    pub row: usize,
    /// Table column (0 outside a table).
    pub column: usize,
}

impl RichCaret {
    /// A plain caret at `offset`.
    pub fn new(offset: usize) -> Self {
        Self {
            offset,
            ..Self::default()
        }
    }

    /// The line-end caret at `offset`.
    pub fn at_line_end(offset: usize) -> Self {
        Self {
            offset,
            at_line_end: true,
            ..Self::default()
        }
    }
}

/// A place on a line the caret can sit, with its x (block coordinates,
/// clamped into the text column).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RichCaretStop {
    /// The caret this stop shows.
    pub caret: RichCaret,
    /// Its x.
    pub x: f64,
}

/// One laid-out line, with the caret stops on it in order.  Offsets inside a
/// character (before a combining mark) or inside an atom have no stop.  The
/// offset where a line wraps is a plain stop at the start of the next line;
/// the spaces a wrap happens at stay at the end of the line before (past
/// `width`), so each of their offsets still has its own stop.  A wrap with no
/// space adds an `at_line_end` stop at the end of the line before.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichLayoutLine {
    /// The fragments, left to right.
    pub fragments: Vec<RichLayoutFragment>,
    /// The caret stops, in order.
    pub caret_stops: Vec<RichCaretStop>,
    /// The block offset of the line's first item.
    pub start: usize,
    /// The block offset just after the line's last item: the next line's start.
    pub end: usize,
    /// The block's last line.
    pub is_last: bool,
    /// The bottom of the line's box (y up), including half the line gap.
    pub bottom: f64,
    /// The top of the line's box.
    pub top: f64,
    /// The baseline.
    pub baseline: f64,
    /// Height above the baseline.
    pub ascent: f64,
    /// Depth below the baseline, positive.
    pub descent: f64,
    /// The x of the line's first caret position: the block's text left plus
    /// the alignment offset.
    pub left: f64,
    /// The width of the line's content, not counting spaces it wrapped at or
    /// a trailing hard break; alignment centres this width.
    pub width: f64,
}
