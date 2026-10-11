//! The Code block toolbar button: text blocks become one fenced code block and
//! a code block becomes paragraphs again, one per line.  Ports agg-sharp's
//! `RichCodeBlockOperations.cs`.  Unlike the other block operations
//! ([`super::block_operations`]) this adds and removes blocks, so it returns
//! the selection remapped onto the new blocks.
//!
//! Code offsets count characters, as every caret offset here does (see the
//! module header in `mod.rs`).

use super::block::{RichBlock, RichBlockKind};
use super::block_operations::{block_range, set_block_kind};
use super::document::{DocPosition, RichDocument};
use super::edit_error::{check_range, RichEditError};
use super::edit_operations::blank_line;
use super::group_repair;
use super::inline::{RichInline, RichInlineContent};
use super::selection::RichSelection;

/// When the selection starts in a code block, that block becomes one
/// paragraph per non-blank line.  Otherwise the text blocks the selection
/// touches (a range ending at the very start of a block does not touch it, as
/// in [`super::block_operations`]) join into one ```` ``` ```` code block,
/// their plain text separated by `\n`; they leave any list or quote first, and
/// character styles are dropped since code has none (an atom keeps its
/// markdown as text).  A range that also touches a table, Raw or code block
/// changes nothing: there is no single code block a first-time user would
/// expect from that.
pub fn toggle_code_block(
    document: &mut RichDocument,
    selection: RichSelection,
) -> Result<RichSelection, RichEditError> {
    if !document.blocks.is_empty() {
        check_range(document, selection.anchor, selection.caret)?;
    }
    if !can_toggle_code_block(document, selection) {
        return Ok(selection);
    }

    let start_index = selection.start().block_index;
    if document.blocks[start_index].kind == RichBlockKind::CodeBlock {
        return Ok(code_to_paragraphs(document, start_index, selection));
    }

    paragraphs_to_code(document, selection)
}

/// Whether [`toggle_code_block`] would change anything: the selection starts
/// in a code block, or every block it touches is a text block.  The toolbar
/// disables its button exactly where this is false.
pub fn can_toggle_code_block(document: &RichDocument, selection: RichSelection) -> bool {
    let start = selection.start().block_index;
    let Some(block) = document.blocks.get(start) else {
        return false;
    };
    if block.kind == RichBlockKind::CodeBlock {
        return true;
    }

    let Some((first, last)) = block_range(document, selection) else {
        return false;
    };
    document.blocks[first..=last]
        .iter()
        .all(RichBlock::is_text_block)
}

/// One paragraph per non-blank line.  Blank lines are dropped: a blank
/// paragraph collapses on save anyway, so keeping them would only leave empty
/// lines the user cannot keep.  All-blank code gives one empty paragraph.
fn code_to_paragraphs(
    document: &mut RichDocument,
    index: usize,
    selection: RichSelection,
) -> RichSelection {
    let code_text = document.blocks[index].code_text.replace("\r\n", "\n");
    let lines: Vec<&str> = code_text.split('\n').collect();
    let line_lengths: Vec<usize> = lines.iter().map(|line| line.chars().count()).collect();

    // kept[k] is the line paragraph k comes from (C#'s string.IsNullOrWhiteSpace skips the rest).
    let kept: Vec<usize> = (0..lines.len())
        .filter(|&i| !lines[i].chars().all(char::is_whitespace))
        .collect();

    let blank = blank_line(document).to_string();
    let first_separator = document.blocks[index].separator_before.clone();
    let paragraphs: Vec<RichBlock> = (0..kept.len().max(1))
        .map(|k| {
            let mut paragraph = RichBlock::new(RichBlockKind::Paragraph);
            paragraph.separator_before = if k == 0 {
                first_separator.clone()
            } else {
                blank.clone()
            };
            paragraph.dirty = true;
            if let Some(&line) = kept.get(k) {
                paragraph.inlines.push(RichInline::run(lines[line]));
            }
            paragraph
        })
        .collect();
    let count = paragraphs.len();

    document.blocks.splice(index..=index, paragraphs);
    group_repair::repair(document, index, index + count - 1);

    let map = |position: DocPosition| -> DocPosition {
        if position.block_index < index {
            return position;
        }

        if position.block_index > index {
            return DocPosition {
                block_index: position.block_index + count - 1,
                ..position
            };
        }

        if kept.is_empty() {
            return DocPosition::new(index, 0);
        }

        // The code offset counts each '\n' as one character, exactly as the line split consumed them.
        let mut offset = position.offset;
        let mut line = 0;
        while line < lines.len() - 1 && offset > line_lengths[line] {
            offset -= line_lengths[line] + 1;
            line += 1;
        }

        // A position on a dropped blank line moves to the start of the next kept line, or the end of the last.
        match kept.iter().position(|&l| l >= line) {
            None => {
                let k = kept.len() - 1;
                DocPosition::new(index + k, line_lengths[kept[k]])
            }
            Some(k) => {
                let offset = if kept[k] == line {
                    offset.min(line_lengths[line])
                } else {
                    0
                };
                DocPosition::new(index + k, offset)
            }
        }
    };

    RichSelection::new(map(selection.anchor), map(selection.caret))
}

fn paragraphs_to_code(
    document: &mut RichDocument,
    selection: RichSelection,
) -> Result<RichSelection, RichEditError> {
    let Some((first, last)) = block_range(document, selection) else {
        return Ok(selection);
    };

    // Leaving lists and quotes first lets block_operations do the group bookkeeping; it adds or removes no
    // blocks, so first..=last still names the same blocks.
    set_block_kind(document, selection, 0)?;

    let mut lines = Vec::new();
    let mut line_starts = Vec::new();
    let mut code_length = 0;
    for block in &document.blocks[first..=last] {
        let line = plain_text(&block.inlines, usize::MAX);
        line_starts.push(code_length);
        code_length += line.chars().count() + 1;
        lines.push(line);
    }

    let mut code_block = RichBlock::new(RichBlockKind::CodeBlock);
    code_block.code_text = lines.join("\n");
    code_block.code_fence = "```".to_string();
    code_block.separator_before = if first == 0 {
        document.blocks[0].separator_before.clone()
    } else {
        blank_line(document).to_string()
    };
    code_block.dirty = true;

    // Positions are mapped before the blocks go, while their inlines still say how long each atom's text is.
    let map = |position: DocPosition| -> DocPosition {
        if position.block_index < first {
            return position;
        }

        if position.block_index > last {
            return DocPosition {
                block_index: position.block_index - (last - first),
                ..position
            };
        }

        let inlines = &document.blocks[position.block_index].inlines;
        let line = position.block_index - first;
        let before = plain_text(inlines, position.offset).chars().count();
        DocPosition::new(first, line_starts[line] + before)
    };

    let mapped = RichSelection::new(map(selection.anchor), map(selection.caret));
    document
        .blocks
        .splice(first..=last, std::iter::once(code_block));
    group_repair::repair(document, first, first);
    Ok(mapped)
}

/// The code text of the inlines up to caret offset `up_to`: run text as is,
/// and an atom (image, line break) as its markdown, so nothing the user wrote
/// is lost when it becomes code.
fn plain_text(inlines: &[RichInline], up_to: usize) -> String {
    let mut text = String::new();
    let mut offset = 0;
    for inline in inlines {
        if offset >= up_to {
            break;
        }

        match &inline.content {
            RichInlineContent::Run(run) => {
                text.extend(run.text.chars().take(up_to - offset));
            }
            RichInlineContent::Atom(atom) => text.push_str(&atom.raw_markdown),
        }

        offset += inline.len();
    }

    text
}
