//! A finger scrolls a popup menu taller than the window.
//!
//! A menu that overflows scrolls with the wheel and its scroll bar
//! (`state_scroll.rs`); on a phone there is neither, so a finger drag that
//! starts on the menu moves its rows by the finger's travel, clamped as the
//! wheel is.  The drag picks no row, at any point, and a tap afterwards still
//! picks the row under it.  The touches go through the real `App`
//! (`App::on_touch_*`, the touch emulation and its middle-press offer), on
//! the virtual clock so a slow run never turns a drag into a long press.

use std::cell::RefCell;
use std::rc::Rc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect};
use crate::gfx_ctx::GfxCtx;
use crate::touch_state::{TouchDeviceId, TouchId};
use crate::widget::{App, Widget};

use super::super::geometry::BAR_H;
use super::super::model::{MenuEntry, MenuItem};
use super::tests_conformance::{
    center, open_close_items, reset_env, test_font, Harness, Window, VIEWPORT,
};
use super::{MenuBar, TopMenu};

const DEV: TouchDeviceId = TouchDeviceId(0);
const FINGER: TouchId = TouchId(1);
const ROWS: usize = 20;

/// Twenty 48 px rows: far taller than the 400 px window.
fn tall_items() -> Vec<MenuEntry> {
    (0..ROWS)
        .map(|i| MenuItem::action(format!("Row {i}"), format!("{i}")).row_height(48.0))
        .map(MenuEntry::from)
        .collect()
}

/// The desktop baseline, then a phone: the menus are finger-sized from the
/// start, as they are on a phone whose shell declares its profile.
fn phone() {
    reset_env();
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::MobileAndroid);
}

/// Window point (Y-up) to the screen point (Y-down) touches arrive at.
fn screen(p: Point) -> (f64, f64) {
    (p.x, VIEWPORT.height - p.y)
}

/// A finger that goes down at `from` and moves `dy` window px up (negative:
/// down) in steps, then lifts.  The first step crosses the tap tolerance.
fn finger_drag(app: &mut App, from: Point, dy: f64) {
    let (x, y) = screen(from);
    app.on_touch_start(DEV, FINGER, x, y, None);
    let steps = 6;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        app.on_touch_move(DEV, FINGER, x, y - dy * t, None);
    }
    app.on_touch_end(DEV, FINGER);
}

/// A finger tap at window point `p`.
fn tap(app: &mut App, p: Point) {
    let (x, y) = screen(p);
    app.on_touch_start(DEV, FINGER, x, y, None);
    app.on_touch_end(DEV, FINGER);
}

/// The root panel's rect and scroll offset (`None` when it does not scroll).
fn root_panel(h: &Harness) -> (Rect, Option<f64>) {
    let menu = h.menu.borrow();
    let layout = menu.state.layouts(&menu.items, VIEWPORT).remove(0);
    (layout.rect, layout.scroll.map(|s| s.offset))
}

/// The root panel's largest scroll offset.
fn max_offset(h: &Harness) -> f64 {
    let menu = h.menu.borrow();
    let layout = menu.state.layouts(&menu.items, VIEWPORT).remove(0);
    layout
        .scroll
        .expect("scrolls")
        .max_offset(layout.rect.height)
}

/// The window rect of root row `row`, scrolled where the panel is.
fn row_rect(h: &Harness, row: usize) -> Rect {
    super::tests_conformance::row_rect(&h.menu.borrow(), row)
}

fn fully_inside(inner: Rect, outer: Rect) -> bool {
    inner.y >= outer.y - 1e-9 && inner.y + inner.height <= outer.y + outer.height + 1e-9
}

#[test]
fn a_finger_drag_scrolls_an_overflowing_menu_by_its_travel() {
    let _guard = crate::input_profile::profile_test_lock();
    let _clock = crate::clock::scoped_virtual(None);
    phone();
    let mut h = Harness::show(tall_items());
    let (panel, offset) = root_panel(&h);
    assert_eq!(
        offset,
        Some(0.0),
        "the menu overflows and starts at its top"
    );

    // Up by 150 px: the rows follow the finger, showing 150 px further down.
    let from = Point::new(panel.x + 40.0, panel.y + 60.0);
    finger_drag(&mut h.app, from, 150.0);
    assert_eq!(root_panel(&h).1, Some(150.0));
    assert!(h.actions.borrow().is_empty(), "a drag picks no row");
    assert!(h.is_open(), "and leaves the menu up");

    // Down by 50 px brings 50 px of it back.
    finger_drag(&mut h.app, Point::new(from.x, panel.y + 300.0), -50.0);
    assert_eq!(root_panel(&h).1, Some(100.0));

    // Never past either end.
    finger_drag(&mut h.app, Point::new(from.x, panel.y + 40.0), 2000.0);
    assert_eq!(root_panel(&h).1, Some(max_offset(&h)));
    finger_drag(&mut h.app, Point::new(from.x, panel.y + 300.0), -3000.0);
    assert_eq!(root_panel(&h).1, Some(0.0));
    assert!(h.actions.borrow().is_empty());
}

#[test]
fn the_last_row_comes_into_reach_and_a_tap_then_picks_it() {
    let _guard = crate::input_profile::profile_test_lock();
    let _clock = crate::clock::scoped_virtual(None);
    phone();
    let mut h = Harness::show(tall_items());
    let (panel, _) = root_panel(&h);
    let last = ROWS - 1;
    assert!(
        !fully_inside(row_rect(&h, last), panel),
        "the last row starts out of the window"
    );

    // Swipe up twice, each lift on a different row than the press.
    let x = panel.x + 40.0;
    finger_drag(&mut h.app, Point::new(x, panel.y + 30.0), 340.0);
    finger_drag(&mut h.app, Point::new(x, panel.y + 30.0), 340.0);
    assert!(fully_inside(row_rect(&h, last), panel), "Row 19 is in view");
    assert!(h.actions.borrow().is_empty(), "no lift picked a row");

    let at = center(row_rect(&h, last));
    tap(&mut h.app, at);
    assert_eq!(*h.actions.borrow(), vec![format!("{last}")]);
}

#[test]
fn a_menu_that_fits_ignores_a_finger_drag() {
    let _guard = crate::input_profile::profile_test_lock();
    let _clock = crate::clock::scoped_virtual(None);
    phone();
    let mut h = Harness::show(open_close_items());
    let (panel, offset) = root_panel(&h);
    assert_eq!(offset, None, "two rows fit");

    let open = h.center_of_row(0);
    finger_drag(&mut h.app, open, -60.0);
    assert_eq!(root_panel(&h), (panel, None), "nothing moved");
    assert!(h.actions.borrow().is_empty(), "the drag picked nothing");
    assert!(h.is_open());

    let close = h.center_of_row(1);
    tap(&mut h.app, close);
    assert_eq!(*h.actions.borrow(), vec!["close".to_string()]);
}

#[test]
fn the_wheel_still_scrolls_after_a_finger_drag() {
    let _guard = crate::input_profile::profile_test_lock();
    let _clock = crate::clock::scoped_virtual(None);
    phone();
    let mut h = Harness::show(tall_items());
    let (panel, _) = root_panel(&h);
    let inside = Point::new(panel.x + 40.0, panel.y + 100.0);
    finger_drag(&mut h.app, inside, 100.0);
    assert_eq!(root_panel(&h).1, Some(100.0));

    crate::touch_state::clear_last_touch_event_for_testing();
    let (x, y) = screen(inside);
    // One notch back shows the next 40 px.
    h.app.on_mouse_wheel(x, y, -1.0);
    assert_eq!(root_panel(&h).1, Some(140.0));
    assert!(h.actions.borrow().is_empty());
}

/// One frame of `app` (the bar learns its window position as it paints).
fn paint_frame(app: &mut App) {
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());
    app.paint(&mut ctx);
}

/// The menu bar in `app`'s window.
fn menu_bar(app: &App) -> &MenuBar {
    app.root().children()[0]
        .as_any()
        .and_then(|any| any.downcast_ref::<MenuBar>())
        .expect("the window's only child is the MenuBar")
}

/// The open bar menu's root panel and the window rect of its row `row`
/// (the bar's popup is laid out bar-local).
fn bar_panel_and_row(app: &App, row: usize) -> (Rect, Option<f64>, Rect) {
    let bar = menu_bar(app);
    let o = bar.bounds();
    let layout = bar
        .popup
        .state
        .layouts(&bar.popup.items, VIEWPORT)
        .remove(0);
    let shift = |r: Rect| Rect::new(r.x + o.x, r.y + o.y, r.width, r.height);
    let rect = layout
        .rows
        .iter()
        .find(|r| r.item_index == Some(row))
        .map(|r| r.rect)
        .expect("row is laid out");
    (
        shift(layout.rect),
        layout.scroll.map(|s| s.offset),
        shift(rect),
    )
}

#[test]
fn a_finger_scrolls_a_menu_bar_menu_too() {
    let _guard = crate::input_profile::profile_test_lock();
    let _clock = crate::clock::scoped_virtual(None);
    phone();
    let actions = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&actions);
    let bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new("File", tall_items())],
        move |action| sink.borrow_mut().push(action.to_string()),
    );
    let bar_rect = Rect::new(0.0, VIEWPORT.height - BAR_H, VIEWPORT.width, BAR_H);
    let mut app = App::new(Box::new(Window {
        bounds: Rect::default(),
        children: vec![Box::new(bar)],
        rects: vec![bar_rect],
    }));
    app.layout(VIEWPORT);

    // A frame tells the bar where it sits, so its menu scrolls inside the
    // window; a tap on the title opens File.
    paint_frame(&mut app);
    tap(&mut app, Point::new(8.0, VIEWPORT.height - BAR_H * 0.5));
    assert!(menu_bar(&app).popup.is_open(), "the tap opened File");
    paint_frame(&mut app);
    let last = ROWS - 1;
    let (panel, offset, _) = bar_panel_and_row(&app, last);
    assert_eq!(offset, Some(0.0), "File overflows the window");

    let x = panel.x + 40.0;
    finger_drag(&mut app, Point::new(x, panel.y + 30.0), 300.0);
    assert_eq!(bar_panel_and_row(&app, last).1, Some(300.0));
    finger_drag(&mut app, Point::new(x, panel.y + 30.0), 2000.0);
    let (panel, _, row) = bar_panel_and_row(&app, last);
    assert!(fully_inside(row, panel), "Row 19 is in view");
    assert!(actions.borrow().is_empty(), "the drags picked nothing");

    tap(&mut app, center(row));
    assert_eq!(*actions.borrow(), vec![format!("{last}")]);
}
