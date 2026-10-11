//! agg-sharp's `RichMarkdownWriterCodeTableTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichMarkdownWriterCodeTableTests.cs`), ported
//! 1:1 against `markdown_editing::markdown_writer`.
//!
//! Waiting for the reader (`RichMarkdownParser`) and the HTML judge, and
//! ported with it: `RewrittenCodeAndTablesRenderTheSame` and the parse-back
//! half of `TableWritesPipeRowsWithAlignments`.

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
}
