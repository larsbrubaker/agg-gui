//! agg-sharp's `RichMarkdownWriterGroupTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichMarkdownWriterGroupTests.cs`), ported 1:1:
//! changed lists, quotes and alignment wrappers, built through the edit ops —
//! the markdown written, and that reading it back gives the same block kinds,
//! depths, groups and alignment.  A C# `[Arguments]` row is one call of the
//! test's body here, all rows in the one test.

use agg_gui::widgets::markdown_editing::block_operations as block_ops;
use agg_gui::widgets::markdown_editing::edit_operations as ops;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer::write;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichAlignment, RichBlock, RichBlockKind, RichDocument, RichSelection,
};

fn range(first: usize, last: usize) -> RichSelection {
    RichSelection::new(DocPosition::new(first, 0), DocPosition::new(last, 1))
}

/// Writes the document, checks the markdown when `expected` is given, and checks it reads back to the same
/// structure.
fn assert_writes(document: &RichDocument, expected: Option<&str>) {
    let markdown = write(document);
    if let Some(expected) = expected {
        assert_eq!(markdown, expected);
    }

    assert_eq!(shape(&parse(&markdown)), shape(document), "{markdown:?}");
}

fn same<T>(a: &Option<std::rc::Rc<T>>, b: &Option<std::rc::Rc<T>>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if std::rc::Rc::ptr_eq(a, b))
}

/// Each block's kind, depth, list kind, alignment, text and which list/quote it shares with the block
/// before. An empty paragraph writes nothing, so it is left out.
fn shape(document: &RichDocument) -> String {
    let mut lines = Vec::new();
    let mut previous: Option<&RichBlock> = None;
    for block in &document.blocks {
        if block.kind == RichBlockKind::Paragraph && block.inlines.is_empty() {
            continue;
        }

        let (same_list, same_quote) = match previous {
            Some(previous) => (
                same(&block.list_group, &previous.list_group),
                same(&block.quote_group, &previous.quote_group),
            ),
            None => (false, false),
        };
        let depth = block
            .list
            .as_ref()
            .map(|l| l.depth.to_string())
            .unwrap_or_default();
        let ordered = block
            .list
            .as_ref()
            .map(|l| l.ordered.to_string())
            .unwrap_or_default();
        let text: String = block.inlines.iter().filter_map(|i| i.text()).collect();
        lines.push(format!(
            "{:?} depth={depth} ordered={ordered} {:?} text={text} sameList={same_list} sameQuote={same_quote}",
            block.kind, block.alignment
        ));
        previous = Some(block);
    }

    lines.join("\n")
}

#[test]
fn bullets_on_two_paragraphs_write_one_tight_list() {
    let mut document = parse("A\n\nB\n");
    block_ops::toggle_list(&mut document, range(0, 1), false);
    assert_writes(&document, Some("- A\n- B\n"));
}

#[test]
fn numbered_list_keeps_its_start_and_nests_a_bullet() {
    let mut document = parse("3. a\n4. b\n5. c\n");
    block_ops::indent(&mut document, range(1, 1));
    block_ops::toggle_list(&mut document, range(1, 1), false);
    assert_writes(&document, Some("3. a\n   - b\n4. c\n"));
}

#[test]
fn indent_then_outdent_restores_the_list() {
    let mut document = parse("- a\n- b\n- c\n");
    block_ops::indent(&mut document, range(1, 1));
    assert_writes(&document, Some("- a\n  - b\n- c\n"));
    block_ops::outdent(&mut document, range(1, 1));
    assert_writes(&document, Some("- a\n- b\n- c\n"));
}

#[test]
fn loose_list_stays_loose_when_regenerated() {
    let mut document = parse("- a\n\n- bc\n");
    ops::split_block(&mut document, DocPosition::new(1, 1)).unwrap();
    assert_writes(&document, Some("- a\n\n- b\n\n- c\n"));
}

#[test]
fn adjacent_lists_stay_separate() {
    let mut document = parse("- a\n- b\n\nBetween\n\n- c\n");
    let whole = ops::whole_block(&document, 2);
    ops::delete_selection(&mut document, whole).unwrap();
    let mut lists: Vec<_> = Vec::new();
    for block in document
        .blocks
        .iter()
        .filter(|b| b.kind == RichBlockKind::ListItem)
    {
        if !lists
            .iter()
            .any(|g| same(&Some(std::rc::Rc::clone(g)), &block.list_group))
        {
            lists.push(block.list_group.clone().expect("a list item is in a list"));
        }
    }
    assert_eq!(lists.len(), 2);
    assert_writes(&document, None);
}

#[test]
fn quote_with_two_paragraphs() {
    let mut document = parse("A\n\nB\n");
    block_ops::toggle_quote(&mut document, range(0, 1));
    assert_writes(&document, Some("> A\n>\n> B\n"));
}

#[test]
fn centered_heading_and_paragraph_share_one_wrapper() {
    let mut document = parse("# T\n\nP\n");
    block_ops::set_alignment(&mut document, range(0, 1), RichAlignment::Center);
    assert_writes(
        &document,
        Some("<div align=\"center\">\n\n# T\n\nP\n\n</div>\n"),
    );
}

#[test]
fn center_then_right_write_two_wrappers_apart() {
    let mut document = parse("A\n\nB\n");
    block_ops::set_alignment(&mut document, range(0, 0), RichAlignment::Center);
    block_ops::set_alignment(&mut document, range(1, 1), RichAlignment::Right);
    assert_writes(
        &document,
        Some("<div align=\"center\">\n\nA\n\n</div>\n\n<div align=\"right\">\n\nB\n\n</div>\n"),
    );
}

#[test]
fn deleting_the_first_centered_block_leaves_no_orphan_tag() {
    let mut document = parse("<div align=\"center\">\n\nA\n\nB\n\n</div>\n\nC\n");
    let whole = ops::whole_block(&document, 0);
    ops::delete_selection(&mut document, whole).unwrap();
    assert_writes(
        &document,
        Some("<div align=\"center\">\n\nB\n\n</div>\n\nC\n"),
    );
}

#[test]
fn crlf_document_keeps_its_line_ending() {
    let mut document = parse("A\r\n\r\nB\r\n");
    block_ops::toggle_quote(&mut document, range(0, 1));
    assert_writes(&document, Some("> A\r\n>\r\n> B\r\n"));
}

/// The usual way to build a list — type an item, press Enter — leaves an empty item, which must write as
/// an item and read back as an empty, editable item wherever it sits.
#[test]
fn empty_list_item_survives_write_and_read() {
    let rows: [(&str, usize, usize, bool, Option<&str>); 9] = [
        ("- a\n- b\n", 1, 1, false, Some("- a\n- b\n-\n")),
        ("- a\n- b\n", 0, 1, false, Some("- a\n-\n- b\n")),
        ("1. a\n2. b\n", 1, 1, false, Some("1. a\n2. b\n3.\n")),
        ("1. a\n2. b\n", 0, 1, false, Some("1. a\n2.\n3. b\n")),
        ("- a\n  - b\n", 1, 1, false, Some("- a\n  - b\n  -\n")),
        ("- a\n- b\n", 0, 1, true, None),
        ("1. a\n2. b\n", 0, 1, true, None),
        ("P\n\n- a\n", 1, 0, false, None),
        ("P\n\n1. a\n", 1, 0, false, None),
    ];
    for (markdown, block, offset, indent_new_item, expected) in rows {
        let mut document = parse(markdown);
        ops::split_block(&mut document, DocPosition::new(block, offset)).unwrap();
        if indent_new_item {
            block_ops::indent(&mut document, range(block + 1, block + 1));
        }

        let empty_items = document
            .blocks
            .iter()
            .filter(|b| b.kind == RichBlockKind::ListItem && b.inlines.is_empty())
            .count();
        assert_eq!(empty_items, 1, "{markdown:?} at {block},{offset}");
        assert_writes(&document, expected);
    }
}

#[test]
fn enter_on_an_empty_middle_item_leaves_two_lists() {
    let mut document = parse("- a\n- b\n- c\n");
    ops::split_block(&mut document, DocPosition::new(1, 1)).unwrap();
    ops::split_block(&mut document, DocPosition::new(2, 0)).unwrap();
    assert_eq!(document.blocks[2].kind, RichBlockKind::Paragraph);
    assert_writes(&document, None);
}

/// A clean block indented as deep as a list item's content, once it sits right after that list, would be
/// read into the last item.
#[test]
fn indented_block_after_a_rewritten_list_stays_out() {
    let rows = [
        "- a\n- b\n\nP\n\n    code\n",
        "- a\n- b\n\nP\n\n  ~~~\n  code\n  ~~~\n",
        "- a\n- b\n\nP\n\n   Q\n",
    ];
    for markdown in rows {
        let mut document = parse(markdown);
        let whole = ops::whole_block(&document, 2);
        ops::delete_selection(&mut document, whole).unwrap();
        ops::insert_text(&mut document, DocPosition::new(1, 1), "x", None).unwrap();
        assert_writes(&document, None);
    }
}

#[test]
fn indented_block_after_an_untouched_list_stays_out() {
    let mut document = parse("- a\n- b\n\nP\n\n    code\n");
    let whole = ops::whole_block(&document, 2);
    ops::delete_selection(&mut document, whole).unwrap();
    assert_writes(&document, None);
}

#[test]
fn rewritten_code_keeps_the_space_before_its_info_string() {
    let mut document = parse("~~~ c#\nx\n~~~\n");
    document.blocks[0].dirty = true;
    assert_eq!(write(&document), "~~~ c#\nx\n~~~\n");
}
