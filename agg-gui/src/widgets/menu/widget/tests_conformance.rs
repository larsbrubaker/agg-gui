//! agg-sharp `PopupMenuConformanceTests` (Tests/Agg.Tests/Agg.UI) through the
//! real `App` with a real widget under the menu.
//!
//! The C# harness opens a `PopupMenu` over a window whose backdrop counts the
//! mouse traffic it receives; these ports do the same with a real
//! [`Button`] beneath the menu, watched through [`observe_events`] so the
//! tree under test is not changed by the watching.  The press and release go
//! through `App::on_mouse_down` / `App::on_mouse_up`, the same routing a shell
//! uses, so the tests pin what reaches the widget beneath — not just the menu
//! state, which `menu/mod.rs`'s unit tests cover.
//!
//! The context menu is hosted the way every agg-gui menu host does it
//! (`popup_local.rs`): modal and claiming the pointer while open.  The same
//! outside-press rule is pinned for the other popup menu kinds: a menu-bar
//! menu, a submenu chain and the `ComboBox` drop-down.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{observe_events, App, EventObserver, Widget, WidgetId};
use crate::widgets::{Button, ComboBox};

use super::super::geometry::BAR_H;
use super::super::model::{MenuEntry, MenuItem};
use super::super::state::MenuResponse;
use super::{MenuBar, PopupMenu, TopMenu};

const VIEWPORT: Size = Size {
    width: 600.0,
    height: 400.0,
};

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Desktop input baseline (other test files flip the shared profile, the
/// touch latch and the scales).
fn reset_env() {
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
    crate::device_scale::set_device_scale(1.0);
    crate::widget::set_current_viewport(VIEWPORT);
}

/// C# `MenuHarness`'s window: children at fixed window-space rects, later
/// children over earlier ones.
struct Window {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    rects: Vec<Rect>,
}

impl Widget for Window {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        for (child, rect) in self.children.iter_mut().zip(&self.rects) {
            child.layout(Size::new(rect.width, rect.height));
            child.set_bounds(*rect);
        }
        available
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// C# `MenuHarness.Anchor` with the menu shown from it: hosts a `PopupMenu`
/// opened at its own top-left corner, modal and claiming the pointer while the
/// menu is open (the contract in `popup_local.rs`), and counts the actions the
/// menu fires.
struct Anchor {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    menu: Rc<RefCell<PopupMenu>>,
    actions: Rc<RefCell<Vec<String>>>,
}

impl Widget for Anchor {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
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
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn hit_test_global_overlay(&self, _local_pos: Point) -> bool {
        self.menu.borrow().is_open()
    }
    fn has_active_modal(&self) -> bool {
        self.menu.borrow().is_open()
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        let (result, response) = self.menu.borrow_mut().handle_local_event(event, VIEWPORT);
        if let MenuResponse::Action(action) = response {
            self.actions.borrow_mut().push(action);
        }
        result
    }
}

/// C# `ClickCountingWidget` on a real [`Button`]: what the button received
/// (through an event observer) and how many times it fired.
#[derive(Default)]
struct Counts {
    mouse_downs: Cell<u32>,
    mouse_ups: Cell<u32>,
    focus_gains: Cell<u32>,
    clicks: Cell<u32>,
}

/// A real button that counts its traffic, and the observer that watches it.
fn counting_button(name: &str) -> (Box<dyn Widget>, Rc<Counts>, EventObserver) {
    let counts = Rc::new(Counts::default());
    let clicked = Rc::clone(&counts);
    let button: Box<dyn Widget> = Box::new(
        Button::new(name, test_font())
            .on_click(move || clicked.clicks.set(clicked.clicks.get() + 1)),
    );
    let seen = Rc::clone(&counts);
    let observer = observe_events(WidgetId::of(button.as_ref()), move |event| match event {
        Event::MouseDown { .. } => seen.mouse_downs.set(seen.mouse_downs.get() + 1),
        Event::MouseUp { .. } => seen.mouse_ups.set(seen.mouse_ups.get() + 1),
        Event::FocusGained => seen.focus_gains.set(seen.focus_gains.get() + 1),
        _ => {}
    });
    (button, counts, observer)
}

fn assert_got_nothing(counts: &Counts, what: &str) {
    assert_eq!(counts.mouse_downs.get(), 0, "{what} must see no press");
    assert_eq!(counts.mouse_ups.get(), 0, "{what} must see no release");
    assert_eq!(counts.clicks.get(), 0, "{what} must not click");
    assert_eq!(counts.focus_gains.get(), 0, "{what} must not take focus");
}

/// Press and release at window point `p` (Y-up), as a shell delivers them.
fn click(app: &mut App, p: Point, button: MouseButton) {
    let y = VIEWPORT.height - p.y;
    app.on_mouse_down(p.x, y, button, Modifiers::default());
    app.on_mouse_up(p.x, y, button, Modifiers::default());
}

/// C# `MenuHarness`: a full-window counting backdrop, an anchor at (10, 370)
/// and a menu shown from it.
struct Harness {
    app: App,
    menu: Rc<RefCell<PopupMenu>>,
    actions: Rc<RefCell<Vec<String>>>,
    beneath: Rc<Counts>,
    /// C# `AddWidgetUnderMenu`: a counting button exactly under a row.
    buried: Option<Rc<Counts>>,
    _observers: Vec<EventObserver>,
}

impl Harness {
    fn show(items: Vec<MenuEntry>) -> Self {
        Self::build(items, None)
    }

    /// [`Harness::show`] with a counting button exactly where top-level row
    /// `row` is, under the menu in the window's child order.
    fn show_with_button_under_row(items: Vec<MenuEntry>, row: usize) -> Self {
        Self::build(items, Some(row))
    }

    fn build(items: Vec<MenuEntry>, buried_row: Option<usize>) -> Self {
        let anchor_rect = Rect::new(10.0, 370.0, 50.0, 20.0);
        let mut popup = PopupMenu::new(items);
        popup.set_root_origin(Point::new(anchor_rect.x, anchor_rect.y));
        popup.open_at_local(Point::ORIGIN);
        let menu = Rc::new(RefCell::new(popup));
        let actions = Rc::new(RefCell::new(Vec::new()));

        let (beneath_button, beneath, observer) = counting_button("Beneath");
        let mut observers = vec![observer];
        let mut children = vec![beneath_button];
        let mut rects = vec![Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height)];
        let mut buried = None;
        if let Some(row) = buried_row {
            let rect = row_rect(&menu.borrow(), row);
            let (button, counts, observer) = counting_button("Buried");
            observers.push(observer);
            children.push(button);
            rects.push(rect);
            buried = Some(counts);
        }
        children.push(Box::new(Anchor {
            bounds: Rect::default(),
            children: Vec::new(),
            menu: Rc::clone(&menu),
            actions: Rc::clone(&actions),
        }));
        rects.push(anchor_rect);

        let mut app = App::new(Box::new(Window {
            bounds: Rect::default(),
            children,
            rects,
        }));
        app.layout(VIEWPORT);
        Self {
            app,
            menu,
            actions,
            beneath,
            buried,
            _observers: observers,
        }
    }

    /// Window-space rects of the open menu's panels.
    fn panels(&self) -> Vec<Rect> {
        let menu = self.menu.borrow();
        menu.state
            .layouts(&menu.items, VIEWPORT)
            .iter()
            .map(|l| l.rect)
            .collect()
    }

    /// C# `PointOutsideMenu`: right of the widest panel, below the lowest.
    fn point_outside_menu(&self) -> Point {
        let panels = self.panels();
        let right = panels
            .iter()
            .map(|r| r.x + r.width)
            .fold(f64::MIN, f64::max);
        let bottom = panels.iter().map(|r| r.y).fold(f64::MAX, f64::min);
        Point::new(right + 20.0, bottom - 20.0)
    }

    fn menu_contains(&self, p: Point) -> bool {
        self.panels()
            .iter()
            .any(|r| p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height)
    }

    /// Centre of top-level row `row`, in window space (C# `CenterOf`).
    fn center_of_row(&self, row: usize) -> Point {
        center(row_rect(&self.menu.borrow(), row))
    }

    fn is_open(&self) -> bool {
        self.menu.borrow().is_open()
    }
}

/// The window-space rect of `menu`'s top-level row `row`.
fn row_rect(menu: &PopupMenu, row: usize) -> Rect {
    let layouts = menu.state.layouts(&menu.items, VIEWPORT);
    layouts[0]
        .rows
        .iter()
        .find(|r| r.item_index == Some(row))
        .map(|r| r.rect)
        .expect("row is laid out")
}

fn center(rect: Rect) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5)
}

fn open_close_items() -> Vec<MenuEntry> {
    vec![
        MenuItem::action("Open", "open").into(),
        MenuItem::action("Close", "close").into(),
    ]
}

/// CONVERTED from agg-sharp: C# lets the press that dismisses a menu from
/// outside reach the widget beneath (`Beneath.MouseDowns == 1`,
/// `LeftClicks == 1`, and the focus moves to it).  Here the press only closes
/// the menu, as native Mac and Windows menus do (Lars, 2026-10-08): the
/// widget beneath sees no press, no release, no click and takes no focus.
/// MatterCAD records this as divergence 3065.
#[test]
fn an_outside_press_both_dismisses_the_menu_and_reaches_the_widget_beneath() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_close_items());

    let outside = h.point_outside_menu();
    assert!(
        !h.menu_contains(outside),
        "the dismissing press has to land off every menu panel"
    );

    click(&mut h.app, outside, MouseButton::Left);

    assert!(!h.is_open(), "an outside press closes the menu");
    assert_got_nothing(&h.beneath, "the widget beneath the outside press");
}

/// C# `ClickingARowNeverReachesAButtonDirectlyUnderIt`: a button exactly
/// under the clicked row sees neither the press nor the release.
#[test]
fn clicking_a_row_never_reaches_a_button_directly_under_it() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show_with_button_under_row(open_close_items(), 0);

    let row = h.center_of_row(0);
    click(&mut h.app, row, MouseButton::Left);

    assert_eq!(*h.actions.borrow(), vec!["open".to_string()]);
    assert!(!h.is_open());
    let buried = h.buried.as_ref().expect("a button under the row");
    assert_got_nothing(buried, "the button under the row");
    assert_got_nothing(&h.beneath, "the backdrop");
}

/// C# `ActionClickFiresOnceAndDoesNotLeakToTheWidgetBeneath`: the action
/// fires once, closes the menu, and the widget the menu covers sees nothing
/// of the press or the release that follows it.
#[test]
fn action_click_fires_once_and_does_not_leak_to_the_widget_beneath() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_close_items());

    let row = h.center_of_row(0);
    click(&mut h.app, row, MouseButton::Left);

    assert_eq!(*h.actions.borrow(), vec!["open".to_string()]);
    assert!(!h.is_open(), "an action item closes the menu it belongs to");
    assert_got_nothing(&h.beneath, "the widget beneath the menu");
}

/// A press outside with a submenu open closes the whole chain and is
/// consumed like a press outside a single panel.
#[test]
fn an_outside_press_with_a_submenu_open_closes_the_chain_without_reaching_the_widget_beneath() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let items = vec![
        MenuItem::action("Open", "open").into(),
        MenuItem::submenu("More", vec![MenuItem::action("Leaf", "leaf").into()]).into(),
    ];
    let mut h = Harness::show(items);
    let more = h.center_of_row(1);
    click(&mut h.app, more, MouseButton::Left);
    assert_eq!(h.panels().len(), 2, "pressing More opens its submenu");
    assert_eq!(h.beneath.mouse_downs.get(), 0);

    let outside = h.point_outside_menu();
    assert!(!h.menu_contains(outside));
    click(&mut h.app, outside, MouseButton::Left);

    assert!(!h.is_open(), "the outside press closes the whole chain");
    assert_got_nothing(&h.beneath, "the widget beneath the outside press");
}

/// A right press outside dismisses too, and does not reach the widget
/// beneath (no context menu of its own opens from the same press).
#[test]
fn an_outside_right_press_dismisses_without_reaching_the_widget_beneath() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_close_items());

    let outside = h.point_outside_menu();
    click(&mut h.app, outside, MouseButton::Right);

    assert!(!h.is_open());
    assert_got_nothing(&h.beneath, "the widget beneath the right press");
}

/// A menu-bar menu: an outside press closes it and the widget beneath sees
/// nothing.  Opening it from its title stays a normal click on the bar.
#[test]
fn an_outside_press_closes_a_menu_bar_menu_without_reaching_the_widget_beneath() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (beneath_button, beneath, _observer) = counting_button("Beneath");
    let bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new("File", open_close_items())],
        |_| {},
    );
    let bar_rect = Rect::new(0.0, VIEWPORT.height - BAR_H, VIEWPORT.width, BAR_H);
    let mut app = App::new(Box::new(Window {
        bounds: Rect::default(),
        children: vec![beneath_button, Box::new(bar)],
        rects: vec![
            Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height),
            bar_rect,
        ],
    }));
    app.layout(VIEWPORT);
    let bar_is_open = |app: &App| app.root().children()[1].has_active_modal();

    click(
        &mut app,
        Point::new(8.0, bar_rect.y + 8.0),
        MouseButton::Left,
    );
    assert!(bar_is_open(&app), "clicking File opens its menu");

    click(&mut app, Point::new(590.0, 10.0), MouseButton::Left);

    assert!(!bar_is_open(&app), "an outside press closes the menu");
    assert_got_nothing(&beneath, "the widget beneath the outside press");
}

/// The `ComboBox` drop-down: an outside press closes it and the widget
/// beneath sees nothing; a press on the combo's own box still just toggles
/// it closed.
#[test]
fn an_outside_press_closes_a_combo_box_drop_down_without_reaching_the_widget_beneath() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (beneath_button, beneath, _observer) = counting_button("Beneath");
    let combo = ComboBox::new(vec!["Alpha", "Beta", "Gamma"], 0, test_font());
    let combo_rect = Rect::new(10.0, 340.0, 200.0, 24.0);
    let mut app = App::new(Box::new(Window {
        bounds: Rect::default(),
        children: vec![beneath_button, Box::new(combo)],
        rects: vec![
            Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height),
            combo_rect,
        ],
    }));
    app.layout(VIEWPORT);
    let combo_is_open = |app: &App| {
        app.root().children()[1]
            .properties()
            .contains(&("open", "true".to_string()))
    };
    let on_box = Point::new(combo_rect.x + 20.0, combo_rect.y + 12.0);

    click(&mut app, on_box, MouseButton::Left);
    assert!(combo_is_open(&app), "clicking the box opens the list");
    click(&mut app, Point::new(590.0, 10.0), MouseButton::Left);
    assert!(!combo_is_open(&app), "an outside press closes the list");
    assert_got_nothing(&beneath, "the widget beneath the outside press");

    click(&mut app, on_box, MouseButton::Left);
    assert!(combo_is_open(&app));
    click(&mut app, on_box, MouseButton::Left);
    assert!(
        !combo_is_open(&app),
        "a press on the combo's own box toggles the list closed"
    );
    assert_got_nothing(&beneath, "the widget beneath the combo");
}
