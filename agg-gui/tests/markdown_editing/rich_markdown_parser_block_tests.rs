//! agg-sharp's `RichMarkdownParserBlockTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichMarkdownParserBlockTests.cs`), ported 1:1
//! against `markdown_editing::markdown_parser`: block shapes for lists, quotes
//! and `<div align>` groups.  Their byte-identical round trips are in the
//! corpus of `rich_markdown_parser_tests`.

use std::rc::Rc;

use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer;
use agg_gui::widgets::markdown_editing::{
    InlineAtomKind, RichAlignment, RichBlock, RichBlockKind, RichDocument,
};

// List items show as marker + depth (e.g. "-1", ".0"); other blocks as their kind.
fn shape(document: &RichDocument) -> String {
    document
        .blocks
        .iter()
        .map(|b| match (&b.kind, &b.list) {
            (RichBlockKind::ListItem, Some(list)) => {
                format!("{}{}:{}", list.marker, list.depth, b.original_source)
            }
            _ => format!("{:?}:{}", b.kind, b.original_source),
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn plain_text(block: &RichBlock) -> String {
    block
        .inlines
        .iter()
        .map(|inline| inline.as_run().map_or("?".to_string(), |r| r.text.clone()))
        .collect()
}

fn list(block: &RichBlock) -> &agg_gui::widgets::markdown_editing::RichListInfo {
    block.list.as_ref().expect("a list item")
}

/// How many distinct list groups (by identity) the blocks hold.
fn distinct_list_groups(document: &RichDocument) -> usize {
    let mut seen: Vec<_> = Vec::new();
    for group in document.blocks.iter().filter_map(|b| b.list_group.clone()) {
        if !seen.iter().any(|g| Rc::ptr_eq(g, &group)) {
            seen.push(group);
        }
    }
    seen.len()
}

fn same<T>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if Rc::ptr_eq(a, b))
}

#[test]
fn nested_list_items_flatten_with_depth() {
    let document = parse("- a\n  - b\n    - c\n  - d\n- e\n");
    assert_eq!(shape(&document), "-0:- a|-1:- b|-2:- c|-1:- d|-0:- e");
    assert_eq!(document.blocks[1].separator_before, "\n  ");
    assert_eq!(document.blocks[2].separator_before, "\n    ");
    assert_eq!(plain_text(&document.blocks[2]), "c");
    assert!(document.blocks.iter().all(|b| !list(b).ordered));
}

#[test]
fn ordered_lists_keep_start_and_delimiter() {
    let document = parse("3. three\n4. four\n\n10) ten\n11) eleven\n");
    assert_eq!(
        shape(&document),
        ".0:3. three|.0:4. four|)0:10) ten|)0:11) eleven"
    );
    assert!(document.blocks.iter().all(|b| list(b).ordered));
    assert_eq!(list(&document.blocks[0]).start_number, 3);
    assert_eq!(list(&document.blocks[1]).start_number, 3);
    assert_eq!(list(&document.blocks[2]).start_number, 10);
    assert_eq!(list(&document.blocks[3]).start_number, 10);
    assert_eq!(plain_text(&document.blocks[3]), "eleven");
}

#[test]
fn bullet_markers_and_loose_items_are_kept() {
    let bullets = parse("* star\n* star two\n\n+ plus\n+ plus two\r\n");
    assert_eq!(
        shape(&bullets),
        "*0:* star|*0:* star two|+0:+ plus|+0:+ plus two"
    );

    let loose = parse("- loose\n\n- items\n\n\n- far apart\n");
    assert_eq!(shape(&loose), "-0:- loose|-0:- items|-0:- far apart");
    assert_eq!(loose.blocks[2].separator_before, "\n\n\n");
}

#[test]
fn task_checkbox_is_a_raw_atom_and_wrapped_lines_stay_in_the_item() {
    let document = parse("- [ ] task\n- [x] done\n  wrapped line\n");
    assert_eq!(
        shape(&document),
        "-0:- [ ] task|-0:- [x] done\n  wrapped line"
    );
    let atom = document.blocks[1].inlines[0].as_atom().expect("an atom");
    assert_eq!(atom.kind, InlineAtomKind::Raw);
    assert_eq!(atom.raw_markdown, "[x]");
    assert_eq!(plain_text(&document.blocks[1]), "? done wrapped line");
}

#[test]
fn list_with_an_unmodelled_item_is_one_raw_block() {
    let paragraphs = parse("- one para\n\n  second para\n- next\n");
    assert_eq!(
        shape(&paragraphs),
        "Raw:- one para\n\n  second para\n- next"
    );

    let code = parse("Intro\n\n- item\n\n  ```\n  code\n  ```\n");
    assert_eq!(
        shape(&code),
        "Paragraph:Intro|Raw:- item\n\n  ```\n  code\n  ```"
    );
}

#[test]
fn quote_paragraphs_are_quote_blocks() {
    let single = parse("> one para\n> two lines\n");
    assert_eq!(shape(&single), "Quote:> one para\n> two lines");
    assert_eq!(plain_text(&single.blocks[0]), "one para two lines");

    // The '>' line between paragraphs starts the second block, so the separator is a bare line break.
    let two = parse("> first\n>\n> second\n>\n");
    assert_eq!(shape(&two), "Quote:> first|Quote:>\n> second\n>");
    assert_eq!(two.blocks[1].separator_before, "\n");
    assert_eq!(plain_text(&two.blocks[1]), "second");

    let indented = parse("  > indented\r\n  >\r\n  > quote\r\n");
    assert_eq!(shape(&indented), "Quote:> indented|Quote:  >\r\n  > quote");
    assert_eq!(indented.blocks[0].separator_before, "  ");
    assert_eq!(indented.blocks[1].separator_before, "\r\n");
}

#[test]
fn nested_quote_is_raw() {
    let document = parse("> outer\n>\n> > nested\n");
    assert_eq!(shape(&document), "Raw:> outer\n>\n> > nested");
}

#[test]
fn align_group_folds_into_alignment_and_keeps_its_wrapper_bytes() {
    let document = parse(
        "<div align=\"center\">\n\n# Title\n\nCentered **text**\n\nMore\n\n</div>\n\nAfter\n",
    );
    assert_eq!(
        shape(&document),
        "Heading:# Title|Paragraph:Centered **text**|Paragraph:More|Paragraph:After"
    );
    let alignments: Vec<_> = document
        .blocks
        .iter()
        .map(|b| format!("{:?}", b.alignment))
        .collect();
    assert_eq!(alignments.join(","), "Center,Center,Center,Left");
    let group = document.blocks[0].align_group.clone();
    assert!(group.is_some());
    assert!(same(&document.blocks[1].align_group, &group));
    assert!(same(&document.blocks[2].align_group, &group));
    assert!(document.blocks[3].align_group.is_none());
    let group = group.expect("a group");
    let group = group.borrow();
    assert_eq!(group.alignment, RichAlignment::Center);
    assert_eq!(group.original_member_count, 3);
    assert_eq!(group.open_source, "<div align=\"center\">\n\n");
    assert_eq!(group.close_source, "\n\n</div>");
    assert_eq!(document.blocks[0].separator_before, "");
    assert_eq!(document.blocks[3].separator_before, "\n\n");

    let right = parse("<div align=\"right\">\r\n\r\nRight\r\n\r\n</div>");
    assert_eq!(shape(&right), "Paragraph:Right");
    assert_eq!(right.blocks[0].alignment, RichAlignment::Right);
    let right_group = right.blocks[0]
        .align_group
        .as_ref()
        .expect("a group")
        .borrow();
    assert_eq!(right_group.open_source, "<div align=\"right\">\r\n\r\n");
    assert_eq!(right_group.close_source, "\r\n\r\n</div>");
}

#[test]
fn unclosed_nested_or_non_text_align_groups_stay_raw() {
    let unclosed = parse("<div align=\"center\">\n\nNever closed\n");
    assert_eq!(
        shape(&unclosed),
        "Raw:<div align=\"center\">|Paragraph:Never closed"
    );
    assert_eq!(unclosed.blocks[1].alignment, RichAlignment::Left);

    let nested = parse("<div>\n\n<div align=\"center\">\n\nNested\n\n</div>\n\n</div>\n");
    assert_eq!(
        shape(&nested),
        "Raw:<div>|Raw:<div align=\"center\">|Paragraph:Nested|Raw:</div>|Raw:</div>"
    );

    let list = parse("<div align=\"center\">\n\n- list\n\n</div>\n");
    assert_eq!(
        shape(&list),
        "Raw:<div align=\"center\">|-0:- list|Raw:</div>"
    );
}

#[test]
fn indented_code_that_looks_like_an_align_tag_is_not_a_group() {
    let document = parse("Intro\n\n    <div align=\"center\">\n\nX\n\n    </div>\n");
    assert_eq!(
        shape(&document),
        "Paragraph:Intro|CodeBlock:    <div align=\"center\">|Paragraph:X|CodeBlock:    </div>"
    );
    assert_eq!(document.blocks[2].alignment, RichAlignment::Left);
    assert!(document.blocks[2].align_group.is_none());
}

#[test]
fn groups_are_explicit_objects() {
    let source = "<div align=\"center\">\n\nA\n\n</div>\n\n<div align=\"left\">\n\nB\n\n</div>\n";
    let aligns = parse(source);
    assert_eq!(shape(&aligns), "Paragraph:A|Paragraph:B");
    assert!(!same(
        &aligns.blocks[0].align_group,
        &aligns.blocks[1].align_group
    ));
    let left = aligns.blocks[1]
        .align_group
        .as_ref()
        .expect("a group")
        .borrow()
        .alignment;
    assert_eq!(left, RichAlignment::Left);
    assert_eq!(markdown_writer::write(&aligns), source);

    let separate = parse("> a\n\n> b");
    assert_eq!(shape(&separate), "Quote:> a|Quote:> b");
    assert!(!same(
        &separate.blocks[0].quote_group,
        &separate.blocks[1].quote_group
    ));

    let one = parse("> a\n>\n> b");
    assert!(same(&one.blocks[0].quote_group, &one.blocks[1].quote_group));
    let count = one.blocks[0]
        .quote_group
        .as_ref()
        .expect("a group")
        .borrow()
        .original_member_count;
    assert_eq!(count, 2);

    let lists = parse("- a\n* b\n+ c");
    assert_eq!(shape(&lists), "-0:- a|*0:* b|+0:+ c");
    assert_eq!(distinct_list_groups(&lists), 3);

    let nested = parse("- a\n  - b\n- c\n");
    assert_eq!(distinct_list_groups(&nested), 1);
    let count = nested.blocks[0]
        .list_group
        .as_ref()
        .expect("a group")
        .borrow()
        .original_member_count;
    assert_eq!(count, 3);
}

#[test]
fn document_clone_keeps_group_sharing_but_not_identity() {
    let document =
        parse("<div align=\"center\">\n\nA\n\nB\n\n</div>\n\n> q\n>\n> r\n\n- x\n  - y\n");
    let copy = document.deep_clone();
    for (original, cloned) in document.blocks.iter().zip(&copy.blocks) {
        assert!(
            original.align_group.is_none() || !same(&original.align_group, &cloned.align_group)
        );
        assert!(
            original.quote_group.is_none() || !same(&original.quote_group, &cloned.quote_group)
        );
        assert!(original.list_group.is_none() || !same(&original.list_group, &cloned.list_group));
    }

    assert!(same(
        &copy.blocks[1].align_group,
        &copy.blocks[0].align_group
    ));
    assert!(same(
        &copy.blocks[3].quote_group,
        &copy.blocks[2].quote_group
    ));
    assert!(same(&copy.blocks[5].list_group, &copy.blocks[4].list_group));

    copy.blocks[0]
        .align_group
        .as_ref()
        .expect("a group")
        .borrow_mut()
        .open_source = "changed".to_string();
    let open = document.blocks[0]
        .align_group
        .as_ref()
        .expect("a group")
        .borrow()
        .open_source
        .clone();
    assert_eq!(open, "<div align=\"center\">\n\n");
    assert_eq!(
        markdown_writer::write(&document),
        markdown_writer::write(&document.deep_clone())
    );

    // Cloning a block alone keeps the block in its groups.
    assert!(same(
        &document.blocks[0].clone().align_group,
        &document.blocks[0].align_group
    ));
}

#[test]
fn changed_group_is_regenerated_not_written_from_original_bytes() {
    // Clean members, but the group changed: the stale wrapper bytes must not be written.
    let mut removed = parse("- a\n- b\n");
    removed.blocks.remove(1);
    assert_eq!(markdown_writer::write(&removed), "- a\n");

    let mut realigned = parse("<div align=\"center\">\n\nA\n\n</div>\n");
    realigned.blocks[0].alignment = RichAlignment::Right;
    assert_eq!(
        markdown_writer::write(&realigned),
        "<div align=\"right\">\n\nA\n\n</div>\n"
    );
}
