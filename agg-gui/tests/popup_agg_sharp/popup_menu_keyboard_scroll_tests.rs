//! The scrolling tests of agg-sharp `PopupMenuKeyboardTests`
//! (Tests/Agg.Tests/Agg.UI/PopupMenuKeyboardTests.cs): stepping the highlight
//! through an overflowing menu keeps the highlighted row in view with the
//! least movement. The other keyboard tests are ported in
//! `widgets/menu/state_keyboard_tests.rs`.
//!
//! C#'s `KeyMenuHarness` shows a context menu in a 400 x 300 window; here that
//! is [`PopupMenu::open_at`]. Its `HostedMenu` is a `PopupWidget` with a
//! 200 px `maxHeight`; agg-gui menus have no height cap of their own, so the
//! hosted menu opens from a bar title with exactly 200 px of room below it
//! (and less above), which gives the same 200 px scrolling panel. Items are
//! 48 px rows.

use agg_gui::widgets::menu::geometry::PopupLayout;
use agg_gui::widgets::menu::{MenuAnchorKind, MenuEntry, MenuItem, PopupMenu};
use agg_gui::{Event, Key, Point, Size};

const WINDOW: Size = Size {
    width: 400.0,
    height: 300.0,
};

fn recent_menu() -> PopupMenu {
    PopupMenu::new(
        (0..20)
            .map(|i| {
                MenuEntry::Item(
                    MenuItem::action(format!("Recent {i}"), format!("{i}")).row_height(48.0),
                )
            })
            .collect(),
    )
}

fn key_down(menu: &mut PopupMenu, key: Key) {
    menu.handle_event(
        &Event::KeyDown {
            key,
            modifiers: Default::default(),
        },
        WINDOW,
    );
}

fn panel(menu: &PopupMenu) -> PopupLayout {
    menu.state.layouts(&menu.items, WINDOW).remove(0)
}

fn highlighted(menu: &PopupMenu) -> usize {
    menu.state.hover_path.as_ref().expect("a highlighted row")[0]
}

/// A menu capped at 200 px: a bar title with 200 px of room under it.
fn hosted_menu() -> PopupMenu {
    let mut menu = recent_menu();
    // 4 px window margin + 200 px of room below the anchor; 72 above it.
    menu.state
        .open_at(Point::new(10.0, 204.0), MenuAnchorKind::Bar);
    menu.state.set_anchor_span(20.0);
    menu.state.set_space_origin(Some(Point::ORIGIN));
    menu
}

#[test]
fn stepping_scrolls_the_highlighted_row_into_view_in_a_tall_menu() {
    let mut menu = recent_menu();
    menu.open_at(Point::new(10.0, 290.0));
    // The menu really did get the scrolling treatment, otherwise this proves nothing
    assert!(panel(&menu).scroll.is_some());

    // Walk to the very last row
    for _ in 0..20 {
        key_down(&mut menu, Key::ArrowDown);
    }
    assert_eq!(highlighted(&menu), 19);

    let layout = panel(&menu);
    let row = layout.rows[19].rect;
    assert!(row.bottom() >= layout.rect.bottom() - 0.001);
    assert!(row.top() <= layout.rect.top() + 0.001);
}

#[test]
fn stepping_a_popup_widget_hosted_menu_leaves_an_already_visible_row_where_it_is() {
    let mut menu = hosted_menu();
    let layout = panel(&menu);
    // The clamp really did engage, otherwise there is no scrolling to be wrong about
    let scroll = layout.scroll.expect("the hosted menu scrolls");
    assert!((layout.rect.height - 200.0).abs() < 0.001);
    assert!(layout.rect.height < scroll.content_height);
    let scroll_at_top = scroll.offset;

    // Step into the middle of the list first, where the offset is not pinned
    for _ in 0..8 {
        key_down(&mut menu, Key::ArrowDown);
    }
    let scrolled_into_the_middle = panel(&menu).scroll.expect("scrolls").offset;
    assert_ne!(scrolled_into_the_middle, scroll_at_top);

    // The row above the highlight is on screen already, so backing onto it asks for no scroll
    key_down(&mut menu, Key::ArrowUp);
    assert_eq!(highlighted(&menu), 6);
    let offset = panel(&menu).scroll.expect("scrolls").offset;
    assert!((offset - scrolled_into_the_middle).abs() < 0.001);
}

#[test]
fn stepping_to_an_off_screen_row_in_a_hosted_menu_scrolls_the_minimum() {
    let mut menu = hosted_menu();
    // Eight rows into a viewport that holds roughly four is well past the fold
    for _ in 0..8 {
        key_down(&mut menu, Key::ArrowDown);
    }
    assert_eq!(highlighted(&menu), 7);
    let layout = panel(&menu);
    let row = layout.rows[7].rect;
    // Stepping downward off the bottom edge leaves the row against that edge
    assert!((row.bottom() - layout.rect.bottom()).abs() <= 1.0);
}
