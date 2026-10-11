//! Turning text blocks into list items or quote paragraphs, and the group
//! repair a block operation ends with: the container half of agg-sharp's
//! `RichBlockOperations.cs` (`MakeListItems`, `MakeQuotes`, `RepairGroups`,
//! `JoinIfTouching`, `Absorb`, `UnifyTopLevelMarkers`, `ListOrdered`).  The
//! public block operations that call these are [`super::block_operations`];
//! the invariant repair both share with the text edits is `group_repair`.

use std::cell::RefCell;
use std::rc::Rc;

use super::block::{RichAlignment, RichBlock, RichBlockKind, RichListInfo};
use super::block_operations::text_blocks;
use super::document::RichDocument;
use super::group_repair::{self, Group, MemberCount};
use super::groups::{shared, RichListGroup, RichListGroupRef, RichQuoteGroup};

/// Restores the group invariants after an op changed blocks `first..=last`:
/// each group is one contiguous run, each list has one top-level kind and
/// starts at depth 0 with no skipped level, and lists or quotes touching the
/// edit are joined (`join`) or, where they stay apart, forced to regenerate.
pub(super) fn repair_groups(document: &mut RichDocument, first: usize, last: usize, join: bool) {
    if !join {
        group_repair::repair(document, first, last);
        return;
    }

    group_repair::split_all_into_runs(&mut document.blocks);
    let count = document.blocks.len();
    if count > 1 {
        for i in first.max(1)..=(count - 1).min(last + 1) {
            join_if_touching(document, i);
        }
    }

    unify_top_level_markers(document, first, last);
    group_repair::normalize_list_depths(document);
    group_repair::force_regeneration_around(document, first, last);
}

/// Strips what a block cannot keep inside a list or quote: heading level,
/// alignment and its wrapper.
fn clear_for_container(block: &mut RichBlock) {
    block.heading_level = 0;
    block.alignment = RichAlignment::Left;
    block.align_group = None;
    block.dirty = true;
}

/// Turns the text blocks `first..=last` into list items of one kind.  A nested
/// item converts in place and stays in its parent's list — taking it out would
/// tear it from its parent.  A top-level item and a new item (headings
/// included; new items are top-level) join the list above when it is of the
/// same kind, otherwise a new list, and a top-level item brings its unselected
/// subtree along so children stay with their parent.  `repair_groups` then
/// splits off whatever no longer lines up and joins a matching list below.
pub(super) fn make_list_items(
    document: &mut RichDocument,
    first: usize,
    last: usize,
    ordered: bool,
    marker: char,
    start_number: i32,
) {
    let above = if first > 0 {
        document.blocks[first - 1].list_group.clone()
    } else {
        None
    };
    let group: RichListGroupRef = match above {
        Some(above) if list_ordered(document, &above) == ordered => above,
        _ => shared(RichListGroup::default()),
    };
    group.borrow_mut().original_member_count = 0;
    let group_slot = Some(Rc::clone(&group));
    let blocks = &mut document.blocks;
    let mut end = last;
    for i in first..=last {
        if !blocks[i].is_text_block() {
            continue;
        }

        if blocks[i].kind == RichBlockKind::ListItem {
            let block = &mut blocks[i];
            let mut depth = 0;
            if let Some(list) = block.list.as_mut() {
                if list.ordered != ordered {
                    list.ordered = ordered;
                    list.marker = marker;
                    list.start_number = start_number;
                    if let Some(own) = &block.list_group {
                        own.borrow_mut().original_member_count = 0;
                    }
                    block.dirty = true;
                }
                depth = list.depth;
            }

            if depth > 0 {
                continue;
            }

            let subtree_end = group_repair::subtree_end(blocks, i);
            for child in &mut blocks[i + 1..=subtree_end] {
                if !group_repair::same(&child.list_group, &group_slot) {
                    child.list_group = group_slot.clone();
                    child.dirty = true;
                }
            }

            end = end.max(subtree_end);
        } else {
            let block = &mut blocks[i];
            block.quote_group = None;
            clear_for_container(block);
            block.kind = RichBlockKind::ListItem;
            block.list = Some(RichListInfo {
                ordered,
                marker,
                start_number,
                ..RichListInfo::default()
            });
        }

        let block = &mut blocks[i];
        if !group_repair::same(&block.list_group, &group_slot) {
            block.list_group = group_slot.clone();
            block.dirty = true;
        }
    }

    repair_groups(document, first, end, true);
}

/// Turns the text blocks `first..=last` into paragraphs of one quote: the
/// quote above when one touches the selection, otherwise a new one (a quote
/// below is joined by `repair_groups`).
pub(super) fn make_quotes(document: &mut RichDocument, first: usize, last: usize) {
    let above = if first > 0 {
        document.blocks[first - 1].quote_group.clone()
    } else {
        None
    };
    let group = above.unwrap_or_else(|| shared(RichQuoteGroup::default()));
    group.borrow_mut().original_member_count = 0;
    let group_slot = Some(group);
    for block in text_blocks(document, first, last) {
        if block.kind == RichBlockKind::Quote && group_repair::same(&block.quote_group, &group_slot)
        {
            continue;
        }

        block.list = None;
        block.list_group = None;
        clear_for_container(block);
        block.kind = RichBlockKind::Quote;
        block.quote_group = group_slot.clone();
    }

    repair_groups(document, first, last, true);
}

/// Whether a list is numbered: the kind of its first (top-level) item.  A list
/// has one top-level kind; `repair_groups` splits a list where that changes.
fn list_ordered(document: &RichDocument, group: &RichListGroupRef) -> bool {
    let slot = Some(Rc::clone(group));
    document
        .blocks
        .iter()
        .find(|b| group_repair::same(&b.list_group, &slot))
        .and_then(|b| b.list.as_ref())
        .is_some_and(|l| l.ordered)
}

/// Joins the list or quote starting at block `index` into a different one
/// ending right above it — lists only when both are of the same top-level
/// kind.
fn join_if_touching(document: &mut RichDocument, index: usize) {
    let before = &document.blocks[index - 1];
    let after = &document.blocks[index];
    if let (Some(into), Some(_)) = (&before.quote_group, &after.quote_group) {
        if !group_repair::same(&before.quote_group, &after.quote_group) {
            let into = Rc::clone(into);
            absorb(&mut document.blocks, index, into, |b| &mut b.quote_group);
        }
    }

    let before = &document.blocks[index - 1];
    let after = &document.blocks[index];
    if let (Some(into), Some(absorbed)) = (&before.list_group, &after.list_group) {
        if !Rc::ptr_eq(into, absorbed)
            && list_ordered(document, into) == list_ordered(document, absorbed)
        {
            let into = Rc::clone(into);
            absorb(&mut document.blocks, index, into, |b| &mut b.list_group);
        }
    }
}

/// Moves the run of blocks from `index` that share block `index`'s group into
/// `into`, marking them dirty; `into` then regenerates.
fn absorb<T: MemberCount>(
    blocks: &mut [RichBlock],
    index: usize,
    into: Rc<RefCell<T>>,
    group_of: impl Fn(&mut RichBlock) -> &mut Group<T>,
) {
    let absorbed = group_of(&mut blocks[index]).clone();
    let into_slot = Some(Rc::clone(&into));
    for block in blocks[index..].iter_mut() {
        let slot = group_of(block);
        if !group_repair::same(slot, &absorbed) {
            break;
        }
        *slot = into_slot.clone();
        block.dirty = true;
    }

    into.borrow_mut().set_original_member_count(0);
}

/// Top-level items of a list share one marker and start number, taken from its
/// first item, so items that joined a list number on from it rather than
/// restarting.
fn unify_top_level_markers(document: &mut RichDocument, first: usize, last: usize) {
    let blocks = &mut document.blocks;
    if blocks.is_empty() {
        return;
    }

    // Distinct by identity, in first-appearance order (C#'s HashSet; the order cannot reach an output).
    let mut groups: Vec<RichListGroupRef> = Vec::new();
    for block in &blocks[first.saturating_sub(1)..=(blocks.len() - 1).min(last + 1)] {
        if let Some(group) = &block.list_group {
            if !groups.iter().any(|g| Rc::ptr_eq(g, group)) {
                groups.push(Rc::clone(group));
            }
        }
    }

    for group in groups {
        let slot = Some(group);
        let Some(lead) = blocks
            .iter()
            .find(|b| group_repair::same(&b.list_group, &slot))
            .and_then(|b| b.list.clone())
        else {
            continue;
        };
        for item in blocks.iter_mut() {
            if !group_repair::same(&item.list_group, &slot) {
                continue;
            }
            let Some(list) = item.list.as_mut() else {
                continue;
            };
            if list.depth != 0 {
                continue;
            }
            if list.marker != lead.marker || list.start_number != lead.start_number {
                list.marker = lead.marker;
                list.start_number = lead.start_number;
                item.dirty = true;
            }
        }
    }
}
