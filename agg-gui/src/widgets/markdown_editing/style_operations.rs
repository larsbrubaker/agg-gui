//! Character style and link edits over a selection.  Ports the operations of
//! agg-sharp's `RichStyleOperations.cs`; the style types they use are
//! [`super::inline_style`].
//!
//! A range may cross blocks: text blocks take part, Raw and code blocks are
//! skipped (they have no character styles).  A range within one table covers
//! the selected text of each cell from its start cell to its end cell in
//! reading order (the cell walk of
//! [`super::table_code_operations::delete_range`]); a range spanning several
//! blocks skips tables, because a table cell is not part of the reading-order
//! text a multi-block selection covers.  Atoms (images, breaks) in the range
//! take bold, italic, strike and links like text, so a bolded selection
//! containing an image writes as one bold span; they never take inline code.
//! Ops split runs at the range edges, merge equal neighbours afterwards (no
//! `**a****b**` fragments) and mark changed blocks dirty.
//!
//! A range end the document does not have is a [`RichEditError`] where C#'s
//! indexers throw; a caret on a block that does not exist reads as no style,
//! as in C#.

use super::document::{DocPosition, RichDocument};
use super::edit_error::{check_position, check_range, RichEditError};
use super::edit_operations;
use super::inline::RichInline;
use super::inline_style::{set_style, style_of, RichInlineStyle, RichPendingStyle, RichStyleState};
use super::inlines;
use super::selection::RichSelection;
use super::RichBlockKind;

/// Applies `style` to every character in the selection, or removes it when
/// every character already has it (the word processor rule a first-time user
/// expects).  Atoms count and change for Bold, Italic and Strike; for Code they
/// neither count nor change.  A collapsed selection changes nothing; use
/// [`RichPendingStyle`].  Returns the selection unchanged, so the same text
/// stays selected for the next toggle.
pub fn toggle_style(
    document: &mut RichDocument,
    selection: RichSelection,
    style: RichInlineStyle,
) -> Result<RichSelection, RichEditError> {
    let segments = segments(document, selection)?;
    let state = measure(document, &segments);
    if state.inlines == 0 || style.is_none() {
        return Ok(selection);
    }

    let apply = !state.all.contains(style);
    for segment in &segments {
        let changed = edit_segment(document, segment, |inline| set_style(inline, style, apply));
        finish(document, segment, changed);
    }

    Ok(selection)
}

/// The style state over a selection for the toolbar.  For a collapsed caret it
/// is the style typed text would get there ([`typing_style`]) with `pending`
/// applied, and the link the caret is in or touches.  Never changes the
/// document.
pub fn style_at(
    document: &RichDocument,
    selection: RichSelection,
    pending: RichPendingStyle,
) -> Result<RichStyleState, RichEditError> {
    if selection.is_empty() {
        let Some(typing) = typing_style(document, selection.caret) else {
            return Ok(RichStyleState::default());
        };

        check_caret(document, selection.caret)?;
        let typing = pending.apply_pending(&typing);
        let flags = style_of(&typing);
        let link = inlines_at(document, selection.caret)
            .and_then(|list| link_extent(list, selection.caret.offset).2.cloned());
        return Ok(RichStyleState {
            all: flags,
            any: flags,
            link_url: link.as_ref().and_then(|l| l.link_url.clone()),
            link_title: link.and_then(|l| l.link_title),
        });
    }

    let state = measure(document, &segments(document, selection)?);
    Ok(RichStyleState {
        all: state.all,
        any: state.any,
        link_url: state.link.as_ref().and_then(|l| l.link_url.clone()),
        link_title: state.link.and_then(|l| l.link_title),
    })
}

/// The style text typed at `position` would get — exactly `insert_text`'s
/// rule (inherit from the nearest run before the caret; a link or inline code
/// stops at its end), applied to a table cell too.  Plain in an empty document
/// (typing there makes a paragraph).  `None` in a code block or Raw block,
/// which have no character style, and on a block the document does not have.
pub fn typing_style(document: &RichDocument, position: DocPosition) -> Option<RichInline> {
    if document.blocks.is_empty() {
        return Some(RichInline::run(""));
    }

    let list = inlines_at(document, position)?;
    // The one rule insert_text uses for paragraphs and table cells alike, handed back as a copy the caller can change.
    Some(edit_operations::style_at(list, position.offset).with_text(""))
}

/// Makes the selection a link to `url`.  A range: its text and atoms become the
/// link's content, so an image in the range stays inside the one link.  A caret
/// inside or at the edge of a link: that whole link's target changes, keeping
/// its title unless `title` is given.  A caret elsewhere (an empty document
/// included): the url is inserted as linked text — trimmed and on one line,
/// never inline code — and the caret goes after it.  The link is always written
/// inline (`[text](url)`): any reference label is dropped, since the user typed
/// a target, not a label.  Returns the selection to show next.
pub fn set_link(
    document: &mut RichDocument,
    selection: RichSelection,
    url: &str,
    title: Option<&str>,
) -> Result<RichSelection, RichEditError> {
    if url.is_empty() {
        return Ok(selection);
    }

    let mut title = title.map(str::to_string);
    let segments = if selection.is_empty() {
        let caret = selection.caret;
        let extent = if document.blocks.is_empty() {
            None
        } else {
            let Some(list) = inlines_at(document, caret) else {
                return Ok(selection);
            };
            check_caret(document, caret)?;
            let (from, to, link) = link_extent(list, caret.offset);
            link.map(|link| (from, to, link.link_title.clone()))
        };

        let Some((from, to, link_title)) = extent else {
            return insert_link_text(document, caret, url, title.as_deref());
        };

        if title.is_none() {
            title = link_title;
        }
        vec![Segment::at(caret, from, to)]
    } else {
        segments(document, selection)?
    };

    for segment in &segments {
        let changed = edit_segment(document, segment, |inline| {
            set_link_on(inline, Some(url), title.as_deref())
        });
        finish(document, segment, changed);
    }

    Ok(selection)
}

/// Removes links from the selection, keeping their text and other styles.  A
/// caret inside or at the edge of a link removes that whole link; a range
/// unlinks only the text it covers.
pub fn remove_link(
    document: &mut RichDocument,
    selection: RichSelection,
) -> Result<RichSelection, RichEditError> {
    let segments = if selection.is_empty() {
        let caret = selection.caret;
        let Some(list) = inlines_at(document, caret) else {
            return Ok(selection);
        };
        check_caret(document, caret)?;
        let (from, to, link) = link_extent(list, caret.offset);
        if link.is_none() {
            return Ok(selection);
        }

        vec![Segment::at(caret, from, to)]
    } else {
        segments(document, selection)?
    };

    for segment in &segments {
        let changed = edit_segment(document, segment, |inline| set_link_on(inline, None, None));
        finish(document, segment, changed);
    }

    Ok(selection)
}

/// A caret on an existing block must be a position the block has (C#'s
/// `Locate` throws on an offset past the end).
fn check_caret(document: &RichDocument, caret: DocPosition) -> Result<(), RichEditError> {
    check_position(document, caret)
}

/// Sets (or, with no url, clears) the inline's link.  Returns true when
/// anything changed.
fn set_link_on(inline: &mut RichInline, url: Option<&str>, title: Option<&str>) -> bool {
    let title = if url.is_none() { None } else { title };
    let changed = inline.link_url.as_deref() != url
        || inline.link_title.as_deref() != title
        || inline.link_label.is_some();
    inline.link_url = url.map(str::to_string);
    inline.link_title = title.map(str::to_string);
    inline.link_label = None;
    changed
}

/// Inserts the url as linked text at a caret that is not on a link.  The shown
/// text is the url trimmed with control characters (a pasted newline or tab)
/// made spaces, so it stays one line; the link target is the url as given.  The
/// insertion goes through `insert_text`, which also makes the paragraph an
/// empty document needs.
fn insert_link_text(
    document: &mut RichDocument,
    caret: DocPosition,
    url: &str,
    title: Option<&str>,
) -> Result<RichSelection, RichEditError> {
    // C#'s string.Trim and char.IsControl, per character.
    let shown: String = url
        .trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if shown.is_empty() {
        return Ok(RichSelection::at(caret));
    }

    let mut template = typing_style(document, caret).unwrap_or_else(|| RichInline::run(""));
    // Inside inline code the typing style is code, but a link typed there is a new piece of prose.
    if let Some(run) = template.as_run_mut() {
        run.code = false;
    }
    set_link_on(&mut template, Some(url), title);
    edit_operations::insert_text(document, caret, &shown, Some(&template))
}

/// The inlines a caret is in: a text block's, or the table cell's.  `None` for
/// code and Raw blocks, a block the document does not have, or a cell outside
/// the table.
fn inlines_at(document: &RichDocument, position: DocPosition) -> Option<&Vec<RichInline>> {
    let block = document.blocks.get(position.block_index)?;
    if block.kind == RichBlockKind::Table {
        return block
            .table_rows
            .get(position.row)?
            .get(position.column)
            .map(|cell| &cell.inlines);
    }

    block.inlines_at(0, 0)
}

/// The offsets of the link a caret is in or touches, and one of its inlines:
/// the inline before the caret is preferred (so a caret just after a link's
/// text finds it), then the one after.  `(offset, offset, None)` when there is
/// no link.  The offset must be within the inlines.
fn link_extent(list: &[RichInline], offset: usize) -> (usize, usize, Option<&RichInline>) {
    let mut link_index = None;
    if offset > 0 {
        if let Ok((before, _)) = inlines::locate(list, offset, false) {
            if list.get(before).is_some_and(is_link) {
                link_index = Some(before);
            }
        }
    }

    if link_index.is_none() && offset < inlines::length(list) {
        if let Ok((after, _)) = inlines::locate(list, offset, true) {
            if list.get(after).is_some_and(is_link) {
                link_index = Some(after);
            }
        }
    }

    let Some(link_index) = link_index else {
        return (offset, offset, None);
    };

    // One link can span several inlines (a bold word or an image inside it), so grow over neighbours with the
    // same target.
    let link = &list[link_index];
    let mut first = link_index;
    while first > 0 && same_link(&list[first - 1], link) {
        first -= 1;
    }

    let mut last = link_index;
    while last + 1 < list.len() && same_link(&list[last + 1], link) {
        last += 1;
    }

    let from = inlines::length(&list[..first]);
    let to = inlines::length(&list[..=last]);
    (from, to, Some(link))
}

fn is_link(inline: &RichInline) -> bool {
    inline.link_url.is_some() || inline.link_label.is_some()
}

fn same_link(a: &RichInline, b: &RichInline) -> bool {
    a.link_url == b.link_url && a.link_title == b.link_title && a.link_label == b.link_label
}

/// One contiguous stretch of a selection inside one inline list: a block's own
/// inlines, or (in a table) one cell's.
#[derive(Clone, Copy, Debug)]
struct Segment {
    block: usize,
    row: usize,
    column: usize,
    from: usize,
    to: usize,
}

impl Segment {
    /// The stretch `from..to` of the inline list a caret is in.
    fn at(caret: DocPosition, from: usize, to: usize) -> Self {
        Self {
            block: caret.block_index,
            row: caret.row,
            column: caret.column,
            from,
            to,
        }
    }
}

fn segment_inlines<'a>(document: &'a RichDocument, segment: &Segment) -> &'a [RichInline] {
    let block = &document.blocks[segment.block];
    if block.kind == RichBlockKind::Table {
        &block.table_rows[segment.row][segment.column].inlines
    } else {
        &block.inlines
    }
}

fn segment_inlines_mut<'a>(
    document: &'a mut RichDocument,
    segment: &Segment,
) -> &'a mut Vec<RichInline> {
    let block = &mut document.blocks[segment.block];
    if block.kind == RichBlockKind::Table {
        &mut block.table_rows[segment.row][segment.column].inlines
    } else {
        &mut block.inlines
    }
}

/// Breaks a selection into per-block (and, in a table, per-cell) stretches,
/// skipping blocks with no character styles and tables a multi-block selection
/// crosses.  Empty stretches are left out.
fn segments(
    document: &RichDocument,
    selection: RichSelection,
) -> Result<Vec<Segment>, RichEditError> {
    let mut segments = Vec::new();
    let start = selection.start();
    let end = selection.end();
    if start == end {
        return Ok(segments);
    }

    check_range(document, start, end)?;
    if start.block_index == end.block_index {
        let block = &document.blocks[start.block_index];
        if block.kind == RichBlockKind::Table {
            // The same reading-order cell walk as table_code_operations::delete_range.
            let columns = block.table_rows[0].len();
            for cell in start.row * columns + start.column..=end.row * columns + end.column {
                let row = cell / columns;
                let column = cell % columns;
                let from = if row == start.row && column == start.column {
                    start.offset
                } else {
                    0
                };
                let to = if row == end.row && column == end.column {
                    end.offset
                } else {
                    block.text_length(row, column)
                };
                if from < to {
                    segments.push(Segment {
                        block: start.block_index,
                        row,
                        column,
                        from,
                        to,
                    });
                }
            }

            return Ok(segments);
        }
    }

    for i in start.block_index..=end.block_index {
        let block = &document.blocks[i];
        if !block.is_text_block() {
            continue;
        }

        let from = if i == start.block_index {
            start.offset
        } else {
            0
        };
        let to = if i == end.block_index {
            end.offset
        } else {
            block.text_length(0, 0)
        };
        if from < to {
            segments.push(Segment {
                block: i,
                row: 0,
                column: 0,
                from,
                to,
            });
        }
    }

    Ok(segments)
}

#[derive(Default)]
struct Measurement {
    inlines: usize,
    all: RichInlineStyle,
    any: RichInlineStyle,
    link: Option<RichInline>,
}

/// The flags common to and present in the selection's text and atoms, and the
/// link when all of it is one link.  An atom counts for Bold, Italic and Strike
/// but not Code (it can never be code, so it neither clears Code's `all` nor
/// sets its `any`).  Reads without splitting, so `style_at` never changes the
/// document.
fn measure(document: &RichDocument, segments: &[Segment]) -> Measurement {
    let mut count = 0;
    let mut any_run = false;
    let mut all = RichInlineStyle::ALL;
    let mut any = RichInlineStyle::NONE;
    let mut link: Option<&RichInline> = None;
    let mut one_link = segments.len() == 1;
    for segment in segments {
        let mut start = 0;
        for inline in segment_inlines(document, segment) {
            let end = start + inline.len();
            let covered = end.min(segment.to) as isize - start.max(segment.from) as isize;
            start = end;
            if covered <= 0 {
                continue;
            }

            count += 1;
            let style = style_of(inline);
            if inline.is_run() {
                any_run = true;
                all = all & style;
            } else {
                all = all & (style | RichInlineStyle::CODE);
            }

            any = any | style;
            if !is_link(inline) || link.is_some_and(|link| !same_link(link, inline)) {
                one_link = false;
            }

            link = link.or(Some(inline));
        }
    }

    if count == 0 {
        return Measurement::default();
    }

    if !any_run {
        // Only atoms: none of them is code, so Code is not common to the selection.
        all = all & !RichInlineStyle::CODE;
    }

    Measurement {
        inlines: count,
        all,
        any,
        link: if one_link { link.cloned() } else { None },
    }
}

/// Splits the segment's runs at its edges, applies `edit` to each inline (run
/// or atom) inside it, and returns whether any edit changed something.
fn edit_segment(
    document: &mut RichDocument,
    segment: &Segment,
    mut edit: impl FnMut(&mut RichInline) -> bool,
) -> bool {
    let list = segment_inlines_mut(document, segment);
    // Both offsets were checked against this list, and the second split lands after the first.
    let (Ok(first), Ok(last)) = (
        inlines::split_at(list, segment.from),
        inlines::split_at(list, segment.to),
    ) else {
        return false;
    };
    let mut changed = false;
    for inline in &mut list[first..last] {
        changed |= edit(inline);
    }
    changed
}

/// Merges the runs split at the segment's edges back together, and marks the
/// block dirty only when a style actually changed, so an op that changed
/// nothing leaves the block writing its original markdown.
fn finish(document: &mut RichDocument, segment: &Segment, changed: bool) {
    inlines::merge_adjacent(segment_inlines_mut(document, segment));
    if changed {
        document.blocks[segment.block].dirty = true;
    }
}
