//! Rust-only tests: the viewer reads grid tables as Markdig reads them
//! (`grid_tables.rs`), each expectation taken from Markdig's own HTML for the
//! same source.  agg-sharp's `MarkdownFeatureRenderingTests` has no grid-table
//! cases to port.

use super::parse::parse_markdown;
use super::{MarkdownRunStyle, ParagraphItem};

/// Each table's rows and whether it has a header row.
fn tables(markdown: &str) -> Vec<(Vec<Vec<String>>, bool)> {
    parse_markdown(markdown)
        .into_iter()
        .filter_map(|item| match item {
            ParagraphItem::Table { rows, header, .. } => Some((rows, header)),
            _ => None,
        })
        .collect()
}

fn rows(cells: &[&[&str]]) -> Vec<Vec<String>> {
    cells
        .iter()
        .map(|row| row.iter().map(|cell| cell.to_string()).collect())
        .collect()
}

/// The text of each flow paragraph.
fn paragraphs(markdown: &str) -> Vec<String> {
    parse_markdown(markdown)
        .into_iter()
        .filter_map(|item| match item {
            ParagraphItem::Flow { items, .. } => Some(
                items
                    .iter()
                    .filter_map(|item| match item {
                        super::InlineItem::Text { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn rust_only_grid_table_with_header_rows_lines_and_spans() {
    // Markdig: <th>H1 x</th><th>H2 y</th>, then a row spanning both columns
    // ("**c** d | e"), a row "f" | "", and "span two" spanning both.
    let source = "+-----+-----+\n| H1  | H2  |\n| x   | y   |\n+=====+=====+\n\
                  | **c** d | e |\n| f   |  |\n+-----+-----+\n| span two  |\n+-----+-----+\nafter";
    assert_eq!(
        tables(source),
        [(
            rows(&[
                &["H1 x", "H2 y"],
                &["c d | e", ""],
                &["f", ""],
                &["span two", ""],
            ]),
            true
        )]
    );
    assert_eq!(paragraphs(source), ["after"]);

    // The bold of `**c**` reaches the cell's spans.
    let items = parse_markdown(source);
    let Some(ParagraphItem::Table { cell_styles, .. }) = items.first() else {
        panic!("expected a table");
    };
    let bold = MarkdownRunStyle {
        bold: true,
        italic: false,
        strikethrough: false,
    };
    let spans = &cell_styles[1][0];
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].start, spans[0].end, spans[0].style), (0, 1, bold));
}

#[test]
fn rust_only_grid_table_without_header_and_around_text() {
    // Markdig: <p>para</p>, a table of a/b and c/d with no <thead>, <p>text</p>.
    let source = "para\n+---+---+\n| a | b |\n+---+---+\n| c | d |\n+---+---+\ntext\n";
    assert_eq!(tables(source), [(rows(&[&["a", "b"], &["c", "d"]]), false)]);
    assert_eq!(paragraphs(source), ["para", "text"]);

    // A row's lines join, an empty line adds nothing, past the last column is dropped,
    // and a line starting with neither `+` nor `|` ends the table (unterminated row kept).
    let source = "+---+---+\n| a | b | c |\n|   | d |\nnot a row";
    assert_eq!(tables(source), [(rows(&[&["a", "b d"]]), false)]);
    assert_eq!(paragraphs(source), ["not a row"]);
}

#[test]
fn rust_only_grid_tables_in_code_stay_code() {
    let source = "```\n+---+\n| a |\n+---+\n```\n\n    +---+\n    | a |\n    +---+\n";
    assert!(tables(source).is_empty());
}
