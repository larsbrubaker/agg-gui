//! agg-sharp `CancelledReleaseTests` (Tests/Agg.Tests/Agg.UI/
//! CancelledReleaseTests.cs): a press whose release is cancelled ends the
//! press but activates nothing.  C#'s cancelled up (`MouseEventArgs.Cancelled`)
//! is agg-gui's [`Event::MouseCaptureLost`](crate::Event::MouseCaptureLost):
//! the App sends it when the window deactivates mid-press (the platform took
//! the pointer) and when the touch layer rules a finger gesture a drag
//! (`touch_emulation`'s `CancelPress`).
//!
//! `ADragValueDoesNotOpenItsEditor` and `AVirtualTableRowIsNotClicked` are
//! not ported: MatterCAD uses neither `DragValue` nor `VirtualTable`.
//!
//! An agg-gui popup row is chosen on its press (a click), not on its release
//! as C#'s `MenuItem` is, so the cancelled release the C# test sends has
//! nothing left to cancel; the press of a gesture ruled a drag does.  Here
//! the C# item's cancelled release is a finger drag across the row: the
//! touch layer offers its press as a middle press once it is past the tap
//! tolerance, and the menu takes it without picking the row or closing
//! (`PopupMenuState::handle_event`), so a finger that wobbles while choosing
//! keeps the menu.

use crate::event::{Modifiers, MouseButton};
use crate::geometry::Point;
use crate::touch_state::{TouchDeviceId, TouchId};
use crate::widget::App;

use super::super::model::{MenuEntry, MenuItem};
use super::tests_conformance::{click, reset_env, Harness, VIEWPORT};

const DEV: TouchDeviceId = TouchDeviceId(0);
const FINGER: TouchId = TouchId(1);

/// Press at window point `p` (Y-up), as a shell delivers it.
fn press(app: &mut App, p: Point) {
    app.on_mouse_down(
        p.x,
        VIEWPORT.height - p.y,
        MouseButton::Left,
        Modifiers::default(),
    );
}

#[test]
fn a_widget_does_not_click() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    // The harness's backdrop is a real counting `Button` filling the window.
    let mut h = Harness::closed(vec![MenuItem::action("Open", "open").into()]);
    press(&mut h.app, Point::new(200.0, 100.0));
    assert_eq!(
        h.beneath.mouse_downs.get(),
        1,
        "the press reached the button"
    );
    assert!(h.app.captured_path().is_some());

    h.app.on_window_deactivated();
    h.app.on_window_activated();

    assert_eq!(h.beneath.clicks.get(), 0);
    assert!(h.app.captured_path().is_none(), "the capture is over");
}

#[test]
fn a_menu_item_is_not_selected() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let items: Vec<MenuEntry> = vec![
        MenuItem::action("Item", "item").into(),
        MenuItem::action("Other", "other").into(),
    ];
    let mut h = Harness::show(items);
    let row = h.center_of_row(0);

    // A finger drag that starts on the row: past the tap tolerance it is a
    // drag, not a tap, so its lift clicks nothing.
    let (sx, sy) = (row.x, VIEWPORT.height - row.y);
    h.app.on_touch_start(DEV, FINGER, sx, sy, None);
    h.app.on_touch_move(DEV, FINGER, sx + 40.0, sy, None);
    h.app.on_touch_end(DEV, FINGER);
    assert!(h.actions.borrow().is_empty(), "a drag selects nothing");
    assert!(h.is_open(), "and leaves the menu up for a real pick");

    crate::touch_state::clear_last_touch_event_for_testing();
    reset_env();
    let row = h.center_of_row(0);
    click(&mut h.app, row, MouseButton::Left);
    assert_eq!(*h.actions.borrow(), vec!["item".to_string()]);
}
