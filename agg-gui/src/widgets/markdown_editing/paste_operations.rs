//! The model half of copy and paste, free of any clipboard.  Ports agg-sharp's
//! `RichPasteOperations.cs`: [`slice`] cuts a selection out as a fragment
//! document, [`visible_text`] is that fragment as the plain text a reader sees,
//! [`parse_clipboard_text`] reads text from outside the editor as a fragment,
//! and [`insert_document`] (in [`super::paste_insert`]) pastes a fragment at a
//! caret the way word processors do.
//!
//! Where C# would throw on a selection the document does not have, [`slice`]
//! returns a [`RichEditError`] like the edit operations do.

use super::block::{RichBlock, RichBlockKind};
use super::document::{DocPosition, RichDocument};
use super::edit_error::{check_block, check_range, RichEditError};
use super::edit_operations::end_of;
use super::group_repair;
use super::groups::GroupRemap;
use super::inline::{InlineAtomKind, RichInline, RichInlineContent};
use super::inlines;
use super::markdown_parser::parse_with;
use super::selection::RichSelection;

pub use super::paste_insert::insert_document;

/// The most line breaks a pasted paragraph keeps as one paragraph; see
/// [`parse_clipboard_text`].
pub const MAX_PASTED_LINE_BREAKS: usize = 20;

/// A copy of the selected content as its own document.  A range inside one
/// block is a single paragraph of the selected inlines (styles and links
/// kept, the block's kind not — a few words of a heading paste as words); a
/// range inside one code block is a paragraph of its plain text.  Across
/// blocks the end blocks are cut at the selection and keep their kind, and a
/// code block, table or Raw block at an end comes whole unless the range only
/// touches it — the same blocks [`super::edit_operations::delete_range`]
/// removes, so a cut pastes back what it took.  The fragment's groups are its
/// own and every block is dirty, so it writes as fresh markdown and editing
/// the source never reaches it.
pub fn slice(
    document: &RichDocument,
    selection: RichSelection,
) -> Result<RichDocument, RichEditError> {
    let mut fragment = RichDocument::new();
    if selection.is_empty() || document.blocks.is_empty() {
        return Ok(fragment);
    }

    let blocks = &document.blocks;
    let start = selection.start();
    let end = selection.end();
    if selection.whole_block {
        check_block(document, start.block_index)?;
        fragment.blocks.push(blocks[start.block_index].clone());
    } else if start.block_index == end.block_index {
        check_range(document, start, end)?;
        let block = &blocks[start.block_index];
        let same_cell = start.row == end.row && start.column == end.column;
        if block.kind == RichBlockKind::CodeBlock {
            let text: String = block
                .code_text
                .chars()
                .skip(start.offset)
                .take(end.offset - start.offset)
                .collect();
            fragment.blocks.push(paragraph(vec![RichInline::run(text)]));
        } else if block.is_text_block() || (block.kind == RichBlockKind::Table && same_cell) {
            let cell = block
                .inlines_at(start.row, start.column)
                .map_or(&[][..], Vec::as_slice);
            fragment
                .blocks
                .push(paragraph(slice_inlines(cell, start.offset, end.offset)));
        } else if block.kind == RichBlockKind::Table {
            fragment.blocks.push(slice_table(block, start, end));
        } else {
            fragment.blocks.push(block.clone());
        }
    } else {
        check_range(document, start, end)?;
        let selected = blocks
            .iter()
            .enumerate()
            .take(end.block_index + 1)
            .skip(start.block_index);
        for (i, block) in selected {
            // A block the range only touches (it starts at a block's end, as Shift+Down from a line's end
            // does, or ends at the next block's start) has nothing selected; copying it would paste an empty
            // list item or paragraph.
            let touched_only = (i == start.block_index && start == end_of(document, i))
                || (i == end.block_index && end == DocPosition::new(i, 0));
            if touched_only {
                continue;
            }

            let mut copy = block.clone();
            if block.is_text_block() {
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
                copy.inlines = slice_inlines(&block.inlines, from, to);
            }

            fragment.blocks.push(copy);
        }
    }

    detach(&mut fragment.blocks, "\n\n");
    group_repair::normalize_list_depths(&mut fragment);
    Ok(fragment)
}

/// The fragment as a reader sees it, for the clipboard's plain text: blocks on
/// their own lines, table cells separated by tabs and rows by lines, a hard
/// break as a line break, an autolink as its address.  Images and inline HTML
/// have no text, and a Raw block gives its markdown since that is all it has.
pub fn visible_text(fragment: &RichDocument) -> String {
    let mut text = String::new();
    for (i, block) in fragment.blocks.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }

        match block.kind {
            RichBlockKind::CodeBlock => text.push_str(&block.code_text),
            RichBlockKind::Raw => text.push_str(block.original_source.trim()),
            RichBlockKind::Table => {
                for (r, row) in block.table_rows.iter().enumerate() {
                    if r > 0 {
                        text.push('\n');
                    }

                    for (column, cell) in row.iter().enumerate() {
                        if column > 0 {
                            text.push('\t');
                        }

                        append_inlines(&mut text, &cell.inlines);
                    }
                }
            }
            _ => append_inlines(&mut text, &block.inlines),
        }
    }

    text
}

/// Clipboard text from outside the editor as a fragment.  It is read as
/// markdown, except that a single line break stays a line break (a hard
/// break) rather than joining the lines as markdown would, so pasted plain
/// text keeps its lines; a blank line still starts a paragraph.  A paragraph
/// that would hold more than [`MAX_PASTED_LINE_BREAKS`] breaks becomes one
/// paragraph per line instead, so a pasted log or listing stays cheap to edit.
pub fn parse_clipboard_text(text: &str) -> RichDocument {
    let mut parsed = parse_with(text, true);
    let blocks = &mut parsed.blocks;
    for i in (0..blocks.len()).rev() {
        let block = &blocks[i];
        let breaks = block
            .inlines
            .iter()
            .filter(|inline| is_hard_break(inline))
            .count();
        if block.kind != RichBlockKind::Paragraph || breaks <= MAX_PASTED_LINE_BREAKS {
            continue;
        }

        let mut lines = Vec::new();
        let mut line = Vec::new();
        for inline in &block.inlines {
            if is_hard_break(inline) {
                lines.push(with_inlines(block, std::mem::take(&mut line)));
            } else {
                line.push(inline.clone());
            }
        }

        lines.push(with_inlines(block, line));
        blocks.splice(i..=i, lines);
    }

    parsed
}

/// True for a hard line break atom.
pub(super) fn is_hard_break(inline: &RichInline) -> bool {
    inline
        .as_atom()
        .is_some_and(|atom| atom.kind == InlineAtomKind::HardBreak)
}

fn with_inlines(block: &RichBlock, inlines: Vec<RichInline>) -> RichBlock {
    let mut copy = block.clone();
    copy.inlines = inlines;
    copy.dirty = true;
    copy
}

/// The rows a multi-cell selection covers, holding only the covered text: the
/// cells between the ends in reading order, the end cells cut at the
/// selection, the rest empty — exactly what
/// [`super::table_code_operations::delete_range`] clears, so a cut copies what
/// it removed.
fn slice_table(table: &RichBlock, start: DocPosition, end: DocPosition) -> RichBlock {
    let mut copy = table.clone();
    let columns = table.table_rows[0].len();
    let first = start.row * columns + start.column;
    let last = end.row * columns + end.column;
    for (row, cells) in copy.table_rows.iter_mut().enumerate() {
        for (column, copied) in cells.iter_mut().enumerate().take(columns) {
            let cell = row * columns + column;
            let source = &table.table_rows[row][column].inlines;
            let from = if cell == first { start.offset } else { 0 };
            let to = if cell == last {
                end.offset
            } else {
                inlines::length(source)
            };
            copied.inlines = if cell < first || cell > last {
                Vec::new()
            } else {
                slice_inlines(source, from, to)
            };
        }
    }

    copy.table_rows = copy.table_rows[start.row..=end.row].to_vec();
    copy
}

/// A new paragraph holding `inlines`.
pub(super) fn paragraph(inlines: Vec<RichInline>) -> RichBlock {
    let mut block = RichBlock::new(RichBlockKind::Paragraph);
    block.inlines = inlines;
    block
}

/// The inlines from offset `from` to `to`, cloned so the source is not split.
/// Both offsets were checked against the block, so the splits cannot fail.
fn slice_inlines(source: &[RichInline], from: usize, to: usize) -> Vec<RichInline> {
    let mut copy = source.to_vec();
    let first = inlines::split_at(&mut copy, from).unwrap_or(copy.len());
    let last = inlines::split_at(&mut copy, to).unwrap_or(copy.len());
    copy.drain(first..last.max(first)).collect()
}

/// Gives copied blocks groups of their own, marked as made by an edit
/// (`original_member_count` 0) so they are regenerated rather than written
/// from the source's bytes; marks them dirty and spaces them with
/// `blank_line`.
pub(super) fn detach(blocks: &mut [RichBlock], blank_line: &str) {
    let mut align = GroupRemap::new();
    let mut quote = GroupRemap::new();
    let mut list = GroupRemap::new();
    for (i, block) in blocks.iter_mut().enumerate() {
        block.align_group = align.remap(&block.align_group);
        if let Some(group) = &block.align_group {
            group.borrow_mut().original_member_count = 0;
        }
        block.quote_group = quote.remap(&block.quote_group);
        if let Some(group) = &block.quote_group {
            group.borrow_mut().original_member_count = 0;
        }
        block.list_group = list.remap(&block.list_group);
        if let Some(group) = &block.list_group {
            group.borrow_mut().original_member_count = 0;
        }
        block.separator_before = if i == 0 {
            String::new()
        } else {
            blank_line.to_string()
        };
        block.dirty = true;
    }
}

fn append_inlines(text: &mut String, inlines: &[RichInline]) {
    for inline in inlines {
        match &inline.content {
            RichInlineContent::Run(run) => text.push_str(&run.text),
            RichInlineContent::Atom(atom) => match atom.kind {
                InlineAtomKind::HardBreak => text.push('\n'),
                InlineAtomKind::Autolink => {
                    text.push_str(atom.raw_markdown.trim_matches(|c| c == '<' || c == '>'))
                }
                InlineAtomKind::Raw => text.push_str(&atom.raw_markdown),
                InlineAtomKind::Image | InlineAtomKind::Html => {}
            },
        }
    }
}
