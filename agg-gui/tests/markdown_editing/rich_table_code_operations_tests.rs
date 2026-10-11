//! agg-sharp's `RichTableCodeOperationsTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichTableCodeOperationsTests.cs`), ported
//! 1:1: edits inside code blocks and table cells, and the table's row, column
//! and insert-table operations.
//!
//! Caret offsets count characters, not UTF-16 units (see the
//! `markdown_editing` module header): offsets past an emoji are one less than
//! C#'s.

use agg_gui::widgets::markdown_editing::edit_operations as ops;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::table_code_operations as table_ops;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichAlignment, RichBlock, RichBlockKind, RichDocument, RichSelection,
};

const CODE: &str = "Intro\n\n```cs\nab\ncd\n```\n";

const TABLE: &str = "| h1 | h2 |\n| --- | :-: |\n| a | b |\n";

fn cell(table: &RichBlock, row: usize, column: usize) -> String {
    table.table_rows[row][column]
        .inlines
        .iter()
        .map(|i| i.text().unwrap_or("@"))
        .collect()
}

/// The table as text, rows joined by '/', cells by ',', so a shape reads at a glance.
fn grid(table: &RichBlock) -> String {
    table
        .table_rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            (0..row.len())
                .map(|c| cell(table, r, c))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn block_text(block: &RichBlock) -> String {
    block
        .inlines
        .iter()
        .map(|i| i.text().expect("a run"))
        .collect()
}

fn t(row: usize, column: usize, offset: usize) -> DocPosition {
    DocPosition::in_cell(0, offset, row, column)
}

/// A caret the editor can show: its block exists, a table's row and column
/// exist (0 elsewhere), and the offset is within that block or cell.
fn is_valid(document: &RichDocument, position: DocPosition) -> bool {
    let Some(block) = document.blocks.get(position.block_index) else {
        return false;
    };
    if block.kind == RichBlockKind::Table {
        if position.row >= block.table_rows.len()
            || position.column >= block.table_rows[position.row].len()
        {
            return false;
        }
    } else if position.row != 0 || position.column != 0 {
        return false;
    }

    position.offset <= document.text_length(position)
}

#[test]
fn typing_in_code_keeps_newlines_and_emoji_delete_whole() {
    let mut document = parse(CODE);
    assert!(!document.blocks[1].dirty);

    let selection = ops::insert_text(&mut document, DocPosition::new(1, 2), "\n\u{1F600}x", None);
    assert_eq!(document.blocks[1].code_text, "ab\n\u{1F600}x\ncd");
    assert!(document.blocks[1].dirty);
    // C#'s 6 counts the emoji as two UTF-16 units.
    assert_eq!(selection.caret, DocPosition::new(1, 5));
    assert_eq!(document.blocks.len(), 2);

    // Backspace takes x, then the whole emoji, then the newline.
    let selection = ops::backspace(&mut document, selection.caret);
    let selection = ops::backspace(&mut document, selection.caret);
    assert_eq!(document.blocks[1].code_text, "ab\n\ncd");
    assert_eq!(selection.caret, DocPosition::new(1, 3));
    let selection = ops::backspace(&mut document, selection.caret);
    assert_eq!(document.blocks[1].code_text, "ab\ncd");

    // Forward Delete takes the newline after the caret; at the end nothing happens.
    ops::delete(&mut document, selection.caret);
    assert_eq!(document.blocks[1].code_text, "abcd");
    let selection = ops::delete(&mut document, DocPosition::new(1, 4));
    assert_eq!(document.blocks[1].code_text, "abcd");
    assert!(is_valid(&document, selection.caret));

    // Backspace at a non-empty code block's start does nothing; the block never merges.
    ops::backspace(&mut document, DocPosition::new(1, 0));
    assert_eq!(document.blocks.len(), 2);
    assert_eq!(document.blocks[1].kind, RichBlockKind::CodeBlock);
}

#[test]
fn enter_in_code_adds_a_line_and_twice_at_the_end_leaves_the_block() {
    let mut document = parse(CODE);

    // Enter mid-text is a newline, never a split.
    let selection = ops::split_block(&mut document, DocPosition::new(1, 1));
    assert_eq!(document.blocks[1].code_text, "a\nb\ncd");
    assert_eq!(selection.caret, DocPosition::new(1, 2));

    let length = document.blocks[1].code_text.chars().count();
    let selection = ops::split_block(&mut document, DocPosition::new(1, length));
    assert_eq!(document.blocks[1].code_text, "a\nb\ncd\n");
    assert_eq!(document.blocks.len(), 2);

    let selection = ops::split_block(&mut document, selection.caret);
    assert_eq!(document.blocks[1].code_text, "a\nb\ncd");
    assert_eq!(document.blocks.len(), 3);
    assert_eq!(document.blocks[2].kind, RichBlockKind::Paragraph);
    assert!(document.blocks[2].dirty);
    assert_eq!(selection.caret, DocPosition::new(2, 0));
}

#[test]
fn code_range_delete_and_whole_block_selection() {
    let mut document = parse(CODE);
    let selection = ops::delete_range(
        &mut document,
        DocPosition::new(1, 4),
        DocPosition::new(1, 1),
    );
    assert_eq!(document.blocks[1].code_text, "ad");
    assert_eq!(selection.caret, DocPosition::new(1, 1));

    // Selecting all of the code's text and deleting leaves an empty code block; pasting then replaces it.
    let all = RichSelection::new(DocPosition::new(1, 0), DocPosition::new(1, 2));
    let selection = ops::delete_selection(&mut document, all);
    assert_eq!(document.blocks[1].kind, RichBlockKind::CodeBlock);
    assert_eq!(document.blocks[1].code_text, "");
    ops::insert_text(&mut document, selection.caret, "new\ncode", None);
    assert_eq!(document.blocks[1].code_text, "new\ncode");
    assert_eq!(document.blocks.len(), 2);

    // The block selected whole, as Backspace from the paragraph after it selects it, goes.
    document
        .blocks
        .push(RichBlock::new(RichBlockKind::Paragraph));
    let whole = ops::backspace(&mut document, DocPosition::new(2, 0));
    assert!(whole.whole_block);
    ops::delete_selection(&mut document, whole);
    assert_eq!(document.blocks.len(), 2);
    assert!(document
        .blocks
        .iter()
        .all(|b| b.kind == RichBlockKind::Paragraph));
}

#[test]
fn backspace_in_an_empty_code_block_makes_it_a_paragraph() {
    let mut document = parse("```\nx\n```\n");
    let selection = ops::backspace(&mut document, DocPosition::new(0, 1));
    let selection = ops::backspace(&mut document, selection.caret);
    assert_eq!(document.blocks[0].kind, RichBlockKind::Paragraph);
    assert!(document.blocks[0].dirty);
    assert!(is_valid(&document, selection.caret));

    // Nothing of the code block is left behind for the writer or layout to trip over.
    let block = &document.blocks[0];
    let left = format!(
        "{}{}{}{}",
        block.code_text, block.code_fence, block.code_info, block.code_language
    );
    assert_eq!(left, "");
}

#[test]
fn selecting_a_one_cell_tables_text_deletes_the_text_not_the_table() {
    let mut document = parse("| h |\n| --- |\n");
    let selection =
        ops::delete_selection(&mut document, RichSelection::new(t(0, 0, 0), t(0, 0, 1)));
    assert_eq!(document.blocks.len(), 1);
    assert_eq!(document.blocks[0].kind, RichBlockKind::Table);
    assert_eq!(grid(&document.blocks[0]), "");
    assert_eq!(selection.caret, t(0, 0, 0));
}

#[test]
fn pasting_lines_into_the_last_row_leaves_the_block_below_intact() {
    let mut document = parse(&format!("{TABLE}\nHello\n"));
    let selection = ops::insert_text(&mut document, t(1, 0, 1), "a\nb\nc", None);
    assert_eq!(grid(&document.blocks[0]), "h1,h2/aa,b");
    assert_eq!(document.blocks.len(), 4);

    // The rest of the paste went on as ordinary lines in new paragraphs between the table and Hello.
    assert_eq!(block_text(&document.blocks[1]), "b");
    assert_eq!(block_text(&document.blocks[2]), "c");
    assert!(!document.blocks[3].dirty);
    assert_eq!(block_text(&document.blocks[3]), "Hello");
    assert_eq!(selection.caret, DocPosition::new(2, 1));
}

#[test]
fn typing_in_a_cell_stays_in_that_cell_and_takes_its_style() {
    let mut document = parse("| h |\n| --- |\n| **b** |\n");
    let selection = ops::insert_text(&mut document, t(1, 0, 1), "\u{1F600}", None);
    let table = &document.blocks[0];
    assert_eq!(cell(table, 1, 0), "b\u{1F600}");
    assert_eq!(table.table_rows[1][0].inlines.len(), 1);
    assert!(table.table_rows[1][0].inlines[0].is_run() && table.table_rows[1][0].inlines[0].bold);
    assert!(table.dirty);
    // C#'s 3 counts the emoji as two UTF-16 units.
    assert_eq!(selection.caret, t(1, 0, 2));

    // Backspace removes the emoji whole and stops at the cell's start.
    let selection = ops::backspace(&mut document, selection.caret);
    let selection = ops::backspace(&mut document, selection.caret);
    assert_eq!(cell(&document.blocks[0], 1, 0), "");
    let selection = ops::backspace(&mut document, selection.caret);
    assert_eq!(grid(&document.blocks[0]), "h/");
    assert_eq!(selection.caret, t(1, 0, 0));

    // Delete at a cell's end never reaches the next cell.
    ops::delete(&mut document, t(0, 0, 1));
    assert_eq!(grid(&document.blocks[0]), "h/");
    ops::delete(&mut document, t(0, 0, 0));
    assert_eq!(grid(&document.blocks[0]), "/");
}

#[test]
fn line_breaks_in_a_cell_move_down_the_column_and_out_of_the_table() {
    let mut document = parse(TABLE);
    let selection = ops::insert_text(&mut document, t(0, 1, 2), "x\ny\nz", None);
    assert_eq!(grid(&document.blocks[0]), "h1,h2x/a,by");

    // The last line left the table into a new paragraph below it.
    assert_eq!(document.blocks.len(), 2);
    assert_eq!(document.blocks[1].kind, RichBlockKind::Paragraph);
    assert_eq!(selection.caret, DocPosition::new(1, 1));
    assert_eq!(document.blocks[1].inlines.len(), 1);
    assert_eq!(document.blocks[1].inlines[0].text(), Some("z"));
}

#[test]
fn range_delete_across_cells_clears_text_and_keeps_the_grid() {
    let mut document = parse(TABLE);
    let selection = ops::delete_range(&mut document, t(1, 0, 1), t(0, 1, 1));
    assert_eq!(grid(&document.blocks[0]), "h1,h/,b");
    assert_eq!(selection.caret, t(0, 1, 1));

    // Covering the whole table removes it; an empty paragraph is left to type in.
    let whole = ops::whole_block(&document, 0);
    ops::delete_selection(&mut document, whole);
    assert_eq!(document.blocks.len(), 1);
    assert_eq!(document.blocks[0].kind, RichBlockKind::Paragraph);
}

#[test]
fn tab_and_shift_tab_walk_the_cells_and_tab_in_the_last_cell_adds_a_row() {
    let mut document = parse(TABLE);
    let caret = table_ops::next_cell(&mut document, t(0, 0, 0)).caret;
    assert_eq!(caret, t(0, 1, 2));
    let caret = table_ops::next_cell(&mut document, caret).caret;
    assert_eq!(caret, t(1, 0, 1));
    let caret = table_ops::previous_cell(&mut document, caret).caret;
    assert_eq!(caret, t(0, 1, 2));
    assert_eq!(
        table_ops::previous_cell(&mut document, t(0, 0, 0)).caret,
        t(0, 0, 2)
    );
    assert!(!document.blocks[0].dirty);

    let caret = table_ops::next_cell(&mut document, t(1, 1, 0)).caret;
    assert_eq!(caret, t(2, 0, 0));
    assert_eq!(grid(&document.blocks[0]), "h1,h2/a,b/,");
    assert!(document.blocks[0].dirty);
    assert!(is_valid(&document, caret));
}

#[test]
fn rows_insert_and_delete_but_the_header_stays() {
    let mut document = parse(TABLE);
    let caret = table_ops::insert_row(&mut document, t(1, 1, 0), true).caret;
    assert_eq!(grid(&document.blocks[0]), "h1,h2/a,b/,");
    assert_eq!(caret, t(2, 1, 0));

    // Above the header means just below it: the header is always the first row.
    let caret = table_ops::insert_row(&mut document, t(0, 0, 0), false).caret;
    assert_eq!(grid(&document.blocks[0]), "h1,h2/,/a,b/,");
    assert_eq!(caret, t(1, 0, 0));

    let refused = table_ops::delete_row(&mut document, t(0, 0, 1));
    assert_eq!(grid(&document.blocks[0]), "h1,h2/,/a,b/,");
    assert_eq!(refused.caret, t(0, 0, 1));

    let caret = table_ops::delete_row(&mut document, t(3, 1, 0)).caret;
    assert_eq!(caret, t(2, 1, 1));
    let caret = table_ops::delete_row(&mut document, t(1, 0, 0)).caret;
    let caret = table_ops::delete_row(&mut document, caret).caret;
    assert_eq!(grid(&document.blocks[0]), "h1,h2");
    assert_eq!(caret, t(0, 0, 2));
    assert!(is_valid(&document, caret));
}

#[test]
fn columns_insert_and_delete_with_their_alignments() {
    use RichAlignment::Center;
    let mut document = parse(TABLE);
    let caret = table_ops::insert_column(&mut document, t(1, 1, 0), false).caret;
    let table = &document.blocks[0];
    assert_eq!(grid(table), "h1,,h2/a,,b");
    assert_eq!(table.column_alignments, vec![None, None, Some(Center)]);
    assert_eq!(caret, t(1, 1, 0));
    assert!(table.dirty);

    let caret = table_ops::insert_column(&mut document, t(0, 2, 0), true).caret;
    assert_eq!(grid(&document.blocks[0]), "h1,,h2,/a,,b,");
    assert_eq!(
        document.blocks[0].column_alignments,
        vec![None, None, Some(Center), None]
    );
    assert_eq!(caret, t(0, 3, 0));

    let caret = table_ops::delete_column(&mut document, t(1, 0, 0)).caret;
    assert_eq!(grid(&document.blocks[0]), ",h2,/,b,");
    assert_eq!(
        document.blocks[0].column_alignments,
        vec![None, Some(Center), None]
    );
    assert_eq!(caret, t(1, 0, 0));

    let caret = table_ops::delete_column(&mut document, t(1, 2, 0)).caret;
    assert_eq!(caret, t(1, 1, 1));
    table_ops::delete_column(&mut document, t(0, 0, 0));
    assert_eq!(grid(&document.blocks[0]), "h2/b");
    assert_eq!(document.blocks[0].column_alignments, vec![Some(Center)]);

    // The only column is refused.
    let refused = table_ops::delete_column(&mut document, t(1, 0, 1));
    assert_eq!(grid(&document.blocks[0]), "h2/b");
    assert_eq!(refused.caret, t(1, 0, 1));
}

#[test]
fn insert_table_adds_an_empty_grid_after_the_block_or_its_list() {
    let mut document = parse("Intro\n\n- one\n- two\n\nEnd\n");
    let selection = table_ops::insert_table(&mut document, DocPosition::new(0, 2), 2, 3);
    let table = &document.blocks[1];
    assert_eq!(table.kind, RichBlockKind::Table);
    assert_eq!(grid(table), ",,/,,/,,");
    assert_eq!(table.column_alignments, vec![None, None, None]);
    assert!(table.dirty);
    assert_eq!(selection.caret, DocPosition::in_cell(1, 0, 0, 0));
    assert!(is_valid(&document, selection.caret));

    // From the first list item the table goes after the whole list, not between its items.
    let selection = table_ops::insert_table(&mut document, DocPosition::new(2, 0), 0, 1);
    assert_eq!(document.blocks[4].kind, RichBlockKind::Table);
    assert_eq!(grid(&document.blocks[4]), "");
    assert_eq!(document.blocks[4].table_rows.len(), 1);
    assert_eq!(selection.caret.block_index, 4);
    assert_eq!(document.blocks[5].kind, RichBlockKind::Paragraph);
}
