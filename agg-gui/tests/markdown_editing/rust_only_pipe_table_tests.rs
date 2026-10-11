//! Rust-only tests: the rich editor's reader finds pipe tables as Markdig does
//! (`markdown_editing::pipe_tables`), where pulldown-cmark follows GFM.  Each
//! expectation is what agg-sharp's `RichMarkdownParser` reads, checked against
//! Markdig with the viewer's pipeline.

use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer;
use agg_gui::widgets::markdown_editing::{RichAlignment, RichBlockKind, RichDocument};

fn kinds(document: &RichDocument) -> Vec<RichBlockKind> {
    document.blocks.iter().map(|block| block.kind).collect()
}

fn cell_text(document: &RichDocument, block: usize, row: usize, column: usize) -> String {
    document.blocks[block].table_rows[row][column]
        .inlines
        .iter()
        .filter_map(|inline| inline.as_run().map(|run| run.text.clone()))
        .collect()
}

#[test]
fn rust_only_code_span_pipe_in_the_header_still_makes_a_table() {
    // GFM reads three header cells against two delimiter cells: no table.
    let source = "| a\\|b | `c|d` |\n|---|---|\n| x | `y|z` |";
    let document = parse(source);
    assert_eq!(kinds(&document), [RichBlockKind::Table]);
    assert_eq!(cell_text(&document, 0, 0, 0), "a|b");
    assert_eq!(cell_text(&document, 0, 0, 1), "c|d");
    assert_eq!(cell_text(&document, 0, 1, 1), "y|z");
    assert_eq!(markdown_writer::write(&document), source);
}

#[test]
fn rust_only_header_and_delimiter_widths_need_not_match() {
    let document = parse("| a | b |\n|:--|--:|\n| c |");
    assert_eq!(kinds(&document), [RichBlockKind::Table]);
    assert_eq!(
        document.blocks[0].column_alignments,
        [Some(RichAlignment::Left), Some(RichAlignment::Right)]
    );

    // A header wider than its delimiter row is a Markdig table the rich view does not
    // edit: its last column has no alignment to write.
    let document = parse("| a | b | c |\n|---|---|");
    assert_eq!(kinds(&document), [RichBlockKind::Raw]);
}

#[test]
fn rust_only_a_line_without_a_pipe_makes_the_table_text() {
    // GFM takes "plain" for a body row; Markdig reads the whole paragraph as text.
    let source = "| a | b |\n|---|---|\nplain";
    let document = parse(source);
    assert_ne!(kinds(&document), [RichBlockKind::Table]);
    assert_eq!(markdown_writer::write(&document), source);
}

#[test]
fn rust_only_a_table_under_a_paragraph_line_is_its_own_block() {
    let source = "foo\n| `a|b` | c |\n|---|---|";
    let document = parse(source);
    assert_eq!(
        kinds(&document),
        [RichBlockKind::Paragraph, RichBlockKind::Table]
    );
    assert_eq!(document.blocks[1].separator_before, "\n");
    assert_eq!(cell_text(&document, 1, 0, 0), "a|b");
    assert_eq!(markdown_writer::write(&document), source);
}
