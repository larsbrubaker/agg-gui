//! The standalone `RadioButton` (`widgets/radio_button.rs`): a click checks
//! it and unchecks every other radio button in the same parent before any
//! callback runs, a disabled option ignores the pointer, and `check_child`
//! is the programmatic check. The C# port of the sibling rule is
//! `MouseInteractionTests.RadioButtonSiblingsAreChildren` in
//! `agg-gui-automation`.

use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::event::Event;
use crate::text::Font;
use crate::widgets::{check_radio_child, RadioButton};
use crate::WidgetId;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

type Log = Rc<RefCell<Vec<String>>>;

/// A column of three radio buttons ("0" checked) logging their callbacks;
/// the middle one is disabled when `disable_middle`.
fn app_with_radios(log: &Log, disable_middle: bool) -> App {
    let mut col = FlexColumn::new();
    for i in 0..3 {
        let (changed, clicked) = (Rc::clone(log), Rc::clone(log));
        let radio = RadioButton::new(format!("Option {i}"), font())
            .with_checked(i == 0)
            .with_enabled(!(disable_middle && i == 1))
            .on_checked_state_changed(move |c| changed.borrow_mut().push(format!("{i}={c}")))
            .on_click(move || clicked.borrow_mut().push(format!("click {i}")));
        col.push(Box::new(radio), 0.0);
    }
    let mut app = App::new(Box::new(col));
    app.layout(Size::new(300.0, 300.0));
    app
}

fn checked(app: &App) -> Vec<bool> {
    app.root()
        .children()
        .iter()
        .map(|c| {
            c.as_any()
                .and_then(|a| a.downcast_ref::<RadioButton>())
                .expect("a radio button")
                .checked()
        })
        .collect()
}

/// Click the centre of child `i` (Y-down screen coordinates).
fn click(app: &mut App, i: usize) {
    let b = app.root().children()[i].bounds();
    let (x, y) = (b.x + 10.0, 300.0 - (b.y + b.height * 0.5));
    app.on_mouse_move(x, y);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
}

#[test]
fn rust_only_a_click_unchecks_the_siblings_before_the_callbacks_see_it() {
    let log: Log = Rc::default();
    let mut app = app_with_radios(&log, false);
    click(&mut app, 2);
    assert_eq!(checked(&app), vec![false, false, true]);
    // C#'s order: the siblings are unchecked, the clicked one reports its
    // change, then its Click handlers run.
    assert_eq!(*log.borrow(), vec!["0=false", "2=true", "click 2"]);

    // Clicking the checked option again changes nothing but still clicks.
    log.borrow_mut().clear();
    click(&mut app, 2);
    assert_eq!(checked(&app), vec![false, false, true]);
    assert_eq!(*log.borrow(), vec!["click 2"]);
}

#[test]
fn rust_only_a_disabled_option_ignores_clicks() {
    let log: Log = Rc::default();
    let mut app = app_with_radios(&log, true);
    assert!(!app.root().children()[1].is_enabled());
    click(&mut app, 1);
    assert_eq!(checked(&app), vec![true, false, false]);
    assert!(log.borrow().is_empty());
}

#[test]
fn rust_only_check_child_checks_one_and_unchecks_the_rest() {
    let log: Log = Rc::default();
    let mut app = app_with_radios(&log, false);
    check_radio_child(app.root_mut(), 1);
    assert_eq!(checked(&app), vec![false, true, false]);
    assert_eq!(*log.borrow(), vec!["0=false", "1=true"]);
}

#[test]
fn rust_only_an_observer_sees_the_events_a_widget_receives() {
    let log: Log = Rc::default();
    let mut app = app_with_radios(&log, false);
    let seen = Rc::new(Cell::new(0));
    let s = Rc::clone(&seen);
    let id = WidgetId::of(app.root().children()[2].as_ref());
    let observer = crate::observe_events(id, move |event| {
        if matches!(event, Event::MouseUp { .. }) {
            s.set(s.get() + 1);
        }
    });
    click(&mut app, 2);
    assert_eq!(seen.get(), 1);
    drop(observer);
    click(&mut app, 2);
    assert_eq!(seen.get(), 1);
}
