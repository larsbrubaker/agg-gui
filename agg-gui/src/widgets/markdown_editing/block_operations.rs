//! Block-level formatting on a [`RichDocument`]: Normal/Heading, bullet and
//! numbered lists, quotes, alignment, list indent/outdent and the line-start
//! shortcuts (`# `, `- `, `1. `, `> `), plus the toolbar's [`RichBlockState`].
//! Ports agg-sharp's `RichBlockOperations.cs`; turning blocks into list items
//! or quotes and the group joining that follows live in
//! [`super::block_containers`].
//!
//! Each op applies to every text block the selection touches (a selection
//! ending at the very start of a block does not touch it, as in other
//! editors), mutates in place and marks changed blocks dirty.  No op adds or
//! removes blocks, so the caller's selection stays valid.  A selection past
//! the document's end touches nothing, as in C#.
//!
//! Group rules live in `group_repair`, shared with
//! [`super::edit_operations`]: a block that changes kind leaves its list or
//! quote, a re-aligned block leaves its alignment group; whenever a group
//! stops being one contiguous run, each later run gets a cloned group with
//! `original_member_count` 0; lists start at depth 0 with no level skipped;
//! two different lists or quotes left side by side around the edit are forced
//! to regenerate.  Turning a list or quote on joins a touching list of the
//! same kind or a touching quote, since a first-time user sees one list there.

use super::block::{RichAlignment, RichBlock, RichBlockKind};
use super::block_containers::{make_list_items, make_quotes, repair_groups};
use super::document::{DocPosition, RichDocument};
use super::edit_error::RichEditError;
use super::group_repair;
use super::inline::RichInline;
use super::inlines;
use super::selection::RichSelection;

/// What the selected blocks' lists are, for the toolbar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RichListKind {
    /// No selected block is a list item.
    #[default]
    None,
    /// Every selected block is a bullet item.
    Bullet,
    /// Every selected block is a numbered item.
    Numbered,
    /// Some selected blocks are list items, but they are not all items of one
    /// kind.
    Mixed,
}

/// What the formatting toolbar shows for a selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RichBlockState {
    /// 0 for Normal (list items and quotes read as Normal), 1-6 for a heading,
    /// `None` when the blocks disagree.
    pub heading_level: Option<u8>,
    /// Bullet / Numbered only when every selected block is an item of that
    /// kind.
    pub list: RichListKind,
    /// True when every selected block is a quote paragraph.
    pub quote: bool,
    /// The alignment of the selected paragraphs and headings; `None` when they
    /// disagree or there are none.
    pub alignment: Option<RichAlignment>,
    /// False when no paragraph or heading is selected: alignment is not offered
    /// in lists and quotes.
    pub can_align: bool,
}

/// Makes every selected text block a paragraph (`heading_level` 0, "Normal")
/// or a heading of that level.  List items and quotes leave their list or
/// quote; paragraphs and headings keep their alignment.  A level above 6 is
/// [`RichEditError::HeadingLevelOutOfRange`] (C#'s
/// `ArgumentOutOfRangeException`).
pub fn set_block_kind(
    document: &mut RichDocument,
    selection: RichSelection,
    heading_level: u8,
) -> Result<(), RichEditError> {
    if heading_level > 6 {
        return Err(RichEditError::HeadingLevelOutOfRange(heading_level));
    }

    let Some((first, last)) = block_range(document, selection) else {
        return Ok(());
    };
    let kind = if heading_level == 0 {
        RichBlockKind::Paragraph
    } else {
        RichBlockKind::Heading
    };
    for block in text_blocks(document, first, last) {
        if block.kind == kind && block.heading_level == heading_level {
            continue;
        }

        leave_list_and_quote(block);
        block.kind = kind;
        block.heading_level = heading_level;
        block.dirty = true;
    }

    repair_groups(document, first, last, false);
    Ok(())
}

/// The bullet / numbered toolbar button.  When every selected block is
/// already a list item of that kind they all become paragraphs; otherwise all
/// become list items of that kind: an item of the other kind converts in
/// place, anything else (headings included — a list item holds one paragraph)
/// becomes a top-level item, joining a touching list of the same kind above or
/// below.
pub fn toggle_list(document: &mut RichDocument, selection: RichSelection, ordered: bool) {
    let Some((first, last)) = block_range(document, selection) else {
        return;
    };
    let selected: Vec<&RichBlock> = document.blocks[first..=last]
        .iter()
        .filter(|b| b.is_text_block())
        .collect();
    if selected.is_empty() {
        return;
    }

    let all_this_kind = selected.iter().all(|b| {
        b.kind == RichBlockKind::ListItem && b.list.as_ref().is_some_and(|l| l.ordered == ordered)
    });
    if all_this_kind {
        make_paragraphs(document, first, last);
        return;
    }

    make_list_items(
        document,
        first,
        last,
        ordered,
        if ordered { '.' } else { '-' },
        1,
    );
}

/// The quote toolbar button: when every selected block is a quote paragraph
/// they all leave the quote; otherwise all become paragraphs of one quote,
/// joining a quote that touches the selection.
pub fn toggle_quote(document: &mut RichDocument, selection: RichSelection) {
    let Some((first, last)) = block_range(document, selection) else {
        return;
    };
    let mut selected = document.blocks[first..=last]
        .iter()
        .filter(|b| b.is_text_block())
        .peekable();
    if selected.peek().is_none() {
        return;
    }

    if selected.all(|b| b.kind == RichBlockKind::Quote) {
        make_paragraphs(document, first, last);
        return;
    }

    make_quotes(document, first, last);
}

/// Aligns the selected paragraphs and headings.  List items and quotes are
/// skipped: alignment is not offered inside them.  A re-aligned block leaves
/// its `<div align>` group; the writer derives new wrappers.
pub fn set_alignment(
    document: &mut RichDocument,
    selection: RichSelection,
    alignment: RichAlignment,
) {
    let Some((first, last)) = block_range(document, selection) else {
        return;
    };
    for block in text_blocks(document, first, last) {
        if !can_align(block) || block.alignment == alignment {
            continue;
        }

        block.alignment = alignment;
        block.align_group = None;
        block.dirty = true;
    }

    repair_groups(document, first, last, false);
}

/// Tab in a list: each selected item goes one level deeper with its subtree
/// (the deeper items after it), but only under an item above it in the same
/// list that is at least as deep — markdown cannot skip a level, and the first
/// item of a list has nothing to nest under.  Returns true when the selection
/// holds list items, so the caller knows Tab was a list command even when
/// nothing could move.
pub fn indent(document: &mut RichDocument, selection: RichSelection) -> bool {
    let Some((first, last)) = block_range(document, selection) else {
        return false;
    };
    let blocks = &mut document.blocks;
    let mut any_item = false;
    let mut moved_through: Option<usize> = None;
    for i in first..=last {
        if blocks[i].kind != RichBlockKind::ListItem {
            continue;
        }

        any_item = true;

        // An item moved as part of an earlier item's subtree is already one level deeper.
        if moved_through.is_some_and(|through| i <= through) {
            continue;
        }

        let depth = blocks[i].list.as_ref().map_or(0, |l| l.depth);
        let can_nest = i > 0
            && group_repair::same(&blocks[i - 1].list_group, &blocks[i].list_group)
            && blocks[i - 1]
                .list
                .as_ref()
                .is_some_and(|l| l.depth >= depth);
        if !can_nest {
            continue;
        }

        let through = group_repair::subtree_end(blocks, i);
        for block in &mut blocks[i..=through] {
            if let Some(list) = block.list.as_mut() {
                list.depth += 1;
            }
            block.dirty = true;
        }

        moved_through = Some(through);
        group_repair::adopt_level_style(blocks, i);
    }

    repair_groups(document, first, last, false);
    any_item
}

/// Shift-Tab in a list: each selected item comes up one level with its
/// subtree; a top-level item leaves the list and becomes a paragraph (the
/// items after it become a list of their own, lifted to start at the top
/// level).  Returns true when the selection holds list items.
pub fn outdent(document: &mut RichDocument, selection: RichSelection) -> bool {
    let Some((first, last)) = block_range(document, selection) else {
        return false;
    };
    let blocks = &mut document.blocks;
    let mut any_item = false;
    let mut moved_through: Option<usize> = None;
    for i in first..=last {
        if blocks[i].kind != RichBlockKind::ListItem {
            continue;
        }

        any_item = true;
        if moved_through.is_some_and(|through| i <= through) {
            continue;
        }

        blocks[i].dirty = true;
        if blocks[i].list.as_ref().map_or(0, |l| l.depth) == 0 {
            // Its children are lifted by repair_groups once they are split off as a list of their own.
            leave_list_and_quote(&mut blocks[i]);
            blocks[i].kind = RichBlockKind::Paragraph;
            continue;
        }

        let through = group_repair::subtree_end(blocks, i);
        for block in &mut blocks[i..=through] {
            if let Some(list) = block.list.as_mut() {
                list.depth -= 1;
            }
            block.dirty = true;
        }

        moved_through = Some(through);
        group_repair::adopt_level_style(blocks, i);
    }

    repair_groups(document, first, last, false);
    any_item
}

/// Called after the user types a space.  In a Paragraph whose text before the
/// caret is exactly a marker and that space — `#`, `##`, `###` (heading 1-3),
/// `-` or `*` (bullet), `N.` (numbered from N) or `>` (quote) — removes the
/// marker, converts the block (joining a touching list or quote as the toolbar
/// does) and returns the caret at the block's start.  Returns `None` and
/// changes nothing otherwise (C#'s `false`, its `out` caret left where it
/// was).
pub fn try_apply_line_start_shortcut(
    document: &mut RichDocument,
    caret: DocPosition,
) -> Option<DocPosition> {
    let index = caret.block_index;
    let block = document.blocks.get(index)?;
    if block.kind != RichBlockKind::Paragraph || caret.offset < 2 {
        return None;
    }

    let typed = text_before(&block.inlines, caret.offset)?;
    let marker = typed.strip_suffix(' ')?;
    let convert = if marker == "#" || marker == "##" || marker == "###" {
        Shortcut::Heading(marker.len() as u8)
    } else if marker == "-" || marker == "*" {
        Shortcut::Bullet(marker.chars().next().unwrap_or('-'))
    } else if marker == ">" {
        Shortcut::Quote
    } else {
        Shortcut::Numbered(list_number(marker)?)
    };

    let block = &mut document.blocks[index];
    // text_before found `caret.offset` characters of plain runs, so the offset is within the inlines.
    let end = inlines::split_at(&mut block.inlines, caret.offset).ok()?;
    block.inlines.drain(..end);
    block.dirty = true;
    match convert {
        Shortcut::Heading(level) => {
            // A level of 1-3 is always valid.
            set_block_kind(
                document,
                RichSelection::at(DocPosition::new(index, 0)),
                level,
            )
            .ok()?;
        }
        Shortcut::Bullet(marker) => make_list_items(document, index, index, false, marker, 1),
        Shortcut::Quote => make_quotes(document, index, index),
        Shortcut::Numbered(start) => make_list_items(document, index, index, true, '.', start),
    }

    Some(DocPosition::new(index, 0))
}

/// What a line-start shortcut turns its paragraph into.
enum Shortcut {
    Heading(u8),
    Bullet(char),
    Quote,
    Numbered(i32),
}

/// The toolbar state for the text blocks the selection touches (Normal, no
/// list, no quote, alignment disabled when it touches none).
pub fn block_state_at(document: &RichDocument, selection: RichSelection) -> RichBlockState {
    let none = RichBlockState {
        heading_level: Some(0),
        ..RichBlockState::default()
    };
    let Some((first, last)) = block_range(document, selection) else {
        return none;
    };
    let selected: Vec<&RichBlock> = document.blocks[first..=last]
        .iter()
        .filter(|b| b.is_text_block())
        .collect();
    if selected.is_empty() {
        return none;
    }

    let levels = distinct(selected.iter().map(|b| {
        if b.kind == RichBlockKind::Heading {
            b.heading_level
        } else {
            0
        }
    }));
    let list_kinds = distinct(selected.iter().map(|b| match (&b.kind, &b.list) {
        (RichBlockKind::ListItem, Some(list)) if list.ordered => RichListKind::Numbered,
        (RichBlockKind::ListItem, _) => RichListKind::Bullet,
        _ => RichListKind::None,
    }));
    let list = if list_kinds.len() == 1 {
        list_kinds[0]
    } else if list_kinds.iter().any(|k| *k != RichListKind::None) {
        RichListKind::Mixed
    } else {
        RichListKind::None
    };
    let alignments = distinct(
        selected
            .iter()
            .filter(|b| can_align(b))
            .map(|b| b.alignment),
    );
    RichBlockState {
        heading_level: (levels.len() == 1).then(|| levels[0]),
        list,
        quote: selected.iter().all(|b| b.kind == RichBlockKind::Quote),
        alignment: (alignments.len() == 1).then(|| alignments[0]),
        can_align: !alignments.is_empty(),
    }
}

/// The distinct values in first-appearance order (LINQ's `Distinct`).
fn distinct<T: PartialEq>(values: impl Iterator<Item = T>) -> Vec<T> {
    let mut out = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

fn can_align(block: &RichBlock) -> bool {
    matches!(
        block.kind,
        RichBlockKind::Paragraph | RichBlockKind::Heading
    )
}

/// `N.` with one to nine digits — CommonMark's limit, which also keeps the
/// number within an `i32`.
fn list_number(marker: &str) -> Option<i32> {
    let digits = marker.strip_suffix('.')?;
    let valid =
        !digits.is_empty() && digits.len() <= 9 && digits.bytes().all(|c| c.is_ascii_digit());
    if !valid {
        return None;
    }
    digits.parse().ok()
}

/// The blocks a selection touches.  A non-empty selection that ends at offset
/// 0 of a later block stops before it: dragging to the start of the next line
/// selects the lines above, not that one.  `None` when it touches no block (an
/// empty document, or a selection past the end), so every op is a no-op there.
pub(super) fn block_range(
    document: &RichDocument,
    selection: RichSelection,
) -> Option<(usize, usize)> {
    let start = selection.start();
    let end = selection.end();
    let mut last = end.block_index;
    if !selection.is_empty()
        && last > start.block_index
        && end.offset == 0
        && end.row == 0
        && end.column == 0
    {
        last -= 1;
    }

    let last = last.min(document.blocks.len().checked_sub(1)?);
    (start.block_index <= last).then_some((start.block_index, last))
}

/// The text blocks `first..=last`, mutably.
pub(super) fn text_blocks(
    document: &mut RichDocument,
    first: usize,
    last: usize,
) -> impl Iterator<Item = &mut RichBlock> {
    document.blocks[first..=last]
        .iter_mut()
        .filter(|b| b.is_text_block())
}

/// Drops a block's list and quote membership; the caller sets its new kind.
/// The alignment group is kept: only a paragraph or heading is ever in one,
/// and those stay aligned when they change between the two.
fn leave_list_and_quote(block: &mut RichBlock) {
    block.list = None;
    block.list_group = None;
    block.quote_group = None;
}

fn make_paragraphs(document: &mut RichDocument, first: usize, last: usize) {
    for block in text_blocks(document, first, last) {
        leave_list_and_quote(block);
        block.kind = RichBlockKind::Paragraph;
        block.dirty = true;
    }

    repair_groups(document, first, last, false);
}

/// The plain text of the first `offset` characters, or `None` when an atom or
/// inline code is in it (a marker typed in code or after an image is content,
/// not a shortcut) or the block is shorter.
fn text_before(list: &[RichInline], offset: usize) -> Option<String> {
    let mut text = String::new();
    let mut length = 0;
    for inline in list {
        if length >= offset {
            break;
        }

        let run = inline.as_run().filter(|run| !run.code)?;
        text.push_str(&run.text);
        length += run.text.chars().count();
    }

    (length >= offset).then(|| text.chars().take(offset).collect())
}
