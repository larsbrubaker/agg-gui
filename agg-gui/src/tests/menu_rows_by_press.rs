//! Popup menus size their rows by the press that opened them: a menu a
//! finger opened (a tap or a long press) gets finger-sized rows
//! ([`TOUCH_MIN`]), a menu the mouse or the keyboard opened gets the
//! style's own rows, every time, even after the screen has been touched.
//! A submenu takes its parent's size, and a phone or tablet input profile
//! sizes every menu for fingers.  Driven through `App`'s touch and mouse
//! entry points, the way the shells call them, on the virtual UI clock.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use super::touch_pan_fixtures::{reset, screen, Board, Finger};
use crate::input_profile::{profile_test_lock, set_input_profile, InputProfile};
use crate::text::Font;
use crate::touch_emulation::LONG_PRESS_DELAY;
use crate::widgets::menu::{
    reset_menu_style, MenuBar, MenuEntry, MenuItem, PopupMenu, TopMenu, ROW_H, TOUCH_MIN,
};
use crate::{App, DrawCtx, Event, EventResult, Key, Modifiers, MouseButton, Rect, Size, Widget};

const WINDOW: f64 = 400.0;
const VIEWPORT: Size = Size {
    width: WINDOW,
    height: WINDOW,
};
const FINGERS: Finger = Finger { height: WINDOW };
const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

/// A window-filling widget that opens its context menu at any press while
/// the menu is closed, and hands every event to the menu while it is open.
struct MenuHost {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    menu: Rc<RefCell<PopupMenu>>,
}

impl Widget for MenuHost {
    fn type_name(&self) -> &'static str {
        "MenuHost"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        let mut menu = self.menu.borrow_mut();
        if menu.is_open() {
            menu.handle_event(event, VIEWPORT);
            return EventResult::Consumed;
        }
        if let Event::MouseDown { pos, .. } = event {
            menu.open_at(*pos);
        }
        EventResult::Consumed
    }
}

fn items() -> Vec<MenuEntry> {
    vec![
        MenuEntry::Item(MenuItem::action("Cut", "cut")),
        MenuEntry::Item(MenuItem::submenu(
            "More",
            vec![
                MenuEntry::Item(MenuItem::action("First", "first")),
                MenuEntry::Item(MenuItem::action("Second", "second")),
            ],
        )),
    ]
}

/// The same start for every test: desktop profile, no touch yet, the
/// default menu style, and the virtual clock.
fn start() -> crate::clock::ClockGuard {
    reset(1.0);
    set_input_profile(InputProfile::Desktop);
    reset_menu_style();
    crate::clock::scoped_virtual(None)
}

/// An `App` whose whole window is a [`MenuHost`].
fn host_app() -> (App, Rc<RefCell<PopupMenu>>) {
    let menu = Rc::new(RefCell::new(PopupMenu::new(items())));
    let host = MenuHost {
        bounds: Rect::default(),
        children: Vec::new(),
        menu: Rc::clone(&menu),
    };
    let root =
        Board::new(WINDOW, WINDOW).place(Rect::new(0.0, 0.0, WINDOW, WINDOW), Box::new(host));
    let mut app = App::new(Box::new(root));
    app.layout(VIEWPORT);
    (app, menu)
}

/// Heights of every row of every open level, root first.
fn row_heights(menu: &PopupMenu) -> Vec<Vec<f64>> {
    menu.state
        .layouts(&menu.items, VIEWPORT)
        .iter()
        .map(|level| level.rows.iter().map(|row| row.rect.height).collect())
        .collect()
}

/// A mouse click with `button` at the Y-up window point `(x, y)`.
fn click(app: &mut App, x: f64, y: f64, button: MouseButton) {
    let (sx, sy) = screen(WINDOW, x, y);
    app.on_mouse_move(sx, sy);
    app.on_mouse_down(sx, sy, button, Modifiers::default());
    app.on_mouse_up(sx, sy, button, Modifiers::default());
}

fn tap(app: &mut App, x: f64, y: f64) {
    FINGERS.down(app, x, y);
    FINGERS.up(app);
}

fn long_press(app: &mut App, x: f64, y: f64) {
    FINGERS.down(app, x, y);
    crate::clock::advance(LONG_PRESS_DELAY);
    app.layout(VIEWPORT);
    FINGERS.up(app);
}

/// Let time pass between gestures, so nothing reads one as part of another.
fn pause() {
    crate::clock::advance(Duration::from_secs(1));
}

#[test]
fn each_open_sizes_rows_by_the_press_that_opened_the_menu() {
    let _profile = profile_test_lock();
    let _clock = start();
    let (mut app, menu) = host_app();

    tap(&mut app, 100.0, 300.0);
    assert!(menu.borrow().is_open(), "a tap opens the menu");
    assert_eq!(row_heights(&menu.borrow()), vec![vec![TOUCH_MIN; 2]]);
    menu.borrow_mut().close();
    pause();

    click(&mut app, 100.0, 300.0, MouseButton::Right);
    assert!(menu.borrow().is_open(), "a right click opens the menu");
    assert_eq!(
        row_heights(&menu.borrow()),
        vec![vec![ROW_H; 2]],
        "the mouse opened it, so it has the style's rows despite the earlier touch"
    );
    menu.borrow_mut().close();
    pause();

    long_press(&mut app, 100.0, 300.0);
    assert!(menu.borrow().is_open(), "a long press opens the menu");
    assert_eq!(row_heights(&menu.borrow()), vec![vec![TOUCH_MIN; 2]]);
}

#[test]
fn a_submenu_keeps_the_size_of_the_menu_it_opened_from() {
    let _profile = profile_test_lock();
    let _clock = start();
    let (mut app, menu) = host_app();

    // Opened by a finger; the mouse then rests on "More" to open its
    // submenu.  The submenu is part of the finger's menu.
    tap(&mut app, 100.0, 300.0);
    pause();
    let more = menu.borrow().state.layouts(&menu.borrow().items, VIEWPORT)[0].rows[1].rect;
    let (sx, sy) = screen(WINDOW, more.x + 20.0, more.y + more.height / 2.0);
    app.on_mouse_move(sx, sy);
    crate::clock::advance(Duration::from_secs(1));
    app.layout(VIEWPORT);
    app.on_mouse_move(sx, sy);
    assert_eq!(
        row_heights(&menu.borrow()),
        vec![vec![TOUCH_MIN; 2], vec![TOUCH_MIN; 2]],
        "both levels of a touch-opened menu are finger-sized"
    );
    menu.borrow_mut().close();
    pause();

    // Opened by the mouse; a finger then taps "More".  The submenu matches
    // its parent rather than the tap.
    click(&mut app, 100.0, 300.0, MouseButton::Right);
    pause();
    let more = menu.borrow().state.layouts(&menu.borrow().items, VIEWPORT)[0].rows[1].rect;
    tap(&mut app, more.x + 20.0, more.y + more.height / 2.0);
    assert_eq!(
        row_heights(&menu.borrow()),
        vec![vec![ROW_H; 2], vec![ROW_H; 2]],
        "both levels of a mouse-opened menu keep the style's rows"
    );
}

#[test]
fn a_phone_profile_sizes_every_menu_for_fingers() {
    let _profile = profile_test_lock();
    let _clock = start();
    set_input_profile(InputProfile::MobileIOS);
    let (mut app, menu) = host_app();
    click(&mut app, 100.0, 300.0, MouseButton::Right);
    let heights = row_heights(&menu.borrow());
    set_input_profile(InputProfile::Desktop);
    assert_eq!(heights, vec![vec![TOUCH_MIN; 2]]);
}

/// An `App` with a `MenuBar` across the top of the window: "File" (two
/// rows) and "Edit" (one row, so a test can tell which is open).
fn bar_app() -> App {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let bar = MenuBar::new(
        font,
        vec![
            TopMenu::new("File", items()),
            TopMenu::new(
                "Edit",
                vec![MenuEntry::Item(MenuItem::action("Paste", "paste"))],
            ),
        ],
        |_| {},
    );
    let root = Board::new(WINDOW, WINDOW).place(Rect::new(0.0, 370.0, WINDOW, 30.0), Box::new(bar));
    let mut app = App::new(Box::new(root));
    app.layout(VIEWPORT);
    app
}

/// A point on the "File" title (window space, Y up): titles start at the
/// bar's left edge and are at least 52 px wide, and the titles rise from
/// the bottom of the bar's slot (Y up), at least 26 px tall.
const FILE_TITLE: (f64, f64) = (20.0, 383.0);

fn bar(app: &mut App) -> &mut MenuBar {
    app.root_mut().children_mut()[0]
        .as_any_mut()
        .and_then(|any| any.downcast_mut::<MenuBar>())
        .expect("the menu bar")
}

fn bar_popup_rows(app: &mut App) -> Vec<Vec<f64>> {
    let popup = bar(app).popup();
    assert!(popup.is_open(), "the bar's menu is open");
    row_heights(popup)
}

/// Close whatever the bar has open, as Escape does.
fn close_bar_menu(app: &mut App) {
    bar(app).on_event(&Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });
    assert!(!bar(app).popup().is_open());
}

#[test]
fn a_menu_bar_drop_down_opened_by_the_mouse_after_touches_has_normal_rows() {
    let _profile = profile_test_lock();
    let _clock = start();
    let mut app = bar_app();
    let (x, y) = FILE_TITLE;

    tap(&mut app, x, y);
    assert_eq!(bar_popup_rows(&mut app), vec![vec![TOUCH_MIN; 2]]);
    close_bar_menu(&mut app);
    pause();

    click(&mut app, x, y, MouseButton::Left);
    assert_eq!(
        bar_popup_rows(&mut app),
        vec![vec![ROW_H; 2]],
        "the mouse opened it, so it has the style's rows despite the earlier tap"
    );
}

#[test]
fn a_menu_bar_drop_down_reached_by_the_keyboard_has_normal_rows() {
    let _profile = profile_test_lock();
    let _clock = start();
    let mut app = bar_app();
    let (x, y) = FILE_TITLE;

    tap(&mut app, x, y);
    assert_eq!(bar_popup_rows(&mut app), vec![vec![TOUCH_MIN; 2]]);
    // The right arrow moves to "Edit": a menu the keyboard opened.
    bar(&mut app).on_event(&Event::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
    });
    assert_eq!(
        bar_popup_rows(&mut app),
        vec![vec![ROW_H]],
        "Edit's one row, at the style's height"
    );
}
