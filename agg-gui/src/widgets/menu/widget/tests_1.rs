//! MenuBar interaction tests — desktop drag/release and mobile backdrop dismiss.

use std::sync::Arc;

use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Size};
use crate::text::Font;
use crate::widget::Widget;

use super::super::geometry::BAR_H;
use super::super::model::MenuItem;
use super::{MenuBar, TopMenu};

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Desktop drag-and-release in neutral space cancels the popup —
/// the user opened a menu, dragged off the menu bar / popup body,
/// and released somewhere unrelated.  Without this, dragging out
/// of a menu would leave it open with no obvious way to close it
/// from the same gesture.
#[test]
fn desktop_drag_release_in_neutral_space_closes_popup() {
    crate::touch_state::clear_last_touch_event_for_testing();
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new").into()],
        )],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    // Press File — popup opens, drag-release armed.
    bar.on_event(&Event::MouseDown {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(bar.popup.is_open());

    // Drag through neutral space (off the bar, off the popup
    // body) and release there — popup must close.
    let neutral = Point::new(280.0, 170.0);
    bar.on_event(&Event::MouseMove { pos: neutral });
    bar.on_event(&Event::MouseUp {
        pos: neutral,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        !bar.popup.is_open(),
        "drag-release in neutral space must close the popup",
    );
}

/// Mobile backdrop dismiss: with a popup open, tapping outside the
/// menu bar AND outside the popup body closes it.  The touch shell
/// fires MouseMove + MouseDown + MouseUp at the tap position, all
/// within the touch-synthesis window.  The MouseDown lands outside
/// any top-menu rect so the bar's "tap on top menu" path doesn't
/// run; popup.handle_event sees an outside-click and closes.
#[test]
fn mobile_backdrop_tap_dismisses_popup() {
    crate::touch_state::clear_last_touch_event_for_testing();
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new").into()],
        )],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    // Open File via a tap.
    let file_pos = Point::new(8.0, 8.0);
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseMove { pos: file_pos });
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseDown {
        pos: file_pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: file_pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(bar.popup.is_open());

    // Tap outside both the menu bar and the popup body.
    let backdrop = Point::new(280.0, 170.0);
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseMove { pos: backdrop });
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseDown {
        pos: backdrop,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: backdrop,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        !bar.popup.is_open(),
        "tapping outside the menu bar and popup body must dismiss the popup on mobile",
    );
}

/// Desktop hover-switch: with a popup open, moving the cursor over
/// a different top menu's bar (button NOT held — i.e. AFTER the
/// click that opened the first menu has already released) opens
/// that other menu.  Standard desktop menubar behaviour.
#[test]
fn hover_after_release_switches_open_top_menu_on_desktop() {
    crate::touch_state::clear_last_touch_event_for_testing();
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![MenuItem::action("New", "file.new").into()]),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "edit.copy").into()]),
        ],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    // Click File and release.
    bar.on_event(&Event::MouseDown {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(bar.open_index, Some(0));

    // Hover over Edit (button NOT held) — should switch open menu.
    bar.on_event(&Event::MouseMove {
        pos: Point::new(60.0, 8.0),
    });
    assert_eq!(
        bar.open_index,
        Some(1),
        "moving the cursor over a different top menu after release \
         must switch the open popup (desktop hover-switch)"
    );
}

/// Desktop drag-and-release on a sibling top menu's bar: the
/// popup switches to the new menu and stays open after release.
/// Spec row 3.
#[test]
fn desktop_drag_and_release_on_sibling_keeps_new_menu_open() {
    crate::touch_state::clear_last_touch_event_for_testing();
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![MenuItem::action("New", "file.new").into()]),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "edit.copy").into()]),
        ],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    bar.on_event(&Event::MouseDown {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseMove {
        pos: Point::new(60.0, 8.0),
    });
    assert_eq!(bar.open_index, Some(1), "drag-switch must reach Edit");
    bar.on_event(&Event::MouseUp {
        pos: Point::new(60.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        bar.popup.is_open(),
        "release on sibling top-menu bar must keep its popup open"
    );
    assert_eq!(bar.open_index, Some(1));
}

/// Desktop drag-switch to sibling, drag off, release in neutral
/// space: closes.  Spec row 4.
#[test]
fn desktop_drag_switch_then_release_off_closes() {
    crate::touch_state::clear_last_touch_event_for_testing();
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![MenuItem::action("New", "file.new").into()]),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "edit.copy").into()]),
        ],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    bar.on_event(&Event::MouseDown {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseMove {
        pos: Point::new(60.0, 8.0),
    });
    // Now drag off the bar AND off the popup body.
    bar.on_event(&Event::MouseMove {
        pos: Point::new(280.0, 170.0),
    });
    bar.on_event(&Event::MouseUp {
        pos: Point::new(280.0, 170.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(!bar.popup.is_open());
}

/// Desktop press-press-press without intervening releases: A opens,
/// B opens (switch), neutral closes.  Spec row 5.
#[test]
fn desktop_press_press_press_neutral_closes_active_menu() {
    crate::touch_state::clear_last_touch_event_for_testing();
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![MenuItem::action("New", "file.new").into()]),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "edit.copy").into()]),
        ],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    bar.on_event(&Event::MouseDown {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseDown {
        pos: Point::new(60.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(bar.open_index, Some(1));
    bar.on_event(&Event::MouseDown {
        pos: Point::new(280.0, 170.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(!bar.popup.is_open());
}

/// Mobile: tap currently-open top menu again toggles closed.
/// Spec row 2 of Mobile.
#[test]
fn mobile_tap_currently_open_top_menu_closes() {
    crate::touch_state::clear_last_touch_event_for_testing();
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new").into()],
        )],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    let file_pos = Point::new(8.0, 8.0);
    // First tap: open.
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseMove { pos: file_pos });
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseDown {
        pos: file_pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: file_pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(bar.popup.is_open());

    // Second tap on the same top-menu bar: toggle close.
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseMove { pos: file_pos });
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseDown {
        pos: file_pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: file_pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(!bar.popup.is_open());
}

/// Click-to-close-toggle: after closing the menu by clicking its
/// own bar item, the bar item must NOT keep painting the hover
/// highlight (cursor is still over it but the user just dismissed
/// the popup, so the item reading as "still selected" is wrong).
/// Hover suppression clears once the cursor moves to a different
/// item (or off the bar).
#[test]
fn click_close_suppresses_hover_until_cursor_leaves() {
    crate::touch_state::clear_last_touch_event_for_testing();
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![MenuItem::action("New", "file.new").into()]),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "edit.copy").into()]),
        ],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));
    // Open File then click File again to close.  Cursor stayed
    // over File the whole time.
    bar.on_event(&Event::MouseDown {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseDown {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(!bar.popup.is_open());
    assert_eq!(
        bar.suppress_hover_for,
        Some(0),
        "click-to-close must suppress hover on the just-closed bar item",
    );

    // Move the cursor over Edit — suppression clears for File and
    // Edit gets normal hover.
    bar.on_event(&Event::MouseMove {
        pos: Point::new(60.0, 8.0),
    });
    assert_eq!(bar.suppress_hover_for, None);
    assert_eq!(bar.hover_index, Some(1));
}

/// ESC closes the menu (universal dismiss).  Already covered by
/// the popup-state-level outside-click test, but the bar-level
/// path is asserted here so a future event-routing change can't
/// silently break it.
#[test]
fn escape_closes_active_menu() {
    crate::touch_state::clear_last_touch_event_for_testing();
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new").into()],
        )],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));
    bar.on_event(&Event::MouseDown {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(bar.popup.is_open());

    bar.on_event(&Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });
    assert!(!bar.popup.is_open(), "ESC must close the active menu");
}
