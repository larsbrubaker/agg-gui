//! Rust-only tests for `code_block_operations` (agg-sharp's
//! `RichCodeBlockOperations.cs`) and `edit_commands` (`IRichEditCommands.cs`).
//! agg-sharp covers both only through `MarkdownFormatToolbarTests`, which
//! drives them from the toolbar widget; until that toolbar is ported these pin
//! the same scenarios on the model, plus the `Result` paths Rust adds.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::widgets::markdown_editing::code_block_operations as code_ops;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer::write;
use agg_gui::widgets::markdown_editing::style_operations;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichDocumentCommands, RichEditCommands, RichEditError, RichInlineStyle,
    RichPendingStyle, RichSelection,
};

fn p(block: usize, offset: usize) -> DocPosition {
    DocPosition::new(block, offset)
}

#[test]
fn rust_only_code_block_round_trips() {
    let mut document = parse("one\n\n**two**\n\nafter\n");
    let selection = RichSelection::new(p(0, 1), p(1, 2));
    let mapped = code_ops::toggle_code_block(&mut document, selection).expect("a valid selection");
    assert_eq!(document.blocks.len(), 2);
    assert_eq!(document.blocks[0].code_text, "one\ntwo");
    assert_eq!(write(&document), "```\none\ntwo\n```\n\nafter\n");
    assert_eq!(mapped, RichSelection::new(p(0, 1), p(0, 6)));

    let back = code_ops::toggle_code_block(&mut document, mapped).expect("a valid selection");
    assert_eq!(write(&document), "one\n\ntwo\n\nafter\n");
    assert_eq!(back, RichSelection::new(p(0, 1), p(1, 2)));
}

#[test]
fn rust_only_code_block_from_a_list_item_leaves_the_list() {
    let mut document = parse("- one\n- two\n- three\n");
    code_ops::toggle_code_block(&mut document, RichSelection::at(p(1, 0))).expect("a valid caret");
    assert_eq!(write(&document), "- one\n\n```\ntwo\n```\n\n- three\n");
}

#[test]
fn rust_only_code_block_over_all_blocks() {
    let mut document = parse("one\n\ntwo\n");
    code_ops::toggle_code_block(&mut document, RichSelection::new(p(0, 1), p(1, 2)))
        .expect("a valid selection");
    assert_eq!(write(&document), "```\none\ntwo\n```\n");
}

#[test]
fn rust_only_code_block_is_disabled_where_it_cannot_apply() {
    let mut document = parse("before\n\n| a | b |\n| - | - |\n| c | d |\n");
    assert!(code_ops::can_toggle_code_block(
        &document,
        RichSelection::at(p(0, 1))
    ));
    let in_table = RichSelection::at(DocPosition::in_cell(1, 0, 1, 1));
    assert!(!code_ops::can_toggle_code_block(&document, in_table));
    let into_table = RichSelection::new(p(0, 1), DocPosition::in_cell(1, 1, 0, 0));
    assert!(!code_ops::can_toggle_code_block(&document, into_table));

    // Where it cannot apply, toggling changes nothing.
    let before = write(&document);
    let result = code_ops::toggle_code_block(&mut document, into_table).expect("a valid selection");
    assert_eq!(result, into_table);
    assert_eq!(write(&document), before);
}

#[test]
fn rust_only_code_block_to_paragraphs_drops_blank_lines() {
    let mut document = parse("```\na\n\nb\n```\n");
    let mapped = code_ops::toggle_code_block(&mut document, RichSelection::at(p(0, 2)))
        .expect("a valid caret");
    assert_eq!(document.blocks.len(), 2);
    assert_eq!(write(&document), "a\n\nb\n");
    assert_eq!(mapped.caret, p(1, 0));
}

#[test]
fn rust_only_code_block_keeps_atoms_as_their_markdown() {
    // An image's caret stop maps onto its whole markdown in the code text.
    let mut document = parse("a ![i](x.png) b\n");
    let mapped = code_ops::toggle_code_block(&mut document, RichSelection::at(p(0, 4)))
        .expect("a valid caret");
    assert_eq!(document.blocks[0].code_text, "a ![i](x.png) b");
    assert_eq!(mapped.caret, p(0, "a ![i](x.png) ".chars().count()));
}

#[test]
fn rust_only_code_block_outside_the_document_is_an_error() {
    let mut document = parse("one\n");
    let outside = RichSelection::at(p(3, 0));
    assert_eq!(
        code_ops::toggle_code_block(&mut document, outside),
        Err(RichEditError::PositionOutOfRange(p(3, 0)))
    );
    assert!(!code_ops::can_toggle_code_block(&document, outside));
    assert_eq!(write(&document), "one\n");
}

#[test]
fn rust_only_document_commands_store_the_selection_before_raising() {
    let mut commands =
        RichDocumentCommands::new(parse("Hello world\n"), RichSelection::new(p(0, 0), p(0, 5)));
    let events = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&events);
    commands.on_document_changed(Box::new(move || log.borrow_mut().push("document")));
    let log = Rc::clone(&events);
    commands.on_selection_changed(Box::new(move || log.borrow_mut().push("selection")));

    // An edit that keeps the selection raises only the document event.
    commands
        .apply_edit(&mut |document, selection| {
            style_operations::toggle_style(document, selection, RichInlineStyle::BOLD)?;
            Ok((selection, true))
        })
        .expect("a valid edit");
    assert_eq!(write(commands.document()), "**Hello** world\n");
    assert_eq!(*events.borrow(), vec!["document"]);

    // Moving the selection clears the pending style and raises the selection event after the document's.
    events.borrow_mut().clear();
    commands.set_pending_style(RichPendingStyle::default().toggle(RichInlineStyle::ITALIC));
    assert_eq!(*events.borrow(), vec!["selection"]);
    events.borrow_mut().clear();
    commands
        .apply_edit(&mut |_, _| Ok((RichSelection::at(p(0, 2)), true)))
        .expect("a valid edit");
    assert_eq!(*events.borrow(), vec!["document", "selection"]);
    assert!(commands.pending_style().is_empty());
    assert_eq!(commands.selection(), RichSelection::at(p(0, 2)));

    // A refused edit changes nothing and raises nothing.
    events.borrow_mut().clear();
    let refused = commands.apply_edit(&mut |_, _| Err(RichEditError::RangeSpansBlocks));
    assert_eq!(refused, Err(RichEditError::RangeSpansBlocks));
    assert!(events.borrow().is_empty());
    assert_eq!(commands.selection(), RichSelection::at(p(0, 2)));

    commands.focus_text();
    assert_eq!(commands.focus_requests(), 1);
}

#[test]
fn rust_only_bullets_pasted_into_a_numbered_list_take_its_numbering() {
    // Pasted items that join the caret's list adopt its level style (paste_insert's adopt_level_style step).
    let mut document = parse("1. a\n2. b\n");
    let fragment = parse("- x\n- y");
    agg_gui::widgets::markdown_editing::paste_operations::insert_document(
        &mut document,
        p(0, 1),
        &fragment,
    )
    .expect("a valid caret");
    assert_eq!(write(&document), "1. a\n2. x\n3. y\n4. b\n");
}
