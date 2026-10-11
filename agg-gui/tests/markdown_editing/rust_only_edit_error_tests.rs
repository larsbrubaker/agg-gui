//! Rust-only tests for the edit operations' errors: where agg-sharp's
//! operations throw on a caller's mistake (a position the document does not
//! have, a table operation on a non-table block, a code/table range across
//! two blocks), the Rust operations return a `RichEditError` and leave the
//! document unchanged instead of panicking.

use agg_gui::widgets::markdown_editing::edit_operations as ops;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer::write;
use agg_gui::widgets::markdown_editing::style_operations as style_ops;
use agg_gui::widgets::markdown_editing::table_code_operations as table_ops;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichBlockKind, RichEditError, RichInlineStyle, RichPendingStyle, RichSelection,
};

const SOURCE: &str = "one\n\n| a | b |\n|---|---|\n| c | d |\n\n```\ncode\n```\n";

#[test]
fn rust_only_positions_outside_the_document_are_errors() {
    let mut document = parse(SOURCE);
    let before = write(&document);
    let missing_block = DocPosition::new(9, 0);
    let past_end = DocPosition::new(0, 4);
    let missing_cell = DocPosition::in_cell(1, 0, 5, 0);

    for position in [missing_block, past_end, missing_cell] {
        let error = Err(RichEditError::PositionOutOfRange(position));
        assert_eq!(ops::insert_text(&mut document, position, "x", None), error);
        assert_eq!(ops::split_block(&mut document, position), error);
        assert_eq!(ops::backspace(&mut document, position), error);
        assert_eq!(ops::delete(&mut document, position), error);
        assert_eq!(
            ops::delete_range(&mut document, DocPosition::new(0, 0), position),
            error
        );
    }

    let whole = RichSelection {
        whole_block: true,
        ..RichSelection::at(missing_block)
    };
    assert_eq!(
        ops::delete_selection(&mut document, whole),
        Err(RichEditError::PositionOutOfRange(missing_block))
    );
    assert_eq!(write(&document), before);
}

#[test]
fn rust_only_table_operations_on_other_blocks_are_errors() {
    let mut document = parse(SOURCE);
    let paragraph = DocPosition::new(0, 0);
    let error = Err(RichEditError::NotATable(RichBlockKind::Paragraph));
    assert_eq!(table_ops::next_cell(&mut document, paragraph), error);
    assert_eq!(table_ops::previous_cell(&mut document, paragraph), error);
    assert_eq!(table_ops::insert_row(&mut document, paragraph, true), error);
    assert_eq!(table_ops::delete_row(&mut document, paragraph), error);
    assert_eq!(
        table_ops::insert_column(&mut document, paragraph, true),
        error
    );
    assert_eq!(table_ops::delete_column(&mut document, paragraph), error);
    assert_eq!(
        table_ops::insert_text(&mut document, paragraph, "x", None),
        error
    );

    let code = DocPosition::new(2, 0);
    assert_eq!(
        table_ops::insert_row(&mut document, code, false),
        Err(RichEditError::NotATable(RichBlockKind::CodeBlock))
    );
    assert!(!document.blocks.iter().any(|b| b.dirty));
}

#[test]
fn rust_only_code_and_table_range_across_blocks_is_an_error() {
    let mut document = parse(SOURCE);
    assert_eq!(
        table_ops::delete_range(
            &mut document,
            DocPosition::in_cell(1, 0, 0, 0),
            DocPosition::new(2, 1)
        ),
        Err(RichEditError::RangeSpansBlocks)
    );
    assert!(!document.blocks.iter().any(|b| b.dirty));
}

#[test]
fn rust_only_style_range_outside_the_document_is_an_error() {
    let mut document = parse(SOURCE);
    let selection = RichSelection::new(DocPosition::new(0, 0), DocPosition::new(0, 9));
    assert_eq!(
        style_ops::toggle_style(&mut document, selection, RichInlineStyle::BOLD),
        Err(RichEditError::PositionOutOfRange(DocPosition::new(0, 9)))
    );
    assert!(style_ops::style_at(&document, selection, RichPendingStyle::default()).is_err());

    // A caret on a block that does not exist reads as no style, as in C#.
    let missing = RichSelection::at(DocPosition::new(9, 0));
    assert_eq!(
        style_ops::style_at(&document, missing, RichPendingStyle::default()),
        Ok(Default::default())
    );
    assert!(!document.blocks[0].dirty);
}
