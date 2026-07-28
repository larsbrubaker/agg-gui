//! Tests for the programmatic focus-request channel ([`crate::focus`]).
//!
//! Split out of `widgets.rs` so that file stays under the workspace
//! 800-line cap. Verifies that `focus::request_focus(id)` moves keyboard
//! focus to the matching widget on the next `layout`, and that a request
//! with no matching widget is consumed as a no-op.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;
use crate::{Event, EventResult};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;

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

// ── Tab focus traversal: dispatch-first opt-out ────────────────────────────
//
// `App::on_key_down` offers a plain Tab / Shift+Tab to the focused widget first
// and only advances focus when the widget Ignores it (an editor consuming Tab
// to indent must therefore keep focus). Ctrl/Meta+Tab skips the dispatch and
// always traverses — the guaranteed escape hatch out of a Tab-consuming widget.

/// A focusable leaf that records whether it currently holds focus and, when
/// `consume_tab` is set, consumes plain Tab (like an editor that indents).
struct TabSpy {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    consume_tab: bool,
    focused: Rc<Cell<bool>>,
}

impl TabSpy {
    fn new(consume_tab: bool, focused: Rc<Cell<bool>>) -> Self {
        Self {
            bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
            children: Vec::new(),
            consume_tab,
            focused,
        }
    }
}

impl Widget for TabSpy {
    fn type_name(&self) -> &'static str {
        "TabSpy"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(50.0, 50.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn is_focusable(&self) -> bool {
        true
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::FocusGained => {
                self.focused.set(true);
                EventResult::Consumed
            }
            Event::FocusLost => {
                self.focused.set(false);
                EventResult::Consumed
            }
            // Consume only *plain* Tab; Ctrl/Meta+Tab is never delivered here
            // (the App routes it straight to traversal), but ignore it anyway.
            Event::KeyDown {
                key: Key::Tab,
                modifiers,
            } if self.consume_tab && !modifiers.ctrl && !modifiers.meta => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
}

/// Build an App with two `TabSpy` children; `consume` picks which of them
/// consumes plain Tab. Returns the app and the two focus-state cells.
fn two_spy_app(consume: [bool; 2]) -> (App, Rc<Cell<bool>>, Rc<Cell<bool>>) {
    let a_focused = Rc::new(Cell::new(false));
    let b_focused = Rc::new(Cell::new(false));
    let mut root = Container::new().with_padding(4.0);
    root.children_mut()
        .push(Box::new(TabSpy::new(consume[0], Rc::clone(&a_focused))));
    root.children_mut()
        .push(Box::new(TabSpy::new(consume[1], Rc::clone(&b_focused))));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(200.0, 200.0));
    (app, a_focused, b_focused)
}

/// A focused widget that consumes Tab keeps focus — traversal does not fire.
#[test]
fn focused_widget_consuming_tab_keeps_focus() {
    let (mut app, a_focused, b_focused) = two_spy_app([true, true]);
    // First Tab (nothing focused) lands on the first widget.
    app.on_key_down(Key::Tab, Modifiers::default());
    assert!(a_focused.get(), "first Tab focuses the first widget");
    assert!(!b_focused.get());
    // A second Tab is consumed by the focused widget, so focus stays put.
    app.on_key_down(Key::Tab, Modifiers::default());
    assert!(a_focused.get(), "a Tab-consuming widget retains focus");
    assert!(
        !b_focused.get(),
        "focus must not advance to the second widget"
    );
}

/// A focused widget that ignores Tab still advances focus, exactly as before.
#[test]
fn focused_widget_ignoring_tab_advances_focus() {
    let (mut app, a_focused, b_focused) = two_spy_app([false, false]);
    app.on_key_down(Key::Tab, Modifiers::default());
    assert!(a_focused.get(), "first Tab focuses the first widget");
    app.on_key_down(Key::Tab, Modifiers::default());
    assert!(!a_focused.get(), "focus left the first widget");
    assert!(b_focused.get(), "an ignoring widget lets Tab advance focus");
}

/// Shift+Tab still traverses in reverse for widgets that ignore Tab.
#[test]
fn shift_tab_reverse_traversal_for_ignoring_widget() {
    let (mut app, a_focused, b_focused) = two_spy_app([false, false]);
    // Forward twice → second widget focused.
    app.on_key_down(Key::Tab, Modifiers::default());
    app.on_key_down(Key::Tab, Modifiers::default());
    assert!(b_focused.get(), "two Tabs reach the second widget");
    // Shift+Tab steps back to the first.
    app.on_key_down(
        Key::Tab,
        Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    assert!(a_focused.get(), "Shift+Tab reverses to the first widget");
    assert!(!b_focused.get());
}

/// Ctrl+Tab advances focus even when the focused widget would consume plain
/// Tab — the guaranteed escape hatch out of a Tab-trapping editor.
#[test]
fn ctrl_tab_advances_focus_past_a_tab_consumer() {
    let (mut app, a_focused, b_focused) = two_spy_app([true, true]);
    app.on_key_down(Key::Tab, Modifiers::default());
    assert!(
        a_focused.get(),
        "first Tab focuses the first (consuming) widget"
    );
    app.on_key_down(
        Key::Tab,
        Modifiers {
            ctrl: true,
            ..Default::default()
        },
    );
    assert!(
        !a_focused.get(),
        "Ctrl+Tab escapes the Tab-consuming widget"
    );
    assert!(
        b_focused.get(),
        "Ctrl+Tab advances focus to the second widget"
    );
}

/// The clipboard bridge gates on `Widget::accepts_text_input` — the property
/// `focused_is_text_input` delegates to. Editors opt in via the trait; a Button
/// (focusable, but not text-bearing) never does. Guards the trait delegation
/// against a regression back to a hardcoded type-name list.
#[test]
fn accepts_text_input_gates_the_clipboard_bridge() {
    use crate::widget::Widget;
    use crate::widgets::rich_text::{uniform_resolver, RichDoc, RichTextEdit};

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let editor = RichTextEdit::new(RichDoc::new(), uniform_resolver(Arc::clone(&font)));
    assert!(
        editor.accepts_text_input(),
        "RichTextEdit accepts text input"
    );
    let button = Button::new("ok", Arc::clone(&font));
    assert!(
        !button.accepts_text_input(),
        "a Button does not accept text input"
    );
}
