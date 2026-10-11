//! agg-sharp's `RichPasteOperationsTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichPasteOperationsTests.cs`), ported 1:1:
//! pasting fragments into text, lists, quotes, alignment wrappers, code
//! blocks and table cells, slicing selections out, and reading clipboard text.

use std::rc::Rc;

use agg_gui::widgets::markdown_editing::edit_operations;
use agg_gui::widgets::markdown_editing::inlines;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer::write;
use agg_gui::widgets::markdown_editing::paste_operations as paste;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichAlignment, RichBlock, RichBlockKind, RichSelection,
};

fn paste_markdown(
    markdown: &str,
    at: DocPosition,
    fragment_markdown: &str,
) -> (String, RichSelection) {
    let mut document = parse(markdown);
    let fragment = parse(fragment_markdown);
    let caret = paste::insert_document(&mut document, at, &fragment).expect("a valid caret");
    (write(&document), caret)
}

fn paste_text(markdown: &str, at: DocPosition, clipboard_text: &str) -> String {
    let mut document = parse(markdown);
    paste::insert_document(
        &mut document,
        at,
        &paste::parse_clipboard_text(clipboard_text),
    )
    .expect("a valid caret");
    write(&document)
}

#[test]
fn paragraph_pastes_mid_word_keeping_its_style() {
    let (markdown, caret) = paste_markdown("Hello world\n", DocPosition::new(0, 3), "**big**");
    assert_eq!(markdown, "Hel**big**lo world\n");
    assert_eq!(caret.caret, DocPosition::new(0, 6));
}

#[test]
fn paragraphs_split_the_caret_block() {
    let (markdown, caret) = paste_markdown(
        "Hello world\n",
        DocPosition::new(0, 5),
        "one\n\ntwo\n\nthree",
    );
    assert_eq!(markdown, "Helloone\n\ntwo\n\nthree world\n");
    assert_eq!(caret.caret, DocPosition::new(2, 5));
}

#[test]
fn list_comes_in_as_blocks_between_the_halves() {
    let (markdown, _) = paste_markdown("Before after\n", DocPosition::new(0, 7), "- a\n- b");
    assert_eq!(markdown, "Before \n\n- a\n- b\n\nafter\n");
}

#[test]
fn heading_on_an_empty_line_leaves_no_blank_paragraph() {
    // The empty line a user pressed Enter for.
    let mut document = parse("First");
    let mut empty = RichBlock::new(RichBlockKind::Paragraph);
    empty.separator_before = "\n\n".to_string();
    empty.dirty = true;
    document.blocks.push(empty);
    let caret = paste::insert_document(&mut document, DocPosition::new(1, 0), &parse("# Title"))
        .expect("a valid caret");
    assert_eq!(document.blocks.len(), 2);
    assert_eq!(document.blocks[1].kind, RichBlockKind::Heading);
    assert_eq!(write(&document), "First\n\n# Title");
    assert_eq!(caret.caret, DocPosition::new(1, 5));
}

#[test]
fn table_pastes_as_a_table() {
    let (markdown, _) = paste_markdown(
        "End\n",
        DocPosition::new(0, 3),
        "| a | b |\n|---|---|\n| 1 | 2 |",
    );
    assert!(markdown.starts_with("End\n\n| a | b |"), "{markdown:?}");
    let document = parse(&markdown);
    assert_eq!(document.blocks.len(), 2);
    assert_eq!(document.blocks[1].kind, RichBlockKind::Table);
}

#[test]
fn code_block_takes_plain_text_only() {
    let (markdown, caret) =
        paste_markdown("```\nxy\n```\n", DocPosition::new(0, 1), "**b**\n\n- c");
    assert_eq!(markdown, "```\nxb\ncy\n```\n");
    assert_eq!(caret.caret, DocPosition::new(0, 4));
}

#[test]
fn slice_then_paste_round_trips_across_blocks() {
    let source = parse("# Head line\n\nSome **bold** text\n");
    let fragment = paste::slice(
        &source,
        RichSelection::new(DocPosition::new(0, 5), DocPosition::new(1, 9)),
    )
    .expect("a valid selection");
    assert_eq!(paste::visible_text(&fragment), "line\nSome bold");
    assert_eq!(write(&fragment), "# line\n\nSome **bold**");

    // The source is untouched and the fragment pastes more than once.
    assert_eq!(write(&source), "# Head line\n\nSome **bold** text\n");
    let mut target = parse("x\n");
    paste::insert_document(&mut target, DocPosition::new(0, 1), &fragment).expect("a valid caret");
    paste::insert_document(&mut target, DocPosition::new(0, 0), &fragment).expect("a valid caret");
    assert_eq!(target.blocks.len(), 4);
}

#[test]
fn clipboard_text_keeps_its_lines() {
    let parsed = paste::parse_clipboard_text("123 Main St\nSpringfield\n\nNext");
    assert_eq!(parsed.blocks.len(), 2);
    assert_eq!(
        paste::visible_text(&parsed),
        "123 Main St\nSpringfield\nNext"
    );
}

#[test]
fn clipboard_text_with_many_lines_becomes_a_paragraph_per_line() {
    let lines: Vec<String> = (0..paste::MAX_PASTED_LINE_BREAKS + 2)
        .map(|i| format!("line {i}"))
        .collect();

    let parsed = paste::parse_clipboard_text(&lines.join("\n"));
    assert_eq!(parsed.blocks.len(), lines.len());
    assert_eq!(inlines::length(&parsed.blocks[3].inlines), "line 3".len());
    assert!(!parsed.blocks[3]
        .inlines
        .iter()
        .any(|inline| !inline.is_run()));
}

#[test]
fn list_pasted_into_a_list_stays_one_numbered_list() {
    let markdown = paste_text("1. a\n2. b\n", DocPosition::new(0, 1), "1. x\n2. y");
    assert_eq!(markdown, "1. a\n2. x\n3. y\n4. b\n");
}

#[test]
fn paragraphs_pasted_into_a_quote_stay_in_it() {
    let markdown = paste_text("> A B\n", DocPosition::new(0, 1), "x\n\ny\n\nz");
    let document = parse(&markdown);
    assert_eq!(document.blocks.len(), 3);
    let first = document.blocks[0]
        .quote_group
        .clone()
        .expect("a quote group");
    assert!(document
        .blocks
        .iter()
        .all(|block| block.kind == RichBlockKind::Quote
            && block
                .quote_group
                .as_ref()
                .is_some_and(|group| Rc::ptr_eq(group, &first))));
}

#[test]
fn paragraphs_pasted_into_a_centered_section_stay_centered() {
    let markdown = paste_text(
        "<div align=\"center\">\n\nA B\n\n</div>\n",
        DocPosition::new(0, 1),
        "x\n\ny\n\nz",
    );
    let document = parse(&markdown);
    assert_eq!(document.blocks.len(), 3);
    assert!(document
        .blocks
        .iter()
        .all(|block| block.alignment == RichAlignment::Center));
}

#[test]
fn multi_cell_table_slice_holds_only_the_covered_text() {
    let mut source = parse("| a | b |\n|---|---|\n| cc | d |\n");
    let selection = RichSelection::new(
        DocPosition::in_cell(0, 0, 0, 1),
        DocPosition::in_cell(0, 1, 1, 0),
    );
    let fragment = paste::slice(&source, selection).expect("a valid selection");
    assert_eq!(paste::visible_text(&fragment), "\tb\nc\t");

    // What a cut removes is exactly what was copied.
    edit_operations::delete_selection(&mut source, selection).expect("a valid selection");
    assert_eq!(paste::visible_text(&source), "a\t\nc\td");
}

#[test]
fn paste_into_crlf_document_writes_only_crlf() {
    let markdown = paste_text(
        "a b\r\n\r\nend\r\n",
        DocPosition::new(0, 1),
        "x\ny\n\n<div>\nhi\n</div>\n\nz",
    );
    assert!(markdown.contains("<div>"), "{markdown:?}");
    assert!(!markdown.replace("\r\n", "").contains('\n'), "{markdown:?}");
}

#[test]
fn table_cell_takes_a_paragraph_formatted_and_blocks_as_one_line() {
    const TABLE: &str = "| a | b |\n|---|---|\n| c | d |\n";
    let cell = DocPosition::in_cell(0, 1, 1, 0);
    let formatted = paste_text(TABLE, cell, "**x** y");
    assert!(formatted.contains("| c**x** y |"), "{formatted:?}");
    let blocks = paste_text(TABLE, cell, "# h\n\n- i");
    assert!(blocks.contains("| ch i |"), "{blocks:?}");
}

#[test]
fn slice_skips_blocks_the_range_only_touches() {
    let source = parse("- a\n- b\n- c\n");
    let fragment = paste::slice(
        &source,
        RichSelection::new(DocPosition::new(0, 1), DocPosition::new(2, 0)),
    )
    .expect("a valid selection");
    assert_eq!(fragment.blocks.len(), 1);
    assert_eq!(paste::visible_text(&fragment), "b");
}
