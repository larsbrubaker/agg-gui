//! agg-sharp `PopupMenuKeyboardTests` (Tests/Agg.Tests/Agg.UI) through the
//! real `App`: keys go in through `App::on_key_down` / `on_key_up`, as a
//! shell sends them, to a context menu on the `tests_conformance.rs`
//! harness (C# `KeyMenuHarness`) or to a menu opened from a menu bar title
//! (C# `HostedMenu`, a `PopupWidget`-hosted menu; agg-gui's hosted menus are
//! the bar's, as `tests_conformance_2.rs` converts them).
//!
//! C# names the highlighted row (`"Two Menu Item"`); here it is the
//! menu's `hover_path`. C#'s "the window behind saw an unhandled key" is
//! the app's global key handler, which only runs for keys nothing took.
//!
//! The submenu tests of the class (Right, Enter on a submenu row, Left at
//! both depths, Escape over a chain) are pinned on the menu state in
//! `menu/state_keyboard_tests.rs`, which names each.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::event::{Key, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::Rect;
use crate::gfx_ctx::GfxCtx;
use crate::widget::{App, Widget};

use super::super::geometry::{PopupLayout, BAR_H};
use super::super::model::{MenuEntry, MenuItem};
use super::tests_conformance::{reset_env, test_font, Harness, Window, VIEWPORT};
use super::{MenuBar, TopMenu};

fn key_down(app: &mut App, key: Key) {
    app.on_key_down(key, Modifiers::default());
}

/// Rows named `names`, each firing its own name.
fn rows(names: &[&str]) -> Vec<MenuEntry> {
    names
        .iter()
        .map(|name| MenuItem::action(*name, *name).into())
        .collect()
}

/// C#'s twenty `Recent {i}` rows with `MinimumSize = (150, 48)`.
fn recent_rows() -> Vec<MenuEntry> {
    (0..20)
        .map(|i| {
            MenuItem::action(format!("Recent {i}"), format!("{i}"))
                .row_height(48.0)
                .into()
        })
        .collect()
}

fn highlighted(h: &Harness) -> Option<Vec<usize>> {
    h.menu.borrow().state.hover_path.clone()
}

/// Count the keys that reach the app behind the menu unhandled.
fn count_leaks(app: &mut App) -> Rc<Cell<u32>> {
    let leaks = Rc::new(Cell::new(0));
    let seen = Rc::clone(&leaks);
    app.set_global_key_handler(move |_, _| {
        seen.set(seen.get() + 1);
        false
    });
    leaks
}

#[test]
fn down_from_nothing_highlights_the_first_enabled_row() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(rows(&["One", "Two", "Three"]));
    assert_eq!(
        highlighted(&h),
        None,
        "a freshly shown menu highlights nothing"
    );

    key_down(&mut h.app, Key::ArrowDown);

    assert_eq!(highlighted(&h), Some(vec![0]));
}

#[test]
fn up_from_nothing_highlights_the_last_enabled_row() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(rows(&["One", "Two", "Three"]));

    key_down(&mut h.app, Key::ArrowUp);

    assert_eq!(highlighted(&h), Some(vec![2]));
}

#[test]
fn stepping_wraps_at_both_ends() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(rows(&["One", "Two"]));

    key_down(&mut h.app, Key::ArrowDown);
    key_down(&mut h.app, Key::ArrowDown);
    assert_eq!(highlighted(&h), Some(vec![1]));

    key_down(&mut h.app, Key::ArrowDown);
    assert_eq!(
        highlighted(&h),
        Some(vec![0]),
        "Down off the end wraps to the first row"
    );

    key_down(&mut h.app, Key::ArrowUp);
    assert_eq!(
        highlighted(&h),
        Some(vec![1]),
        "Up off the front wraps to the last row"
    );
}

#[test]
fn stepping_skips_disabled_rows_and_separators() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        MenuItem::action("One", "one").into(),
        MenuEntry::Separator,
        MenuItem::action("Disabled", "disabled").disabled().into(),
        MenuItem::action("Three", "three").into(),
    ]);

    key_down(&mut h.app, Key::ArrowDown);
    assert_eq!(highlighted(&h), Some(vec![0]));

    key_down(&mut h.app, Key::ArrowDown);
    assert_eq!(
        highlighted(&h),
        Some(vec![3]),
        "the separator and the disabled row are not stops"
    );

    // and wrapping the other way skips them too
    key_down(&mut h.app, Key::ArrowUp);
    assert_eq!(highlighted(&h), Some(vec![0]));
}

#[test]
fn arrow_keys_are_consumed_so_they_do_not_reach_the_window_behind_the_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(rows(&["One", "Two"]));
    let leaks = count_leaks(&mut h.app);

    key_down(&mut h.app, Key::ArrowDown);
    key_down(&mut h.app, Key::ArrowUp);

    assert_eq!(leaks.get(), 0);
    assert_eq!(highlighted(&h), Some(vec![1]), "the menu took both keys");
}

#[test]
fn stepping_scrolls_the_highlighted_row_into_view_in_a_tall_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(recent_rows());
    // the menu really did get the scrolling treatment, otherwise this
    // proves nothing
    assert!(top_layout(&h.menu.borrow()).scroll.is_some());

    // walk to the very last row
    for _ in 0..20 {
        key_down(&mut h.app, Key::ArrowDown);
    }

    assert_eq!(highlighted(&h), Some(vec![19]));
    let layout = top_layout(&h.menu.borrow());
    let row = row_rect(&layout, 19);
    assert!(
        row.y >= layout.rect.y - 0.001,
        "{row:?} in {:?}",
        layout.rect
    );
    assert!(row.y + row.height <= layout.rect.y + layout.rect.height + 0.001);
}

fn top_layout(menu: &super::PopupMenu) -> PopupLayout {
    menu.state.layouts(&menu.items, VIEWPORT).remove(0)
}

fn row_rect(layout: &PopupLayout, row: usize) -> Rect {
    layout
        .rows
        .iter()
        .find(|r| r.item_index == Some(row))
        .map(|r| r.rect)
        .expect("row is laid out")
}

fn paint_frame(app: &mut App) {
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// C# `HostedMenu`: a menu bar along the window's top whose one menu,
/// `Menu`, holds `items`, opened by a click on its title.
struct Hosted {
    app: App,
    actions: Rc<RefCell<Vec<String>>>,
}

impl Hosted {
    fn show(items: Vec<MenuEntry>) -> Self {
        let actions = Rc::new(RefCell::new(Vec::new()));
        let fired = Rc::clone(&actions);
        let bar = MenuBar::new(
            test_font(),
            vec![TopMenu::new("Menu", items)],
            move |action| fired.borrow_mut().push(action.to_string()),
        );
        let children: Vec<Box<dyn Widget>> = vec![Box::new(bar)];
        let mut app = App::new(Box::new(Window {
            bounds: Rect::default(),
            children,
            rects: vec![Rect::new(
                0.0,
                VIEWPORT.height - BAR_H,
                VIEWPORT.width,
                BAR_H,
            )],
        }));
        app.layout(VIEWPORT);
        // A shell paints a frame before the user can press and after every
        // event; the bar learns where it sits in the window from its
        // overlay paint, which its menu needs to clamp and scroll.
        paint_frame(&mut app);
        let (x, y) = (8.0, BAR_H * 0.5);
        app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
        paint_frame(&mut app);
        app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
        paint_frame(&mut app);
        let hosted = Self { app, actions };
        assert!(hosted.is_open(), "the title click opened the menu");
        assert!(hosted.actions.borrow().is_empty(), "and chose nothing");
        hosted
    }

    fn with_bar<R>(&self, f: impl FnOnce(&MenuBar) -> R) -> R {
        let bar = self.app.root().children()[0]
            .as_any()
            .and_then(|any| any.downcast_ref::<MenuBar>())
            .expect("the window's child is the bar");
        f(bar)
    }

    fn is_open(&self) -> bool {
        self.with_bar(|bar| bar.popup.is_open())
    }

    fn highlighted(&self) -> Option<Vec<usize>> {
        self.with_bar(|bar| bar.popup.state.hover_path.clone())
    }

    fn layout(&self) -> PopupLayout {
        self.with_bar(|bar| top_layout(&bar.popup))
    }
}

/// CONVERTED: C# clamps the hosted popup to `maxHeight: 200`, about four
/// 48 px rows, and steps 8 rows to land mid-list. agg-gui's bar menu is
/// bounded by the window instead (about seven rows here), so this steps 10
/// to be just as far past the fold and still clear of the far end.
const MID_LIST_STEPS: usize = 10;

#[test]
fn stepping_a_popup_widget_hosted_menu_leaves_an_already_visible_row_where_it_is() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut hosted = Hosted::show(recent_rows());
    let layout = hosted.layout();
    let scroll = layout.scroll.expect("the clamp really did engage");
    assert!(layout.rect.height < scroll.content_height);
    let scroll_at_top = scroll.offset;

    // Step into the middle of the list first. At either end the scroll
    // position is pinned by its own limits, so a re-centering bug hides
    // there - only an unpinned list can show it.
    for _ in 0..MID_LIST_STEPS {
        key_down(&mut hosted.app, Key::ArrowDown);
    }
    let in_the_middle = hosted.layout().scroll.expect("scrolls").offset;
    assert_ne!(
        in_the_middle, scroll_at_top,
        "the list is genuinely scrolled off its starting position, not pinned there"
    );

    // the row above the highlight is on screen already, so backing onto it
    // asks for no scroll at all
    key_down(&mut hosted.app, Key::ArrowUp);

    assert_eq!(hosted.highlighted(), Some(vec![MID_LIST_STEPS - 2]));
    let after = hosted.layout().scroll.expect("scrolls").offset;
    assert!(
        (after - in_the_middle).abs() <= 0.001,
        "that row was already fully visible, so nothing needed to move"
    );
}

#[test]
fn stepping_to_an_off_screen_row_in_a_hosted_menu_scrolls_the_minimum() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut hosted = Hosted::show(recent_rows());

    // well past the fold
    for _ in 0..MID_LIST_STEPS {
        key_down(&mut hosted.app, Key::ArrowDown);
    }
    let row_index = MID_LIST_STEPS - 1;
    assert_eq!(hosted.highlighted(), Some(vec![row_index]));

    // Stepping downward off the bottom edge leaves the row against that
    // edge. Centering it would leave most of a viewport of rows below it,
    // which is what this rules out.
    let layout = hosted.layout();
    let row = row_rect(&layout, row_index);
    assert!(
        (row.y - layout.rect.y).abs() <= 1.0,
        "row {row:?} sits on the panel's bottom edge {:?}",
        layout.rect
    );
}

#[test]
fn escape_closes_an_active_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(rows(&["One", "Two"]));
    let leaks = count_leaks(&mut h.app);
    assert!(h.is_open());

    key_down(&mut h.app, Key::Escape);

    assert!(!h.is_open());
    // MatterCAD's root window cancels the current scene operation on an
    // unhandled Escape - a leak here would cancel a drag behind the menu
    assert_eq!(leaks.get(), 0, "Escape is handled");
}

#[test]
fn escape_closes_a_popup_widget_hosted_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut hosted = Hosted::show(rows(&["One", "Two"]));
    let leaks = count_leaks(&mut hosted.app);

    key_down(&mut hosted.app, Key::Escape);

    assert!(!hosted.is_open());
    assert_eq!(leaks.get(), 0, "Escape is handled");
}

#[test]
fn arrow_keys_step_a_popup_widget_hosted_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut hosted = Hosted::show(rows(&["One", "Two"]));

    key_down(&mut hosted.app, Key::ArrowDown);
    assert_eq!(hosted.highlighted(), Some(vec![0]));

    key_down(&mut hosted.app, Key::ArrowDown);
    assert_eq!(
        hosted.highlighted(),
        Some(vec![1]),
        "the key must not be delivered twice"
    );
}

#[test]
fn stepping_then_activating_runs_the_highlighted_row() {
    for activation in [Key::Enter, Key::Char(' ')] {
        let _guard = crate::input_profile::profile_test_lock();
        reset_env();
        let mut h = Harness::show(rows(&["Open", "Close"]));

        key_down(&mut h.app, Key::ArrowDown);
        key_down(&mut h.app, Key::ArrowDown);
        assert_eq!(highlighted(&h), Some(vec![1]));

        key_down(&mut h.app, activation.clone());
        h.app.on_key_up(activation.clone(), Modifiers::default());

        assert_eq!(
            *h.actions.borrow(),
            vec!["Close".to_string()],
            "{activation:?}"
        );
        assert!(
            !h.is_open(),
            "{activation:?}: choosing a row takes the menu down, just as clicking it does"
        );
    }
}

#[test]
fn activating_with_nothing_highlighted_does_nothing() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(rows(&["Open", "Close"]));

    key_down(&mut h.app, Key::Enter);
    h.app.on_key_up(Key::Enter, Modifiers::default());

    assert!(h.actions.borrow().is_empty());
    assert!(h.is_open());
}
