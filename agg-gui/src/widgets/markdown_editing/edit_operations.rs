//! Text edits on a [`RichDocument`]: typing, deleting a range, Enter,
//! Backspace and Delete.  Ports agg-sharp's `RichEditOperations.cs`.  Each
//! operation mutates the document in place, marks every block it changes
//! dirty, and returns the [`RichSelection`] to show next (usually a caret;
//! Backspace/Delete next to a Raw block select it whole instead of merging
//! into it).
//!
//! Group rules (see [`super::groups`]): a split copies the block's groups, a
//! merge keeps the survivor's, and a block that leaves its list or quote has
//! its reference cleared.  When it leaves from the middle, the members after
//! it move to a new group of the same kind, so every group stays one
//! contiguous run (`group_repair`) — the writer sees the changed member counts
//! and regenerates both parts.  A removal that leaves two lists or two quotes
//! side by side forces both to regenerate, and list depths always stay
//! expressible (each list starts at depth 0, no item more than one level below
//! the item before it).
//!
//! Edits inside one code block or table go to
//! [`super::table_code_operations`]; across blocks code blocks and tables
//! behave like Raw blocks (removed whole when any of them is in a deleted
//! range, selected whole rather than merged into).
//!
//! Positions passed in must be caret positions the document has (a block that
//! exists, an offset within it): like C#'s indexers, an operation panics on
//! one that is not.  This file also holds the document's line-ending rule
//! ([`blank_line`]), which [`super::markdown_writer`] writes regenerated
//! separators with, and the caret's character stops.

use super::block::{RichBlock, RichBlockKind};
use super::document::{DocPosition, RichDocument};
use super::group_repair;
use super::inline::RichInline;
use super::inlines;
use super::selection::RichSelection;
use super::table_code_operations;

/// Types `text` at `position`.  Each `\n` (or `\r\n`) acts like Enter
/// ([`split_block`]), so pasting lines into a list makes list items.  The text
/// takes `style` when given (a pending style the user picked), otherwise the
/// style of the nearest run before the caret — never from an atom, so typing
/// after an image continues the text before it; at a block's start it takes
/// the first run's style.  A link or inline code does not extend past its end.
/// At a Raw block the text goes into a new paragraph above (caret before it)
/// or below (caret after it).  Empty text changes nothing.
pub fn insert_text(
    document: &mut RichDocument,
    position: DocPosition,
    text: &str,
    style: Option<&RichInline>,
) -> RichSelection {
    if text.is_empty() {
        return RichSelection::at(position);
    }

    add_paragraph_if_empty(document);
    if is_code_or_table(&document.blocks[position.block_index]) {
        return table_code_operations::insert_text(document, position, text, style);
    }

    let mut caret = text_caret(document, position);
    let style = match style {
        Some(style) => style.clone(),
        None => style_at(&document.blocks[caret.block_index].inlines, caret.offset),
    };
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            caret = split_block(document, caret).caret;
        }

        if !line.is_empty() {
            let block = &mut document.blocks[caret.block_index];
            let index = split_at(&mut block.inlines, caret.offset);
            block.inlines.insert(index, style.with_text(line));
            inlines::merge_adjacent(&mut block.inlines);
            block.dirty = true;
            caret.offset += line.chars().count();
        }
    }

    RichSelection::at(caret)
}

/// Deletes a selection: a [`RichSelection::whole_block`] selection removes its
/// block, any other is a [`delete_range`].  This is what Delete, Backspace and
/// typing over a selection call.
pub fn delete_selection(document: &mut RichDocument, selection: RichSelection) -> RichSelection {
    if document.blocks.is_empty() {
        return RichSelection::default();
    }

    if selection.whole_block {
        let index = selection.anchor.block_index;
        return remove_blocks(document, index, index);
    }

    delete_range(document, selection.anchor, selection.caret)
}

/// Removes everything between two positions (in either order).  Within one
/// text block it removes the text.  Across blocks the blocks in between go
/// whole; if both ends are in text blocks the end block's remainder joins the
/// start block, which keeps its kind, alignment and groups.  A Raw (or
/// code/table) block at either end is removed whole if any of it is in the
/// range, and then nothing merges across it.  Within one code block or table
/// only the covered text goes ([`table_code_operations::delete_range`]), even
/// when that is all of it; removing such a block on its own takes
/// [`delete_selection`] with a [`RichSelection::whole_block`] selection.
pub fn delete_range(document: &mut RichDocument, a: DocPosition, b: DocPosition) -> RichSelection {
    let start = a.min(b);
    let end = a.max(b);
    if document.blocks.is_empty() {
        return RichSelection::default();
    }

    if start == end {
        return RichSelection::at(start);
    }

    if start.block_index == end.block_index {
        let block = &mut document.blocks[start.block_index];
        if block.is_text_block() {
            remove_text(block, start.offset, end.offset);
            return RichSelection::at(start);
        }

        if is_code_or_table(block) {
            return table_code_operations::delete_range(document, start, end);
        }

        return remove_blocks(document, start.block_index, start.block_index);
    }

    let first = start.block_index;
    let last = end.block_index;
    let first_is_text = document.blocks[first].is_text_block();
    let last_is_text = document.blocks[last].is_text_block();
    if first_is_text && last_is_text {
        let first_length = document.blocks[first].text_length(0, 0);
        remove_text(&mut document.blocks[first], start.offset, first_length);
        remove_text(&mut document.blocks[last], 0, end.offset);
        append_inlines(document, first, last);
        remove_block_range(document, first + 1, last - first);
        return RichSelection::at(start);
    }

    // A non-text end is kept only when the range merely touches it (starts at its end or ends at its start).
    let keep_first = first_is_text || start == end_of(document, first);
    let keep_last = last_is_text || end == DocPosition::new(last, 0);
    if first_is_text {
        let first_length = document.blocks[first].text_length(0, 0);
        remove_text(&mut document.blocks[first], start.offset, first_length);
    }

    if last_is_text {
        remove_text(&mut document.blocks[last], 0, end.offset);
    }

    let remove_from = if keep_first { first + 1 } else { first };
    // `last` is after `first`, so it is at least 1 and `last - 1` cannot underflow.
    let remove_to = if keep_last { last - 1 } else { last };
    if remove_from > remove_to {
        return RichSelection::at(start);
    }

    let after_removal = remove_blocks(document, remove_from, remove_to);
    if keep_first {
        RichSelection::at(start)
    } else {
        after_removal
    }
}

/// Enter at `position`.  Normally splits the block in two: the new block below
/// copies its kind, heading level, list info, alignment and groups.  Special
/// cases a first-time user expects: Enter in an empty list item outdents it
/// (depth > 0) or turns it into a paragraph that leaves the list; Enter in an
/// empty quote paragraph leaves the quote; Enter at the start of a non-empty
/// heading adds an empty paragraph above it; Enter at the end of a heading
/// starts a paragraph, not another heading.  At a Raw block it adds an empty
/// paragraph above (caret before the block) or below (caret after it).
pub fn split_block(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    add_paragraph_if_empty(document);
    let index = position.block_index;
    let kind = document.blocks[index].kind;
    if kind == RichBlockKind::Raw {
        return RichSelection::at(insert_paragraph_beside_raw(document, position));
    }

    if is_code_or_table(&document.blocks[index]) {
        return table_code_operations::split_block(document, position);
    }

    let length = document.blocks[index].text_length(0, 0);
    if length == 0 && kind == RichBlockKind::ListItem {
        outdent_or_leave_list(document, index);
        return RichSelection::at(position);
    }

    if length == 0 && kind == RichBlockKind::Quote {
        leave_quote(document, index);
        return RichSelection::at(position);
    }

    if kind == RichBlockKind::Heading && position.offset == 0 && length > 0 {
        let mut above = new_block_like(document, &document.blocks[index], RichBlockKind::Paragraph);

        // The new block takes the heading's place, so it takes the gap before it too.
        let gap = blank_line(document).to_string();
        let block = &mut document.blocks[index];
        above.separator_before = std::mem::replace(&mut block.separator_before, gap);
        document.blocks.insert(index, above);
        return RichSelection::at(DocPosition::new(index + 1, 0));
    }

    let kind = if kind == RichBlockKind::Heading && position.offset == length {
        RichBlockKind::Paragraph
    } else {
        kind
    };
    let mut below = new_block_like(document, &document.blocks[index], kind);
    let block = &mut document.blocks[index];
    let split = split_at(&mut block.inlines, position.offset);
    below.inlines = block.inlines.split_off(split);
    block.dirty = true;
    document.blocks.insert(index + 1, below);
    RichSelection::at(DocPosition::new(index + 1, 0))
}

/// Backspace with a collapsed caret.  Inside text it removes one character (a
/// whole emoji or combining sequence) or atom.  At a block's start: a list
/// item outdents, or at depth 0 becomes a paragraph leaving the list; a quote
/// paragraph leaves the quote; a heading becomes a paragraph; a paragraph
/// merges into the text block before it.  A Raw (or code/table) block before
/// the caret is selected whole instead, so the next Backspace deletes it
/// knowingly.  Just after a Raw block's own caret stop it selects that block;
/// at its start the caret moves to the end of a text block before it.
pub fn backspace(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    backspace_changed(document, position).0
}

/// [`backspace`], also saying whether it changed the document (false when it
/// only moved the caret or selected a block), so the editor leaves no empty
/// undo step (C#'s `out bool changed` overload).
pub fn backspace_changed(
    document: &mut RichDocument,
    position: DocPosition,
) -> (RichSelection, bool) {
    if document.blocks.is_empty() {
        return (RichSelection::default(), false);
    }

    let index = position.block_index;
    if is_code_or_table(&document.blocks[index]) {
        return table_code_operations::backspace_changed(document, position);
    }

    if document.blocks[index].kind == RichBlockKind::Raw {
        if position.offset > 0 {
            return (whole_block(document, index), false);
        }

        if index == 0 {
            return (RichSelection::at(position), false);
        }

        // Nothing merges into a Raw block, so the caret steps back to the end of the block before it.
        let selection = if document.blocks[index - 1].is_text_block() {
            RichSelection::at(end_of(document, index - 1))
        } else {
            whole_block(document, index - 1)
        };
        return (selection, false);
    }

    if position.offset > 0 {
        let block = &mut document.blocks[index];
        let previous = previous_stop(&block.inlines, position.offset);
        remove_text(block, previous, position.offset);
        let caret = DocPosition {
            offset: previous,
            ..position
        };
        return (RichSelection::at(caret), true);
    }

    match document.blocks[index].kind {
        RichBlockKind::ListItem => {
            outdent_or_leave_list(document, index);
            return (RichSelection::at(position), true);
        }
        RichBlockKind::Quote => {
            leave_quote(document, index);
            return (RichSelection::at(position), true);
        }
        RichBlockKind::Heading => {
            // The block keeps its alignment group: it is still a paragraph or heading, which the wrapper holds.
            let block = &mut document.blocks[index];
            block.kind = RichBlockKind::Paragraph;
            block.heading_level = 0;
            block.dirty = true;
            return (RichSelection::at(position), true);
        }
        _ => {}
    }

    if index == 0 {
        return (RichSelection::at(position), false);
    }

    if !document.blocks[index - 1].is_text_block() {
        return (whole_block(document, index - 1), false);
    }

    let caret = DocPosition::new(index - 1, document.blocks[index - 1].text_length(0, 0));
    append_inlines(document, index - 1, index);
    remove_block_range(document, index, 1);
    (RichSelection::at(caret), true)
}

/// Forward Delete with a collapsed caret: the mirror of [`backspace`].  Inside
/// text it removes the next character or atom; at a text block's end the next
/// text block joins this one (this block keeps its kind and groups); a Raw (or
/// code/table) block after the caret is selected whole instead.  Before a Raw
/// block's own caret stop it selects that block.
pub fn delete(document: &mut RichDocument, position: DocPosition) -> RichSelection {
    delete_changed(document, position).0
}

/// [`delete`], also saying whether it changed the document.
pub fn delete_changed(document: &mut RichDocument, position: DocPosition) -> (RichSelection, bool) {
    if document.blocks.is_empty() {
        return (RichSelection::default(), false);
    }

    let index = position.block_index;
    let block = &mut document.blocks[index];
    if is_code_or_table(block) {
        return table_code_operations::delete_changed(document, position);
    }

    if block.kind == RichBlockKind::Raw && position.offset == 0 {
        return (whole_block(document, index), false);
    }

    let length = block.text_length(0, 0);
    if position.offset < length {
        let next = next_stop(&block.inlines, position.offset);
        remove_text(block, position.offset, next);
        return (RichSelection::at(position), true);
    }

    if index == document.blocks.len() - 1 {
        return (RichSelection::at(position), false);
    }

    if !document.blocks[index + 1].is_text_block() {
        return (whole_block(document, index + 1), false);
    }

    if !document.blocks[index].is_text_block() {
        // Text cannot join a Raw block; the caret simply stays after it.
        return (RichSelection::at(position), false);
    }

    append_inlines(document, index, index + 1);
    remove_block_range(document, index + 1, 1);
    (RichSelection::at(position), true)
}

/// A selection of a block as an object, from its first caret stop to its last
/// (for a table, the last cell's end), flagged [`RichSelection::whole_block`]
/// so deleting it removes the block.
pub fn whole_block(document: &RichDocument, index: usize) -> RichSelection {
    RichSelection {
        anchor: DocPosition::new(index, 0),
        caret: end_of(document, index),
        whole_block: true,
    }
}

/// The last caret stop of block `index` (a table's last cell's end).
pub fn end_of(document: &RichDocument, index: usize) -> DocPosition {
    let Some(block) = document.blocks.get(index) else {
        // Only an empty document has no block here; its one caret stop is the start.
        return DocPosition::new(index, 0);
    };
    if block.kind == RichBlockKind::Table && !block.table_rows.is_empty() {
        let row = block.table_rows.len() - 1;
        // C# would index column -1 of an empty last row; a parsed or edited table never has one.
        let column = block.table_rows[row].len().saturating_sub(1);
        return DocPosition::in_cell(index, block.text_length(row, column), row, column);
    }

    DocPosition::new(index, block.text_length(0, 0))
}

/// An empty document (no blocks at all) gets one empty paragraph so typing or
/// Enter has somewhere to go.
fn add_paragraph_if_empty(document: &mut RichDocument) {
    if document.blocks.is_empty() {
        document.blocks.push(RichBlock {
            dirty: true,
            ..RichBlock::new(RichBlockKind::Paragraph)
        });
    }
}

pub(super) fn is_code_or_table(block: &RichBlock) -> bool {
    matches!(block.kind, RichBlockKind::CodeBlock | RichBlockKind::Table)
}

/// The caret typed text goes to: the position itself in a text block, or a
/// new paragraph beside a Raw one.
fn text_caret(document: &mut RichDocument, position: DocPosition) -> DocPosition {
    if document.blocks[position.block_index].kind == RichBlockKind::Raw {
        return insert_paragraph_beside_raw(document, position);
    }

    position
}

fn insert_paragraph_beside_raw(document: &mut RichDocument, position: DocPosition) -> DocPosition {
    let gap = blank_line(document).to_string();
    let mut paragraph = RichBlock {
        separator_before: gap.clone(),
        dirty: true,
        ..RichBlock::new(RichBlockKind::Paragraph)
    };

    if position.offset == 0 {
        let raw = &mut document.blocks[position.block_index];
        paragraph.separator_before = std::mem::replace(&mut raw.separator_before, gap);
        document.blocks.insert(position.block_index, paragraph);
        return DocPosition::new(position.block_index, 0);
    }

    document.blocks.insert(position.block_index + 1, paragraph);
    DocPosition::new(position.block_index + 1, 0)
}

/// The style typed text takes at a caret: the nearest run before it, skipping
/// atoms (an image has no text style); at the start of the block, the first
/// run after it; plain text in a block with no runs.  A link or inline code
/// stops at its end, as in Google Docs: typing just after one keeps its bold,
/// italic and strike but not the link or code, so a first-time user does not
/// grow a link by typing after it.  The result is a run whose text is to be
/// replaced ([`RichInline::with_text`]).
pub fn style_at(list: &[RichInline], offset: usize) -> RichInline {
    if offset > 0 {
        // Default locate looks backwards, so for offset > 0 this is the inline just before the caret.
        if let Ok((index, in_inline)) = inlines::locate(list, offset, false) {
            for i in (0..=index.min(list.len().saturating_sub(1))).rev() {
                let inline = &list[i];
                if inline.is_run() {
                    let inside = i == index && in_inline < inline.len();
                    return if inside {
                        inline.clone()
                    } else {
                        without_link_or_code(inline)
                    };
                }
            }
        }
    }

    list.iter()
        .find(|inline| inline.is_run())
        .cloned()
        .unwrap_or_else(|| RichInline::run(""))
}

fn without_link_or_code(run: &RichInline) -> RichInline {
    let code = run.as_run().is_some_and(|r| r.code);
    if run.link_url.is_none() && run.link_label.is_none() && !code {
        return run.clone();
    }

    let mut style = run.with_text("");
    style.link_url = None;
    style.link_title = None;
    style.link_label = None;
    if let Some(r) = style.as_run_mut() {
        r.code = false;
    }
    style
}

/// [`inlines::split_at`] for an offset the caller knows is within the inlines.
pub(super) fn split_at(list: &mut Vec<RichInline>, offset: usize) -> usize {
    inlines::split_at(list, offset).expect("caret offset within its block")
}

/// Removes plain-text offsets `from..to` from a list of inlines, returning
/// whether anything went.
pub(super) fn remove_inline_range(list: &mut Vec<RichInline>, from: usize, to: usize) -> bool {
    if from >= to {
        return false;
    }

    // split_at(to) runs second, so its split lands after the first one and leaves 'start' valid.
    let start = split_at(list, from);
    let end = split_at(list, to);
    list.drain(start..end);
    inlines::merge_adjacent(list);
    true
}

fn remove_text(block: &mut RichBlock, from: usize, to: usize) {
    if remove_inline_range(&mut block.inlines, from, to) {
        block.dirty = true;
    }
}

/// Moves block `source`'s inlines onto the end of block `survivor`; the caller
/// removes the source block, which takes its group membership with it.
fn append_inlines(document: &mut RichDocument, survivor: usize, source: usize) {
    let moved = std::mem::take(&mut document.blocks[source].inlines);
    let survivor = &mut document.blocks[survivor];
    survivor.inlines.extend(moved);
    inlines::merge_adjacent(&mut survivor.inlines);
    survivor.dirty = true;
}

/// Removes blocks `first..=last` and returns a caret where they were: the
/// start of the block now there, else the end of the one before.  A document
/// left empty gets one empty paragraph so there is somewhere to type.
pub(super) fn remove_blocks(
    document: &mut RichDocument,
    first: usize,
    last: usize,
) -> RichSelection {
    remove_block_range(document, first, last - first + 1);
    if document.blocks.is_empty() {
        add_paragraph_if_empty(document);
        return RichSelection::at(DocPosition::new(0, 0));
    }

    if first < document.blocks.len() {
        return RichSelection::at(DocPosition::new(first, 0));
    }

    RichSelection::at(end_of(document, document.blocks.len() - 1))
}

/// An empty, dirty block of `kind` that sits in the same groups as `like` (a
/// split keeps the group) with its alignment, heading level and list info
/// where the kind uses them.
fn new_block_like(document: &RichDocument, like: &RichBlock, kind: RichBlockKind) -> RichBlock {
    let list_item = kind == RichBlockKind::ListItem;
    RichBlock {
        kind,
        heading_level: if kind == RichBlockKind::Heading {
            like.heading_level
        } else {
            0
        },
        list: if list_item { like.list.clone() } else { None },
        list_group: if list_item {
            like.list_group.clone()
        } else {
            None
        },
        quote_group: if kind == RichBlockKind::Quote {
            like.quote_group.clone()
        } else {
            None
        },
        alignment: like.alignment,
        align_group: like.align_group.clone(),
        separator_before: blank_line(document).to_string(),
        dirty: true,
        ..RichBlock::default()
    }
}

/// Removes `count` blocks at `first` and repairs what the removal joined: the
/// leading gap (after the frontmatter) stays with whichever block is now
/// first, two lists or quotes that became neighbours are both forced to
/// regenerate (their clean bytes back to back would read as one loose list or
/// one quote), and list depths are brought back into a shape markdown can
/// express.
fn remove_block_range(document: &mut RichDocument, first: usize, count: usize) {
    let blocks = &mut document.blocks;
    let leading_gap = (first == 0).then(|| blocks[0].separator_before.clone());
    blocks.drain(first..first + count);
    if let (Some(gap), Some(block)) = (leading_gap, blocks.first_mut()) {
        block.separator_before = gap;
    }

    if first > 0 && first < blocks.len() {
        let (before, after) = (&blocks[first - 1], &blocks[first]);
        group_repair::force_regeneration_if_joined(&before.list_group, &after.list_group);
        group_repair::force_regeneration_if_joined(&before.quote_group, &after.quote_group);
    }

    group_repair::normalize_list_depths(document);
}

/// Backspace/Enter on a list item at its start: a nested item outdents one
/// level and takes its subtree (the deeper items right after it) along, so no
/// child is left two levels below its new parent.  A top-level item becomes a
/// paragraph and leaves the list; the items after it are lifted so their list
/// starts at depth 0, keeping their depths relative to each other.
fn outdent_or_leave_list(document: &mut RichDocument, index: usize) {
    let blocks = &mut document.blocks;
    blocks[index].dirty = true;
    let depth = blocks[index].list.as_ref().map_or(0, |list| list.depth);
    if depth > 0 {
        let subtree_end = group_repair::subtree_end(blocks, index);
        for block in &mut blocks[index..=subtree_end] {
            if let Some(list) = block.list.as_mut() {
                list.depth -= 1;
            }
            block.dirty = true;
        }

        // Exactly what Shift-Tab (RichBlockOperations.Outdent) does, so both give the same list.
        group_repair::adopt_level_style(blocks, index);
        group_repair::repair(document, index, subtree_end);
        return;
    }

    // The items after it become a list of their own; a nested follower (its child) is lifted with its run.
    let block = &mut blocks[index];
    block.kind = RichBlockKind::Paragraph;
    block.list = None;
    block.list_group = None;
    group_repair::repair(document, index, index);
}

fn leave_quote(document: &mut RichDocument, index: usize) {
    let block = &mut document.blocks[index];
    block.kind = RichBlockKind::Paragraph;
    block.quote_group = None;
    block.dirty = true;
    group_repair::split_all_into_runs(&mut document.blocks);
}

/// The gap given to a block an edit creates: one blank line, in the document's
/// line ending (the first line break found in the frontmatter, the blocks or
/// the trailing text).  Inside a group the writer regenerates separators
/// anyway; outside one this is what a new paragraph needs.
pub(crate) fn blank_line(document: &RichDocument) -> &'static str {
    let samples = std::iter::once(document.frontmatter.as_str())
        .chain(document.blocks.iter().flat_map(|block| {
            [
                block.separator_before.as_str(),
                block.original_source.as_str(),
            ]
        }))
        .chain(std::iter::once(document.trailing_text.as_str()));
    for sample in samples {
        if let Some(line_break) = sample.find('\n') {
            return if line_break > 0 && sample.as_bytes()[line_break - 1] == b'\r' {
                "\r\n\r\n"
            } else {
                "\n\n"
            };
        }
    }

    "\n\n"
}

/// The block's caret text: each run's text and one placeholder character per
/// atom.  Character stepping walks this rather than one run, so a combining
/// mark that starts a run still belongs to the letter before it.
fn plain_text(list: &[RichInline]) -> String {
    let mut text = String::new();
    for inline in list {
        match inline.text() {
            Some(run) => text.push_str(run),
            None => text.push('\u{FFFC}'),
        }
    }
    text
}

/// The caret stop before `offset` in the inlines: one atom, or one text
/// element (a combining sequence or emoji cluster is deleted whole, matching
/// the layout's caret stops).
pub(super) fn previous_stop(list: &[RichInline], offset: usize) -> usize {
    previous_text_element(&plain_text(list), offset)
}

/// The caret stop after `offset` in the inlines.
pub(super) fn next_stop(list: &[RichInline], offset: usize) -> usize {
    next_text_element(&plain_text(list), offset)
}

/// The start of the last text element of `text`'s first `offset` characters
/// (the last entry of C#'s
/// `StringInfo.ParseCombiningCharacters(text.Substring(0, offset))`), as a
/// character offset.  Text elements are grapheme clusters as
/// `crate::text::cluster_bounds` finds them.
pub(super) fn previous_text_element(text: &str, offset: usize) -> usize {
    let prefix = &text[..inlines::char_to_byte(text, offset)];
    let bounds = crate::text::cluster_bounds(prefix);
    // bounds ends with prefix.len(); the entry before it is the last element's start.
    let start = bounds.len().checked_sub(2).map_or(0, |i| bounds[i]);
    prefix[..start].chars().count()
}

/// `offset` plus the length of the text element starting there (C#'s
/// `offset + StringInfo.GetNextTextElementLength(text, offset)`), in
/// characters.
pub(super) fn next_text_element(text: &str, offset: usize) -> usize {
    let suffix = &text[inlines::char_to_byte(text, offset)..];
    let bounds = crate::text::cluster_bounds(suffix);
    let end = bounds.get(1).copied().unwrap_or(0);
    offset + suffix[..end].chars().count()
}
