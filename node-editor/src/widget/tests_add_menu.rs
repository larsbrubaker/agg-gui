//! Unit tests for `widget/add_menu.rs`: Shift+A and a right-click on
//! empty canvas open the add menu at the pointer (the host's widget, the
//! host's entries or the built-in submenus), a right-drag pans instead,
//! and Escape closes the menu or goes to the host.
//! Shares the `Memory` model fixture via [`super::tests_common`].

use std::cell::RefCell;

use super::add_menu::{add_menu_rect, AddMenuRequest};
use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes, Memory};
use super::*;
use agg_gui::{Key, MenuEntry, MenuItem, Modifiers, MouseButton, Point};

/// One node with its title bar at (50, 250) in a 400 x 300 pane (view at
/// offset 0, scale 1: local == canvas).
fn editor_with_a_node() -> (NodeEditor, Arc<Mutex<Memory>>) {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    typed.lock().unwrap().node_types = vec![(
        "Shapes".to_string(),
        vec![crate::model::NodeTypeView {
            type_id: "box".to_string(),
            display_name: "Box".to_string(),
            category: "Shapes".to_string(),
        }],
    )];
    seed_nodes(&mut editor, &typed, vec![mk_node(1, "a", [50.0, 250.0])]);
    (editor, typed)
}

fn press(editor: &mut NodeEditor, button: MouseButton, x: f64, y: f64) {
    editor.on_event(&Event::MouseDown {
        pos: Point::new(x, y),
        button,
        modifiers: Modifiers::default(),
    });
}

fn release(editor: &mut NodeEditor, button: MouseButton, x: f64, y: f64) {
    editor.on_event(&Event::MouseUp {
        pos: Point::new(x, y),
        button,
        modifiers: Modifiers::default(),
    });
}

fn move_to(editor: &mut NodeEditor, x: f64, y: f64) {
    editor.on_event(&Event::MouseMove {
        pos: Point::new(x, y),
    });
}

fn key(editor: &mut NodeEditor, key: Key, shift: bool) -> EventResult {
    editor.on_event(&Event::KeyDown {
        key,
        modifiers: Modifiers {
            shift,
            ..Modifiers::default()
        },
    })
}

fn host_entries() -> Vec<MenuEntry> {
    vec![MenuEntry::Item(MenuItem::action("Box", "Box"))]
}

/// A fixed-size stand-in for a host's add-menu widget.
#[derive(Default)]
struct MenuBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for MenuBody {
    fn type_name(&self) -> &'static str {
        "MenuBody"
    }
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
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(100.0, 80.0)
    }
    fn paint(&mut self, _ctx: &mut dyn agg_gui::DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Consumed
    }
}

/// An editor whose add menu is a [`MenuBody`]; the requests land in the
/// returned list.
fn editor_with_widget_menu() -> (NodeEditor, Rc<RefCell<Vec<AddMenuRequest>>>) {
    let (editor, _typed) = editor_with_a_node();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&requests);
    let editor = editor.with_add_menu(move |request| {
        sink.borrow_mut().push(request);
        Some(Box::new(MenuBody::default()) as Box<dyn Widget>)
    });
    (editor, requests)
}

#[test]
fn right_click_on_empty_canvas_opens_the_built_in_add_menu() {
    let (mut editor, _typed) = editor_with_a_node();
    press(&mut editor, MouseButton::Right, 350.0, 50.0);
    assert!(editor.is_add_menu_open());
    let menu = editor.open_context_menu().expect("the built-in add menu");
    assert_eq!(menu.items.len(), 1, "one category submenu");
}

#[test]
fn an_empty_built_in_add_menu_opens_nothing() {
    let (mut editor, typed) = editor_with_a_node();
    typed.lock().unwrap().node_types.clear();
    press(&mut editor, MouseButton::Right, 350.0, 50.0);
    assert!(!editor.is_add_menu_open());
}

#[test]
fn host_add_menu_entries_replace_the_built_in_ones_and_get_the_pick() {
    let (mut editor, typed) = editor_with_a_node();
    typed.lock().unwrap().add_menu = Some(host_entries());
    press(&mut editor, MouseButton::Right, 350.0, 50.0);
    let menu = editor
        .open_context_menu()
        .expect("the host's add menu opens");
    assert_eq!(menu.items.len(), 1);
    editor.handle_popup_action("Box");
    assert_eq!(
        typed.lock().unwrap().add_actions,
        [("Box".to_string(), [350.0, 50.0])]
    );
}

#[test]
fn an_empty_host_add_menu_opens_nothing() {
    let (mut editor, typed) = editor_with_a_node();
    typed.lock().unwrap().add_menu = Some(vec![]);
    press(&mut editor, MouseButton::Right, 350.0, 50.0);
    assert!(!editor.is_add_menu_open());
}

#[test]
fn shift_a_opens_the_add_menu_at_the_pointer() {
    let (mut editor, typed) = editor_with_a_node();
    typed.lock().unwrap().add_menu = Some(host_entries());
    move_to(&mut editor, 300.0, 120.0);
    assert!(key(&mut editor, Key::Char('A'), true).is_consumed());
    assert!(editor.is_add_menu_open());
    editor.handle_popup_action("Box");
    assert_eq!(typed.lock().unwrap().add_actions[0].1, [300.0, 120.0]);
}

#[test]
fn a_without_shift_opens_nothing() {
    let (mut editor, _typed) = editor_with_a_node();
    assert!(!key(&mut editor, Key::Char('a'), false).is_consumed());
    assert!(!editor.is_add_menu_open());
}

#[test]
fn right_drag_pans_and_a_right_click_opens_the_menu_on_release() {
    let (editor, _typed) = editor_with_a_node();
    let mut editor = editor.with_right_drag_pan(true);
    press(&mut editor, MouseButton::Right, 300.0, 100.0);
    assert!(!editor.is_add_menu_open(), "nothing opens on the press");
    move_to(&mut editor, 340.0, 130.0);
    release(&mut editor, MouseButton::Right, 340.0, 130.0);
    assert_eq!(editor.pan(), [40.0, 30.0]);
    assert!(!editor.is_add_menu_open(), "a drag is a pan, not a click");

    press(&mut editor, MouseButton::Right, 200.0, 100.0);
    move_to(&mut editor, 202.0, 101.0);
    release(&mut editor, MouseButton::Right, 202.0, 101.0);
    assert!(editor.is_add_menu_open(), "within 3 px it is a click");
}

#[test]
fn a_right_click_on_a_node_still_opens_its_context_menu() {
    let (editor, _typed) = editor_with_a_node();
    let mut editor = editor.with_right_drag_pan(true);
    press(&mut editor, MouseButton::Right, 60.0, 240.0);
    assert!(editor.open_context_menu().is_some());
    assert!(!editor.is_add_menu_open());
}

#[test]
fn the_host_widget_opens_at_the_pointer_kept_inside_the_editor() {
    let (mut editor, requests) = editor_with_widget_menu();
    press(&mut editor, MouseButton::Right, 350.0, 50.0);
    assert!(editor.is_add_menu_open());
    let request = requests.borrow()[0].clone();
    assert_eq!(request.canvas_pos, [350.0, 50.0]);
    assert_eq!(request.editor_size, Size::new(400.0, 300.0));
    // 100 x 80 at (350, 50): pushed left to fit, and up off the bottom.
    let b = editor.overlay.as_ref().unwrap().bounds();
    assert_eq!((b.x, b.y, b.width, b.height), (300.0, 0.0, 100.0, 80.0));
}

#[test]
fn a_press_outside_the_host_widget_closes_it_and_goes_on() {
    let (mut editor, _requests) = editor_with_widget_menu();
    press(&mut editor, MouseButton::Right, 200.0, 200.0);
    assert!(editor.is_add_menu_open());
    // Inside the menu (top-left at the pointer): it takes the press.
    press(&mut editor, MouseButton::Left, 210.0, 190.0);
    assert!(editor.is_add_menu_open());
    // Outside: closed, and the press reaches the node under it.
    press(&mut editor, MouseButton::Left, 60.0, 240.0);
    assert!(!editor.is_add_menu_open());
    assert!(editor.selected_ids().contains(&NodeId(1)));
}

#[test]
fn the_host_widget_closes_through_its_close_flag() {
    let (mut editor, requests) = editor_with_widget_menu();
    press(&mut editor, MouseButton::Right, 200.0, 200.0);
    requests.borrow()[0].close.set(true);
    editor.layout(Size::new(400.0, 300.0));
    assert!(!editor.is_add_menu_open());
}

#[test]
fn escape_closes_the_add_menu_before_the_host_sees_it() {
    let (mut editor, typed) = editor_with_a_node();
    press(&mut editor, MouseButton::Right, 350.0, 50.0);
    assert!(editor
        .on_unconsumed_key(&Key::Escape, Modifiers::default())
        .is_consumed());
    assert!(!editor.is_add_menu_open());
    assert_eq!(typed.lock().unwrap().escapes, 0);
}

#[test]
fn escape_goes_to_the_host_and_is_consumed_only_when_it_acts() {
    let (mut editor, typed) = editor_with_a_node();
    assert!(!key(&mut editor, Key::Escape, false).is_consumed());
    typed.lock().unwrap().escape_handled = true;
    assert!(key(&mut editor, Key::Escape, false).is_consumed());
    assert_eq!(typed.lock().unwrap().escapes, 2);
}

#[test]
fn add_menu_rect_keeps_the_menu_inside_the_editor() {
    let editor = Size::new(400.0, 300.0);
    let menu = Size::new(100.0, 80.0);
    let r = add_menu_rect(Point::new(50.0, 200.0), menu, editor);
    assert_eq!((r.x, r.y), (50.0, 120.0), "top-left at the pointer");
    let r = add_menu_rect(Point::new(390.0, 295.0), menu, editor);
    assert_eq!((r.x, r.y), (300.0, 215.0));
}
