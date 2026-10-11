//! agg-sharp's `RichDocumentTests` (`Tests/Agg.Tests/MarkdigAgg/RichDocumentTests.cs`),
//! ported 1:1: inline offset arithmetic, positions, table cells, block lengths
//! and the document's deep clone.

use agg_gui::widgets::markdown_editing::inlines;
use agg_gui::widgets::markdown_editing::{
    DocPosition, InlineAtomKind, OffsetOutOfRange, RichAlignment, RichBlock, RichBlockKind,
    RichDocument, RichInline, RichListInfo, RichTableCell,
};

/// "ab" + image atom + "cd" (bold): plain text "ab?cd", length 5.
fn mixed_inlines() -> Vec<RichInline> {
    vec![
        RichInline::run("ab"),
        RichInline::atom(InlineAtomKind::Image, "![x](x.png)"),
        RichInline::run("cd").bold(),
    ]
}

fn run_text(inline: &RichInline) -> &str {
    inline.text().expect("a run")
}

fn cell(text: &str) -> RichTableCell {
    RichTableCell {
        inlines: vec![RichInline::run(text)],
    }
}

#[test]
fn locate_resolves_offsets_across_runs_and_atoms() {
    let inlines = mixed_inlines();
    assert_eq!(inlines::length(&inlines), 5);

    let locate = |offset, prefer_next| inlines::locate(&inlines, offset, prefer_next).unwrap();
    assert_eq!(locate(0, false), (0, 0));
    assert_eq!(locate(1, false), (0, 1));

    // Boundaries resolve to the end of the earlier inline unless the caller prefers the next one.
    assert_eq!(locate(2, false), (0, 2));
    assert_eq!(locate(2, true), (1, 0));
    assert_eq!(locate(3, false), (1, 1));
    assert_eq!(locate(3, true), (2, 0));
    assert_eq!(locate(4, false), (2, 1));
    assert_eq!(locate(5, false), (2, 2));
    assert_eq!(locate(5, true), (2, 2));

    for offset in 0..=5 {
        for prefer_next in [false, true] {
            let (index, in_inline) = locate(offset, prefer_next);
            assert_eq!(inlines::offset_of(&inlines, index, in_inline), Ok(offset));
        }
    }

    assert_eq!(inlines::locate(&[], 0, false), Ok((0, 0)));
}

#[test]
fn split_at_makes_a_boundary_and_merge_adjacent_undoes_it() {
    let mut inlines = mixed_inlines();

    let index = inlines::split_at(&mut inlines, 4).unwrap();
    assert_eq!(index, 3);
    assert_eq!(inlines.len(), 4);
    assert_eq!(run_text(&inlines[2]), "c");
    assert_eq!(run_text(&inlines[3]), "d");
    assert!(inlines[3].bold);

    // Offsets already on a boundary do not split.
    assert_eq!(inlines::split_at(&mut inlines, 2), Ok(1));
    assert_eq!(inlines::split_at(&mut inlines, 0), Ok(0));
    assert_eq!(inlines::split_at(&mut inlines, 5), Ok(4));
    assert_eq!(inlines.len(), 4);

    inlines::merge_adjacent(&mut inlines);
    assert_eq!(inlines.len(), 3);
    assert_eq!(run_text(&inlines[2]), "cd");
}

#[test]
fn merge_adjacent_keeps_different_styles_and_atoms_apart_and_drops_empty_runs() {
    let mut inlines = vec![
        RichInline::run("a"),
        RichInline::run(""),
        RichInline::run("b"),
        RichInline::run("c").italic(),
        RichInline::run("d").italic().link("https://x"),
        RichInline::atom(InlineAtomKind::HardBreak, "  \n"),
        RichInline::run("e"),
    ];

    inlines::merge_adjacent(&mut inlines);

    // Joined with a separator so the check covers order as well as membership.
    let texts: Vec<String> = inlines.iter().map(ToString::to_string).collect();
    assert_eq!(texts.join("|"), "ab|c|d|  \n|e");
}

#[test]
fn merge_adjacent_keeps_runs_with_different_link_titles_apart() {
    let mut a = RichInline::run("a").link("u");
    a.link_title = Some("one".into());
    let mut b = RichInline::run("b").link("u");
    b.link_title = Some("two".into());
    let mut inlines = vec![a, b];

    inlines::merge_adjacent(&mut inlines);

    assert_eq!(inlines.len(), 2);
    assert_eq!(inlines[1].clone().link_title.as_deref(), Some("two"));
}

#[test]
fn runs_with_different_link_labels_are_different_styles() {
    let inline = RichInline::run("a").link("u");
    let mut reference = RichInline::run("b").link("u");
    reference.link_label = Some("ref".into());
    let mut other_reference = RichInline::run("c").link("u");
    other_reference.link_label = Some("other".into());

    assert!(!inline.has_same_style(&reference));
    assert!(!reference.has_same_style(&other_reference));
    assert!(reference.has_same_style(&reference.with_text("d")));
    assert_eq!(reference.clone().link_label.as_deref(), Some("ref"));
}

#[test]
fn locate_and_offset_of_throw_out_of_range() {
    let inlines = mixed_inlines();

    // C#'s Locate(inlines, -1) case: offsets are usize, so a negative offset cannot be expressed.
    assert!(matches!(
        inlines::locate(&inlines, 6, false),
        Err(OffsetOutOfRange(_))
    ));
    assert!(inlines::offset_of(&inlines, 4, 0).is_err());
    assert!(inlines::offset_of(&inlines, 0, 3).is_err());
    assert!(inlines::offset_of(&inlines, 1, 2).is_err());
    assert!(inlines::offset_of(&inlines, 3, 1).is_err());
    assert_eq!(inlines::offset_of(&inlines, 3, 0), Ok(5));
}

#[test]
fn locate_skips_leading_and_trailing_empty_runs() {
    let inlines = vec![
        RichInline::run(""),
        RichInline::run("ab"),
        RichInline::run(""),
    ];

    // Offset 0 lands in the first non-empty run, not the empty run at index 0.
    assert_eq!(inlines::locate(&inlines, 0, false), Ok((1, 0)));
    assert_eq!(inlines::locate(&inlines, 2, false), Ok((1, 2)));
    assert_eq!(inlines::locate(&inlines, 2, true), Ok((2, 0)));
}

#[test]
fn split_at_the_end_or_on_an_empty_list_does_not_split() {
    let mut trailing_empty = vec![RichInline::run("ab"), RichInline::run("")];
    let index = inlines::split_at(&mut trailing_empty, 2).unwrap();
    assert_eq!(index, 1);
    assert_eq!(trailing_empty.len(), 2);
    assert_eq!(run_text(&trailing_empty[0]), "ab");

    let mut empty = Vec::new();
    assert_eq!(inlines::split_at(&mut empty, 0), Ok(0));
    assert_eq!(empty.len(), 0);
}

#[test]
fn positions_order_by_block_then_cell_then_offset() {
    let a = DocPosition::new(1, 5);
    let b = DocPosition::new(2, 0);
    assert!(a < b);
    assert!(b > a);
    assert!(DocPosition::new(1, 5) == a);
    assert_eq!(std::cmp::min(b, a), a);
    assert_eq!(std::cmp::max(a, b), b);

    // Within a table: rows first, then columns, then the offset in the cell.
    let late_in_first_row = DocPosition::in_cell(3, 9, 0, 1);
    let start_of_second_row = DocPosition::in_cell(3, 0, 1, 0);
    assert!(late_in_first_row < start_of_second_row);
    assert!(DocPosition::in_cell(3, 9, 0, 0) < late_in_first_row);
}

#[test]
fn table_cells_are_addressed_by_row_and_column() {
    let mut table = RichBlock::new(RichBlockKind::Table);
    table
        .column_alignments
        .extend([None, Some(RichAlignment::Right)]);
    table.table_rows.push(vec![cell("Name"), cell("Qty")]);
    table.table_rows.push(vec![cell("Bolt"), cell("12")]);

    let mut document = RichDocument::new();
    let mut intro = RichBlock::new(RichBlockKind::Paragraph);
    intro.inlines.push(RichInline::run("intro"));
    document.blocks.push(intro);
    document.blocks.push(table);

    assert_eq!(document.text_length(DocPosition::new(0, 0)), 5);
    assert_eq!(document.text_length(DocPosition::in_cell(1, 0, 1, 1)), 2);
    assert_eq!(document.text_length(DocPosition::in_cell(1, 0, 0, 0)), 4);
    let table = &document.blocks[1];
    assert_eq!(table.inlines_at(1, 0).unwrap()[0].to_string(), "Bolt");
}

#[test]
fn code_and_raw_blocks_have_their_own_lengths() {
    let mut code = RichBlock::new(RichBlockKind::CodeBlock);
    code.code_text = "a\nbc".into();
    code.code_language = "cs".into();
    let mut raw = RichBlock::new(RichBlockKind::Raw);
    raw.original_source = "<hr/>".into();

    assert_eq!(code.text_length(0, 0), 4);
    assert_eq!(raw.text_length(0, 0), 1);
    assert!(!code.is_text_block());
}

#[test]
fn clone_shares_no_mutable_state() {
    let mut table = RichBlock::new(RichBlockKind::Table);
    table.column_alignments.push(Some(RichAlignment::Center));
    table.table_rows.push(vec![cell("cell")]);
    let mut item = RichBlock::new(RichBlockKind::ListItem);
    item.list = Some(RichListInfo {
        ordered: true,
        depth: 1,
        marker: '.',
        start_number: 3,
        ..RichListInfo::default()
    });
    item.inlines.push(RichInline::run("item").bold());
    item.original_source = "3. **item**".into();
    item.separator_before = "\n\n".into();
    let mut original = RichDocument {
        frontmatter: "---\na: 1\n---\n".into(),
        trailing_text: "\n".into(),
        ..RichDocument::default()
    };
    original.blocks.push(item);
    original.blocks.push(table);

    let mut copy = original.deep_clone();
    {
        let copy_item = &mut copy.blocks[0];
        copy_item.inlines[0].as_run_mut().unwrap().text = "changed".into();
        copy_item.inlines.push(RichInline::run("more"));
        copy_item.list.as_mut().unwrap().depth = 0;
        copy_item.dirty = true;
    }
    copy.blocks[1].table_rows[0][0].inlines.clear();
    copy.blocks[1].column_alignments[0] = Some(RichAlignment::Left);
    copy.blocks.remove(1);

    let item = &original.blocks[0];
    let table = &original.blocks[1];
    assert_eq!(original.blocks.len(), 2);
    assert_eq!(item.inlines.len(), 1);
    assert_eq!(run_text(&item.inlines[0]), "item");
    assert_eq!(item.list.as_ref().unwrap().depth, 1);
    assert!(!item.dirty);
    assert_eq!(table.table_rows[0][0].inlines.len(), 1);
    assert_eq!(table.column_alignments[0], Some(RichAlignment::Center));

    // Unchanged values carried over.
    let copy_item = &copy.blocks[0];
    assert_eq!(copy.frontmatter, original.frontmatter);
    assert_eq!(copy_item.list.as_ref().unwrap().start_number, 3);
    assert_eq!(copy_item.separator_before, "\n\n");
}

#[test]
fn rust_only_deep_clone_remaps_each_group_once() {
    use agg_gui::widgets::markdown_editing::{shared_group, RichListGroup};
    use std::rc::Rc;

    let group = shared_group(RichListGroup {
        original_member_count: 2,
    });
    let mut original = RichDocument::new();
    for _ in 0..2 {
        let mut item = RichBlock::new(RichBlockKind::ListItem);
        item.list_group = Some(Rc::clone(&group));
        original.blocks.push(item);
    }

    let copy = original.deep_clone();
    let first = copy.blocks[0].list_group.as_ref().unwrap();
    let second = copy.blocks[1].list_group.as_ref().unwrap();
    // The copies still share one group, but not the original's.
    assert!(Rc::ptr_eq(first, second));
    assert!(!Rc::ptr_eq(first, &group));
    first.borrow_mut().original_member_count = 0;
    assert_eq!(group.borrow().original_member_count, 2);
}
