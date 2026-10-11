//! Offset arithmetic over a list of inlines (a paragraph's or a table cell's).
//! Ports agg-sharp's `RichInlines.cs`.  A plain-text offset counts each run's
//! characters and each atom as one (see the module header in `mod.rs` on
//! characters versus UTF-16 units).
//!
//! C#'s `ArgumentOutOfRangeException` becomes [`OffsetOutOfRange`]; C#'s
//! negative-offset case cannot arise with `usize` offsets.

use std::fmt;

use super::inline::RichInline;

/// An offset or inline index outside its list (C#'s
/// `ArgumentOutOfRangeException`), naming the argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffsetOutOfRange(pub &'static str);

impl fmt::Display for OffsetOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is out of range", self.0)
    }
}

impl std::error::Error for OffsetOutOfRange {}

/// The plain-text length of the inlines: the largest valid caret offset.
pub fn length(inlines: &[RichInline]) -> usize {
    inlines.iter().map(RichInline::len).sum()
}

/// Finds the inline holding a plain-text offset, as `(inline index, offset in
/// inline)`.  An offset on the boundary between two inlines is ambiguous: by
/// default it resolves to the end of the earlier one (where typed text
/// inherits its style); with `prefer_next` it resolves to the start of the
/// later one.  Empty runs are skipped: offset 0 resolves to the start of the
/// first non-empty inline (so not inline 0 after an empty leading run), and the
/// end resolves into the last inline, even an empty trailing run.  An empty
/// list gives `(0, 0)` with index equal to the count.  Errors when offset is
/// past the length.
///
/// Because the default looks backwards, an offset just after an atom resolves
/// to the atom's end: an edit op choosing the style for typed text must take it
/// from the nearest run, not the atom.
pub fn locate(
    inlines: &[RichInline],
    offset: usize,
    prefer_next: bool,
) -> Result<(usize, usize), OffsetOutOfRange> {
    if offset > length(inlines) {
        return Err(OffsetOutOfRange("offset"));
    }
    let mut start = 0;
    for (i, inline) in inlines.iter().enumerate() {
        let end = start + inline.len();
        let inside = if prefer_next || offset == 0 {
            offset < end
        } else {
            offset <= end
        };
        if inside {
            return Ok((i, offset - start));
        }
        start = end;
    }
    // Only reached at the very end (prefer_next, or trailing empty runs);
    // there is no later inline to prefer.
    match inlines.last() {
        None => Ok((0, 0)),
        Some(last) => Ok((inlines.len() - 1, last.len())),
    }
}

/// The plain-text offset of a position inside one inline: the inverse of
/// [`locate`].  `inline_index` may equal the count (the end) only with
/// `offset_in_inline` 0; anything else out of range errors.
pub fn offset_of(
    inlines: &[RichInline],
    inline_index: usize,
    offset_in_inline: usize,
) -> Result<usize, OffsetOutOfRange> {
    if inline_index > inlines.len() {
        return Err(OffsetOutOfRange("inline_index"));
    }
    let inline_length = inlines.get(inline_index).map_or(0, RichInline::len);
    if offset_in_inline > inline_length {
        return Err(OffsetOutOfRange("offset_in_inline"));
    }
    Ok(length(&inlines[..inline_index]) + offset_in_inline)
}

/// Makes `offset` fall on an inline boundary, splitting the run it lands in
/// when needed, and returns the index of the first inline at or after it (the
/// count when offset is the end).  Edit ops call this at both ends of a range
/// so they can restyle or remove whole inlines.
pub fn split_at(inlines: &mut Vec<RichInline>, offset: usize) -> Result<usize, OffsetOutOfRange> {
    let (index, in_inline) = locate(inlines, offset, true)?;
    if index == inlines.len() || in_inline == 0 {
        return Ok(index);
    }
    if in_inline == inlines[index].len() {
        return Ok(index + 1);
    }
    // Atoms are length 1, so a strictly interior offset is always inside a run.
    let inline = &mut inlines[index];
    let tail = match inline.as_run_mut() {
        Some(run) => {
            let byte = char_to_byte(&run.text, in_inline);
            run.text.split_off(byte)
        }
        None => return Ok(index + 1),
    };
    let tail = inline.with_text(tail);
    inlines.insert(index + 1, tail);
    Ok(index + 1)
}

/// Joins neighbouring runs of identical style and drops empty runs, so edits
/// never leave fragments that would write as needless delimiter pairs
/// (`**a****b**`).
pub fn merge_adjacent(inlines: &mut Vec<RichInline>) {
    inlines.retain(|inline| !(inline.is_run() && inline.is_empty()));
    let mut i = inlines.len();
    while i > 1 {
        i -= 1;
        if inlines[i - 1].is_run()
            && inlines[i].is_run()
            && inlines[i - 1].has_same_style(&inlines[i])
        {
            let current = inlines.remove(i);
            if let (Some(previous), Some(current)) = (inlines[i - 1].as_run_mut(), current.as_run())
            {
                previous.text.push_str(&current.text);
            }
        }
    }
}

/// The byte index of character `chars` in `text` (its length past the end).
pub fn char_to_byte(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(byte, _)| byte)
}
