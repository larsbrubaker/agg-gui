//! MenuBar tests for the per-title child widgets (`MenuTitle`), the
//! settable bar height, and the on-open items provider (`top_menu.rs`,
//! `bar.rs`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{find_widget_by_id, hit_test_subtree, Widget};

use super::super::geometry::BAR_H;
use super::super::model::{MenuEntry, MenuItem};
use super::{MenuBar, TopMenu};

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Desktop baseline, as in the sibling MenuBar test files.
fn reset_env() {
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
}

fn file_edit_bar() -> MenuBar {
    MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", vec![MenuItem::action("New", "file.new").into()]),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "edit.copy").into()]),
        ],
        |_| {},
    )
}

fn press(bar: &mut MenuBar, pos: Point) -> EventResult {
    bar.on_event(&Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    })
}

#[test]
fn each_title_is_a_named_child_on_its_rect() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut bar = file_edit_bar();
    bar.layout(Size::new(300.0, BAR_H));

    assert_eq!(bar.children().len(), 2);
    let ids: Vec<_> = bar.children().iter().map(|c| c.id()).collect();
    assert_eq!(ids, [Some("File Menu"), Some("Edit Menu")]);
    for (child, menu) in bar.children().iter().zip(bar.menus()) {
        assert_eq!(child.bounds(), menu.rect);
        assert_eq!(child.type_name(), "MenuTitle");
    }
    assert!(find_widget_by_id(&bar, "Edit Menu").is_some());
}

#[test]
fn title_id_can_be_set() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new("File", Vec::new()).with_id("Sheet File")],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));
    assert_eq!(bar.children()[0].id(), Some("Sheet File"));
}

#[test]
fn title_children_follow_set_menus() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut bar = file_edit_bar();
    bar.layout(Size::new(300.0, BAR_H));
    bar.set_menus(vec![TopMenu::new("View", Vec::new())]);
    bar.layout(Size::new(300.0, BAR_H));
    let ids: Vec<_> = bar.children().iter().map(|c| c.id()).collect();
    assert_eq!(ids, [Some("View Menu")]);
}

/// The children never claim the pointer: a press at a title child's
/// centre hit-tests to the bar itself, which opens that title's menu.
#[test]
fn clicking_a_title_child_centre_opens_its_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    crate::widget::set_current_viewport(Size::new(300.0, 200.0));
    let mut bar = file_edit_bar();
    let size = bar.layout(Size::new(300.0, BAR_H));
    bar.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let edit = bar.children()[1].bounds();
    let centre = Point::new(edit.x + edit.width * 0.5, edit.y + edit.height * 0.5);

    assert_eq!(hit_test_subtree(&bar, centre), Some(vec![]));
    assert_eq!(press(&mut bar, centre), EventResult::Consumed);
    assert_eq!(bar.open_index, Some(1));
    assert!(bar.popup.is_open());
}

#[test]
fn bar_height_defaults_to_bar_h() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut bar = file_edit_bar();
    assert_eq!(bar.bar_height(), BAR_H);
    let size = bar.layout(Size::new(300.0, 100.0));
    assert_eq!(size.height, BAR_H);
}

#[test]
fn bar_height_can_be_compact() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut bar = file_edit_bar().with_bar_height(20.0);
    let size = bar.layout(Size::new(300.0, 100.0));
    assert_eq!(size.height, 20.0);
    for menu in bar.menus() {
        assert_eq!(menu.rect.height, 20.0);
    }
    assert_eq!(bar.children()[0].bounds().height, 20.0);
}

/// On a touch device the compact height is floored at the touch-grown bar
/// so titles stay tappable.
#[test]
fn compact_bar_height_still_grows_on_touch() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::MobileOther);
    let touch_h = super::super::geometry::menu_bar_height();
    let mut bar = file_edit_bar().with_bar_height(20.0);
    let size = bar.layout(Size::new(300.0, 100.0));
    reset_env();
    assert!(touch_h > 20.0);
    assert_eq!(size.height, touch_h);
}

#[test]
fn items_provider_supplies_entries_when_the_menu_opens() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    crate::widget::set_current_viewport(Size::new(300.0, 200.0));
    let calls = Rc::new(Cell::new(0));
    let calls_in = Rc::clone(&calls);
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("File", Vec::new()).with_items_provider(move || {
                calls_in.set(calls_in.get() + 1);
                let label = format!("Open #{}", calls_in.get());
                vec![MenuItem::action(label, "file.open").into()]
            }),
        ],
        |_| {},
    );
    bar.layout(Size::new(300.0, BAR_H));
    assert_eq!(calls.get(), 0, "the provider runs only when the menu opens");

    press(&mut bar, Point::new(8.0, 8.0));
    assert_eq!(calls.get(), 1);
    let MenuEntry::Item(item) = &bar.popup.items[0] else {
        panic!("expected an item");
    };
    assert_eq!(item.label, "Open #1");

    // Close and reopen: the provider runs again with the fresh state.
    press(&mut bar, Point::new(8.0, 8.0));
    assert!(!bar.popup.is_open());
    press(&mut bar, Point::new(8.0, 8.0));
    assert_eq!(calls.get(), 2);
    let MenuEntry::Item(item) = &bar.popup.items[0] else {
        panic!("expected an item");
    };
    assert_eq!(item.label, "Open #2");
}

/// Shortcuts on a closed bar match against the provider's current entries,
/// so an item disabled since the last open never fires.
#[test]
fn items_provider_gates_shortcuts_on_a_closed_bar() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let enabled = Rc::new(Cell::new(false));
    let enabled_in = Rc::clone(&enabled);
    let actions = Rc::new(RefCell::new(Vec::new()));
    let actions_in = Rc::clone(&actions);
    let mut bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "File",
            vec![MenuItem::action("New", "file.new")
                .shortcut("Ctrl+N")
                .into()],
        )
        .with_items_provider(move || {
            let item = MenuItem::action("New", "file.new").shortcut("Ctrl+N");
            let item = if enabled_in.get() {
                item
            } else {
                item.disabled()
            };
            vec![item.into()]
        })],
        move |action| actions_in.borrow_mut().push(action.to_string()),
    );
    let key = Key::Char('n');
    let mods = crate::platform::command_modifiers();

    assert_eq!(bar.on_unconsumed_key(&key, mods), EventResult::Ignored);
    assert!(actions.borrow().is_empty());

    enabled.set(true);
    assert_eq!(bar.on_unconsumed_key(&key, mods), EventResult::Consumed);
    assert_eq!(actions.borrow().as_slice(), ["file.new"]);
}
