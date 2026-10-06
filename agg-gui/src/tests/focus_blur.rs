//! Tests for clearing keyboard focus from widget code
//! ([`crate::focus::request_blur`] / [`crate::focus::release_focus`]).
//!
//! Sibling of `focus.rs` (which covers `request_focus`). Verifies that a
//! focused `TextField` can drop focus from its own `on_change` handler — the
//! field receives `FocusLost` and later keys reach the unconsumed-key path —
//! and that `release_focus` is a no-op for a widget that doesn't hold focus.

use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;

/// A focused field that calls `request_blur` from its own `on_change` handler
/// loses focus on the next frame: it receives `FocusLost` (seen through its
/// `on_edit_complete`, which fires on focus loss after an uncommitted edit),
/// nothing is focused afterwards, and the next key goes to the unconsumed-key
/// path. (`on_change` rather than `on_enter`: Enter commits the edit itself,
/// so `FocusLost` would then have nothing left to report.)
#[test]
fn request_blur_from_own_handler_clears_focus() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    const FIELD_ID: u64 = 31;
    let completed: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let completed_cb = Rc::clone(&completed);
    let mut root = Container::new().with_padding(4.0);
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(14.0)
            .with_focus_id(FIELD_ID)
            .on_change(|_| crate::focus::request_blur())
            .on_edit_complete(move |t| *completed_cb.borrow_mut() = Some(t.to_string())),
    ));

    let mut app = App::new(Box::new(root));
    let unconsumed: Rc<RefCell<Vec<Key>>> = Rc::new(RefCell::new(Vec::new()));
    let unconsumed_cb = Rc::clone(&unconsumed);
    app.set_global_key_handler(move |key, _| {
        unconsumed_cb.borrow_mut().push(key);
        true
    });
    crate::focus::request_focus(FIELD_ID);
    app.layout(Size::new(200.0, 200.0));
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));

    app.on_key_down(Key::Char('a'), Modifiers::default());
    assert!(
        unconsumed.borrow().is_empty(),
        "the focused field takes 'a'"
    );
    // The request is applied on the next frame, not inside the handler.
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));
    assert!(completed.borrow().is_none());

    app.layout(Size::new(200.0, 200.0));
    assert!(
        app.focused_widget_type_name().is_none(),
        "request_blur leaves nothing focused"
    );
    assert_eq!(
        completed.borrow().as_deref(),
        Some("a"),
        "the field received FocusLost and committed its edit"
    );
    assert!(crate::focus::take_blur_request().is_none(), "one-shot");

    app.on_key_down(Key::Char('b'), Modifiers::default());
    assert_eq!(
        *unconsumed.borrow(),
        vec![Key::Char('b')],
        "with no focus the key takes the unconsumed-key path"
    );
}

/// `release_focus(id)` only blurs the widget with that id; when another
/// widget holds focus it is a no-op (and is consumed, not left pending).
#[test]
fn release_focus_for_unfocused_widget_is_noop() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    const A: u64 = 1;
    const B: u64 = 2;
    let mut root = Container::new().with_padding(4.0);
    for id in [A, B] {
        root.children_mut().push(Box::new(
            TextField::new(Arc::clone(&font))
                .with_font_size(14.0)
                .with_focus_id(id),
        ));
    }
    let mut app = App::new(Box::new(root));
    crate::focus::request_focus(A);
    app.layout(Size::new(200.0, 200.0));
    assert!(app.focused_is_text_input());

    crate::focus::release_focus(B);
    app.layout(Size::new(200.0, 200.0));
    assert_eq!(
        app.focused_widget_type_name(),
        Some("TextField"),
        "releasing a widget that doesn't hold focus keeps the current focus"
    );
    assert!(crate::focus::take_blur_request().is_none());

    // Releasing the holder does clear it.
    crate::focus::release_focus(A);
    app.layout(Size::new(200.0, 200.0));
    assert!(app.focused_widget_type_name().is_none());
}

/// The latest of `request_focus` / `request_blur` wins; `release_focus`
/// cancels only a pending focus request for its own id.
#[test]
fn blur_and_focus_requests_latest_wins() {
    crate::focus::request_focus(5);
    crate::focus::request_blur();
    assert_eq!(crate::focus::take_focus_request(), None);
    assert_eq!(
        crate::focus::take_blur_request(),
        Some(crate::focus::BlurRequest::Any)
    );

    crate::focus::request_blur();
    crate::focus::request_focus(5);
    assert_eq!(crate::focus::take_blur_request(), None);
    assert_eq!(crate::focus::take_focus_request(), Some(5));

    crate::focus::request_focus(5);
    crate::focus::release_focus(6);
    assert_eq!(crate::focus::take_focus_request(), Some(5));
    crate::focus::request_focus(6);
    crate::focus::release_focus(6);
    assert_eq!(crate::focus::take_focus_request(), None);
    // release_focus(6) left an Owner request; clear_blur_request drops it.
    crate::focus::clear_blur_request();
    assert_eq!(crate::focus::take_blur_request(), None);
}
