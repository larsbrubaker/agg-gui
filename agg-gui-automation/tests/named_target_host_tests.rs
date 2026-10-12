//! Rust-only tests of named targets reported by widgets that host menus:
//! a menu item's own automation name (agg-sharp `MenuItemModel.AutomationName`)
//! in place of the `"{Text} Menu Item"` convention, a zero-size host (a
//! context menu anchored at a point) whose rows still count as on show,
//! `wait_for_widget_enabled` waiting on a target's own enabled flag, and
//! agg-gui's own context-menu hosts (a text field) reporting their open rows.

mod support;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::layout_props::WidgetBase;
use agg_gui::widget::current_viewport;
use agg_gui::widgets::menu::{menu_row_name, MenuEntry, MenuItem};
use agg_gui::{
    clock, ui_thread, MenuBar, NamedTarget, Point, PopupMenu, Rect, Size, TextField, TopMenu,
    Widget,
};
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationError, AutomationRunner, AutomationWindow, RunOptions,
    WaitOpts,
};
use support::{set, switch, Switch};

/// A widget at fixed bounds (possibly zero-size) that reports named targets:
/// the rows of an open [`PopupMenu`] it hosts, and one plain target
/// `"Plain Target"` whose enabled flag reads a shared switch.
struct TargetHost {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    menu: Option<PopupMenu>,
    plain_enabled: Switch,
}

impl TargetHost {
    fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            base: WidgetBase::new(),
            children: Vec::new(),
            menu: None,
            plain_enabled: switch(true),
        }
    }

    /// Host an open menu of `items` anchored at `at` (host-local).
    fn with_open_menu(mut self, items: Vec<MenuEntry>, at: Point) -> Self {
        let mut menu = PopupMenu::new(items);
        menu.open_at(at);
        self.menu = Some(menu);
        self
    }

    fn plain_enabled_when(mut self, enabled: &Switch) -> Self {
        self.plain_enabled = Arc::clone(enabled);
        self
    }
}

impl Widget for TargetHost {
    fn type_name(&self) -> &'static str {
        "TargetHost"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn find_named_targets(&self, name: &str, out: &mut Vec<NamedTarget>) {
        if let Some(menu) = &self.menu {
            menu.named_row_targets(name, current_viewport(), out);
        }
        if name == "Plain Target" {
            out.push(
                NamedTarget::new(Rect::new(10.0, 10.0, 40.0, 20.0))
                    .with_enabled(self.plain_enabled.load(Ordering::SeqCst)),
            );
        }
    }
}

/// Run `body` against a 300 × 200 window holding `children`.
fn run_with<S: 'static, R: Send + 'static>(
    build: impl FnOnce() -> (Vec<Box<dyn Widget>>, S) + Send + 'static,
    body: impl FnOnce(&mut AutomationRunner, &S) -> R + Send + 'static,
) -> Result<R, AutomationError> {
    show_window_and_execute_tests(
        RunOptions::default(),
        move || {
            let (children, state) = build();
            let mut window = AutomationWindow::new(300.0, 200.0);
            for child in children {
                window.add_child(child);
            }
            (window, state)
        },
        move |runner, state| {
            let result = body(runner, state);
            runner.mark_test_complete();
            result
        },
    )
}

#[test]
fn rust_only_menu_row_name_prefers_the_items_automation_name() {
    let plain = MenuItem::action("Save", "save");
    assert_eq!(menu_row_name(&plain), "Save Menu Item");
    let named = MenuItem::action("Save", "save").with_automation_name("Custom Save Name");
    assert_eq!(named.automation_name.as_deref(), Some("Custom Save Name"));
    assert_eq!(menu_row_name(&named), "Custom Save Name");
}

#[test]
fn rust_only_a_menu_bar_row_is_found_and_clicked_by_its_automation_name() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let actions = Rc::new(RefCell::new(Vec::<String>::new()));
            let sink = Rc::clone(&actions);
            let font = Arc::new(agg_gui::fonts::standard_ui_font());
            let items = vec![
                MenuEntry::Item(MenuItem::action("Open", "open")),
                MenuEntry::Item(
                    MenuItem::action("Save", "save").with_automation_name("Custom Save Name"),
                ),
            ];
            let mut bar = MenuBar::new(font, vec![TopMenu::new("File", items)], move |a| {
                sink.borrow_mut().push(a.to_string())
            });
            let size = bar.layout(Size::new(300.0, 200.0));
            bar.set_bounds(Rect::new(0.0, 200.0 - size.height, 300.0, size.height));
            let mut window = AutomationWindow::new(300.0, 200.0);
            window.add_child(Box::new(bar));
            (window, actions)
        },
        |runner, actions| {
            runner.click_by_name("File Menu");
            assert!(runner.wait_for_name("Custom Save Name", 2.0));
            // The convention name gives way to the automation name; the
            // unnamed row keeps the convention.
            assert!(!runner.name_exists("Save Menu Item", 0.0, false));
            assert!(runner.name_exists("Open Menu Item", 0.0, true));

            runner.click_by_name("Custom Save Name");
            runner.wait_for(|_| !actions.borrow().is_empty(), 2.0, 50);
            assert_eq!(*actions.borrow(), vec!["save".to_string()]);
            runner.mark_test_complete();
        },
    )
    .expect("the test body passes");
}

#[test]
fn rust_only_a_zero_size_hosts_menu_rows_are_on_show() {
    let found = run_with(
        || {
            let items = vec![
                MenuEntry::Item(MenuItem::action("Cut", "cut")),
                MenuEntry::Item(MenuItem::action("Paste", "paste").disabled()),
            ];
            // A context menu's host: nothing of it shows, its menu does.
            let host = TargetHost::new(Rect::new(100.0, 150.0, 0.0, 0.0))
                .with_open_menu(items, Point::new(0.0, 0.0));
            (vec![Box::new(host) as Box<dyn Widget>], ())
        },
        |runner, _| {
            let shown = runner.wait_for_name("Cut Menu Item", 2.0);
            let paste = runner
                .get_object_by_name("Paste Menu Item", &WaitOpts::secs(0.0))
                .map(|hit| hit.target_enabled());
            (shown, paste)
        },
    )
    .expect("the test body passes");

    assert_eq!(found, (true, Some(false)));
}

#[test]
fn rust_only_a_target_under_a_hidden_widget_is_not_on_show() {
    let found = run_with(
        || {
            let visible = switch(false);
            let parent = support::Switchable::new("parent", Rect::new(0.0, 0.0, 200.0, 200.0))
                .visible_when(&visible)
                .with_child(Box::new(TargetHost::new(Rect::new(0.0, 0.0, 0.0, 0.0))));
            (vec![Box::new(parent) as Box<dyn Widget>], ())
        },
        |runner, _| {
            (
                runner.name_exists("Plain Target", 0.0, true),
                runner.name_exists("Plain Target", 0.0, false),
            )
        },
    )
    .expect("the test body passes");

    assert_eq!(found, (false, true));
}

#[test]
fn rust_only_wait_for_widget_enabled_waits_for_a_targets_own_flag() {
    let waited = run_with(
        || {
            let enabled = switch(false);
            let host =
                TargetHost::new(Rect::new(20.0, 20.0, 100.0, 100.0)).plain_enabled_when(&enabled);
            (vec![Box::new(host) as Box<dyn Widget>], enabled)
        },
        |runner, enabled| {
            let enabled = Switch::clone(enabled);
            ui_thread::run_on_idle_after(Duration::from_secs_f64(0.3), move || set(&enabled, true));
            let started = clock::now();
            runner.wait_for_widget_enabled("Plain Target", 2.0);
            clock::since(started)
        },
    )
    .expect("the target becomes enabled");

    assert!(waited >= Duration::from_millis(300));
}

#[test]
fn rust_only_wait_for_widget_enabled_fails_when_a_target_stays_disabled() {
    let result = run_with(
        || {
            let host = TargetHost::new(Rect::new(20.0, 20.0, 100.0, 100.0))
                .plain_enabled_when(&switch(false));
            (vec![Box::new(host) as Box<dyn Widget>], ())
        },
        |runner, _| {
            runner.wait_for_widget_enabled("Plain Target", 0.5);
        },
    );

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            "WaitForWidgetEnabled Failed: [Plain Target] not visible and enabled after [0.5] seconds"
                .to_string()
        ))
    );
}

#[test]
fn rust_only_a_text_fields_context_menu_rows_are_named_targets() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let mut window = AutomationWindow::new(300.0, 200.0);
            let mut field = TextField::new(Arc::new(agg_gui::fonts::standard_ui_font()))
                .with_text("Some Text")
                .with_name("field");
            field.set_bounds(Rect::new(50.0, 90.0, 200.0, 24.0));
            window.add_child(Box::new(field));
            (window, ())
        },
        |runner, _| {
            assert!(!runner.name_exists("Select All Menu Item", 0.0, false));
            runner.right_click_by_name("field");
            assert!(runner.wait_for_name("Select All Menu Item", 2.0));
            runner.wait_for_widget_enabled("Select All Menu Item", 2.0);
            // Nothing is selected, so Cut is on show but disabled.
            let cut = runner
                .get_object_by_name("Cut Menu Item", &WaitOpts::default())
                .expect("Cut is on show");
            assert!(!cut.target_enabled());

            runner.click_by_name("Select All Menu Item");
            assert!(runner.wait_for_widget_disappear("Select All Menu Item", 2.0));
            let root = runner.driver().root();
            let field = agg_gui_automation::tree_query::find_by_name(root, "field")
                .first()
                .and_then(|h| h.downcast::<TextField>(root))
                .map(TextField::selection);
            assert_eq!(field.as_deref(), Some("Some Text"));
            runner.mark_test_complete();
        },
    )
    .expect("the test body passes");
}
