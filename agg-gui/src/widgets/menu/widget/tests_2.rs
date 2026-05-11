//! MenuBar interaction tests — hover, click, drag-release, shortcuts, arrows.

use std::cell::RefCell;
use std::rc::Rc;
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

#[test]
fn moving_across_top_menus_switches_open_popup() {
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![MenuItem::action("New", "file.new").into()]),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "edit.copy").into()]),
        ],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    assert_eq!(
        bar.on_event(&Event::MouseDown {
            pos: Point::new(8.0, 8.0),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );
    assert_eq!(bar.open_index, Some(0));

    assert_eq!(
        bar.on_event(&Event::MouseMove {
            pos: Point::new(60.0, 8.0),
        }),
        EventResult::Consumed
    );
    assert_eq!(bar.open_index, Some(1));
    let Some(super::super::model::MenuEntry::Item(item)) = bar.popup.items.first() else {
        panic!("popup should contain Edit items");
    };
    assert_eq!(item.action.as_deref(), Some("edit.copy"));
}

#[test]
fn top_level_menu_tracks_hover() {
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new").into()],
        )],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    assert_eq!(
        bar.on_event(&Event::MouseMove {
            pos: Point::new(8.0, 8.0),
        }),
        EventResult::Ignored
    );
    assert_eq!(bar.hover_index, Some(0));
}

#[test]
fn hover_change_advances_invalidation_epoch() {
    // Regression: the hover panel paints inside the parent Window's
    // retained backbuffer.  When `set_hover_index` mutates `hover_index`
    // it must bump the invalidation epoch so `dispatch_event` marks the
    // ancestor cache dirty — otherwise the next frame composites a
    // stale bitmap and hover never appears.  The bug previously
    // appeared as: hover only briefly visible when something else
    // (e.g. a resize-edge highlight) happened to dirty the Window.
    crate::touch_state::clear_last_touch_event_for_testing();
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new").into()],
        )],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));

    // Hover off → onto the bar; epoch must advance so retained ancestors
    // re-rasterise.
    let before = crate::animation::invalidation_epoch();
    bar.on_event(&Event::MouseMove {
        pos: Point::new(8.0, 8.0),
    });
    assert!(
        crate::animation::invalidation_epoch() != before,
        "MenuBar hover change must advance the invalidation epoch so the \
         parent Window's backbuffer cache invalidates"
    );

    // No-op move (still over same menu) shouldn't advance the epoch
    // — the cache is already correct for this hover state.
    let before = crate::animation::invalidation_epoch();
    bar.on_event(&Event::MouseMove {
        pos: Point::new(10.0, 8.0),
    });
    assert_eq!(
        crate::animation::invalidation_epoch(),
        before,
        "MouseMove that doesn't change hover_index should not advance \
         the epoch"
    );
}

#[test]
fn mouse_down_drag_release_activates_popup_item() {
    let viewport = Size::new(300.0, 180.0);
    crate::widget::set_current_viewport(viewport);
    let actions = Rc::new(RefCell::new(Vec::new()));
    let actions_for_cb = Rc::clone(&actions);
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new").into()],
        )],
        move |action| actions_for_cb.borrow_mut().push(action.to_string()),
    );
    bar.layout(Size::new(300.0, BAR_H));

    assert_eq!(
        bar.on_event(&Event::MouseDown {
            pos: Point::new(8.0, 8.0),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );
    let row = bar.popup.state.layouts(&bar.popup.items, viewport)[0].rows[0].rect;
    let item_pos = Point::new(row.x + 12.0, row.y + 12.0);

    assert_eq!(
        bar.on_event(&Event::MouseMove { pos: item_pos }),
        EventResult::Consumed
    );
    assert_eq!(
        bar.on_event(&Event::MouseUp {
            pos: item_pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );

    assert_eq!(actions.borrow().as_slice(), ["file.new"]);
    assert!(!bar.popup.is_open());
}

#[test]
fn simple_mouse_click_opens_menu_without_release_activation() {
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

    assert_eq!(
        bar.on_event(&Event::MouseDown {
            pos: Point::new(8.0, 8.0),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );
    assert_eq!(
        bar.on_event(&Event::MouseUp {
            pos: Point::new(8.0, 8.0),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );

    assert!(bar.popup.is_open());
    assert_eq!(bar.open_index, Some(0));
}

/// Clicking the currently-open top menu's bar item closes the popup
/// (toggle).  Standard desktop menubar convention; on mobile it's
/// also the natural way to dismiss a popup without a row tap.
#[test]
fn click_on_currently_open_top_menu_closes_popup() {
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

    // Open via a click on File.
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

    // Click File again — should close (toggle).
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
    assert!(
        !bar.popup.is_open(),
        "click on the currently-open top menu must close it (desktop toggle)"
    );
}

/// Mobile-tap path with the touch shell's synthetic MouseMove preamble:
/// `touchstart` fires `on_mouse_move(pos)` followed by `on_touch_start`,
/// then `touchend` fires `on_mouse_down + on_mouse_up`.  So a tap is
/// actually MouseMove → MouseDown → MouseUp at the same position.
/// When one menu is open, that MouseMove already opens the tapped
/// top menu (the existing hover-driven path); the FOLLOWING MouseDown
/// must NOT close it (the click on the bar would otherwise be seen as
/// outside the popup body and close the popup).
#[test]
fn mobile_tap_sequence_keeps_other_top_menu_open() {
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

    // Tap File — the touch shell calls `note_touch_event` from each
    // touch lifecycle entry point, then synthesises MouseMove(at
    // tap pos) → MouseDown → MouseUp.  Replicate that ordering by
    // marking the touch event before each synthesised mouse event.
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
    assert_eq!(bar.open_index, Some(0));

    // Tap Edit — same touch lifecycle then MouseMove + MouseDown +
    // MouseUp.  The MouseMove must NOT switch the open menu (it's
    // synthesised from touchstart, not a desktop hover); the
    // MouseDown carries the open intent.
    let edit_pos = Point::new(60.0, 8.0);
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseMove { pos: edit_pos });
    assert_eq!(
        bar.open_index,
        Some(0),
        "synthesised pre-tap MouseMove must not switch the open menu — \
         only the subsequent MouseDown should",
    );
    crate::touch_state::note_touch_event();
    bar.on_event(&Event::MouseDown {
        pos: edit_pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    bar.on_event(&Event::MouseUp {
        pos: edit_pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        bar.popup.is_open(),
        "Edit must stay open after the tap completes",
    );
    assert_eq!(bar.open_index, Some(1));
}

/// Mobile-tap path: a tap on the menu bar fires `MouseDown` + `MouseUp`
/// with NO `MouseMove` between them.  When one top menu is already open
/// and the user taps a different top menu, the bar must close the
/// current popup AND open the tapped one in the same event.  Without
/// this, the popup-handler (which sees the MouseDown as an outside
/// click) just closes the open menu, and the tapped menu never opens —
/// the user has to tap twice to switch menus on mobile.
#[test]
fn tap_on_other_top_menu_switches_open_popup() {
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

    // Tap (mouse-down + mouse-up, no move) on File.
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
    assert_eq!(bar.open_index, Some(0));

    // Tap on Edit — no MouseMove in between.  Should switch.
    let edit_pos = Point::new(60.0, 8.0);
    assert_eq!(
        bar.on_event(&Event::MouseDown {
            pos: edit_pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed,
    );
    assert!(
        bar.popup.is_open(),
        "tapping a different top menu must keep a popup open (Edit's), not just close File's"
    );
    assert_eq!(bar.open_index, Some(1));
    let Some(super::super::model::MenuEntry::Item(item)) = bar.popup.items.first() else {
        panic!("popup should contain Edit items after the tap");
    };
    assert_eq!(item.action.as_deref(), Some("edit.copy"));
}

#[test]
fn unconsumed_shortcut_fires_top_menu_action() {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let actions_for_cb = Rc::clone(&actions);
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new")
                .shortcut("Ctrl+N")
                .into()],
        )],
        move |action| actions_for_cb.borrow_mut().push(action.to_string()),
    );

    assert_eq!(
        bar.on_unconsumed_key(
            &Key::Char('n'),
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        ),
        EventResult::Consumed
    );

    assert_eq!(actions.borrow().as_slice(), ["file.new"]);
}

#[test]
fn arrow_keys_switch_open_top_menus() {
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![MenuItem::action("New", "file.new").into()]),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "edit.copy").into()]),
        ],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));
    bar.open_menu(0);

    assert_eq!(
        bar.on_event(&Event::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );
    assert_eq!(bar.open_index, Some(1));

    assert_eq!(
        bar.on_event(&Event::KeyDown {
            key: Key::ArrowLeft,
            modifiers: Modifiers::default(),
        }),
        EventResult::Consumed
    );
    assert_eq!(bar.open_index, Some(0));
}
