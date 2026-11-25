//! Tests for a menu's text entry (`MenuItem::takes_text`, `row_widgets.rs`):
//! the menu opens with the highlight and the focus on it, typing and Escape
//! reach it wherever the highlight is, every key but Up and Down reaches it
//! while it is highlighted, its rows can be replaced while it stays open
//! (`PopupMenu::replace_items`), and a `MenuBar` installs its widget, opens
//! on it and swaps in rows from a `MenuItemsUpdate` (`bar_text_entry.rs`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::Widget;

use super::super::geometry::BAR_H;
use super::super::model::{MenuEntry, MenuItem};
use super::super::state::MenuResponse;
use super::{MenuBar, MenuItemsUpdate, PopupMenu, TopMenu};

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};

fn reset_env() {
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
}

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// What the field has seen: its text, whether it has the focus, and the
/// keys it took that are not typing.
#[derive(Default)]
struct FieldLog {
    text: String,
    focused: bool,
    other_keys: Vec<Key>,
}

/// A search field stand-in: characters and Backspace edit its text, Escape
/// clears non-empty text (and is ignored on empty text, so the menu closes),
/// Left, Right and Enter are its own.
struct Field {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    log: Rc<RefCell<FieldLog>>,
}

impl Widget for Field {
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
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        let mut log = self.log.borrow_mut();
        match event {
            Event::FocusGained => log.focused = true,
            Event::FocusLost => log.focused = false,
            Event::TextInput { text } => log.text.push_str(text),
            Event::KeyDown { key, .. } => match key {
                Key::Char(c) => log.text.push(*c),
                Key::Backspace => {
                    log.text.pop();
                }
                Key::Escape if log.text.is_empty() => return EventResult::Ignored,
                Key::Escape => log.text.clear(),
                Key::ArrowLeft | Key::ArrowRight | Key::Enter => log.other_keys.push(key.clone()),
                _ => return EventResult::Ignored,
            },
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }
}

fn field() -> (Box<dyn Widget>, Rc<RefCell<FieldLog>>) {
    let log = Rc::new(RefCell::new(FieldLog::default()));
    let widget = Field {
        bounds: Rect::default(),
        children: Vec::new(),
        log: Rc::clone(&log),
    };
    (Box::new(widget), log)
}

fn key(key: Key) -> Event {
    Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
    }
}

/// The field's row (id 0), then the rows under it.
fn rows(labels: &[&str]) -> Vec<MenuEntry> {
    let mut items: Vec<MenuEntry> = vec![MenuItem::widget_row(0, 30.0).takes_text().into()];
    items.extend(
        labels
            .iter()
            .map(|label| MenuEntry::from(MenuItem::action(*label, *label))),
    );
    items
}

/// A popup of the field and two rows, opened.
fn search_menu() -> (PopupMenu, Rc<RefCell<FieldLog>>) {
    let (widget, log) = field();
    let mut menu = PopupMenu::new(rows(&["a", "b"]));
    menu.set_row_widget(0, widget);
    menu.open_at(Point::new(20.0, 300.0));
    (menu, log)
}

fn type_text(menu: &mut PopupMenu, text: &str) {
    for c in text.chars() {
        menu.handle_event(&key(Key::Char(c)), VIEWPORT);
    }
}

#[test]
fn the_menu_opens_with_the_highlight_and_the_focus_on_its_text_entry() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (menu, log) = search_menu();
    assert_eq!(menu.state.hover_path, Some(vec![0]));
    assert!(menu.text_row_highlighted());
    assert!(log.borrow().focused);
}

#[test]
fn typing_reaches_the_text_entry_wherever_the_highlight_is() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut menu, log) = search_menu();
    type_text(&mut menu, "a b");
    assert_eq!(log.borrow().text, "a b", "Space is typing too");
    // On the field, Left, Right and Enter are the field's.
    for k in [Key::ArrowLeft, Key::ArrowRight, Key::Enter] {
        let (result, response) = menu.handle_event(&key(k), VIEWPORT);
        assert!(result.is_consumed());
        assert_eq!(response, MenuResponse::None);
    }
    assert_eq!(log.borrow().other_keys.len(), 3);
    assert!(menu.is_open());

    // Down leaves the field; typing still goes to it, and it keeps the focus.
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    assert_eq!(menu.state.hover_path, Some(vec![1]));
    assert!(log.borrow().focused);
    menu.handle_event(&key(Key::Backspace), VIEWPORT);
    menu.handle_event(
        &Event::TextInput {
            text: "é".to_string(),
        },
        VIEWPORT,
    );
    assert_eq!(log.borrow().text, "a é");
    // A shortcut-like chord is not typing.
    menu.handle_event(
        &Event::KeyDown {
            key: Key::Char('x'),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        },
        VIEWPORT,
    );
    assert_eq!(log.borrow().text, "a é");

    // Enter on a row is still the row's pick.
    let (_, response) = menu.handle_event(&key(Key::Enter), VIEWPORT);
    assert_eq!(response, MenuResponse::Action("a".to_string()));
    assert_eq!(log.borrow().other_keys.len(), 3);
}

#[test]
fn escape_clears_the_text_then_closes_the_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut menu, log) = search_menu();
    type_text(&mut menu, "cyl");
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    let (_, response) = menu.handle_event(&key(Key::Escape), VIEWPORT);
    assert_eq!(response, MenuResponse::None);
    assert!(menu.is_open(), "the first Escape clears the text");
    assert_eq!(log.borrow().text, "");
    let (_, response) = menu.handle_event(&key(Key::Escape), VIEWPORT);
    assert_eq!(response, MenuResponse::Closed);
    assert!(!menu.is_open());
    assert!(!log.borrow().focused, "a closed menu's field has no focus");
}

#[test]
fn replaced_rows_keep_the_menu_open_and_the_same_field() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let (mut menu, log) = search_menu();
    type_text(&mut menu, "cy");
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    menu.replace_items(rows(&["Cylinder", "Cone", "Cube"]));
    assert!(menu.is_open());
    assert_eq!(menu.items.len(), 4);
    assert_eq!(menu.state.hover_path, Some(vec![0]), "back on the field");
    assert!(log.borrow().focused);
    type_text(&mut menu, "l");
    assert_eq!(log.borrow().text, "cyl", "the same field, its text kept");
    menu.handle_event(&key(Key::ArrowDown), VIEWPORT);
    let (_, response) = menu.handle_event(&key(Key::Enter), VIEWPORT);
    assert_eq!(response, MenuResponse::Action("Cylinder".to_string()));
}

#[test]
fn a_menu_without_a_text_entry_opens_as_before() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut menu = PopupMenu::new(vec![MenuItem::action("A", "a").into()]);
    menu.open_at(Point::new(20.0, 300.0));
    assert_eq!(menu.state.hover_path, None);
    assert!(!menu.text_row_highlighted());
}

fn press(bar: &mut MenuBar, pos: Point) {
    bar.on_event(&Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

#[test]
fn a_menu_bar_menu_hosts_its_field_opens_on_it_and_takes_new_rows() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    crate::widget::set_current_viewport(Size::new(300.0, 300.0));
    let update = MenuItemsUpdate::new();
    let logs: Rc<RefCell<Vec<Rc<RefCell<FieldLog>>>>> = Rc::default();
    let made = Rc::clone(&logs);
    let picked: Rc<RefCell<Vec<String>>> = Rc::default();
    let picks = Rc::clone(&picked);
    let mut bar = MenuBar::new(
        test_font(),
        vec![
            TopMenu::new("Add", Vec::new())
                .with_items_provider(|| rows(&["Shapes", "Math"]))
                .with_row_widgets(move || {
                    let (widget, log) = field();
                    made.borrow_mut().push(log);
                    vec![(0, widget)]
                })
                .with_items_update(update.clone()),
            TopMenu::new("Edit", vec![MenuItem::action("Copy", "edit.copy").into()]),
        ],
        move |action| picks.borrow_mut().push(action.to_string()),
    );
    bar.layout(Size::new(300.0, BAR_H));
    // Rows asked for while the menu is closed are dropped at the open.
    update.set(rows(&["stale"]));
    press(&mut bar, Point::new(8.0, 8.0));
    assert!(bar.popup.is_open());
    assert_eq!(
        logs.borrow().len(),
        1,
        "the field is made as the menu opens"
    );
    let log = Rc::clone(&logs.borrow()[0]);
    assert!(log.borrow().focused);
    assert!(bar.popup.text_row_highlighted());

    // Left and Right on the field move its caret, not to the next menu.
    bar.on_event(&key(Key::ArrowRight));
    assert!(bar.popup.is_open());
    assert_eq!(bar.open_index, Some(0));
    for c in "cyl".chars() {
        bar.on_event(&key(Key::Char(c)));
    }
    assert_eq!(log.borrow().text, "cyl");

    // The next event swaps the rows in first: Down then steps from the field
    // onto the one result.
    update.set(rows(&["Cylinder"]));
    bar.on_event(&key(Key::ArrowDown));
    assert!(bar.popup.is_open());
    assert_eq!(bar.popup.items.len(), 2, "the field and its one result");
    assert_eq!(bar.popup.state.hover_path, Some(vec![1]));
    assert!(log.borrow().focused, "the field keeps the focus");
    bar.on_event(&key(Key::Enter));
    assert_eq!(picked.borrow().as_slice(), ["Cylinder".to_string()]);
}
