//! agg-sharp's `RichMarkdownWriterCodeTableTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichMarkdownWriterCodeTableTests.cs`), ported
//! 1:1 against `markdown_editing::markdown_writer`.
//!
//! "Renders the same" compares the viewer's HTML form of the two sources
//! (`widgets::markdown::markdown_html`), where C# compares Markdig's HTML.

use agg_gui::widgets::markdown::markdown_html_collapsed;
use agg_gui::widgets::markdown_editing::markdown_parser;
use agg_gui::widgets::markdown_editing::markdown_writer;
use agg_gui::widgets::markdown_editing::{
    InlineAtomKind, RichAlignment, RichBlock, RichBlockKind, RichDocument, RichInline,
    RichTableCell,
};

fn write_code(fence: &str, info: &str, code: &str) -> String {
    let mut document = RichDocument::new();
    let mut block = RichBlock::new(RichBlockKind::CodeBlock);
    block.code_fence = fence.to_string();
    block.code_info = info.to_string();
    block.code_text = code.to_string();
    block.dirty = true;
    document.blocks.push(block);
    markdown_writer::write(&document)
}

fn cell(inlines: Vec<RichInline>) -> RichTableCell {
    RichTableCell { inlines }
}

const CODE_AND_TABLES: [&str; 7] = [
    "Text\n\n```cs title=x\nvar a = 1;\n\nb();\n```\n",
    "~~~~\n```\ninside\n```\n~~~~\n",
    "Text\n\n    indented\n      more\n\nafter\n",
    "```\n```\n",
    "| Left | Center | Right | None |\n|:-----|:------:|------:|------|\n| **b** | *i* | [l](http://x) | `c` |\n",
    "| a\\|b | `c|d` |\n|---|---|\n| <span>x</span> | ![i](a.png) |\n",
    "| a |\r\n|---|\r\n| **b** |\r\n",
];

/// The judge for code blocks and tables: rewriting every one must not change the rendered HTML.
#[test]
fn rewritten_code_and_tables_render_the_same() {
    for markdown in CODE_AND_TABLES {
        let mut document = markdown_parser::parse(markdown);
        let mut edited = 0;
        for block in &mut document.blocks {
            if matches!(block.kind, RichBlockKind::CodeBlock | RichBlockKind::Table) {
                block.dirty = true;
                edited += 1;
            }
        }
        assert!(edited > 0, "{markdown:?}");
        let written = markdown_writer::write(&document);
        assert_eq!(
            markdown_html_collapsed(&written),
            markdown_html_collapsed(markdown),
            "{markdown:?} rewritten as {written:?}"
        );
    }
}

#[test]
fn code_fence_grows_past_runs_in_the_code() {
    assert_eq!(
        write_code("```", "cs", "a\n```\nb"),
        "````cs\na\n```\nb\n````"
    );
    assert_eq!(write_code("~~~", "", "```"), "~~~\n```\n~~~");
    assert_eq!(write_code("", "", "x\n  y"), "    x\n      y");

    // Indented code cannot start with a blank line or be empty, so those write fenced.
    assert_eq!(write_code("", "", "\nx"), "```\n\nx\n```");
    assert_eq!(write_code("", "", ""), "```\n```");
}

#[test]
fn table_writes_pipe_rows_with_alignments() {
    let mut table = RichBlock::new(RichBlockKind::Table);
    table.dirty = true;
    table.table_rows.push(vec![
        cell(vec![RichInline::run("a|b")]),
        cell(vec![RichInline::run("c|d").code()]),
    ]);
    table.table_rows.push(vec![
        cell(vec![
            RichInline::run("x"),
            RichInline::atom(InlineAtomKind::HardBreak, "  \n"),
            RichInline::run("y"),
        ]),
        cell(vec![]),
    ]);
    table
        .column_alignments
        .extend([Some(RichAlignment::Center), None]);
    let mut document = RichDocument::new();
    document.blocks.push(table);

    let written = markdown_writer::write(&document);
    assert_eq!(written, "| a\\|b | `c|d` |\n| :---: | --- |\n| x y |  |");

    let read = markdown_parser::parse(&written);
    let [read] = read.blocks.as_slice() else {
        panic!("expected one block, got {}", read.blocks.len());
    };
    assert_eq!(read.kind, RichBlockKind::Table);
    let [inline] = read.table_rows[0][0].inlines.as_slice() else {
        panic!(
            "expected one inline, got {:?}",
            read.table_rows[0][0].inlines
        );
    };
    let run = inline.as_run().expect("a run");
    assert_eq!(run.text, "a|b");
}
