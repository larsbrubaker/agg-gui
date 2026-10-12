//! agg-sharp `MenuTests` (Tests/Agg.Tests/Agg Automation Tests/MenuTests.cs):
//! a drop-down list opened and closed by clicks.
//!
//! C#'s `DropDownList` is agg-gui's [`ComboBox`]; its `Selected` handlers are
//! the combo's `on_change`, which a click on a row fires every time (also on
//! the row already chosen), as each C# `MenuItem.Selected` does.  The disabled
//! `item3` is `ComboBox::set_item_enabled(2, false)`.
//!
//! Driven through direct `App` events, not agg-gui-automation's runner: the
//! runner finds widgets by name, and the combo's rows are painted by the
//! combo, not widgets of their own, so `ClickByName("item1")` has nothing to
//! find.  A row's click point is the centre of its painted row, as the
//! runner would take from the C# row's bounds.  App events are delivered
//! synchronously, so C#'s `WaitFor` / `WaitForInputToSettle` are already
//! satisfied when each call returns; a frame is painted after each click,
//! as the runner's window draws between inputs.

use super::*;
use crate::text::Font;
use crate::widgets::AbsoluteLayout;
use crate::{Point, Widget};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

const WINDOW: Size = Size {
    width: 300.0,
    height: 200.0,
};
/// The closed list's width; its popup hangs above it (no room below).
const LIST_W: f64 = 120.0;
const CLOSED_H: f64 = 24.0;
const ROW_H: f64 = 22.0;

/// Centre of row `i` of the open three-row list, Y-up window space.
fn row_center(i: usize) -> Point {
    Point::new(
        LIST_W * 0.5,
        CLOSED_H + (2 - i) as f64 * ROW_H + ROW_H * 0.5,
    )
}

/// C#'s `OffMenu`: a 20 x 20 widget at (160, 150).
const OFF_MENU: Point = Point { x: 170.0, y: 160.0 };
/// The closed list itself (C#'s `menu1`).
const MENU1: Point = Point { x: 12.0, y: 12.0 };

/// A frame, as the runner's window draws between inputs: the open list
/// settles which way it hangs when it paints.
fn paint(app: &mut App) {
    let mut fb = Framebuffer::new(WINDOW.width as u32, WINDOW.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

fn click(app: &mut App, p: Point) {
    let y = WINDOW.height - p.y;
    app.on_mouse_down(p.x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(p.x, y, MouseButton::Left, Modifiers::default());
    paint(app);
}

fn is_open(app: &App) -> bool {
    app.root().children()[0]
        .properties()
        .into_iter()
        .any(|(k, v)| k == "open" && v == "true")
}

#[test]
fn open_and_close_menus() {
    let counts: Rc<[Cell<u32>; 3]> = Rc::new(Default::default());
    let seen = Rc::clone(&counts);
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let test_list = ComboBox::new(vec!["item1", "item2", "item3"], 0, font)
        .with_no_selection("no selection")
        .with_item_enabled(2, false)
        .with_max_size(Size::new(LIST_W, CLOSED_H))
        .on_change(move |i| seen[i].set(seen[i].get() + 1));
    let off_menu = SizedBox::new()
        .with_width(20.0)
        .with_height(20.0)
        .with_origin(160.0, 150.0);
    let window = AbsoluteLayout::new()
        .add(Box::new(test_list))
        .add(Box::new(off_menu));
    let mut app = App::new(Box::new(window));
    app.layout(WINDOW);
    let clicks = || [counts[0].get(), counts[1].get(), counts[2].get()];

    assert_eq!(clicks(), [0, 0, 0]);

    click(&mut app, MENU1);
    assert!(is_open(&app));
    click(&mut app, row_center(0));
    assert!(!is_open(&app));
    assert_eq!(clicks(), [1, 0, 0]);

    click(&mut app, MENU1);
    click(&mut app, row_center(1));
    assert!(!is_open(&app));
    assert_eq!(clicks(), [1, 1, 0]);

    click(&mut app, MENU1);
    click(&mut app, row_center(2));
    // The menu was already open: clicking the disabled item leaves it so.
    assert!(is_open(&app));
    assert_eq!(clicks(), [1, 1, 0]);
    click(&mut app, row_center(1));
    assert!(!is_open(&app));
    assert_eq!(clicks(), [1, 2, 0]);

    click(&mut app, MENU1);
    click(&mut app, OFF_MENU);
    assert!(!is_open(&app));

    click(&mut app, MENU1);
    assert!(is_open(&app));

    // Clicking the disabled item must leave the menu open.
    click(&mut app, row_center(2));
    assert!(is_open(&app));
    assert_eq!(clicks()[2], 0);

    // Hovering off the menu is not a click and must not close it either.
    app.on_mouse_move(OFF_MENU.x, WINDOW.height - OFF_MENU.y);
    assert!(is_open(&app));

    click(&mut app, OFF_MENU);
    assert!(!is_open(&app));
}
