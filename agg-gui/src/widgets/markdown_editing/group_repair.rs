//! Restores the group invariants after an edit changed which blocks belong to
//! which list, quote or alignment wrapper: each group is one contiguous run,
//! each list has one top-level kind and starts at depth 0 with no skipped
//! level, and different groups left side by side are regenerated.  Shared by
//! the text and block edit operations so both repair the model the same way.
//! Ports agg-sharp's `RichGroupRepair.cs`.

use std::cell::RefCell;
use std::rc::Rc;

use super::block::RichBlock;
use super::document::RichDocument;
use super::groups::{shared, RichAlignGroup, RichListGroup, RichQuoteGroup};

/// The member count every group kind carries (`RichBlockGroup`).
pub(super) trait MemberCount: Clone {
    fn set_original_member_count(&mut self, count: usize);
}

impl MemberCount for RichAlignGroup {
    fn set_original_member_count(&mut self, count: usize) {
        self.original_member_count = count;
    }
}

impl MemberCount for RichQuoteGroup {
    fn set_original_member_count(&mut self, count: usize) {
        self.original_member_count = count;
    }
}

impl MemberCount for RichListGroup {
    fn set_original_member_count(&mut self, count: usize) {
        self.original_member_count = count;
    }
}

type Group<T> = Option<Rc<RefCell<T>>>;

/// `starts_run(run's first block, block)`: whether `block` breaks its run.
type StartsRun<'a> = &'a dyn Fn(&RichBlock, &RichBlock) -> bool;

fn same<T>(a: &Group<T>, b: &Group<T>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// The whole repair for an operation that changed blocks `first..=last`
/// without adding or removing any: groups split into contiguous runs, lists
/// lifted and normalized, and different lists or quotes left touching around
/// the edit forced to regenerate.
pub(super) fn repair(document: &mut RichDocument, first: usize, last: usize) {
    split_all_into_runs(&mut document.blocks);
    normalize_list_depths(document);
    force_regeneration_around(document, first, last);
}

/// [`force_regeneration_if_joined`] for each pair of neighbours from
/// `first - 1` to `last + 1`.
pub(super) fn force_regeneration_around(document: &mut RichDocument, first: usize, last: usize) {
    let blocks = &document.blocks;
    if blocks.is_empty() {
        return;
    }
    for i in first.max(1)..=(blocks.len() - 1).min(last + 1) {
        force_regeneration_if_joined(&blocks[i - 1].list_group, &blocks[i].list_group);
        force_regeneration_if_joined(&blocks[i - 1].quote_group, &blocks[i].quote_group);
    }
}

/// An item that moved to another depth takes that level's style (kind,
/// marker, numbering and spacing) from its new previous sibling, so it
/// continues the list it joined instead of splitting it; the first item of a
/// new sublist keeps its kind and numbers from 1.  Its subtree keeps its own
/// levels' styles.
pub(super) fn adopt_level_style(blocks: &mut [RichBlock], index: usize) {
    let Some(depth) = blocks[index].list.as_ref().map(|l| l.depth) else {
        return;
    };
    let group = blocks[index].list_group.clone();
    let mut sibling = None;
    for i in (0..index).rev() {
        let Some(list) = blocks[i].list.as_ref() else {
            break;
        };
        if !same(&blocks[i].list_group, &group) || list.depth < depth {
            break;
        }
        if list.depth == depth {
            sibling = Some(list.clone());
            break;
        }
    }
    if let Some(list) = blocks[index].list.as_mut() {
        if let Some(sibling) = &sibling {
            list.ordered = sibling.ordered;
            list.marker = sibling.marker;
        }
        list.start_number = sibling.as_ref().map_or(1, |s| s.start_number);
        list.loose = sibling.as_ref().is_some_and(|s| s.loose);
    }
}

/// Makes every alignment, quote and list group one contiguous run (a list also
/// ends where its top-level kind switches between bullet and number, as
/// markdown's does), then lifts any list whose first item is nested so it
/// starts at the top level.
pub(super) fn split_all_into_runs(blocks: &mut [RichBlock]) {
    split_into_runs(blocks, |b| &mut b.align_group, None);
    split_into_runs(blocks, |b| &mut b.quote_group, None);
    let switches_kind = |run_start: &RichBlock, block: &RichBlock| {
        let (Some(start), Some(list)) = (&run_start.list, &block.list) else {
            return false;
        };
        list.depth == 0 && list.ordered != start.ordered
    };
    split_into_runs(blocks, |b| &mut b.list_group, Some(&switches_kind));
    lift_list_runs(blocks);
}

/// Gives every run of a group after its first a cloned group so each group
/// object is one contiguous run: one object spanning a gap would read to the
/// writer as one list or quote with a stranger inside.  The clone has
/// `original_member_count` 0, so it is always regenerated (its members need
/// not be dirty for that).  `starts_run(run's first block, block)` can also
/// break a run in the middle.
pub(super) fn split_into_runs<T: MemberCount>(
    blocks: &mut [RichBlock],
    group_of: impl Fn(&mut RichBlock) -> &mut Group<T>,
    starts_run: Option<StartsRun<'_>>,
) {
    let mut seen: Vec<Rc<RefCell<T>>> = Vec::new();
    let mut previous: Group<T> = None;
    let mut current: Group<T> = None;
    let mut run_start: Option<usize> = None;
    for index in 0..blocks.len() {
        // Compared against the previous block's group as it was before this pass reassigned it.
        let group = group_of(&mut blocks[index]).clone();
        let breaks = match (starts_run, run_start) {
            (Some(starts), Some(start)) => starts(&blocks[start], &blocks[index]),
            _ => false,
        };
        let continues = group.is_some() && same(&group, &previous) && !breaks;
        previous = group.clone();
        let Some(original) = group else { continue };
        if !continues {
            run_start = Some(index);
            if seen.iter().any(|g| Rc::ptr_eq(g, &original)) {
                let mut copy = original.borrow().clone();
                copy.set_original_member_count(0);
                current = Some(shared(copy));
            } else {
                seen.push(Rc::clone(&original));
                current = Some(Rc::clone(&original));
            }
        }
        if !same(&current, &Some(original)) {
            *group_of(&mut blocks[index]) = current.clone();
        }
    }
}

/// A list whose first item is nested (its parent left the list) has that
/// orphaned subtree (the leading items at least that deep) lifted by the
/// first item's depth, keeping their depths relative to each other.  Items
/// after it (the next top-level item and its own children) keep their depths.
pub(super) fn lift_list_runs(blocks: &mut [RichBlock]) {
    for i in 0..blocks.len() {
        let Some(group) = blocks[i].list_group.clone() else {
            continue;
        };
        let Some(shift) = blocks[i].list.as_ref().map(|l| l.depth) else {
            continue;
        };
        let continues = i > 0 && same(&blocks[i - 1].list_group, &Some(group.clone()));
        if continues || shift == 0 {
            continue;
        }
        let group = Some(group);
        for block in blocks[i..].iter_mut() {
            if !same(&block.list_group, &group) {
                break;
            }
            let Some(list) = block.list.as_mut() else {
                break;
            };
            if list.depth < shift {
                break;
            }
            list.depth -= shift;
            block.dirty = true;
        }
    }
}

/// Lifts list items so each list starts at depth 0 and no item is more than
/// one level deeper than the item before it (deeper has no markdown form).
/// Removing a parent item's line is what usually breaks this.
pub(super) fn normalize_list_depths(document: &mut RichDocument) {
    let mut group: Group<RichListGroup> = None;
    let mut previous_depth: usize = 0;
    for block in document.blocks.iter_mut() {
        if block.list_group.is_none() {
            group = None;
            continue;
        }
        let allowed = if group.is_some() && same(&block.list_group, &group) {
            previous_depth + 1
        } else {
            0
        };
        if let Some(list) = block.list.as_mut() {
            if list.depth > allowed {
                list.depth = allowed;
                block.dirty = true;
            }
            previous_depth = list.depth;
        }
        group = block.list_group.clone();
    }
}

/// Two different lists or quotes side by side are both forced to regenerate:
/// their clean bytes written back to back would read as one loose list or one
/// quote.  An `original_member_count` of 0 never matches a member count.
pub(super) fn force_regeneration_if_joined<T: MemberCount>(before: &Group<T>, after: &Group<T>) {
    if let (Some(before), Some(after)) = (before, after) {
        if !Rc::ptr_eq(before, after) {
            before.borrow_mut().set_original_member_count(0);
            after.borrow_mut().set_original_member_count(0);
        }
    }
}

/// The index of the last item in the subtree of the list item at `index`: the
/// run of deeper items of the same list right after it (the item itself when
/// it has none).
pub(super) fn subtree_end(blocks: &[RichBlock], index: usize) -> usize {
    let item = &blocks[index];
    let depth = item.list.as_ref().map_or(0, |l| l.depth);
    let mut end = index;
    while end + 1 < blocks.len()
        && same(&blocks[end + 1].list_group, &item.list_group)
        && blocks[end + 1]
            .list
            .as_ref()
            .is_some_and(|l| l.depth > depth)
    {
        end += 1;
    }
    end
}
