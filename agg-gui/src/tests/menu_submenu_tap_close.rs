//! A finger closes a submenu by tapping the row that opened it, driven
//! through `App`'s touch entry points the way the shells call them: the
//! touch emulation (`touch_emulation.rs`) replays the tap as a press and
//! release it marks as a finger's (`touch_emulation::last_press_was_touch`),
//! and the menu (`widgets/menu/state.rs` `handle_left_down`) reads that
//! mark.  A mouse click on the same row keeps the submenu open.

use std::sync::Arc;
use std::time::Duration;

use super::touch_pan_fixtures::{reset, screen, Board, Finger};
use crate::input_profile::{profile_test_lock, set_input_profile, InputProfile};
use crate::text::Font;
use crate::widgets::menu::{reset_menu_style, MenuBar, MenuEntry, MenuItem, TopMenu};
use crate::{App, Modifiers, MouseButton, Point, Rect, Size};

const WINDOW: f64 = 400.0;
const VIEWPORT: Size = Size {
    width: WINDOW,
    height: WINDOW,
};
const FINGERS: Finger = Finger { height: WINDOW };
const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
/// Where the bar sits in the window (Y up): across the top.
const BAR: Rect = Rect {
    x: 0.0,
    y: 370.0,
    width: WINDOW,
    height: 30.0,
};
/// A point on the "File" title (window space, Y up).
const FILE_TITLE: (f64, f64) = (20.0, 383.0);
/// Index of the "More" row, the one with a submenu.
const MORE: usize = 1;

fn bar_app() -> App {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let items = vec![
        MenuEntry::Item(MenuItem::action("Cut", "cut")),
        MenuEntry::Item(MenuItem::submenu(
            "More",
            vec![MenuEntry::Item(MenuItem::action("First", "first"))],
        )),
    ];
    let bar = MenuBar::new(font, vec![TopMenu::new("File", items)], |_| {});
    let root = Board::new(WINDOW, WINDOW).place(BAR, Box::new(bar));
    let mut app = App::new(Box::new(root));
    app.layout(VIEWPORT);
    app
}

fn bar(app: &mut App) -> &mut MenuBar {
    app.root_mut().children_mut()[0]
        .as_any_mut()
        .and_then(|any| any.downcast_mut::<MenuBar>())
        .expect("the menu bar")
}

/// The centre of the More row, in window space.
fn more_row(app: &mut App) -> Point {
    let popup = bar(app).popup();
    let row = popup.state.layouts(&popup.items, VIEWPORT)[0].rows[MORE].rect;
    Point::new(
        BAR.x + row.x + row.width * 0.5,
        BAR.y + row.y + row.height * 0.5,
    )
}

fn open_path(app: &mut App) -> Vec<usize> {
    bar(app).popup().state.open_path.clone()
}

fn tap(app: &mut App, p: Point) {
    FINGERS.down(app, p.x, p.y);
    FINGERS.up(app);
    // Let time pass, so nothing reads the next gesture as part of this one.
    crate::clock::advance(Duration::from_secs(1));
    app.layout(VIEWPORT);
}

fn click(app: &mut App, p: Point) {
    let (sx, sy) = screen(WINDOW, p.x, p.y);
    app.on_mouse_down(sx, sy, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(sx, sy, MouseButton::Left, Modifiers::default());
    crate::clock::advance(Duration::from_secs(1));
    app.layout(VIEWPORT);
}

fn start() -> crate::clock::ClockGuard {
    reset(1.0);
    set_input_profile(InputProfile::Desktop);
    reset_menu_style();
    crate::clock::scoped_virtual(None)
}

#[test]
fn a_finger_tap_on_the_row_of_an_open_submenu_closes_it() {
    let _profile = profile_test_lock();
    let _clock = start();
    let mut app = bar_app();

    tap(&mut app, Point::new(FILE_TITLE.0, FILE_TITLE.1));
    assert!(bar(&mut app).popup().is_open(), "a tap opens File");
    let more = more_row(&mut app);
    tap(&mut app, more);
    assert_eq!(open_path(&mut app), vec![MORE], "a tap opens More");

    tap(&mut app, more);
    assert!(
        open_path(&mut app).is_empty(),
        "a second tap on More closes its submenu"
    );
    assert!(bar(&mut app).popup().is_open(), "File stays open");
}

#[test]
fn a_mouse_click_on_the_row_of_an_open_submenu_keeps_it_open() {
    let _profile = profile_test_lock();
    let _clock = start();
    let mut app = bar_app();

    click(&mut app, Point::new(FILE_TITLE.0, FILE_TITLE.1));
    let more = more_row(&mut app);
    click(&mut app, more);
    assert_eq!(open_path(&mut app), vec![MORE], "a click opens More");

    click(&mut app, more);
    assert_eq!(
        open_path(&mut app),
        vec![MORE],
        "a second click leaves More's submenu open"
    );
}
