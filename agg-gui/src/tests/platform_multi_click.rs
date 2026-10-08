//! Real-mouse multi-clicks as the shells deliver them: OS presses go through
//! the [`InputForwarder`] with [`ClickCount::Auto`], and a `TextField`'s own
//! `MultiClickTracker` decides word and line selection. These pin that
//! behaviour, which must not change when a press also carries a click count
//! into the `App` (`App::on_mouse_down_clicks`): only a *stated* count
//! (simulated input, [`ClickCount::Explicit`]) overrides the tracker. The
//! selection is observed by typing over it.

use super::*;
use crate::shell_input::{ClickCount, ForwarderEvent, InputForwarder};
use crate::text::{measure_advance, Font};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const VIEW_H: f64 = 40.0;
const TEXT: &str = "hello world";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// An App whose root is one bound `TextField` holding [`TEXT`].
fn field_app() -> (App, InputForwarder, Rc<RefCell<String>>) {
    let cell = Rc::new(RefCell::new(TEXT.to_string()));
    let field = TextField::new(font()).with_text_cell(Rc::clone(&cell));
    let mut app = App::new(Box::new(field));
    app.layout(Size::new(400.0, VIEW_H));
    (app, InputForwarder::new(), cell)
}

/// Screen x inside "hello" (padding 8, font size 14, no scroll).
fn x_in_hello() -> f64 {
    8.0 + measure_advance(&font(), &TEXT[..2], 14.0)
}

fn press(app: &mut App, fwd: &mut InputForwarder, x: f64, clicks: ClickCount) {
    press_at(app, fwd, (x, VIEW_H * 0.5), clicks);
}

fn press_at(app: &mut App, fwd: &mut InputForwarder, at: (f64, f64), clicks: ClickCount) {
    let at = Some(at);
    fwd.platform(
        app,
        ForwarderEvent::MouseDown {
            at,
            button: MouseButton::Left,
            modifiers: None,
            clicks,
        },
    );
    fwd.platform(
        app,
        ForwarderEvent::MouseUp {
            at,
            button: MouseButton::Left,
            modifiers: None,
        },
    );
}

/// `n` quick real presses at `x`, 100 ms apart.
fn presses(app: &mut App, fwd: &mut InputForwarder, x: f64, n: usize) {
    for _ in 0..n {
        press(app, fwd, x, ClickCount::Auto);
        crate::clock::advance(Duration::from_millis(100));
    }
}

fn type_x(app: &mut App) {
    app.on_key_down(Key::Char('X'), Modifiers::default());
}

#[test]
fn rust_only_a_real_double_click_selects_the_word() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut app, mut fwd, cell) = field_app();
    presses(&mut app, &mut fwd, x_in_hello(), 2);
    type_x(&mut app);
    assert_eq!(cell.borrow().as_str(), "X world");
}

#[test]
fn rust_only_a_real_triple_click_selects_the_field_and_a_fourth_starts_over() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut app, mut fwd, cell) = field_app();
    presses(&mut app, &mut fwd, x_in_hello(), 3);
    type_x(&mut app);
    assert_eq!(cell.borrow().as_str(), "X");

    let (mut app, mut fwd, cell) = field_app();
    presses(&mut app, &mut fwd, x_in_hello(), 4);
    type_x(&mut app);
    assert_eq!(
        cell.borrow().as_str(),
        "heXllo world",
        "the fourth click places the caret"
    );
}

#[test]
fn rust_only_two_slow_real_clicks_select_nothing() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut app, mut fwd, cell) = field_app();
    press(&mut app, &mut fwd, x_in_hello(), ClickCount::Auto);
    crate::clock::advance(Duration::from_millis(500));
    press(&mut app, &mut fwd, x_in_hello(), ClickCount::Auto);
    type_x(&mut app);
    assert_eq!(cell.borrow().as_str(), "heXllo world");
}

/// The window counts a sequence across widgets; the field's tracker counts
/// its own presses. A real press that reaches the field as the second of the
/// window's sequence is still the field's first, so it selects nothing.
#[test]
fn rust_only_a_real_second_press_that_is_the_fields_first_selects_nothing() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut app, mut fwd, cell) = field_app();
    // Just below the window, where no widget takes the press...
    press_at(
        &mut app,
        &mut fwd,
        (x_in_hello(), VIEW_H + 2.0),
        ClickCount::Auto,
    );
    crate::clock::advance(Duration::from_millis(100));
    // ...then 3 px away, just inside the field's bottom edge.
    press_at(
        &mut app,
        &mut fwd,
        (x_in_hello(), VIEW_H - 1.0),
        ClickCount::Auto,
    );
    assert_eq!(fwd.click_count(), 2, "the window saw a double click");
    type_x(&mut app);
    assert_eq!(cell.borrow().as_str(), "heXllo world");
}

/// A stated count (simulated input) is what the press is: a stated 2 on a
/// field that saw no earlier press still selects the word.
#[test]
fn rust_only_a_stated_double_click_selects_the_word_on_its_own() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut app, mut fwd, cell) = field_app();
    press(&mut app, &mut fwd, x_in_hello(), ClickCount::Explicit(2));
    type_x(&mut app);
    assert_eq!(cell.borrow().as_str(), "X world");
}
