//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/MenuTests.cs`,
//! plus the `rust_only_*` tests of the named-target lookup it relies on.
//!
//! C#'s `DropDownList` is agg-gui's [`ComboBox`]; its `MenuItem`s are the
//! combo's rows, named with [`ComboBox::set_item_name`] (C#
//! `menuItem.Name = "item1"`) and disabled with `set_item_enabled`. The rows
//! are drawn, not widgets, so the runner finds them as named targets
//! ([`agg_gui::Widget::find_named_targets`]). Each row's `Selected` counter
//! is the combo's `on_change` with that row's index. The colours C# sets
//! (`MenuItemsBackgroundColor`, `HoverColor`, ...) only change paint and are
//! left at the combo's defaults; no assertion reads them.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widgets::menu::{MenuEntry, MenuItem};
use agg_gui::{Color, ComboBox, MenuBar, Rect, Size, TopMenu, Widget};
use agg_gui_automation::tree_query::find_by_name;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationRunner, AutomationWindow, ProbeWidget, RunOptions,
    WaitOpts,
};

/// Whether the combo named "menu1" is open (C# `testList.IsOpen`).
fn is_open(test_runner: &AutomationRunner) -> bool {
    let root = test_runner.driver().root();
    find_by_name(root, "menu1")
        .first()
        .and_then(|h| h.downcast::<ComboBox>(root))
        .is_some_and(ComboBox::is_open)
}

/// C# `WaitForInputToSettle`: returns once the input the runner has already
/// sent, and everything its handlers queued in turn, has run on the UI
/// thread. Two drains, because a popup decides to close one idle pass late.
fn wait_for_input_to_settle(test_runner: &mut AutomationRunner) {
    assert!(test_runner.wait_for_pending_ui_work(5000));
    assert!(test_runner.wait_for_pending_ui_work(5000));
}

/// The three rows' `Selected` counts.
#[derive(Clone, Default)]
struct ClickCounts(Rc<[Cell<u32>; 3]>);

impl ClickCounts {
    fn get(&self, row: usize) -> u32 {
        self.0[row].get()
    }
}

/// C#'s window: a 300 × 200 `SystemWindow` holding the drop-down "menu1"
/// (rows item1, item2 and a disabled item3) at its origin and the 20 × 20
/// "OffMenu" widget at (160, 150).
fn build_menu_window(counts: ClickCounts) -> AutomationWindow {
    let on_change = counts.clone();
    let mut test_list = ComboBox::new(
        vec!["item1", "item2", "item3"],
        0,
        Arc::new(agg_gui::fonts::standard_ui_font()),
    )
    .with_no_selection("no selection")
    .with_item_name(0, "item1")
    .with_item_name(1, "item2")
    .with_item_name(2, "item3")
    .with_item_enabled(2, false)
    .on_change(move |row| {
        if let Some(count) = on_change.0.get(row) {
            count.set(count.get() + 1);
        }
    })
    .with_name("menu1");
    let size = test_list.layout(Size::new(120.0, 200.0));
    test_list.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));

    let mut menu_test_container = AutomationWindow::new(300.0, 200.0);
    menu_test_container.add_child(Box::new(test_list));
    menu_test_container.add_child(Box::new(
        ProbeWidget::new("OffMenu")
            .with_bounds(Rect::new(160.0, 150.0, 20.0, 20.0))
            .with_background(Color::from_rgb8(0, 255, 255)),
    ));
    menu_test_container
}

fn run(body: impl FnOnce(&mut AutomationRunner, &ClickCounts) + Send + 'static) {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let counts = ClickCounts::default();
            (build_menu_window(counts.clone()), counts)
        },
        body,
    )
    .expect("the test body passes");
}

#[test]
fn open_and_close_menus() {
    run(|test_runner, counts| {
        let item1_click_count = || counts.get(0);
        let item2_click_count = || counts.get(1);
        let item3_click_count = || counts.get(2);
        let wait = |test_runner: &mut AutomationRunner, open: bool| {
            test_runner.wait_for(move |r| is_open(r) == open, 5.0, 50);
        };

        assert_eq!(item1_click_count(), 0);
        assert_eq!(item2_click_count(), 0);
        assert_eq!(item3_click_count(), 0);

        test_runner.click_by_name("menu1");
        test_runner.click_by_name("item1");

        wait(test_runner, false);
        assert!(!is_open(test_runner));
        assert_eq!(item1_click_count(), 1);
        assert_eq!(item2_click_count(), 0);
        assert_eq!(item3_click_count(), 0);

        test_runner.click_by_name("menu1");
        test_runner.click_by_name("item2");

        wait(test_runner, false);
        assert!(!is_open(test_runner));
        assert_eq!(item1_click_count(), 1);
        assert_eq!(item2_click_count(), 1);
        assert_eq!(item3_click_count(), 0);

        test_runner.click_by_name("menu1");
        test_runner.click_by_name("item3");

        // The menu was already open, so waiting for "open" would pass before
        // the click was handled.
        wait_for_input_to_settle(test_runner);
        assert!(is_open(test_runner));
        assert_eq!(item1_click_count(), 1);
        assert_eq!(item2_click_count(), 1);
        assert_eq!(item3_click_count(), 0);
        test_runner.click_by_name("item2");

        wait(test_runner, false);
        assert!(!is_open(test_runner));
        assert_eq!(item1_click_count(), 1);
        assert_eq!(item2_click_count(), 2);
        assert_eq!(item3_click_count(), 0);

        test_runner.click_by_name("menu1");
        test_runner.click_by_name("OffMenu");

        wait(test_runner, false);
        assert!(!is_open(test_runner));

        test_runner.click_by_name("menu1");
        wait(test_runner, true);
        assert!(is_open(test_runner));

        // Clicking the disabled item must leave the menu open. "Still open"
        // is only evidence once the click has been fully handled, so wait
        // for that rather than for time.
        test_runner.click_by_name("item3");
        wait_for_input_to_settle(test_runner);
        assert!(is_open(test_runner));
        assert_eq!(item3_click_count(), 0);

        // Hovering off the menu is not a click and must not close it either.
        test_runner.move_to_by_name("OffMenu", &Default::default());
        wait_for_input_to_settle(test_runner);
        assert!(is_open(test_runner));

        test_runner.click_by_name("OffMenu");
        wait(test_runner, false);
        assert!(!is_open(test_runner));
        test_runner.mark_test_complete();
    });
}

#[test]
fn rust_only_closed_drop_down_rows_are_not_found() {
    run(|test_runner, _| {
        // A closed list shows no rows, so there is nothing to find.
        assert!(!test_runner.name_exists("item1", 0.0, true));
        assert!(!test_runner.name_exists("item1", 0.0, false));
        test_runner.click_by_name("menu1");
        assert!(test_runner.wait_for_name("item1", 2.0));
        test_runner.mark_test_complete();
    });
}

#[test]
fn rust_only_open_drop_down_rows_are_named_targets_with_their_state() {
    run(|test_runner, _| {
        test_runner.click_by_name("menu1");
        assert!(test_runner.wait_for_name("item3", 2.0));

        let item3 = test_runner
            .get_object_by_name("item3", &WaitOpts::default())
            .expect("item3 is on show");
        let target = item3.target.clone().expect("a row is a named target");
        assert!(!target.enabled, "item3 is disabled");
        assert_eq!(target.object_as::<usize>(), Some(&2));
        assert!(!item3.target_enabled());
        // The hit aims at the centre of the row's rectangle, in the combo's
        // local space.
        let r = target.rect_local;
        assert_eq!(item3.offset_hint.x, r.x + r.width / 2.0);
        assert_eq!(item3.offset_hint.y, r.y + r.height / 2.0);
        assert_eq!(
            item3.handle.name(test_runner.driver().root()).as_deref(),
            Some("menu1")
        );

        let item1 = test_runner
            .get_object_by_name("item1", &WaitOpts::default())
            .expect("item1 is on show");
        assert!(item1.target_enabled());
        assert_eq!(
            item1.target.as_ref().and_then(|t| t.object_as::<usize>()),
            Some(&0)
        );

        // The region of a row is the row, not the whole combo.
        let row_region = test_runner
            .get_region_by_name("item1", &WaitOpts::default())
            .expect("item1's region");
        let combo_region = test_runner
            .get_region_by_name("menu1", &WaitOpts::default())
            .expect("menu1's region");
        assert_ne!(row_region.screen_rect, combo_region.screen_rect);
        // A search limited to the closed box does not see the rows above it.
        let in_box = WaitOpts {
            secs_to_wait: 0.0,
            search_region: Some(&combo_region),
            only_visible: true,
        };
        assert!(test_runner.get_widgets_by_name("item1", &in_box).is_empty());
        let in_row = WaitOpts {
            secs_to_wait: 0.0,
            search_region: Some(&row_region),
            only_visible: true,
        };
        assert_eq!(test_runner.get_widgets_by_name("item1", &in_row).len(), 1);
        test_runner.mark_test_complete();
    });
}

#[test]
fn rust_only_unnamed_drop_down_rows_take_the_default_menu_item_name() {
    let combo = ComboBox::new(
        vec!["Apple", "Pear"],
        0,
        Arc::new(agg_gui::fonts::standard_ui_font()),
    )
    .with_item_name(1, "fruit2");
    assert_eq!(combo.item_name(0).as_deref(), Some("Apple Menu Item"));
    assert_eq!(combo.item_name(1).as_deref(), Some("fruit2"));
    assert_eq!(combo.item_name(2), None);
}

/// A 300 × 200 window with a menu bar "Edit" (rows Cut, a separator, a
/// disabled Paste) along its top.
fn build_menu_bar_window(actions: Rc<std::cell::RefCell<Vec<String>>>) -> AutomationWindow {
    let font = Arc::new(agg_gui::fonts::standard_ui_font());
    let items = vec![
        MenuEntry::Item(MenuItem::action("Cut", "cut")),
        MenuEntry::Separator,
        MenuEntry::Item(MenuItem::action("Paste", "paste").disabled()),
    ];
    let mut bar = MenuBar::new(font, vec![TopMenu::new("Edit", items)], move |action| {
        actions.borrow_mut().push(action.to_string())
    });
    let size = bar.layout(Size::new(300.0, 200.0));
    bar.set_bounds(Rect::new(0.0, 200.0 - size.height, 300.0, size.height));
    let mut window = AutomationWindow::new(300.0, 200.0);
    window.add_child(Box::new(bar));
    window
}

#[test]
fn rust_only_menu_bar_rows_are_named_targets_and_click_by_name() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let actions = Rc::new(std::cell::RefCell::new(Vec::new()));
            (build_menu_bar_window(Rc::clone(&actions)), actions)
        },
        |test_runner, actions| {
            assert!(!test_runner.name_exists("Cut Menu Item", 0.0, false));
            test_runner.click_by_name("Edit Menu");
            assert!(test_runner.wait_for_name("Cut Menu Item", 2.0));

            let paste = test_runner
                .get_object_by_name("Paste Menu Item", &WaitOpts::default())
                .expect("Paste is on show");
            assert!(!paste.target_enabled(), "Paste is disabled");
            assert_eq!(
                paste
                    .target
                    .as_ref()
                    .and_then(|t| t.object_as::<Vec<usize>>()),
                Some(&vec![2])
            );

            test_runner.click_by_name("Cut Menu Item");
            test_runner.wait_for(|_| !actions.borrow().is_empty(), 2.0, 50);
            assert_eq!(*actions.borrow(), vec!["cut".to_string()]);
            assert!(test_runner.wait_for_widget_disappear("Cut Menu Item", 2.0));
            test_runner.mark_test_complete();
        },
    )
    .expect("the test body passes");
}
