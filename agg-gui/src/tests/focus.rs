//! Tests for the programmatic focus-request channel ([`crate::focus`]).
//!
//! Split out of `widgets.rs` so that file stays under the workspace
//! 800-line cap. Verifies that `focus::request_focus(id)` moves keyboard
//! focus to the matching widget on the next `layout`, and that a request
//! with no matching widget is consumed as a no-op.

use super::*;
use crate::text::Font;
use std::sync::Arc;

/// `focus::request_focus(id)` moves focus to the matching field on the next
/// `layout`, even with no pointer interaction.
#[test]
fn test_programmatic_focus_request() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());

    const FIELD_ID: u64 = 4242;
    let mut root = Container::new().with_padding(4.0);
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font)).with_font_size(14.0),
    ));
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(14.0)
            .with_focus_id(FIELD_ID),
    ));

    let mut app = App::new(Box::new(root));
    app.layout(Size::new(200.0, 200.0));
    assert!(
        app.focused_widget_type_name().is_none(),
        "nothing focused before a request"
    );

    // Request focus for the second field, then run a frame's layout.
    crate::focus::request_focus(FIELD_ID);
    app.layout(Size::new(200.0, 200.0));

    assert_eq!(
        app.focused_widget_type_name(),
        Some("TextField"),
        "the field with the matching focus id should be focused"
    );
    // The request is one-shot: a later layout with no new request keeps focus
    // but doesn't re-trigger anything.
    assert!(crate::focus::take_focus_request().is_none());
}

/// An unmatched focus id is a no-op (and is consumed, not left pending).
#[test]
fn test_programmatic_focus_request_unmatched_id() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut root = Container::new().with_padding(4.0);
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font)).with_font_size(14.0),
    ));

    let mut app = App::new(Box::new(root));
    crate::focus::request_focus(9999);
    app.layout(Size::new(200.0, 200.0));

    assert!(app.focused_widget_type_name().is_none());
    assert!(
        crate::focus::take_focus_request().is_none(),
        "an unmatched request is still consumed"
    );
}

/// The web clipboard bridge focuses a hidden `<textarea>` only while a text
/// widget is focused; that focus is what makes browsers deliver copy / cut /
/// paste DOM events. A focused `RichTextEdit` must therefore report as a text
/// input — regression guard for the WASM demo where it was omitted and its
/// clipboard shortcuts silently no-op'd.
#[test]
fn rich_text_edit_is_a_focused_text_input() {
    use crate::widgets::rich_text::{uniform_resolver, RichDoc, RichTextEdit};

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());

    const FIELD_ID: u64 = 7777;
    let mut root = Container::new().with_padding(4.0);
    root.children_mut().push(Box::new(
        RichTextEdit::new(RichDoc::new(), uniform_resolver(Arc::clone(&font)))
            .with_font_size(14.0)
            .with_focus_id(FIELD_ID),
    ));

    let mut app = App::new(Box::new(root));
    app.layout(Size::new(200.0, 200.0));
    assert!(
        !app.focused_is_text_input(),
        "nothing focused before a request"
    );

    crate::focus::request_focus(FIELD_ID);
    app.layout(Size::new(200.0, 200.0));

    assert_eq!(app.focused_widget_type_name(), Some("RichTextEdit"));
    assert!(
        app.focused_is_text_input(),
        "a focused RichTextEdit must count as a text input for the clipboard bridge"
    );
}

/// The text-input classifier must agree with each editor's `type_name`.
#[test]
fn text_input_classifier_covers_all_editors() {
    use crate::widget::is_text_input_type_name;
    assert!(is_text_input_type_name("TextField"));
    assert!(is_text_input_type_name("TextArea"));
    assert!(is_text_input_type_name("RichTextEdit"));
    assert!(!is_text_input_type_name("Button"));
}
