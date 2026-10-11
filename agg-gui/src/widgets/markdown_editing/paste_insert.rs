//! Pasting a fragment document at a caret: [`insert_document`], the second
//! half of agg-sharp's `RichPasteOperations.cs` (`InsertDocument` and its
//! helpers `InsertIntoCell`, `JoinCaretGroups`, `UseLineEnding` and
//! `LikeTarget`).  Fragments come from [`super::paste_operations::slice`] (a
//! copy inside the editor) or [`super::paste_operations::parse_clipboard_text`]
//! (text from outside it).
//!
//! C#'s `HashSet<RichBlock>` of blocks that joined the caret's list (matched
//! by reference) becomes one flag per pasted block here.

use super::block::{RichBlock, RichBlockKind};
use super::document::{DocPosition, RichDocument};
use super::edit_error::{check_position, RichEditError};
use super::edit_operations::{blank_line, end_of, insert_text, split_block};
use super::group_repair;
use super::inline::RichInline;
use super::inlines;
use super::paste_operations::{detach, is_hard_break, visible_text};
use super::selection::RichSelection;

/// Pastes `fragment` at `position` and returns the caret after it.  A lone
/// paragraph goes into the text at the caret.  Otherwise the caret's block
/// splits: a leading paragraph joins the text before the caret, a trailing
/// paragraph is joined by the text after it (that part keeps the caret
/// block's kind), and everything else — headings, list items, quotes, code,
/// tables — comes in as blocks between, joining the caret's list, quote or
/// alignment wrapper where they fit it.  An empty text before the caret is
/// dropped rather than left as a blank line above a pasted heading or list.
/// In a code block only the fragment's [`visible_text`] goes in, as there is
/// nowhere for blocks there; a table cell takes a lone paragraph with its
/// styles and anything else as one line of text.  At a Raw block the paste
/// goes into a new paragraph beside it.  Verbatim markdown in the paste takes
/// the document's line ending.  The fragment itself is not changed, so one
/// copy pastes any number of times.
pub fn insert_document(
    document: &mut RichDocument,
    position: DocPosition,
    fragment: &RichDocument,
) -> Result<RichSelection, RichEditError> {
    check_position(document, position)?;
    if fragment.blocks.is_empty() {
        return Ok(RichSelection::at(position));
    }

    if document.blocks.is_empty() {
        let mut paragraph = RichBlock::new(RichBlockKind::Paragraph);
        paragraph.dirty = true;
        document.blocks.push(paragraph);
    }

    let mut position = position;
    let target_kind = document.blocks[position.block_index].kind;
    if target_kind == RichBlockKind::CodeBlock {
        return insert_text(document, position, &visible_text(fragment), None);
    }

    let blank = blank_line(document).to_string();
    let mut pasted = fragment.blocks.clone();
    detach(&mut pasted, &blank);
    use_line_ending(&mut pasted, &blank[..blank.len() / 2]);
    if target_kind == RichBlockKind::Table {
        return insert_into_cell(document, position, &pasted, fragment);
    }

    if target_kind == RichBlockKind::Raw {
        position = split_block(document, position)?.caret;
    }

    // Every pasted block follows something here: the text before the caret, or (when that is dropped) whatever
    // preceded the caret block, whose gap the first block then takes over below.
    pasted[0].separator_before = blank.clone();
    let at = position.block_index;
    let out_of_range = RichEditError::PositionOutOfRange(position);
    let target = &mut document.blocks[at];
    target.dirty = true;
    let split =
        inlines::split_at(&mut target.inlines, position.offset).map_err(|_| out_of_range)?;
    if pasted.len() == 1 && pasted[0].kind == RichBlockKind::Paragraph {
        let added = std::mem::take(&mut pasted[0].inlines);
        let length = inlines::length(&added);
        target.inlines.splice(split..split, added);
        inlines::merge_adjacent(&mut target.inlines);
        return Ok(RichSelection::at(DocPosition {
            offset: position.offset + length,
            ..position
        }));
    }

    let mut target = document.blocks.remove(at);
    let tail = target.inlines.split_off(split);
    if pasted[0].kind == RichBlockKind::Paragraph {
        let first = pasted.remove(0);
        target.inlines.extend(first.inlines);
    }

    let mut tail_block = None;
    let mut caret_offset = 0;
    if pasted
        .last()
        .is_some_and(|last| last.kind == RichBlockKind::Paragraph)
    {
        if let Some(last) = pasted.pop() {
            caret_offset = inlines::length(&last.inlines);
            tail_block = Some(like_target(&target, last.inlines, tail, &blank));
        }
    } else if inlines::length(&tail) > 0 {
        tail_block = Some(like_target(&target, Vec::new(), tail, &blank));
    }

    let joined_list = join_caret_groups(&target, &mut pasted);
    let mut replacement = Vec::with_capacity(pasted.len() + 2);
    let keep_target = inlines::length(&target.inlines) > 0 || pasted.is_empty();
    if !keep_target {
        pasted[0].separator_before = target.separator_before.clone();
    }
    // Where the pasted blocks start among the replacement, for the list flags.
    let pasted_start = at + usize::from(keep_target);
    if keep_target {
        replacement.push(target);
    }

    replacement.extend(pasted);
    let has_tail = tail_block.is_some();
    replacement.extend(tail_block);
    for block in &mut replacement {
        inlines::merge_adjacent(&mut block.inlines);
    }

    let last_index = at + replacement.len() - 1;
    document.blocks.splice(at..at, replacement);
    for (j, joined) in joined_list.iter().enumerate() {
        if *joined {
            group_repair::adopt_level_style(&mut document.blocks, pasted_start + j);
        }
    }

    // A pasted block that did not join the caret's group (a heading in a list) splits it into runs, and
    // pasted list items take the depth rules of their new place.
    group_repair::repair(document, at, last_index);
    let caret = if has_tail {
        DocPosition::new(last_index, caret_offset)
    } else {
        end_of(document, last_index)
    };
    Ok(RichSelection::at(caret))
}

/// A paste into a table cell: a lone paragraph goes in with its styles (a
/// cell has no line breaks, so a hard break becomes a space); anything else
/// is flattened to one line of its text.
fn insert_into_cell(
    document: &mut RichDocument,
    position: DocPosition,
    pasted: &[RichBlock],
    fragment: &RichDocument,
) -> Result<RichSelection, RichEditError> {
    if pasted.len() != 1 || pasted[0].kind != RichBlockKind::Paragraph {
        let line = visible_text(fragment).replace(['\n', '\t'], " ");
        return insert_text(document, position, &line, None);
    }

    let added: Vec<RichInline> = pasted[0]
        .inlines
        .iter()
        .map(|inline| {
            if is_hard_break(inline) {
                let mut space = RichInline::run(" ");
                space.bold = inline.bold;
                space.italic = inline.italic;
                space.strike = inline.strike;
                space
            } else {
                inline.clone()
            }
        })
        .collect();
    let length = inlines::length(&added);
    let table = &mut document.blocks[position.block_index];
    let cell = table
        .inlines_at_mut(position.row, position.column)
        .ok_or(RichEditError::PositionOutOfRange(position))?;
    let split = inlines::split_at(cell, position.offset)
        .map_err(|_| RichEditError::PositionOutOfRange(position))?;
    cell.splice(split..split, added);
    inlines::merge_adjacent(cell);
    table.dirty = true;
    Ok(RichSelection::at(DocPosition {
        offset: position.offset + length,
        ..position
    }))
}

/// Blocks pasted inside a list, quote or alignment wrapper join it rather
/// than splitting it in two: pasted list items join the caret's list (their
/// depths below the caret item's), pasted quote paragraphs its quote,
/// paragraphs and headings its wrapper, and a plain paragraph becomes an item
/// or quote paragraph like the caret's block.  Returns, per block, whether it
/// joined a list, whose numbering style is adopted once it is in place.
fn join_caret_groups(target: &RichBlock, middle: &mut [RichBlock]) -> Vec<bool> {
    let mut joined_list = vec![false; middle.len()];
    for (i, block) in middle.iter_mut().enumerate() {
        let plain = block.kind == RichBlockKind::Paragraph
            && block.quote_group.is_none()
            && block.align_group.is_none();
        let target_item = target.kind == RichBlockKind::ListItem;
        if target_item && block.kind == RichBlockKind::ListItem {
            block.list_group = target.list_group.clone();
            if let (Some(list), Some(target_list)) = (block.list.as_mut(), target.list.as_ref()) {
                list.depth += target_list.depth;
            }
            joined_list[i] = true;
        } else if target_item && plain {
            block.kind = RichBlockKind::ListItem;
            block.list = target.list.clone();
            block.list_group = target.list_group.clone();
        } else if target.kind == RichBlockKind::Quote
            && (plain || block.kind == RichBlockKind::Quote)
        {
            block.kind = RichBlockKind::Quote;
            block.quote_group = target.quote_group.clone();
        }

        if target.align_group.is_some()
            && matches!(
                block.kind,
                RichBlockKind::Paragraph | RichBlockKind::Heading
            )
        {
            block.align_group = target.align_group.clone();
            block.alignment = target.alignment;
        }
    }

    joined_list
}

/// Puts the document's line ending into what pasted blocks write verbatim (a
/// Raw block's source, an atom's markdown), so a CRLF file never gains bare LF
/// lines from a paste.  Code text is `\n` inside the model.
fn use_line_ending(blocks: &mut [RichBlock], newline: &str) {
    let fix = |text: &str| -> String {
        let text = text.replace("\r\n", "\n");
        if newline == "\n" {
            text
        } else {
            text.replace('\n', newline)
        }
    };

    let fix_atoms = |inlines: &mut Vec<RichInline>| {
        for inline in inlines.iter_mut() {
            if let super::inline::RichInlineContent::Atom(atom) = &mut inline.content {
                atom.raw_markdown = fix(&atom.raw_markdown);
            }
        }
    };

    for block in blocks {
        block.original_source = fix(&block.original_source);
        fix_atoms(&mut block.inlines);
        for row in &mut block.table_rows {
            for cell in row {
                fix_atoms(&mut cell.inlines);
            }
        }
    }
}

/// The part of the caret block after the paste: the caret block's kind,
/// level, list info and groups, holding `lead` then `tail`.
fn like_target(
    target: &RichBlock,
    lead: Vec<RichInline>,
    tail: Vec<RichInline>,
    blank_line: &str,
) -> RichBlock {
    let mut block = target.clone();
    block.inlines = lead;
    block.inlines.extend(tail);
    block.separator_before = blank_line.to_string();
    block.dirty = true;
    block
}
