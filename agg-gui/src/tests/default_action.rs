//! Keyboard default / cancel action routing — `Button::with_default_action`
//! / `with_cancel_action` resolved by [`ModalSheet`] (Enter / Escape that
//! bubble to the sheet) and by the [`App`] root when no modal is showing.
//!
//! Split out of `widgets.rs` so that file stays under the 800-line cap.

use super::*;
use crate::widgets::ModalSheet;
use crate::{Event, EventResult, Label, Stack};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

/// A button that counts its clicks.
fn counting_button(label: &str, count: &Rc<Cell<u32>>) -> Button {
    let c = Rc::clone(count);
    Button::new(label, font()).on_click(move || c.set(c.get() + 1))
}

fn press(app: &mut App, key: Key) {
    app.on_key_down(key, Modifiers::default());
}

/// Root scope: with no focus, Enter fires the default button.
#[test]
fn root_enter_fires_the_default_button() {
    let hits = Rc::new(Cell::new(0));
    let other = Rc::new(Cell::new(0));
    let mut col = FlexColumn::new();
    col.push(Box::new(counting_button("Plain", &other)), 0.0);
    col.push(
        Box::new(counting_button("Go", &hits).with_default_action()),
        0.0,
    );
    let mut app = App::new(Box::new(col));
    app.layout(Size::new(300.0, 300.0));
    assert!(app.focused_widget_type_name().is_none());

    press(&mut app, Key::Enter);
    assert_eq!(hits.get(), 1, "default button fires on Enter");
    assert_eq!(other.get(), 0, "only the marked button fires");

    // Other keys do nothing.
    press(&mut app, Key::Char('a'));
    assert_eq!(hits.get(), 1);
}

/// A focused text input consumes Enter itself, so the default never fires
/// while the user is typing.
#[test]
fn focused_text_field_keeps_enter() {
    let hits = Rc::new(Cell::new(0));
    let mut col = FlexColumn::new();
    col.push(Box::new(TextField::new(font()).with_focus_id(7)), 0.0);
    col.push(
        Box::new(counting_button("Go", &hits).with_default_action()),
        0.0,
    );
    let mut app = App::new(Box::new(col));
    app.layout(Size::new(300.0, 300.0));
    crate::focus::request_focus(7);
    app.layout(Size::new(300.0, 300.0));
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));

    press(&mut app, Key::Enter);
    assert_eq!(hits.get(), 0, "text field owns Enter");
}

/// A disabled default button declines; nothing else fires.
#[test]
fn disabled_default_button_does_not_fire() {
    let hits = Rc::new(Cell::new(0));
    let mut col = FlexColumn::new();
    col.push(
        Box::new(
            counting_button("Go", &hits)
                .with_default_action()
                .with_enabled_fn(|| false),
        ),
        0.0,
    );
    let mut app = App::new(Box::new(col));
    app.layout(Size::new(300.0, 300.0));
    press(&mut app, Key::Enter);
    assert_eq!(hits.get(), 0);
}

/// Build `Stack[ app column (root default button), sheet(s) ]`.
struct Harness {
    app: App,
    root_hits: Rc<Cell<u32>>,
}

fn sheet_with_buttons(
    visible: &Rc<Cell<bool>>,
    ok: &Rc<Cell<u32>>,
    cancel: Option<&Rc<Cell<u32>>>,
) -> ModalSheet {
    let mut col = FlexColumn::new();
    col.push(Box::new(TextField::new(font())), 0.0);
    if let Some(cancel) = cancel {
        col.push(
            Box::new(counting_button("Cancel", cancel).with_cancel_action()),
            0.0,
        );
    }
    col.push(
        Box::new(counting_button("OK", ok).with_default_action()),
        0.0,
    );
    ModalSheet::new(Rc::clone(visible), Box::new(col))
}

fn harness(sheets: Vec<ModalSheet>) -> Harness {
    let root_hits = Rc::new(Cell::new(0));
    let mut col = FlexColumn::new();
    col.push(
        Box::new(counting_button("Root default", &root_hits).with_default_action()),
        0.0,
    );
    let mut stack = Stack::new().add(Box::new(col));
    for s in sheets {
        stack = stack.add(Box::new(s));
    }
    let mut app = App::new(Box::new(stack));
    app.layout(Size::new(400.0, 400.0));
    Harness { app, root_hits }
}

/// With a sheet showing, Enter fires the SHEET's default button — not the
/// root one underneath — and the sheet stays open (the button owns that).
#[test]
fn visible_sheet_routes_enter_to_its_own_default() {
    let visible = Rc::new(Cell::new(true));
    let ok = Rc::new(Cell::new(0));
    let mut h = harness(vec![sheet_with_buttons(&visible, &ok, None)]);

    press(&mut h.app, Key::Enter);
    assert_eq!(ok.get(), 1, "sheet default fires");
    assert_eq!(
        h.root_hits.get(),
        0,
        "root default is shadowed by the modal"
    );
    assert!(visible.get(), "firing the default does not auto-close");
}

/// A hidden sheet contributes nothing; the root default fires instead.
#[test]
fn hidden_sheet_does_not_shadow_the_root_default() {
    let visible = Rc::new(Cell::new(false));
    let ok = Rc::new(Cell::new(0));
    let mut h = harness(vec![sheet_with_buttons(&visible, &ok, None)]);

    press(&mut h.app, Key::Enter);
    assert_eq!(ok.get(), 0);
    assert_eq!(h.root_hits.get(), 1);
}

/// Two stacked sheets: only the topmost one's default fires.
#[test]
fn only_the_topmost_sheet_default_fires() {
    let vis_a = Rc::new(Cell::new(true));
    let vis_b = Rc::new(Cell::new(true));
    let ok_a = Rc::new(Cell::new(0));
    let ok_b = Rc::new(Cell::new(0));
    let mut h = harness(vec![
        sheet_with_buttons(&vis_a, &ok_a, None),
        sheet_with_buttons(&vis_b, &ok_b, None),
    ]);

    press(&mut h.app, Key::Enter);
    assert_eq!(ok_b.get(), 1, "topmost sheet");
    assert_eq!(ok_a.get(), 0, "sheet beneath is shadowed");
    assert_eq!(h.root_hits.get(), 0);

    // Dismiss the top sheet: the next Enter reaches sheet A.
    vis_b.set(false);
    h.app.layout(Size::new(400.0, 400.0));
    press(&mut h.app, Key::Enter);
    assert_eq!(ok_a.get(), 1);
    assert_eq!(ok_b.get(), 1);
}

/// Escape fires a `with_cancel_action` button instead of closing the
/// sheet — the button's handler owns dismissal.
#[test]
fn escape_fires_the_cancel_button_and_leaves_closing_to_it() {
    let visible = Rc::new(Cell::new(true));
    let ok = Rc::new(Cell::new(0));
    let cancel = Rc::new(Cell::new(0));
    let mut h = harness(vec![sheet_with_buttons(&visible, &ok, Some(&cancel))]);

    press(&mut h.app, Key::Escape);
    assert_eq!(cancel.get(), 1, "cancel button fires");
    assert_eq!(ok.get(), 0);
    assert!(
        visible.get(),
        "sheet did not close itself — the button decides"
    );
}

/// Without a cancel button, Escape keeps the original close behaviour.
#[test]
fn escape_without_cancel_action_closes_the_sheet() {
    let visible = Rc::new(Cell::new(true));
    let ok = Rc::new(Cell::new(0));
    let mut h = harness(vec![sheet_with_buttons(&visible, &ok, None)]);
    press(&mut h.app, Key::Escape);
    assert!(!visible.get());
}

/// Closure forms: `with_default_action` / `with_cancel_action` on the
/// sheet itself, for content without marked buttons.
#[test]
fn sheet_closure_actions_run_on_enter_and_escape() {
    let visible = Rc::new(Cell::new(true));
    let entered = Rc::new(Cell::new(0));
    let cancelled = Rc::new(Cell::new(0));
    let e = Rc::clone(&entered);
    let c = Rc::clone(&cancelled);
    let sheet = ModalSheet::new(
        Rc::clone(&visible),
        Box::new(Label::new("plain content", font())),
    )
    .with_default_action(move || e.set(e.get() + 1))
    .with_cancel_action(move || c.set(c.get() + 1));
    let mut h = harness(vec![sheet]);

    press(&mut h.app, Key::Enter);
    assert_eq!(entered.get(), 1);
    assert_eq!(h.root_hits.get(), 0);

    press(&mut h.app, Key::Escape);
    assert_eq!(cancelled.get(), 1);
    assert!(
        visible.get(),
        "closure cancel action replaces the built-in close"
    );
}

/// A focused text field INSIDE the sheet still owns Enter.
#[test]
fn focused_field_inside_sheet_keeps_enter() {
    let visible = Rc::new(Cell::new(true));
    let ok = Rc::new(Cell::new(0));
    let mut col = FlexColumn::new();
    col.push(Box::new(TextField::new(font()).with_focus_id(11)), 0.0);
    col.push(
        Box::new(counting_button("OK", &ok).with_default_action()),
        0.0,
    );
    let sheet = ModalSheet::new(Rc::clone(&visible), Box::new(col));
    let mut h = harness(vec![sheet]);
    crate::focus::request_focus(11);
    h.app.layout(Size::new(400.0, 400.0));
    assert_eq!(h.app.focused_widget_type_name(), Some("TextField"));

    press(&mut h.app, Key::Enter);
    assert_eq!(ok.get(), 0);
}

/// Direct dispatch to the sheet (no App): Enter with no default returns
/// Consumed (swallowed, like every other key) so nothing leaks behind.
#[test]
fn sheet_without_default_still_swallows_enter() {
    let visible = Rc::new(Cell::new(true));
    let mut sheet = ModalSheet::new(Rc::clone(&visible), Box::new(Label::new("plain", font())));
    sheet.layout(Size::new(300.0, 300.0));
    let r = sheet.on_event(&Event::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    assert_eq!(r, EventResult::Consumed);
    assert!(visible.get());
}
