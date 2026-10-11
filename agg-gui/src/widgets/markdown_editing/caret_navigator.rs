//! Where the caret keys take the caret in a laid-out rich document — a port
//! of agg-sharp's `RichCaretNavigator.cs` (`RichCaretNavigator`,
//! `RichCaretTarget`).
//!
//! Character steps use the layout's caret stops, so an emoji or combining
//! sequence is one step exactly as it draws; up/down go through the lines as
//! drawn (wrapped lines, then the next table row or block) at a remembered x.
//! A "container" below is what a position's offset counts through: a block,
//! or one table cell.  Each function returns `Ok(None)` when there is nowhere
//! to go (Left at the document's start).
//!
//! C# took the edit widget; here each function takes the document and the
//! [`RichDocumentView`] holding its layouts (what C# read through the widget).
//! A position the document does not have is a [`RichEditError`] (C#'s index
//! exceptions); offsets count chars.
//!
//! C# bugs not ported (pinned by
//! `rust_only_caret_navigator_tests::rust_only_empty_table_row_is_stepped_over`):
//! - Left from the first cell of a row after a row with no cells, and Up/Down
//!   into such a row, indexed column -1 and threw; here that row is stepped
//!   over to the block before or after the table.

use crate::geometry::{Point, Rect};
use crate::widgets::text_caret_navigation::{index_of_next_token, index_of_previous_token};

use super::block::RichBlockKind;
use super::block_layout::{find_stop, hit_test_lines, RichBlockGeometry};
use super::block_painter::RichLayout;
use super::document::{DocPosition, RichDocument};
use super::document_view::RichDocumentView;
use super::edit_error::RichEditError;
use super::edit_operations::end_of;
use super::inline::RichInlineContent;
use super::layout_line::{RichCaret, RichLayoutLine};

/// A caret the keys move to: a position and which side of a wrap it shows on
/// (C#'s `RichCaretTarget`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RichCaretTarget {
    /// Where the caret goes.
    pub position: DocPosition,
    /// Whether it shows at the end of the line before a wrap.
    pub at_line_end: bool,
}

impl RichCaretTarget {
    fn at(position: DocPosition) -> Self {
        Self {
            position,
            at_line_end: false,
        }
    }
}

type Step = Result<Option<RichCaretTarget>, RichEditError>;

/// The atom placeholder C# counted as one character.
const OBJECT_REPLACEMENT: char = '\u{FFFC}';

fn with_offset(position: DocPosition, offset: usize) -> DocPosition {
    DocPosition { offset, ..position }
}

/// The block, row and column must exist and the offset be inside the
/// container.
fn check(document: &RichDocument, position: DocPosition) -> Result<(), RichEditError> {
    let out = Err(RichEditError::PositionOutOfRange(position));
    let Some(block) = document.blocks.get(position.block_index) else {
        return out;
    };
    if block.kind == RichBlockKind::Table
        && block
            .table_rows
            .get(position.row)
            .map_or(true, |row| position.column >= row.len())
    {
        return out;
    }
    if position.offset > document.text_length(position) {
        return out;
    }
    Ok(())
}

/// Right: the next stop in the container, else the start of the next cell or
/// block.
pub fn next(view: &RichDocumentView, document: &RichDocument, position: DocPosition) -> Step {
    check(document, position)?;
    if let Some(offset) = stop_offsets(view, document, position)
        .into_iter()
        .find(|&o| o > position.offset)
    {
        return Ok(Some(RichCaretTarget::at(with_offset(position, offset))));
    }
    Ok(next_container(document, position))
}

/// Left: the stop before, else the end of the previous cell or block.
pub fn previous(view: &RichDocumentView, document: &RichDocument, position: DocPosition) -> Step {
    check(document, position)?;
    if let Some(offset) = stop_offsets(view, document, position)
        .into_iter()
        .rev()
        .find(|&o| o < position.offset)
    {
        return Ok(Some(RichCaretTarget::at(with_offset(position, offset))));
    }
    Ok(previous_container(document, position))
}

/// A word step right (Alt on mac, Ctrl elsewhere) with the plain text field's
/// word rule; at the container's end it steps into the next one, as Right
/// does.
pub fn next_word(document: &RichDocument, position: DocPosition) -> Step {
    check(document, position)?;
    let text = text_of(document, position);
    if position.offset >= text.chars().count() {
        return Ok(next_container(document, position));
    }
    let offset = index_of_next_token(&text, position.offset);
    Ok(Some(RichCaretTarget::at(with_offset(position, offset))))
}

/// A word step left; at the container's start it steps to the end of the
/// previous one.
pub fn previous_word(document: &RichDocument, position: DocPosition) -> Step {
    check(document, position)?;
    if position.offset == 0 {
        return Ok(previous_container(document, position));
    }
    let text = text_of(document, position);
    let offset = index_of_previous_token(&text, position.offset.min(text.chars().count()));
    Ok(Some(RichCaretTarget::at(with_offset(position, offset))))
}

/// Home or End: the first or last stop of the line the caret is drawn on.
/// End of a line that wrapped with no space lands on its end-of-line stop
/// (`at_line_end`), so the caret stays on that line.
pub fn line_edge(
    view: &RichDocumentView,
    document: &RichDocument,
    position: DocPosition,
    at_line_end: bool,
    end: bool,
) -> Result<RichCaretTarget, RichEditError> {
    check(document, position)?;
    let length = document.text_length(position);
    let Some(lines) = lines_of(view, document, position) else {
        let offset = if end { length } else { 0 };
        return Ok(RichCaretTarget::at(with_offset(position, offset)));
    };

    let (line, _) = find_stop(lines, length, position.offset, at_line_end)
        .map_err(|_| RichEditError::PositionOutOfRange(position))?;
    let mut stops = line.caret_stops.clone();
    if stops.is_empty() {
        return Ok(RichCaretTarget {
            position,
            at_line_end,
        });
    }

    // C#'s OrderBy(X).ThenBy(Offset), a stable sort.
    stops.sort_by(|a, b| {
        a.x.total_cmp(&b.x)
            .then(a.caret.offset.cmp(&b.caret.offset))
    });
    let edge = if end {
        stops[stops.len() - 1]
    } else {
        stops[0]
    };
    Ok(RichCaretTarget {
        position: with_offset(position, edge.caret.offset),
        at_line_end: edge.caret.at_line_end,
    })
}

/// One drawn line up (`direction` -1) or down (+1), nearest `x`: the next
/// line of the same block or cell, else the same column of the next table
/// row, else the nearest line of the next block.  Past the first or last line
/// it goes to the document's start or end, as text editors do.  `Ok(None)`
/// before layout.
pub fn vertical(
    view: &RichDocumentView,
    document: &RichDocument,
    position: DocPosition,
    at_line_end: bool,
    x: f64,
    direction: i32,
) -> Step {
    check(document, position)?;
    let Some(lines) = lines_of(view, document, position) else {
        return Ok(None);
    };

    let length = document.text_length(position);
    let (line, _) = find_stop(lines, length, position.offset, at_line_end)
        .map_err(|_| RichEditError::PositionOutOfRange(position))?;
    let index = lines
        .iter()
        .position(|l| std::ptr::eq(l, line))
        .unwrap_or(0) as i64;
    let mut i = index + i64::from(direction);
    while i >= 0 && (i as usize) < lines.len() {
        let candidate = &lines[i as usize];
        // A line holding only the middle of a split atom has no stop; step
        // over it.
        if !candidate.caret_stops.is_empty() {
            let y = (candidate.top + candidate.bottom) / 2.0;
            let hit = hit_test_lines(lines, Point::new(x, y));
            return Ok(Some(RichCaretTarget {
                position: with_offset(position, hit.offset),
                at_line_end: hit.at_line_end,
            }));
        }
        i += i64::from(direction);
    }

    // A point far above or below a layout hits its first or last line (or
    // table row).
    let edge_y = if direction < 0 { -1e9 } else { 1e9 };
    let block = &document.blocks[position.block_index];
    let row = position.row as i64 + i64::from(direction);
    if block.kind == RichBlockKind::Table && row >= 0 && (row as usize) < block.table_rows.len() {
        let row = row as usize;
        if let (Some(RichLayout::Table(table)), Some(last)) = (
            view.layout_of(position.block_index),
            block.table_rows[row].len().checked_sub(1),
        ) {
            let column = position.column.min(last);
            if let Ok(cell) = table.cell(row, column) {
                let hit = cell.hit_test(Point::new(x, edge_y));
                return Ok(Some(RichCaretTarget {
                    position: DocPosition::in_cell(position.block_index, hit.offset, row, column),
                    at_line_end: hit.at_line_end,
                }));
            }
        }
    }

    let next = position.block_index as i64 + i64::from(direction);
    if next < 0 {
        return Ok(Some(RichCaretTarget::at(DocPosition::new(0, 0))));
    }
    let next = next as usize;
    if next >= document.blocks.len() {
        let last = document.blocks.len() - 1;
        return Ok(Some(RichCaretTarget::at(end_of(document, last))));
    }

    // Going up lands on the next block's last line, going down on its first.
    let Some(layout) = view.layout_of(next) else {
        return Ok(None);
    };
    let hit = layout.hit_test(Point::new(x, edge_y));
    Ok(Some(RichCaretTarget {
        position: DocPosition::in_cell(next, hit.offset, hit.row, hit.column),
        at_line_end: hit.at_line_end,
    }))
}

/// The caret's box in [`RichDocumentView`] coordinates, or `None` before
/// layout.
pub fn caret_bounds(
    view: &RichDocumentView,
    document: &RichDocument,
    position: DocPosition,
    at_line_end: bool,
) -> Result<Option<Rect>, RichEditError> {
    check(document, position)?;
    if !has_layout(view, document, position.block_index) {
        return Ok(None);
    }
    let (Some(layout), Some(origin), Some(style)) = (
        view.layout_of(position.block_index),
        view.origin_of(position.block_index),
        view.style(),
    ) else {
        return Ok(None);
    };
    let caret = RichCaret {
        offset: position.offset,
        at_line_end,
        row: position.row,
        column: position.column,
    };
    let rect = layout
        .caret_rect(caret, style.caret_width())
        .map_err(|_| RichEditError::PositionOutOfRange(position))?;
    Ok(Some(Rect::new(
        rect.x,
        rect.y + origin,
        rect.width,
        rect.height,
    )))
}

/// The start of the next table cell in reading order, else of the next block.
fn next_container(document: &RichDocument, position: DocPosition) -> Option<RichCaretTarget> {
    let block = &document.blocks[position.block_index];
    if block.kind == RichBlockKind::Table && !block.table_rows.is_empty() {
        let mut row = position.row;
        let mut column = position.column + 1;
        if column >= block.table_rows[row].len() {
            row += 1;
            column = 0;
        }

        if row < block.table_rows.len() {
            return Some(RichCaretTarget::at(DocPosition::in_cell(
                position.block_index,
                0,
                row,
                column,
            )));
        }
    }

    (position.block_index + 1 < document.blocks.len())
        .then(|| RichCaretTarget::at(DocPosition::new(position.block_index + 1, 0)))
}

/// The end of the previous table cell in reading order, else of the previous
/// block.
fn previous_container(document: &RichDocument, position: DocPosition) -> Option<RichCaretTarget> {
    let block = &document.blocks[position.block_index];
    if block.kind == RichBlockKind::Table
        && !block.table_rows.is_empty()
        && (position.row > 0 || position.column > 0)
    {
        let (row, column) = if position.column > 0 {
            (position.row, Some(position.column - 1))
        } else {
            let row = position.row - 1;
            (row, block.table_rows[row].len().checked_sub(1))
        };
        if let Some(column) = column {
            return Some(RichCaretTarget::at(DocPosition::in_cell(
                position.block_index,
                block.text_length(row, column),
                row,
                column,
            )));
        }
    }

    (position.block_index > 0)
        .then(|| RichCaretTarget::at(end_of(document, position.block_index - 1)))
}

/// The container's caret offsets in order, from its layout; without a layout
/// (no width yet) every offset.
fn stop_offsets(
    view: &RichDocumentView,
    document: &RichDocument,
    position: DocPosition,
) -> Vec<usize> {
    let Some(lines) = lines_of(view, document, position) else {
        return (0..=document.text_length(position)).collect();
    };
    let mut offsets: Vec<usize> = lines
        .iter()
        .flat_map(|l| l.caret_stops.iter().map(|s| s.caret.offset))
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

fn lines_of<'a>(
    view: &'a RichDocumentView,
    document: &RichDocument,
    position: DocPosition,
) -> Option<&'a [RichLayoutLine]> {
    if !has_layout(view, document, position.block_index) {
        return None;
    }

    match view.layout_of(position.block_index)? {
        RichLayout::Text(block) => (!block.lines.is_empty()).then_some(&block.lines[..]),
        RichLayout::Table(table) => table
            .cell(position.row, position.column)
            .ok()
            .map(|cell| &cell.lines[..])
            .filter(|lines| !lines.is_empty()),
    }
}

// Before the widget has a width nothing is laid out, and a block an edit just
// added has no layout until the relayout that follows it.
fn has_layout(view: &RichDocumentView, document: &RichDocument, block_index: usize) -> bool {
    view.style().is_some()
        && view.laid_out_count() == document.blocks.len()
        && block_index < document.blocks.len()
}

/// The container's caret text for word stepping: one placeholder character
/// per atom, as offsets count.
fn text_of(document: &RichDocument, position: DocPosition) -> String {
    let block = &document.blocks[position.block_index];
    match block.kind {
        RichBlockKind::CodeBlock => block.code_text.clone(),
        RichBlockKind::Raw => OBJECT_REPLACEMENT.to_string(),
        _ => block
            .inlines_at(position.row, position.column)
            .map(|inlines| {
                inlines
                    .iter()
                    .map(|inline| match &inline.content {
                        RichInlineContent::Run(run) => run.text.clone(),
                        _ => OBJECT_REPLACEMENT.to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}
