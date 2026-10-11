//! agg-sharp's `RichEditOperationsTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichEditOperationsTests.cs`), ported 1:1:
//! typing, range deletes, Enter, Backspace and Delete on text blocks, and the
//! group invariants they keep.
//!
//! Caret offsets count characters, not UTF-16 units (see the
//! `markdown_editing` module header): the emoji test's offsets are one less
//! than C#'s past the emoji.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::widgets::markdown_editing::edit_operations as ops;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichAlignment, RichBlock, RichBlockKind, RichDocument, RichInline, RichSelection,
};

/// Plain text of a block with each atom shown as '@', so asserts read like what the user sees.
fn text(block: &RichBlock) -> String {
    block
        .inlines
        .iter()
        .map(|i| i.text().unwrap_or("@"))
        .collect()
}

fn texts(document: &RichDocument) -> String {
    document
        .blocks
        .iter()
        .map(text)
        .collect::<Vec<_>>()
        .join("|")
}

fn p(block: usize, offset: usize) -> DocPosition {
    DocPosition::new(block, offset)
}

fn same<T>(a: &Option<Rc<RefCell<T>>>, b: &Option<Rc<RefCell<T>>>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if Rc::ptr_eq(a, b))
}

/// True when the blocks belonging to the group form one unbroken run, the shape the writer rule needs.
fn is_contiguous<T>(
    document: &RichDocument,
    group: &Option<Rc<RefCell<T>>>,
    group_of: impl Fn(&RichBlock) -> &Option<Rc<RefCell<T>>>,
) -> bool {
    let indices: Vec<usize> = document
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| same(group_of(block), group))
        .map(|(index, _)| index)
        .collect();
    indices.is_empty() || indices[indices.len() - 1] - indices[0] + 1 == indices.len()
}

/// The list shape markdown can express: each list group starts at depth 0 and
/// an item is at most one level deeper than the item before it.
fn lists_are_well_formed(document: &RichDocument) -> bool {
    let mut group = None;
    let mut previous_depth: isize = -1;
    for block in &document.blocks {
        if block.list_group.is_none() {
            group = None;
            continue;
        }

        let allowed = if same(&block.list_group, &group) {
            previous_depth + 1
        } else {
            0
        };
        let depth = block.list.as_ref().expect("a list item").depth as isize;
        if depth > allowed {
            return false;
        }

        group = block.list_group.clone();
        previous_depth = depth;
    }

    true
}

fn depths(document: &RichDocument) -> String {
    document
        .blocks
        .iter()
        .map(|b| {
            b.list
                .as_ref()
                .map_or("p".to_string(), |l| l.depth.to_string())
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn whole(anchor: DocPosition, caret: DocPosition) -> RichSelection {
    RichSelection {
        anchor,
        caret,
        whole_block: true,
    }
}

fn is_run(inline: &RichInline, text: &str) -> bool {
    inline.text() == Some(text)
}

#[test]
fn typing_after_bold_continues_bold() {
    let mut document = parse("a **bold** c\n");
    let selection = ops::insert_text(&mut document, p(0, 6), "er", None).unwrap();

    let inlines = &document.blocks[0].inlines;
    assert_eq!(text(&document.blocks[0]), "a bolder c");
    assert_eq!(inlines[1].text(), Some("bolder"));
    assert!(inlines[1].bold);
    assert_eq!(inlines.len(), 3);
    assert_eq!(selection, RichSelection::at(p(0, 8)));
    assert!(document.blocks[0].dirty);
}

#[test]
fn typing_after_an_image_takes_the_preceding_runs_style() {
    let mut document = parse("**ab**![x](x.png)\n");
    ops::insert_text(&mut document, p(0, 3), "z", None).unwrap();

    assert_eq!(text(&document.blocks[0]), "ab@z");
    let typed = &document.blocks[0].inlines[2];
    assert!(typed.bold && is_run(typed, "z"));

    // An explicit pending style wins over the run before the caret.
    ops::insert_text(
        &mut document,
        p(0, 1),
        "p",
        Some(&RichInline::run("").italic()),
    )
    .unwrap();
    let typed = &document.blocks[0].inlines[1];
    assert!(typed.italic && !typed.bold && is_run(typed, "p"));
}

#[test]
fn typing_at_block_start_takes_the_first_runs_style() {
    let mut document = parse("**ab** c\n");
    ops::insert_text(&mut document, p(0, 0), "x", None).unwrap();
    let first = &document.blocks[0].inlines[0];
    assert!(first.bold && is_run(first, "xab"));
}

#[test]
fn inserted_lines_split_into_blocks() {
    let mut document = parse("- hello\n");
    let selection = ops::insert_text(&mut document, p(0, 5), "a\r\nb", None).unwrap();

    assert_eq!(texts(&document), "helloa|b");
    assert_eq!(document.blocks[1].kind, RichBlockKind::ListItem);
    assert!(same(
        &document.blocks[1].list_group,
        &document.blocks[0].list_group
    ));
    assert_eq!(selection.caret, p(1, 1));
    assert!(lists_are_well_formed(&document));
}

#[test]
fn delete_across_three_blocks_merges_the_end_into_the_start() {
    let mut document = parse("# One\n\ntwo\n\n- three\n- four\n");
    let selection = ops::delete_range(&mut document, p(2, 2), p(0, 1)).unwrap();

    assert_eq!(texts(&document), "Oree|four");
    let survivor = &document.blocks[0];
    assert_eq!(survivor.kind, RichBlockKind::Heading);
    assert_eq!(survivor.heading_level, 1);
    assert!(survivor.list_group.is_none());
    assert!(survivor.dirty);
    assert_eq!(selection, RichSelection::at(p(0, 1)));

    // The list lost a member, which the writer reads from its member count.
    let list = document.blocks[1].list_group.clone();
    assert_eq!(list.as_ref().unwrap().borrow().original_member_count, 2);
    let members = document
        .blocks
        .iter()
        .filter(|b| same(&b.list_group, &list))
        .count();
    assert_eq!(members, 1);
    assert!(lists_are_well_formed(&document));
}

#[test]
fn raw_blocks_in_a_range_are_removed_whole() {
    let document = parse("a\n\n---\n\nbc\n");
    assert_eq!(document.blocks[1].kind, RichBlockKind::Raw);

    let mut merged = document.deep_clone();
    ops::delete_range(&mut merged, p(0, 1), p(2, 1)).unwrap();
    assert_eq!(texts(&merged), "ac");

    // Starting on the Raw block selects part of it: it goes whole and nothing merges across it.
    let mut partial = document.deep_clone();
    let selection = ops::delete_range(&mut partial, p(1, 0), p(2, 1)).unwrap();
    assert_eq!(texts(&partial), "a|c");
    assert_eq!(selection, RichSelection::at(p(1, 0)));

    // Ending at the Raw block's start touches it without selecting it.
    let mut touching = document.deep_clone();
    ops::delete_range(&mut touching, p(0, 0), p(1, 0)).unwrap();
    assert_eq!(touching.blocks.len(), 3);
    assert_eq!(touching.blocks[1].kind, RichBlockKind::Raw);
}

#[test]
fn delete_across_an_align_group_edge_leaves_both_groups_consistent() {
    let mut document = parse(
        "<div align=\"center\">\n\none\n\ntwo\n\n</div>\n\n<div align=\"right\">\n\nthree\n\nfour\n\n</div>\n",
    );
    let center = document.blocks[0].align_group.clone();
    let right = document.blocks[2].align_group.clone();
    assert!(center.is_some());
    assert!(right.is_some());

    ops::delete_range(&mut document, p(1, 1), p(2, 2)).unwrap();

    assert_eq!(texts(&document), "one|tree|four");
    assert!(same(&document.blocks[1].align_group, &center));
    assert_eq!(document.blocks[1].alignment, RichAlignment::Center);
    assert!(same(&document.blocks[2].align_group, &right));
    assert!(is_contiguous(&document, &center, |b| &b.align_group));
    assert!(is_contiguous(&document, &right, |b| &b.align_group));
    let members = document
        .blocks
        .iter()
        .filter(|b| same(&b.align_group, &right))
        .count();
    assert_ne!(
        members,
        right.as_ref().unwrap().borrow().original_member_count
    );
}

#[test]
fn enter_in_the_middle_of_a_bold_run_splits_the_run() {
    let mut document = parse("a **bold** c\n");
    let selection = ops::split_block(&mut document, p(0, 4)).unwrap();

    assert_eq!(texts(&document), "a bo|ld c");
    let last = document.blocks[0].inlines.last().unwrap();
    assert!(last.bold && is_run(last, "bo"));
    let first = &document.blocks[1].inlines[0];
    assert!(first.bold && is_run(first, "ld"));
    assert_eq!(document.blocks[1].kind, RichBlockKind::Paragraph);
    assert!(document.blocks.iter().all(|b| b.dirty));
    assert_eq!(selection, RichSelection::at(p(1, 0)));
}

#[test]
fn enter_around_a_heading() {
    let mut start = parse("## Title\n");
    let selection = ops::split_block(&mut start, p(0, 0)).unwrap();
    assert_eq!(texts(&start), "|Title");
    assert_eq!(start.blocks[0].kind, RichBlockKind::Paragraph);
    assert_eq!(start.blocks[1].kind, RichBlockKind::Heading);
    assert_eq!(selection.caret, p(1, 0));

    let mut end = parse("## Title\n");
    ops::split_block(&mut end, p(0, 5)).unwrap();
    assert_eq!(texts(&end), "Title|");
    assert_eq!(end.blocks[1].kind, RichBlockKind::Paragraph);
    assert_eq!(end.blocks[1].heading_level, 0);

    // Splitting the middle keeps the heading on both halves.
    let mut middle = parse("## Title\n");
    ops::split_block(&mut middle, p(0, 2)).unwrap();
    assert_eq!(middle.blocks[1].kind, RichBlockKind::Heading);
    assert_eq!(middle.blocks[1].heading_level, 2);
}

#[test]
fn enter_on_an_empty_top_level_list_item_leaves_the_list() {
    let mut document = parse("- a\n- b\n- c\n");
    let list = document.blocks[0].list_group.clone();
    document.blocks[1].inlines.clear();

    let selection = ops::split_block(&mut document, p(1, 0)).unwrap();

    let left = &document.blocks[1];
    assert_eq!(document.blocks.len(), 3);
    assert_eq!(left.kind, RichBlockKind::Paragraph);
    assert!(left.list.is_none());
    assert!(left.list_group.is_none());
    assert!(left.dirty);
    assert_eq!(selection.caret, p(1, 0));

    // The items after it become their own list, so neither group spans the paragraph.
    assert!(same(&document.blocks[0].list_group, &list));
    let after = document.blocks[2].list_group.clone();
    assert!(after.is_some());
    assert!(!same(&after, &list));
    assert_eq!(after.unwrap().borrow().original_member_count, 0);
    assert!(lists_are_well_formed(&document));
}

#[test]
fn enter_on_an_empty_nested_list_item_outdents() {
    let mut document = parse("- a\n  - b\n");
    assert_eq!(document.blocks[1].list.as_ref().unwrap().depth, 1);
    document.blocks[1].inlines.clear();

    ops::split_block(&mut document, p(1, 0)).unwrap();

    let item = &document.blocks[1];
    assert_eq!(item.kind, RichBlockKind::ListItem);
    assert_eq!(item.list.as_ref().unwrap().depth, 0);
    assert!(same(&item.list_group, &document.blocks[0].list_group));
    assert!(item.dirty);
    assert!(lists_are_well_formed(&document));
}

#[test]
fn enter_on_an_empty_quote_paragraph_leaves_the_quote() {
    let mut document = parse("> a\n>\n> b\n");
    assert_eq!(document.blocks[1].kind, RichBlockKind::Quote);
    document.blocks[1].inlines.clear();

    ops::split_block(&mut document, p(1, 0)).unwrap();

    assert_eq!(document.blocks[1].kind, RichBlockKind::Paragraph);
    assert!(document.blocks[1].quote_group.is_none());
    assert!(document.blocks[0].quote_group.is_some());
}

#[test]
fn backspace_at_block_start() {
    let mut list = parse("- a\n  - b\n- c\n");
    ops::backspace(&mut list, p(1, 0)).unwrap();
    assert_eq!(list.blocks[1].list.as_ref().unwrap().depth, 0);
    ops::backspace(&mut list, p(2, 0)).unwrap();
    assert_eq!(list.blocks[2].kind, RichBlockKind::Paragraph);
    assert!(list.blocks[2].list_group.is_none());
    assert_eq!(texts(&list), "a|b|c");
    assert!(lists_are_well_formed(&list));

    let mut quote = parse("> q\n");
    ops::backspace(&mut quote, p(0, 0)).unwrap();
    assert_eq!(quote.blocks[0].kind, RichBlockKind::Paragraph);
    assert!(quote.blocks[0].quote_group.is_none());
    assert!(quote.blocks[0].dirty);

    let mut heading = parse("# h\n");
    ops::backspace(&mut heading, p(0, 0)).unwrap();
    assert_eq!(heading.blocks[0].kind, RichBlockKind::Paragraph);
    assert_eq!(heading.blocks[0].heading_level, 0);

    let mut paragraphs = parse("- item\n\n**p**\n");
    let selection = ops::backspace(&mut paragraphs, p(1, 0)).unwrap();
    assert_eq!(texts(&paragraphs), "itemp");
    assert_eq!(paragraphs.blocks[0].kind, RichBlockKind::ListItem);
    assert!(paragraphs.blocks[0].inlines[1].is_run() && paragraphs.blocks[0].inlines[1].bold);
    assert_eq!(selection, RichSelection::at(p(0, 4)));
}

#[test]
fn backspace_after_a_raw_block_selects_it_whole() {
    let mut document = parse("a\n\n---\n\nb\n");
    let selection = ops::backspace(&mut document, p(2, 0)).unwrap();

    assert_eq!(document.blocks.len(), 3);
    assert_eq!(selection, whole(p(1, 0), p(1, 1)));
    assert!(!document.blocks.iter().any(|b| b.dirty));
}

#[test]
fn backspace_and_delete_remove_whole_characters() {
    // C#'s offset 3 (after the two UTF-16 units of the emoji) is character offset 2.
    let mut document = parse("a\u{1F600}b\n");
    let selection = ops::backspace(&mut document, p(0, 2)).unwrap();
    assert_eq!(text(&document.blocks[0]), "ab");
    assert_eq!(selection.caret, p(0, 1));

    let mut forward = parse("a\u{1F600}b\n");
    ops::delete(&mut forward, p(0, 1)).unwrap();
    assert_eq!(text(&forward.blocks[0]), "ab");
}

#[test]
fn delete_at_block_end_merges_the_next_text_block() {
    let mut document = parse("# One\n\ntwo\n\n---\n");
    let selection = ops::delete(&mut document, p(0, 3)).unwrap();
    assert_eq!(texts(&document), "Onetwo|");
    assert_eq!(document.blocks[0].kind, RichBlockKind::Heading);
    assert_eq!(selection, RichSelection::at(p(0, 3)));

    // The next block is Raw: it is selected, not merged.
    let selection = ops::delete(&mut document, p(0, 6)).unwrap();
    assert_eq!(selection, whole(p(1, 0), p(1, 1)));
    assert_eq!(document.blocks.len(), 2);
}

#[test]
fn removing_a_block_between_two_lists_forces_both_to_regenerate() {
    let mut document = parse("- a\n- b\n\n---\n\n- c\n");
    let first = document.blocks[0].list_group.clone().unwrap();
    let second = document.blocks[3].list_group.clone().unwrap();
    assert!(!Rc::ptr_eq(&first, &second));

    ops::delete_range(&mut document, p(2, 0), p(2, 1)).unwrap();

    // Both lists are clean, but writing their bytes back to back would read as one loose list.
    assert_eq!(document.blocks.len(), 3);
    assert_eq!(first.borrow().original_member_count, 0);
    assert_eq!(second.borrow().original_member_count, 0);
    assert!(lists_are_well_formed(&document));

    let mut quotes = parse("> a\n\nb\n\n> c\n");
    ops::backspace(&mut quotes, p(1, 0)).unwrap();
    assert_eq!(texts(&quotes), "ab|c");
    let count = |i: usize| {
        quotes.blocks[i]
            .quote_group
            .as_ref()
            .unwrap()
            .borrow()
            .original_member_count
    };
    assert_eq!(count(0), 0);
    assert_eq!(count(1), 0);
}

#[test]
fn leaving_a_list_from_the_middle_lifts_the_nested_tail() {
    let mut document = parse("- a\n- b\n  - c\n    - d\n  - e\n");
    ops::backspace(&mut document, p(1, 0)).unwrap();

    assert_eq!(depths(&document), "0,p,0,1,0");
    assert!(document.blocks[2].dirty);
    assert!(lists_are_well_formed(&document));
}

#[test]
fn outdenting_an_item_takes_its_children_along() {
    let mut document = parse("- a\n  - b\n    - c\n  - d\n");
    ops::backspace(&mut document, p(1, 0)).unwrap();

    assert_eq!(depths(&document), "0,0,1,1");
    assert!(lists_are_well_formed(&document));

    // Deleting a parent's text merges its line away; deeper items left behind are lifted into shape.
    let mut merged = parse("- a\n  - b\n    - c\n");
    ops::delete_range(&mut merged, p(0, 1), p(1, 1)).unwrap();
    assert_eq!(texts(&merged), "a|c");
    assert!(lists_are_well_formed(&merged));
}

#[test]
fn new_blocks_use_the_documents_line_ending() {
    let mut document = parse("hello\r\n");
    ops::split_block(&mut document, p(0, 5)).unwrap();
    assert_eq!(document.blocks[1].separator_before, "\r\n\r\n");
}

#[test]
fn removing_the_first_block_keeps_the_leading_gap() {
    let mut document = parse("---\ntitle: x\n---\n\n<hr>\n\nb\n");
    let lead = document.blocks[0].separator_before.clone();
    assert_eq!(document.blocks[0].kind, RichBlockKind::Raw);

    ops::delete_range(&mut document, p(0, 0), p(0, 1)).unwrap();
    assert_eq!(texts(&document), "b");
    assert_eq!(document.blocks[0].separator_before, lead);
}

#[test]
fn backspace_at_a_raw_blocks_start_moves_to_the_previous_text_block() {
    let mut document = parse("ab\n\n---\n");
    let selection = ops::backspace(&mut document, p(1, 0)).unwrap();
    assert_eq!(selection, RichSelection::at(p(0, 2)));
    assert_eq!(document.blocks.len(), 2);
}

#[test]
fn typing_at_the_end_of_a_link_or_code_does_not_extend_it() {
    let mut document = parse("**[go](u)** `x`\n");
    ops::insert_text(&mut document, p(0, 2), "!", None).unwrap();
    let typed = &document.blocks[0].inlines[1];
    assert_eq!(typed.text(), Some("!"));
    assert!(typed.link_url.is_none());
    assert!(typed.bold);

    let end = document.blocks[0].text_length(0, 0);
    ops::insert_text(&mut document, p(0, end), "y", None).unwrap();
    let last = document.blocks[0].inlines.last().unwrap();
    assert!(!last.as_run().unwrap().code && is_run(last, "y"));

    // Inside or at the start of a link, typing still extends it.
    ops::insert_text(&mut document, p(0, 1), "o", None).unwrap();
    let first = &document.blocks[0].inlines[0];
    assert!(first.link_url.as_deref() == Some("u") && is_run(first, "goo"));
    ops::insert_text(&mut document, p(0, 0), "a", None).unwrap();
    let first = &document.blocks[0].inlines[0];
    assert!(first.link_url.as_deref() == Some("u") && is_run(first, "agoo"));
}

#[test]
fn combining_mark_across_a_run_boundary_is_one_character() {
    let mut document = parse("**e**\u{0301}x\n");
    assert_eq!(document.blocks[0].inlines.len(), 2);
    let selection = ops::backspace(&mut document, p(0, 2)).unwrap();
    assert_eq!(text(&document.blocks[0]), "x");
    assert_eq!(selection.caret, p(0, 0));

    let mut forward = parse("**e**\u{0301}x\n");
    ops::delete(&mut forward, p(0, 0)).unwrap();
    assert_eq!(text(&forward.blocks[0]), "x");
}

#[test]
fn inserting_nothing_changes_nothing() {
    let mut document = parse("a\n\n---\n");
    let selection = ops::insert_text(&mut document, p(1, 1), "", None).unwrap();
    assert_eq!(document.blocks.len(), 2);
    assert!(!document.blocks.iter().any(|b| b.dirty));
    assert_eq!(selection, RichSelection::at(p(1, 1)));
}

#[test]
fn an_empty_document_takes_typing_and_ignores_deletes() {
    let mut document = parse("");
    assert_eq!(
        ops::backspace(&mut document, p(0, 0)).unwrap(),
        RichSelection::at(p(0, 0))
    );
    assert_eq!(
        ops::delete(&mut document, p(0, 0)).unwrap(),
        RichSelection::at(p(0, 0))
    );
    assert_eq!(
        ops::delete_range(&mut document, p(0, 0), p(0, 1)).unwrap(),
        RichSelection::at(p(0, 0))
    );
    let whole_block = ops::whole_block(&document, 0);
    assert_eq!(
        ops::delete_selection(&mut document, whole_block).unwrap(),
        RichSelection::at(p(0, 0))
    );
    assert_eq!(document.blocks.len(), 0);

    // Typing into nothing starts the paragraph a first-time user expects to be there.
    let typed = ops::insert_text(&mut document, p(0, 0), "hi", None).unwrap();
    assert_eq!(texts(&document), "hi");
    assert_eq!(document.blocks[0].kind, RichBlockKind::Paragraph);
    assert_eq!(typed, RichSelection::at(p(0, 2)));

    let mut entered = parse("");
    assert_eq!(
        ops::split_block(&mut entered, p(0, 0)).unwrap(),
        RichSelection::at(p(1, 0))
    );
    assert_eq!(entered.blocks.len(), 2);
}
