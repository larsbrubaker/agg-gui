//! agg-sharp's `RichMarkdownParserCodeTableTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichMarkdownParserCodeTableTests.cs`), ported
//! 1:1 against `markdown_editing::markdown_parser`: block shapes for code
//! blocks and tables.  Every sample here is also in the round-trip corpus of
//! `rich_markdown_parser_tests`.

use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer;
use agg_gui::widgets::markdown_editing::{InlineAtomKind, RichBlock, RichBlockKind, RichDocument};

const BACKTICK_FENCE: &str = "Intro\n\n```cs\nvar x = 1;\n\n  indented();\n```\n\nAfter\n";
const TILDE_FENCE: &str = "~~~~python\r\nprint('~~~')\r\n```\r\n~~~~\r\n";
const LONG_FENCE: &str = "````\n```\ninner\n```\n````\n";
const INDENTED_FENCE: &str = "  ```\n  a\n   b\n  ```\n";
const FENCE_ARGUMENTS: &str = "```cs title=x\ncode\n```\n";
const ENTITY_INFO: &str = "~~~a&#96;b  \ncode\n~~~\n";
const INDENTED_FENCE_AFTER_PARAGRAPH: &str = "Para\n\n ```\n a\n  b\n ```\n";
const EMPTY_FENCE: &str = "```\n```\n";
const FENCE_AT_EOF: &str = "Text\n\n```js\nlast()\n```";
const UNCLOSED_FENCE: &str = "```js\nunclosed\n\nstill code";
const INDENTED_CODE: &str = "Para\n\n    one\n\n    \n    two  \n\n\nAfter\n";
const CRLF_INDENTED_CODE: &str = "    code\r\n    two\r\n";
const ALIGNED_TABLE: &str = "| Left | Center | Right | None |\n|:-----|:------:|------:|------|\n| **b** | *i* | [l](http://x) | `c` |\n";
const ESCAPED_PIPES: &str = "| a | b |\n|---|---|\n| 1 \\| x | `c|d` |\n";
const NO_OUTER_PIPES: &str = "a | b\n--- | :-:\n1 | 2\n";
const SHORT_ROWS: &str = "| a | b | c |\n|---|---|---|\n| 1 |\n| | x |\n";
const CRLF_TABLE: &str = "| a |\r\n|---|\r\n| **b** |\r\n";
const TABLE_ATOMS: &str = "| ![i](a.png) | <span>x</span> |\n|---|---|\n";
const WIDE_ROW: &str = "|a|b|\n|-|-|\n|1|2|3|\n";
const GRID_TABLE: &str = "+---+\n| a |\n+===+\n| b |\n+---+\n";
const TABLE_IN_ALIGN: &str = "<div align=\"center\">\n\n| a |\n|---|\n| 1 |\n\n</div>\n";
const CODE_IN_ALIGN: &str = "<div align=\"center\">\n\n```\ncode\n```\n\n</div>\n";
const CODE_IN_QUOTE: &str = "> ```\n> code\n> ```\n";

pub const SAMPLES: [&str; 23] = [
    BACKTICK_FENCE,
    TILDE_FENCE,
    LONG_FENCE,
    INDENTED_FENCE,
    FENCE_ARGUMENTS,
    ENTITY_INFO,
    INDENTED_FENCE_AFTER_PARAGRAPH,
    EMPTY_FENCE,
    FENCE_AT_EOF,
    UNCLOSED_FENCE,
    INDENTED_CODE,
    CRLF_INDENTED_CODE,
    ALIGNED_TABLE,
    ESCAPED_PIPES,
    NO_OUTER_PIPES,
    SHORT_ROWS,
    CRLF_TABLE,
    TABLE_ATOMS,
    WIDE_ROW,
    GRID_TABLE,
    TABLE_IN_ALIGN,
    CODE_IN_ALIGN,
    CODE_IN_QUOTE,
];

fn kinds(document: &RichDocument) -> String {
    document
        .blocks
        .iter()
        .map(|b| format!("{:?}", b.kind))
        .collect::<Vec<_>>()
        .join("|")
}

fn single(document: &RichDocument) -> &RichBlock {
    assert_eq!(document.blocks.len(), 1);
    &document.blocks[0]
}

// Rows split by '/', cells by '|'; a '|' inside a cell shows as a broken bar so it cannot pass for a split.
fn cells(table: &RichBlock) -> String {
    table
        .table_rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| {
                    cell.inlines
                        .iter()
                        .map(|inline| {
                            inline
                                .as_run()
                                .map_or("?".to_string(), |r| r.text.replace('|', "¦"))
                        })
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn alignments(table: &RichBlock) -> String {
    table
        .column_alignments
        .iter()
        .map(|a| a.map_or("null".to_string(), |a| format!("{a:?}")))
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn fenced_code_keeps_text_language_and_fence() {
    let document = parse(BACKTICK_FENCE);
    assert_eq!(kinds(&document), "Paragraph|CodeBlock|Paragraph");
    let code = &document.blocks[1];
    assert_eq!(code.code_text, "var x = 1;\n\n  indented();");
    assert_eq!(code.code_language, "cs");
    assert_eq!(code.code_fence, "```");
    assert_eq!(
        code.original_source,
        "```cs\nvar x = 1;\n\n  indented();\n```"
    );

    let tilde_doc = parse(TILDE_FENCE);
    let tilde = single(&tilde_doc);
    assert_eq!(tilde.kind, RichBlockKind::CodeBlock);
    assert_eq!(tilde.code_text, "print('~~~')\n```");
    assert_eq!(tilde.code_language, "python");
    assert_eq!(tilde.code_fence, "~~~~");

    let long_doc = parse(LONG_FENCE);
    let long_fence = single(&long_doc);
    assert_eq!(long_fence.code_text, "```\ninner\n```");
    assert_eq!(long_fence.code_fence, "````");
    assert_eq!(long_fence.code_language, "");

    // The fence's indentation is stripped from the content lines, so it belongs to the block's source.
    let indented = parse(INDENTED_FENCE);
    assert_eq!(single(&indented).code_text, "a\n b");
    assert_eq!(
        indented.blocks[0].original_source,
        INDENTED_FENCE.trim_end_matches('\n')
    );
    assert_eq!(indented.blocks[0].separator_before, "");

    let after_paragraph = parse(INDENTED_FENCE_AFTER_PARAGRAPH);
    assert_eq!(
        after_paragraph.blocks[1].original_source,
        " ```\n a\n  b\n ```"
    );
    assert_eq!(after_paragraph.blocks[1].separator_before, "\n\n");
    assert_eq!(after_paragraph.blocks[1].code_text, "a\n b");
}

#[test]
fn info_string_is_kept_as_written() {
    let arguments_doc = parse(FENCE_ARGUMENTS);
    let arguments = single(&arguments_doc);
    assert_eq!(arguments.kind, RichBlockKind::CodeBlock);
    assert_eq!(arguments.code_info, "cs title=x");
    assert_eq!(arguments.code_language, "cs");

    let entity_doc = parse(ENTITY_INFO);
    let entity = single(&entity_doc);
    assert_eq!(entity.kind, RichBlockKind::CodeBlock);
    assert_eq!(entity.code_info, "a&#96;b");
    assert_eq!(entity.code_language, "a&#96;b");
    assert_eq!(entity.code_fence, "~~~");

    let crlf_doc = parse(TILDE_FENCE);
    assert_eq!(single(&crlf_doc).code_info, "python");
    assert_eq!(parse(INDENTED_CODE).blocks[1].code_info, "");
}

#[test]
fn empty_eof_and_unclosed_fences_are_code_blocks() {
    let empty_doc = parse(EMPTY_FENCE);
    let empty = single(&empty_doc);
    assert_eq!(empty.kind, RichBlockKind::CodeBlock);
    assert_eq!(empty.code_text, "");

    let at_eof = parse(FENCE_AT_EOF);
    assert_eq!(at_eof.blocks[1].code_text, "last()");
    assert_eq!(at_eof.blocks[1].original_source, "```js\nlast()\n```");
    assert_eq!(at_eof.trailing_text, "");

    // An unclosed fence runs to the end of the document.
    let unclosed_doc = parse(UNCLOSED_FENCE);
    let unclosed = single(&unclosed_doc);
    assert_eq!(unclosed.kind, RichBlockKind::CodeBlock);
    assert_eq!(unclosed.code_text, "unclosed\n\nstill code");
    assert_eq!(unclosed.original_source, UNCLOSED_FENCE);
}

#[test]
fn indented_code_ends_at_its_last_line() {
    let document = parse(INDENTED_CODE);
    assert_eq!(kinds(&document), "Paragraph|CodeBlock|Paragraph");
    let code = &document.blocks[1];
    assert_eq!(code.code_text, "one\n\n\ntwo  ");
    assert_eq!(code.code_fence, "");
    assert_eq!(code.original_source, "    one\n\n    \n    two  ");
    assert_eq!(document.blocks[2].separator_before, "\n\n\n");

    let crlf = parse(CRLF_INDENTED_CODE);
    assert_eq!(single(&crlf).code_text, "code\ntwo");
    assert_eq!(crlf.blocks[0].original_source, "    code\r\n    two");
    assert_eq!(crlf.trailing_text, "\r\n");
}

#[test]
fn table_cells_read_as_paragraph_inlines() {
    let table_doc = parse(ALIGNED_TABLE);
    let table = single(&table_doc);
    assert_eq!(table.kind, RichBlockKind::Table);
    assert_eq!(cells(table), "Left|Center|Right|None/b|i|l|c");
    assert_eq!(alignments(table), "Left,Center,Right,null");
    let body: Vec<_> = table.table_rows[1]
        .iter()
        .map(|cell| {
            assert_eq!(cell.inlines.len(), 1);
            &cell.inlines[0]
        })
        .collect();
    assert!(body[0].bold);
    assert!(body[1].italic);
    assert_eq!(body[2].link_url.as_deref(), Some("http://x"));
    assert!(body[3].as_run().is_some_and(|r| r.code));
    assert_eq!(table.original_source, ALIGNED_TABLE.trim_end_matches('\n'));

    let crlf_doc = parse(CRLF_TABLE);
    let crlf = single(&crlf_doc);
    assert_eq!(cells(crlf), "a/b");
    assert_eq!(crlf.table_rows[1][0].inlines.len(), 1);
    assert!(crlf.table_rows[1][0].inlines[0].bold);
    assert_eq!(crlf.original_source, "| a |\r\n|---|\r\n| **b** |");

    let atoms_doc = parse(TABLE_ATOMS);
    let atoms = single(&atoms_doc);
    assert_eq!(atoms.kind, RichBlockKind::Table);
    assert_eq!(atoms.table_rows.len(), 1);
    assert_eq!(atoms.table_rows[0][0].inlines.len(), 1);
    assert_eq!(
        atoms.table_rows[0][0].inlines[0].as_atom().map(|a| a.kind),
        Some(InlineAtomKind::Image)
    );
    assert_eq!(atoms.table_rows[0][1].inlines.len(), 3);
}

#[test]
fn escaped_and_code_pipes_stay_in_their_cell() {
    let table_doc = parse(ESCAPED_PIPES);
    let table = single(&table_doc);
    assert_eq!(table.kind, RichBlockKind::Table);
    assert_eq!(cells(table), "a|b/1 ¦ x|c¦d");
    assert_eq!(table.table_rows[1][1].inlines.len(), 1);
    assert!(table.table_rows[1][1].inlines[0]
        .as_run()
        .is_some_and(|r| r.code));

    let bare_doc = parse(NO_OUTER_PIPES);
    let bare = single(&bare_doc);
    assert_eq!(cells(bare), "a|b/1|2");
    assert_eq!(alignments(bare), "null,Center");
}

#[test]
fn short_rows_are_padded_with_empty_cells() {
    let table_doc = parse(SHORT_ROWS);
    let table = single(&table_doc);
    assert_eq!(table.kind, RichBlockKind::Table);
    assert_eq!(cells(table), "a|b|c/1||/|x|");
    assert!(table.table_rows.iter().all(|row| row.len() == 3));
}

#[test]
fn unmodelled_tables_and_code_in_containers_stay_raw() {
    assert_eq!(kinds(&parse(WIDE_ROW)), "Raw");
    assert_eq!(kinds(&parse(GRID_TABLE)), "Raw");
    assert_eq!(kinds(&parse(CODE_IN_QUOTE)), "Raw");

    // An align group holding a table or code block is not folded: the rich view aligns only text.
    let table = parse(TABLE_IN_ALIGN);
    assert_eq!(kinds(&table), "Raw|Table|Raw");
    assert!(table.blocks[1].align_group.is_none());

    let code = parse(CODE_IN_ALIGN);
    assert_eq!(kinds(&code), "Raw|CodeBlock|Raw");
    assert!(code.blocks[1].align_group.is_none());
}

#[test]
fn clone_copies_code_and_cells() {
    let document = parse(&format!("{TILDE_FENCE}\r\n{ALIGNED_TABLE}"));
    let copy = document.deep_clone();
    assert_eq!(copy.blocks[0].code_fence, "~~~~");
    assert!(!std::ptr::eq(
        copy.blocks[1].table_rows[1][0].inlines.as_ptr(),
        document.blocks[1].table_rows[1][0].inlines.as_ptr()
    ));
    assert_eq!(
        markdown_writer::write(&copy),
        markdown_writer::write(&document)
    );
}
