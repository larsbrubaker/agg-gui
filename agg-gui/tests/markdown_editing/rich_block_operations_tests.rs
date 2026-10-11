//! agg-sharp's `RichBlockOperationsTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichBlockOperationsTests.cs`), ported 1:1:
//! headings, lists, quotes, alignment, indent/outdent, the line-start
//! shortcuts and the toolbar's block state, with the group invariants they
//! keep.
//!
//! C#'s `TryApplyLineStartShortcut(..., out caret)` returns `Option` here: a
//! refused shortcut is `None` (C#'s `false` with the caret unchanged).

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::widgets::markdown_editing::block_operations as block_ops;
use agg_gui::widgets::markdown_editing::edit_operations;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichAlignment, RichBlock, RichBlockKind, RichBlockState, RichDocument,
    RichEditError, RichInline, RichListKind, RichSelection,
};

fn text(block: &RichBlock) -> String {
    block
        .inlines
        .iter()
        .map(|i| i.text().unwrap_or("@"))
        .collect()
}

fn p(block: usize, offset: usize) -> DocPosition {
    DocPosition::new(block, offset)
}

// A selection from the start of block a to the end of block b.
fn blocks(document: &RichDocument, a: usize, b: usize) -> RichSelection {
    RichSelection::new(p(a, 0), p(b, document.blocks[b].text_length(0, 0)))
}

fn caret(block: usize) -> RichSelection {
    RichSelection::at(p(block, 0))
}

// One letter per block: p paragraph, hN heading, b/n bullet/numbered item with its depth, q quote, r raw.
fn shape(document: &RichDocument) -> String {
    document
        .blocks
        .iter()
        .map(|b| match b.kind {
            RichBlockKind::Paragraph => "p".to_string(),
            RichBlockKind::Heading => format!("h{}", b.heading_level),
            RichBlockKind::ListItem => {
                let list = b.list.as_ref().expect("a list item has list info");
                format!("{}{}", if list.ordered { "n" } else { "b" }, list.depth)
            }
            RichBlockKind::Quote => "q".to_string(),
            _ => "r".to_string(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn same<T>(a: &Option<Rc<RefCell<T>>>, b: &Option<Rc<RefCell<T>>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// The index of a group among the distinct groups, by first appearance.
fn index_of<T>(groups: &mut Vec<Rc<RefCell<T>>>, group: &Rc<RefCell<T>>) -> usize {
    match groups.iter().position(|g| Rc::ptr_eq(g, group)) {
        Some(index) => index,
        None => {
            groups.push(Rc::clone(group));
            groups.len() - 1
        }
    }
}

// Each block's list info and the index of its list (by first appearance), to compare two models exactly.
fn list_model(document: &RichDocument) -> String {
    let mut groups = Vec::new();
    document
        .blocks
        .iter()
        .map(|b| match (&b.list, &b.list_group) {
            (Some(list), Some(group)) => format!(
                "{}:{}{}{}d{}",
                index_of(&mut groups, group),
                if list.ordered { "True" } else { "False" },
                list.marker,
                list.start_number,
                list.depth
            ),
            _ => "-".to_string(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// True when the blocks holding `group` (by `group_of`) form one unbroken run.
fn is_one_run<T>(
    document: &RichDocument,
    group: &Rc<RefCell<T>>,
    group_of: impl Fn(&RichBlock) -> &Option<Rc<RefCell<T>>>,
) -> bool {
    let slot = Some(Rc::clone(group));
    let indices: Vec<usize> = document
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| same(group_of(b), &slot))
        .map(|(i, _)| i)
        .collect();
    indices[indices.len() - 1] - indices[0] + 1 == indices.len()
}

/// Every group the blocks reference is one unbroken run, each list starts at depth 0 with no skipped level
/// and has one top-level kind, and members agree with their kind (only list items in lists, quotes in quotes).
fn groups_are_well_formed(document: &RichDocument) -> bool {
    for block in &document.blocks {
        let runs = block
            .align_group
            .as_ref()
            .map_or(true, |g| is_one_run(document, g, |b| &b.align_group))
            && block
                .quote_group
                .as_ref()
                .map_or(true, |g| is_one_run(document, g, |b| &b.quote_group))
            && block
                .list_group
                .as_ref()
                .map_or(true, |g| is_one_run(document, g, |b| &b.list_group));
        if !runs {
            return false;
        }
    }

    let mut list = None;
    let mut previous_depth: isize = -1;
    let mut top_ordered = false;
    for block in &document.blocks {
        let aligned_kind = matches!(
            block.kind,
            RichBlockKind::Paragraph | RichBlockKind::Heading
        );
        if (block.kind == RichBlockKind::ListItem) != block.list_group.is_some()
            || (block.kind == RichBlockKind::Quote) != block.quote_group.is_some()
            || (block.align_group.is_some() && !aligned_kind)
        {
            return false;
        }

        if block.list_group.is_none() {
            list = None;
            continue;
        }

        let info = block.list.as_ref().expect("a list item has list info");
        let same_list = list.is_some() && same(&block.list_group, &list);
        let allowed = if same_list { previous_depth + 1 } else { 0 };
        if info.depth as isize > allowed {
            return false;
        }

        if !same_list {
            top_ordered = info.ordered;
        } else if info.depth == 0 && info.ordered != top_ordered {
            return false;
        }

        list = block.list_group.clone();
        previous_depth = info.depth as isize;
    }

    true
}

fn all_in_list_of(document: &RichDocument, index: usize) -> bool {
    let group = &document.blocks[index].list_group;
    document.blocks.iter().all(|b| same(&b.list_group, group))
}

#[test]
fn set_block_kind_makes_headings_and_normal() {
    let mut document = parse("one\n\ntwo\n\nthree\n");
    block_ops::set_block_kind(&mut document, caret(1), 2).unwrap();
    assert_eq!(shape(&document), "p,h2,p");
    let dirty: Vec<bool> = document.blocks.iter().map(|b| b.dirty).collect();
    assert_eq!(dirty, [false, true, false]);

    let all = blocks(&document, 0, 2);
    block_ops::set_block_kind(&mut document, all, 1).unwrap();
    assert_eq!(shape(&document), "h1,h1,h1");

    let two = blocks(&document, 0, 1);
    block_ops::set_block_kind(&mut document, two, 0).unwrap();
    assert_eq!(shape(&document), "p,p,h1");
}

#[test]
fn set_block_kind_takes_list_items_and_quotes_out_of_their_groups() {
    let mut document = parse("- a\n  - b\n- c\n\n> q\n> \n> r\n");
    assert_eq!(shape(&document), "b0,b1,b0,q,q");
    block_ops::set_block_kind(&mut document, caret(0), 3).unwrap();

    // The rest of the list, starting with the heading's former child, is lifted to start at the top level.
    assert_eq!(shape(&document), "h3,b0,b0,q,q");
    assert!(document.blocks[0].list_group.is_none());
    assert!(document.blocks[0].list.is_none());

    block_ops::set_block_kind(&mut document, caret(3), 0).unwrap();
    assert_eq!(shape(&document), "h3,b0,b0,p,q");
    assert!(document.blocks[3].quote_group.is_none());
    assert!(groups_are_well_formed(&document));
}

#[test]
fn selection_ending_at_a_block_start_does_not_touch_it() {
    let mut document = parse("one\n\ntwo\n");
    block_ops::set_block_kind(&mut document, RichSelection::new(p(0, 1), p(1, 0)), 1).unwrap();
    assert_eq!(shape(&document), "h1,p");
}

#[test]
fn toggle_bullet_on_and_off() {
    let mut document = parse("# one\n\ntwo\n\nthree\n");
    let selection = blocks(&document, 0, 1);
    block_ops::toggle_list(&mut document, selection, false);

    // A heading becomes a plain item: a list item holds one paragraph.
    assert_eq!(shape(&document), "b0,b0,p");
    assert!(same(
        &document.blocks[0].list_group,
        &document.blocks[1].list_group
    ));
    let group = document.blocks[0].list_group.as_ref().unwrap();
    assert_eq!(group.borrow().original_member_count, 0);
    assert!(document.blocks[0].dirty && document.blocks[1].dirty && !document.blocks[2].dirty);

    let selection = blocks(&document, 0, 1);
    block_ops::toggle_list(&mut document, selection, false);
    assert_eq!(shape(&document), "p,p,p");
    assert!(document
        .blocks
        .iter()
        .all(|b| b.list_group.is_none() && b.list.is_none()));
}

#[test]
fn bullet_and_numbered_convert_in_place() {
    let mut document = parse("- a\n  - b\n- c\n");
    let selection = blocks(&document, 0, 2);
    block_ops::toggle_list(&mut document, selection, true);
    assert_eq!(shape(&document), "n0,n1,n0");
    assert!(all_in_list_of(&document, 0));
    assert_eq!(document.blocks[0].list.as_ref().unwrap().marker, '.');

    // Numbered again everywhere: toggling off.
    let selection = blocks(&document, 0, 2);
    block_ops::toggle_list(&mut document, selection, true);
    assert_eq!(shape(&document), "p,p,p");
}

#[test]
fn converting_the_middle_of_a_list_splits_it_and_children_follow_their_parent() {
    let mut document = parse("- a\n- b\n  - c\n- d\n");
    block_ops::toggle_list(&mut document, caret(1), true);

    assert_eq!(shape(&document), "b0,n0,b1,b0");
    let b = &document.blocks;
    assert!(same(&b[1].list_group, &b[2].list_group));
    assert!(!same(&b[0].list_group, &b[1].list_group));
    assert!(!same(&b[3].list_group, &b[0].list_group));
    assert!(b.iter().all(|b| b
        .list_group
        .as_ref()
        .unwrap()
        .borrow()
        .original_member_count
        == 0));
    assert!(groups_are_well_formed(&document));

    // Back to bullet: the three lists join into one again.
    block_ops::toggle_list(&mut document, caret(1), false);
    assert_eq!(shape(&document), "b0,b0,b1,b0");
    assert!(all_in_list_of(&document, 0));
    assert!(groups_are_well_formed(&document));
}

#[test]
fn converting_a_nested_item_keeps_it_in_its_parents_list() {
    let mut document = parse("- a\n  - b\n  - c\n- d\n");
    block_ops::toggle_list(&mut document, caret(1), true);

    assert_eq!(shape(&document), "b0,n1,b1,b0");
    assert!(all_in_list_of(&document, 0));
    assert!(groups_are_well_formed(&document));
}

#[test]
fn a_nested_item_stays_in_its_list_when_a_paragraph_below_becomes_an_item() {
    let mut document = parse("- a\n  1. b\n\npara\n");
    assert_eq!(shape(&document), "b0,n1,p");
    let selection = blocks(&document, 1, 2);
    block_ops::toggle_list(&mut document, selection, true);

    assert_eq!(shape(&document), "b0,n1,n0");
    assert!(same(
        &document.blocks[1].list_group,
        &document.blocks[0].list_group
    ));
    assert!(!same(
        &document.blocks[2].list_group,
        &document.blocks[0].list_group
    ));
    assert!(groups_are_well_formed(&document));
}

#[test]
fn outdent_takes_the_style_of_its_new_level() {
    let mut document = parse("- a\n  1. b\n");
    block_ops::outdent(&mut document, caret(1));

    // b joins a's level as a bullet rather than splitting the list in two.
    assert_eq!(shape(&document), "b0,b0");
    assert!(same(
        &document.blocks[1].list_group,
        &document.blocks[0].list_group
    ));
    assert_eq!(document.blocks[1].list.as_ref().unwrap().marker, '-');
}

#[test]
fn indent_takes_the_style_of_its_new_level() {
    // The first item of a new sublist numbers from 1, not from where the outer list was.
    let mut numbered = parse("5. a\n6. b\n");
    block_ops::indent(&mut numbered, caret(1));
    assert_eq!(shape(&numbered), "n0,n1");
    assert_eq!(numbered.blocks[1].list.as_ref().unwrap().start_number, 1);
    assert_eq!(numbered.blocks[0].list.as_ref().unwrap().start_number, 5);

    // Under an existing sublist the item takes that sublist's kind and numbering.
    // (A sublist not starting at 1 cannot interrupt a's text, so it follows a blank line.)
    let mut mixed = parse("- a\n\n  3) x\n\n- b\n");
    assert_eq!(shape(&mixed), "b0,n1,b0");
    block_ops::indent(&mut mixed, caret(2));
    assert_eq!(shape(&mixed), "b0,n1,n1");
    assert_eq!(mixed.blocks[2].list.as_ref().unwrap().marker, ')');
    assert_eq!(mixed.blocks[2].list.as_ref().unwrap().start_number, 3);
}

#[test]
fn a_top_level_item_leaving_keeps_the_subtrees_after_it() {
    // Only a's own children are lifted; c keeps its child d.
    let mut outdented = parse("- a\n  - b\n- c\n  - d\n");
    block_ops::outdent(&mut outdented, caret(0));
    assert_eq!(shape(&outdented), "p,b0,b0,b1");
    assert!(groups_are_well_formed(&outdented));

    let mut backspaced = parse("- a\n  - b\n- c\n  - d\n");
    edit_operations::backspace(&mut backspaced, p(0, 0)).unwrap();
    assert_eq!(shape(&backspaced), "p,b0,b0,b1");

    let mut toggled = parse("- p\n  - c\n    - g\n- q\n  - q1\n");
    block_ops::toggle_list(&mut toggled, caret(1), false);
    assert_eq!(shape(&toggled), "b0,p,b0,b0,b1");
    assert!(groups_are_well_formed(&toggled));
}

#[test]
fn backspace_at_a_nested_items_start_outdents_like_shift_tab() {
    let mut shift_tab = parse("- a\n  1. b\n- c\n");
    block_ops::outdent(&mut shift_tab, caret(1));
    let mut backspace = parse("- a\n  1. b\n- c\n");
    edit_operations::backspace(&mut backspace, p(1, 0)).unwrap();

    assert_eq!(shape(&shift_tab), "b0,b0,b0");
    assert_eq!(shape(&backspace), shape(&shift_tab));
    assert!(all_in_list_of(&backspace, 0));
    assert_eq!(
        backspace.blocks[1].list.as_ref().unwrap().marker,
        shift_tab.blocks[1].list.as_ref().unwrap().marker
    );
    assert_eq!(list_model(&backspace), list_model(&shift_tab));
}

#[test]
fn an_empty_document_is_a_no_op() {
    let mut document = parse("");
    assert_eq!(document.blocks.len(), 0);
    let selection = caret(0);

    assert_eq!(
        block_ops::block_state_at(&document, selection),
        RichBlockState {
            heading_level: Some(0),
            list: RichListKind::None,
            quote: false,
            alignment: None,
            can_align: false,
        }
    );
    block_ops::set_block_kind(&mut document, selection, 1).unwrap();
    block_ops::toggle_list(&mut document, selection, true);
    block_ops::toggle_quote(&mut document, selection);
    block_ops::set_alignment(&mut document, selection, RichAlignment::Center);
    assert!(!block_ops::indent(&mut document, selection));
    assert!(!block_ops::outdent(&mut document, selection));
    assert_eq!(
        block_ops::try_apply_line_start_shortcut(&mut document, p(0, 2)),
        None
    );
    assert_eq!(document.blocks.len(), 0);
}

#[test]
fn new_items_join_a_touching_list_of_the_same_kind() {
    let mut document = parse("1. a\n2. b\n\nc\n\n3. d\n");
    assert!(!same(
        &document.blocks[0].list_group,
        &document.blocks[3].list_group
    ));
    block_ops::toggle_list(&mut document, caret(2), true);

    assert_eq!(shape(&document), "n0,n0,n0,n0");
    assert!(all_in_list_of(&document, 0));

    // The joined items number on from the list's first item rather than restarting.
    assert!(document.blocks.iter().all(|b| {
        let list = b.list.as_ref().unwrap();
        list.start_number == 1 && list.marker == '.'
    }));
    assert!(document.blocks[3].dirty);
}

#[test]
fn a_new_list_beside_a_different_kind_stays_separate_and_regenerates() {
    let mut document = parse("- a\n\nb\n");
    let bullets = document.blocks[0].list_group.clone();
    block_ops::toggle_list(&mut document, caret(1), true);

    assert_eq!(shape(&document), "b0,n0");
    assert!(!same(&document.blocks[1].list_group, &bullets));
    assert_eq!(bullets.as_ref().unwrap().borrow().original_member_count, 0);
    assert!(!document.blocks[0].dirty);
}

#[test]
fn toggle_quote_on_joins_and_off_splits() {
    let mut document = parse("> a\n\n## b\n\n- c\n\n> d\n");
    let selection = blocks(&document, 1, 2);
    block_ops::toggle_quote(&mut document, selection);
    assert_eq!(shape(&document), "q,q,q,q");
    let first = document.blocks[0].quote_group.clone();
    assert!(document.blocks.iter().all(|b| same(&b.quote_group, &first)));
    assert_eq!(document.blocks[1].heading_level, 0);
    assert!(document.blocks[2].list.is_none());

    block_ops::toggle_quote(&mut document, caret(1));
    assert_eq!(shape(&document), "q,p,q,q");
    assert!(!same(
        &document.blocks[2].quote_group,
        &document.blocks[0].quote_group
    ));
    let split = document.blocks[2].quote_group.as_ref().unwrap();
    assert_eq!(split.borrow().original_member_count, 0);
    assert!(groups_are_well_formed(&document));

    // A mixed selection turns the quote on for all of it rather than off.
    let selection = blocks(&document, 0, 1);
    block_ops::toggle_quote(&mut document, selection);
    assert_eq!(shape(&document), "q,q,q,q");
}

#[test]
fn alignment_applies_to_paragraphs_and_headings_only() {
    let mut document = parse("# h\n\np\n\n- item\n\n> quote\n");
    let selection = blocks(&document, 0, 3);
    block_ops::set_alignment(&mut document, selection, RichAlignment::Center);

    let alignments: Vec<RichAlignment> = document.blocks.iter().map(|b| b.alignment).collect();
    assert_eq!(
        alignments,
        [
            RichAlignment::Center,
            RichAlignment::Center,
            RichAlignment::Left,
            RichAlignment::Left
        ]
    );
    assert!(!(document.blocks[2].dirty || document.blocks[3].dirty));

    let state = block_ops::block_state_at(&document, caret(2));
    assert!(!state.can_align);
}

#[test]
fn realigning_leaves_the_wrapper_and_splits_it() {
    let mut document = parse("<div align=\"center\">\n\na\n\nb\n\nc\n\n</div>\n");
    assert_eq!(shape(&document), "p,p,p");
    let wrapper = document.blocks[0].align_group.clone();
    assert!(wrapper.is_some());

    block_ops::set_alignment(&mut document, caret(1), RichAlignment::Right);
    assert!(document.blocks[1].align_group.is_none());
    assert_eq!(document.blocks[1].alignment, RichAlignment::Right);
    assert!(same(&document.blocks[0].align_group, &wrapper));
    assert!(!same(&document.blocks[2].align_group, &wrapper));
    let split = document.blocks[2].align_group.as_ref().unwrap();
    assert_eq!(split.borrow().original_member_count, 0);
    assert_eq!(document.blocks[2].alignment, RichAlignment::Center);
    assert!(groups_are_well_formed(&document));
}

#[test]
fn indent_carries_the_subtree_and_never_skips_a_level() {
    let mut document = parse("- a\n- b\n  - c\n- d\n");

    // The first item has nothing to nest under.
    assert!(block_ops::indent(&mut document, caret(0)));
    assert_eq!(shape(&document), "b0,b0,b1,b0");

    block_ops::indent(&mut document, caret(1));
    assert_eq!(shape(&document), "b0,b1,b2,b0");
    assert!(document.blocks[2].dirty);

    // c is already one level below b; another Tab would skip a level.
    block_ops::indent(&mut document, caret(2));
    assert_eq!(shape(&document), "b0,b1,b2,b0");

    // A selection of siblings indents together; c moves once, with b.
    // Over b..d only d can go deeper: b and c are each already one level below the item above.
    let selection = blocks(&document, 1, 3);
    block_ops::indent(&mut document, selection);
    assert_eq!(shape(&document), "b0,b1,b2,b1");
    assert!(groups_are_well_formed(&document));
}

#[test]
fn indent_of_siblings_keeps_lists_well_formed() {
    let mut document = parse("- a\n- b\n- c\n\npara\n");
    let selection = blocks(&document, 1, 2);
    assert!(block_ops::indent(&mut document, selection));
    assert_eq!(shape(&document), "b0,b1,b1,p");
    assert!(groups_are_well_formed(&document));

    // Tab outside a list is not a list command.
    assert!(!block_ops::indent(&mut document, caret(3)));
}

#[test]
fn outdent_carries_the_subtree_and_leaves_the_list_from_the_top() {
    let mut document = parse("- a\n  - b\n    - c\n  - d\n- e\n");
    block_ops::outdent(&mut document, caret(1));
    assert_eq!(shape(&document), "b0,b0,b1,b1,b0");
    assert!(groups_are_well_formed(&document));

    // Outdenting a top-level item leaves the list; its children start a list of their own.
    block_ops::outdent(&mut document, caret(1));
    assert_eq!(shape(&document), "b0,p,b0,b0,b0");
    let b = &document.blocks;
    assert!(!same(&b[2].list_group, &b[0].list_group));
    let split = b[2].list_group.as_ref().unwrap();
    assert_eq!(split.borrow().original_member_count, 0);
    assert!(groups_are_well_formed(&document));
}

#[test]
fn shortcuts_convert_a_paragraph() {
    let cases = [
        ("# ", "h1"),
        ("## ", "h2"),
        ("### ", "h3"),
        ("- ", "b0"),
        ("* ", "b0"),
        ("1. ", "n0"),
        ("> ", "q"),
    ];
    for (typed, expected) in cases {
        let mut document = parse("text\n");
        edit_operations::insert_text(&mut document, p(0, 0), typed, None).unwrap();
        let caret =
            block_ops::try_apply_line_start_shortcut(&mut document, p(0, typed.chars().count()));

        assert_eq!(caret, Some(p(0, 0)), "{typed}");
        assert_eq!(shape(&document), expected);
        assert_eq!(text(&document.blocks[0]), "text");
        assert!(document.blocks[0].dirty);
        assert!(groups_are_well_formed(&document));
    }
}

#[test]
fn numbered_shortcut_keeps_its_start_and_star_keeps_its_marker() {
    let mut numbered = parse("x\n");
    numbered.blocks[0].inlines = vec![RichInline::run("7. ")];
    assert!(block_ops::try_apply_line_start_shortcut(&mut numbered, p(0, 3)).is_some());
    assert_eq!(numbered.blocks[0].list.as_ref().unwrap().start_number, 7);

    let mut star = parse("x\n");
    star.blocks[0].inlines = vec![RichInline::run("* ")];
    block_ops::try_apply_line_start_shortcut(&mut star, p(0, 2));
    assert_eq!(star.blocks[0].list.as_ref().unwrap().marker, '*');
}

#[test]
fn shortcut_joins_a_list_above() {
    let mut document = parse("- a\n\nb\n");
    document.blocks[1].inlines = vec![RichInline::run("- ")];
    block_ops::try_apply_line_start_shortcut(&mut document, p(1, 2));
    assert_eq!(shape(&document), "b0,b0");
    assert!(same(
        &document.blocks[1].list_group,
        &document.blocks[0].list_group
    ));
}

#[test]
fn non_matching_text_is_not_a_shortcut() {
    let cases = [
        ("#x ", 3),
        ("abc # ", 6),
        ("#### ", 5),
        ("1) ", 3),
        ("a. ", 3),
        ("# ", 1),
        ("-", 1),
        ("1234567890. ", 12),
    ];
    for (typed, offset) in cases {
        let mut document = parse("x\n");
        document.blocks[0].inlines = vec![RichInline::run(typed)];
        let after = block_ops::try_apply_line_start_shortcut(&mut document, p(0, offset));
        assert_eq!(after, None, "{typed}");
        assert_eq!(shape(&document), "p");
        assert_eq!(text(&document.blocks[0]), typed);
    }

    // Only plain paragraphs: in a heading or list item "- " is text.
    let mut heading = parse("# x\n");
    heading.blocks[0].inlines = vec![RichInline::run("- ")];
    assert_eq!(
        block_ops::try_apply_line_start_shortcut(&mut heading, p(0, 2)),
        None
    );

    // A marker in inline code is content.
    let mut code = parse("x\n");
    code.blocks[0].inlines = vec![RichInline::run("# ").code()];
    assert_eq!(
        block_ops::try_apply_line_start_shortcut(&mut code, p(0, 2)),
        None
    );
}

#[test]
fn block_state_reports_shared_and_mixed_state() {
    let mut document = parse("## a\n\n## b\n\n- c\n\n1. d\n\n> e\n");
    let headings = block_ops::block_state_at(&document, blocks(&document, 0, 1));
    assert_eq!(
        headings,
        RichBlockState {
            heading_level: Some(2),
            list: RichListKind::None,
            quote: false,
            alignment: Some(RichAlignment::Left),
            can_align: true,
        }
    );

    let mixed = block_ops::block_state_at(&document, blocks(&document, 1, 3));
    assert_eq!(mixed.heading_level, None);
    assert_eq!(mixed.list, RichListKind::Mixed);

    let bullet = block_ops::block_state_at(&document, caret(2));
    assert_eq!(
        bullet,
        RichBlockState {
            heading_level: Some(0),
            list: RichListKind::Bullet,
            quote: false,
            alignment: None,
            can_align: false,
        }
    );

    let quote = block_ops::block_state_at(&document, caret(4));
    assert!(quote.quote);

    block_ops::set_alignment(&mut document, caret(0), RichAlignment::Right);
    let state = block_ops::block_state_at(&document, blocks(&document, 0, 1));
    assert_eq!(state.alignment, None);
}

#[test]
fn raw_blocks_are_left_alone() {
    let mut document = parse("a\n\n---\n\nb\n");
    let selection = blocks(&document, 0, 2);
    block_ops::toggle_list(&mut document, selection, false);

    // The thematic break stays, between two new lists.
    assert_eq!(shape(&document), "b0,r,b0");
    assert!(!document.blocks[1].dirty);
    assert!(!same(
        &document.blocks[0].list_group,
        &document.blocks[2].list_group
    ));
    assert!(groups_are_well_formed(&document));
}

/// C# throws `ArgumentOutOfRangeException` for a heading level outside 0-6;
/// here it is an error and the document is unchanged.
#[test]
fn rust_only_set_block_kind_refuses_a_level_above_six() {
    let mut document = parse("one\n");
    assert_eq!(
        block_ops::set_block_kind(&mut document, caret(0), 7),
        Err(RichEditError::HeadingLevelOutOfRange(7))
    );
    assert_eq!(shape(&document), "p");
    assert!(!document.blocks[0].dirty);
}
