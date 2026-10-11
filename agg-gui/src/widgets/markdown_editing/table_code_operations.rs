//! Edits inside a code block or a table: typing, Backspace/Delete, Enter and
//! range deletes that stay within one block ([`super::edit_operations`]
//! routes those here), plus the table structure operations behind
//! Tab/Shift-Tab and the toolbar's row, column and insert-table commands.
//! Ports agg-sharp's `RichTableCodeOperations.cs`.  Every operation mutates
//! the document in place, marks the block it changes dirty and returns the
//! selection to show next.  A table caret is [`DocPosition`] row/column/offset,
//! the offset counting through that cell's inlines.
//!
//! Code blocks hold plain text: Enter inserts a newline (the block never
//! splits), and Enter on the empty last line leaves the block, the way a
//! first-time user gets out of it without knowing about fences.  Table cells
//! never take a line break (markdown has none inside a cell): Enter moves down
//! a row instead, and Backspace/Delete never cross a cell's edge, so the
//! grid's shape changes only through the row and column operations.
//!
//! The table operations panic when the position's block is not a table (C#'s
//! `InvalidOperationException`), and [`delete_range`] when its positions are
//! in different blocks (C#'s `ArgumentException`): both are caller errors.

use super::block::{RichBlock, RichBlockKind, RichTableCell};
use super::document::{DocPosition, RichDocument};
use super::edit_operations::{self as edit, blank_line};
use super::inline::RichInline;
use super::inlines;
use super::selection::RichSelection;

/// What Tab types in a code block: four spaces rather than a tab character,
/// the indentation markdown itself counts in and most published code uses, so
/// the code reads the same in every viewer.
pub const CODE_TAB: &str = "    ";

/// Types `text` at `position` in a code block or table cell.  In code every
/// line break is kept as `\n`.  In a cell the text takes `style`, or the same
/// style rule as text blocks (the run before the caret, a link or inline code
/// stopping at its end); each line break acts as Enter ([`split_block`]), so
/// pasted lines fill down the column.
pub fn insert_text(
    document: &mut RichDocument,
    position: DocPosition,
    text: &str,
    style: Option<&RichInline>,
) -> RichSelection {
    let index = position.block_index;
    if text.is_empty() {
        return RichSelection::at(position);
    }

    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let block = &mut document.blocks[index];
    if block.kind == RichBlockKind::CodeBlock {
        let byte = inlines::char_to_byte(&block.code_text, position.offset);
        block.code_text.insert_str(byte, &text);
        block.dirty = true;
        let caret = DocPosition {
            offset: position.offset + text.chars().count(),
            ..position
        };
        return RichSelection::at(caret);
    }

    require_table(block);
    let style = match style {
        Some(style) => style.clone(),
        None => edit::style_at(
            cell_inlines(block, position.row, position.column),
            position.offset,
        ),
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let mut caret = position;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            caret = split_block(document, caret).caret;
            if caret.block_index != index {
                // Enter on the last row left the table; the rest is ordinary text below it.
                let rest = lines[i..].join("\n");
                return edit::insert_text(document, caret, &rest, Some(&style));
            }
        }

        if !line.is_empty() {
            let block = &mut document.blocks[index];
            let cell = cell_inlines_mut(block, caret.row, caret.column);
            let at = edit::split_at(cell, caret.offset);
            cell.insert(at, style.with_text(*line));
            inlines::merge_adjacent(cell);
            block.dirty = true;
            caret.offset += line.chars().count();
        }
    }

    RichSelection::at(caret)
}

/// Removes the text between two positions of one code block or table, never
/// the block itself, even when the range is all of its text
/// ([`edit::delete_selection`] removes a block selected whole).  Across table
/// cells it clears the selected text of each cell in reading order and leaves
/// the grid intact; removing rows or columns is what [`delete_row`] and
/// [`delete_column`] are for.
pub fn delete_range(document: &mut RichDocument, a: DocPosition, b: DocPosition) -> RichSelection {
    let start = a.min(b);
    let end = a.max(b);
    assert!(
        start.block_index == end.block_index,
        "Both positions must be in one block; edit_operations::delete_range handles ranges across blocks."
    );

    let block = &mut document.blocks[start.block_index];
    if block.kind == RichBlockKind::CodeBlock {
        remove_code(block, start.offset, end.offset);
        return RichSelection::at(start);
    }

    require_table(block);
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
        remove_inlines(block, row, column, from, to);
    }

    RichSelection::at(start)
}

/// Enter.  In code it inserts a newline, except on an empty last line (the
/// text ends in a newline and the caret is at its end): that is the second
/// Enter in a row, so the block drops that empty line and an empty paragraph
/// after it takes the caret.  In a table the caret moves to the end of the
/// same column one row down; from the last row it leaves the table into a new
/// empty paragraph after it, as code does, so a table at the end of a document
/// has a way out below it and a paste never runs into the block after.
pub fn split_block(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    let index = position.block_index;
    let block = &mut document.blocks[index];
    if block.kind == RichBlockKind::CodeBlock {
        if position.offset == block.code_text.chars().count() && block.code_text.ends_with('\n') {
            block.code_text.pop();
            block.dirty = true;
            return RichSelection::at(insert_paragraph_after(document, index));
        }

        return insert_text(document, position, "\n", None);
    }

    require_table(block);
    if position.row + 1 < block.table_rows.len() {
        return RichSelection::at(cell_end(block, index, position.row + 1, position.column));
    }

    RichSelection::at(insert_paragraph_after(document, index))
}

/// Backspace with a collapsed caret: removes one character (a whole emoji or
/// combining sequence) or atom.  At a cell's start nothing happens, so a cell
/// never merges into its neighbour.  At the start of a code block nothing
/// happens either, unless the block is empty: then it becomes an empty
/// paragraph, the way a novice expects to take back a code block they just
/// made.
pub fn backspace(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    backspace_changed(document, position).0
}

/// [`backspace`], also saying whether it changed the document (false at a
/// cell's start), so the editor leaves no empty undo step.
pub fn backspace_changed(
    document: &mut RichDocument,
    position: DocPosition,
) -> (RichSelection, bool) {
    let block = &mut document.blocks[position.block_index];
    if block.kind == RichBlockKind::CodeBlock {
        if position.offset > 0 {
            let previous = edit::previous_text_element(&block.code_text, position.offset);
            remove_code(block, previous, position.offset);
            let caret = DocPosition {
                offset: previous,
                ..position
            };
            return (RichSelection::at(caret), true);
        }

        if block.code_text.is_empty() {
            // Cleared so the writer and layout see a plain paragraph, with no fence or language left over.
            block.kind = RichBlockKind::Paragraph;
            block.inlines.clear();
            block.code_text.clear();
            block.code_fence.clear();
            block.code_info.clear();
            block.code_language.clear();
            block.dirty = true;
            return (RichSelection::at(position), true);
        }

        return (RichSelection::at(position), false);
    }

    require_table(block);
    if position.offset == 0 {
        return (RichSelection::at(position), false);
    }

    let stop = edit::previous_stop(
        cell_inlines(block, position.row, position.column),
        position.offset,
    );
    remove_inlines(block, position.row, position.column, stop, position.offset);
    let caret = DocPosition {
        offset: stop,
        ..position
    };
    (RichSelection::at(caret), true)
}

/// Forward Delete with a collapsed caret: removes the next character or atom;
/// at the end of a cell or a code block nothing happens.
pub fn delete(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    delete_changed(document, position).0
}

/// [`delete`], also saying whether it changed the document.
pub fn delete_changed(document: &mut RichDocument, position: DocPosition) -> (RichSelection, bool) {
    let block = &mut document.blocks[position.block_index];
    if block.kind == RichBlockKind::CodeBlock {
        let changed = position.offset < block.code_text.chars().count();
        if changed {
            let next = edit::next_text_element(&block.code_text, position.offset);
            remove_code(block, position.offset, next);
        }

        return (RichSelection::at(position), changed);
    }

    require_table(block);
    let cell = cell_inlines(block, position.row, position.column);
    let changed = position.offset < inlines::length(cell);
    if changed {
        let next = edit::next_stop(cell, position.offset);
        remove_inlines(block, position.row, position.column, position.offset, next);
    }

    (RichSelection::at(position), changed)
}

/// Tab in a table: the caret goes to the end of the next cell in reading
/// order.  Tab in the last cell adds a body row and goes to its first cell, as
/// in a word processor, so a table grows by typing.
pub fn next_cell(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    next_cell_added_row(document, position).0
}

/// [`next_cell`], also saying whether it added a row (otherwise it only moved
/// the caret).
pub fn next_cell_added_row(
    document: &mut RichDocument,
    position: DocPosition,
) -> (RichSelection, bool) {
    let mut added_row = false;
    let block = require_table(&mut document.blocks[position.block_index]);
    let columns = block.table_rows[0].len();
    let mut row = position.row;
    let mut column = position.column + 1;
    if column == columns {
        column = 0;
        row += 1;
        if row == block.table_rows.len() {
            block.table_rows.push(new_row(columns));
            block.dirty = true;
            added_row = true;
        }
    }

    (
        RichSelection::at(cell_end(block, position.block_index, row, column)),
        added_row,
    )
}

/// Shift-Tab in a table: the caret goes to the end of the previous cell; in
/// the first cell, to that cell's end.
pub fn previous_cell(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    let block = require_table(&mut document.blocks[position.block_index]);
    let columns = block.table_rows[0].len();
    let cell = (position.row * columns + position.column).saturating_sub(1);
    RichSelection::at(cell_end(
        block,
        position.block_index,
        cell / columns,
        cell % columns,
    ))
}

/// Adds an empty body row above or below the caret's row; the caret goes to
/// the new row's cell in the same column.  Markdown's header is always the
/// first row, so a row asked for above the header goes just below it instead.
pub fn insert_row(
    document: &mut RichDocument,
    position: DocPosition,
    below: bool,
) -> RichSelection {
    let block = require_table(&mut document.blocks[position.block_index]);
    let row = if below {
        position.row + 1
    } else {
        position.row
    }
    .max(1);
    let columns = block.table_rows[0].len();
    block.table_rows.insert(row, new_row(columns));
    block.dirty = true;
    RichSelection::at(DocPosition::in_cell(
        position.block_index,
        0,
        row,
        position.column,
    ))
}

/// Removes the caret's row; the caret goes to the end of the same column in
/// the row now there, or the row above when it was the last.  The header row
/// is refused (nothing changes): a markdown table cannot exist without one,
/// while a table with only a header is valid, so the last body row may go.
pub fn delete_row(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    let block = require_table(&mut document.blocks[position.block_index]);
    if position.row == 0 {
        return RichSelection::at(position);
    }

    block.table_rows.remove(position.row);
    block.dirty = true;
    let row = position.row.min(block.table_rows.len() - 1);
    RichSelection::at(cell_end(block, position.block_index, row, position.column))
}

/// Adds an empty column left or right of the caret's column, with no
/// alignment (`None`, written `---`); the caret goes to the new column's cell
/// in the same row.
pub fn insert_column(
    document: &mut RichDocument,
    position: DocPosition,
    right: bool,
) -> RichSelection {
    let block = require_table(&mut document.blocks[position.block_index]);
    let column = if right {
        position.column + 1
    } else {
        position.column
    };
    for row in &mut block.table_rows {
        row.insert(column, RichTableCell::default());
    }

    let at = column.min(block.column_alignments.len());
    block.column_alignments.insert(at, None);
    block.dirty = true;
    RichSelection::at(DocPosition::in_cell(
        position.block_index,
        0,
        position.row,
        column,
    ))
}

/// Removes the caret's column and its alignment; the caret goes to the end of
/// the cell now in that column (or the one left of it).  The only column is
/// refused (nothing changes): removing a whole table is a selection delete,
/// not a column operation.
pub fn delete_column(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    let block = require_table(&mut document.blocks[position.block_index]);
    let columns = block.table_rows[0].len();
    if columns <= 1 {
        return RichSelection::at(position);
    }

    for row in &mut block.table_rows {
        row.remove(position.column);
    }

    if position.column < block.column_alignments.len() {
        block.column_alignments.remove(position.column);
    }

    block.dirty = true;
    let column = position.column.min(columns - 2);
    RichSelection::at(cell_end(block, position.block_index, position.row, column))
}

/// Adds a new table of empty cells — a header row plus `body_rows` body rows,
/// each `columns` wide (at least one), no column alignments — after the
/// caret's block, and puts the caret in the first header cell.  When that
/// block is in a list, quote or alignment wrapper the table goes after the
/// whole wrapper: the model holds no table inside one, and a stranger in the
/// middle would split the group.
pub fn insert_table(
    document: &mut RichDocument,
    position: DocPosition,
    body_rows: usize,
    columns: usize,
) -> RichSelection {
    let columns = columns.max(1);
    let blocks = &document.blocks;
    let current = &blocks[position.block_index];
    let mut after = position.block_index;
    while after + 1 < blocks.len() && shares_group(&blocks[after + 1], current) {
        after += 1;
    }

    let mut table = RichBlock {
        separator_before: blank_line(document).to_string(),
        dirty: true,
        ..RichBlock::new(RichBlockKind::Table)
    };
    for _ in 0..=body_rows {
        table.table_rows.push(new_row(columns));
    }
    table.column_alignments = vec![None; columns];

    document.blocks.insert(after + 1, table);
    RichSelection::at(DocPosition::in_cell(after + 1, 0, 0, 0))
}

fn shares_group(block: &RichBlock, other: &RichBlock) -> bool {
    fn same<T>(a: &Option<std::rc::Rc<T>>, b: &Option<std::rc::Rc<T>>) -> bool {
        matches!((a, b), (Some(a), Some(b)) if std::rc::Rc::ptr_eq(a, b))
    }
    same(&block.list_group, &other.list_group)
        || same(&block.quote_group, &other.quote_group)
        || same(&block.align_group, &other.align_group)
}

fn require_table(block: &mut RichBlock) -> &mut RichBlock {
    assert!(
        block.kind == RichBlockKind::Table,
        "A {:?} block is not a table.",
        block.kind
    );
    block
}

fn cell_inlines(block: &RichBlock, row: usize, column: usize) -> &Vec<RichInline> {
    &block.table_rows[row][column].inlines
}

fn cell_inlines_mut(block: &mut RichBlock, row: usize, column: usize) -> &mut Vec<RichInline> {
    &mut block.table_rows[row][column].inlines
}

fn new_row(columns: usize) -> Vec<RichTableCell> {
    vec![RichTableCell::default(); columns]
}

fn cell_end(table: &RichBlock, index: usize, row: usize, column: usize) -> DocPosition {
    DocPosition::in_cell(index, table.text_length(row, column), row, column)
}

fn insert_paragraph_after(document: &mut RichDocument, index: usize) -> DocPosition {
    let paragraph = RichBlock {
        separator_before: blank_line(document).to_string(),
        dirty: true,
        ..RichBlock::new(RichBlockKind::Paragraph)
    };
    document.blocks.insert(index + 1, paragraph);
    DocPosition::new(index + 1, 0)
}

fn remove_code(block: &mut RichBlock, from: usize, to: usize) {
    if from < to {
        let start = inlines::char_to_byte(&block.code_text, from);
        let end = inlines::char_to_byte(&block.code_text, to);
        block.code_text.replace_range(start..end, "");
        block.dirty = true;
    }
}

fn remove_inlines(block: &mut RichBlock, row: usize, column: usize, from: usize, to: usize) {
    if edit::remove_inline_range(cell_inlines_mut(block, row, column), from, to) {
        block.dirty = true;
    }
}
