//! Removing the focused, hovered or pointer-captured widget from the tree.
//!
//! `App` addresses those widgets by child-index path, anchored to their
//! identities (`widget/app/path_anchor.rs`). When an app drops the widget (or
//! an ancestor of it) the path no longer names anything; using it indexed out
//! of bounds and panicked (`tree_paths::widget_at_path_ref`). A stale path is
//! now dropped: focus clears (the dropped widget gets no `FocusLost`, it is
//! gone), and hover and capture fall back to whatever the pointer is over.

use crate::{
    App, DrawCtx, Event, EventResult, Font, Modifiers, MouseButton, Rect, Size, TextField,
};
use crate::{Key, Widget};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const SIZE: Size = Size {
    width: 400.0,
    height: 40.0,
};

/// A container laying its children out side by side, 200 px each. When
/// `drop_last` is set, its next layout removes its last child (an app
/// rebuilding its content on layout).
struct Row {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    drop_last: Rc<Cell<bool>>,
    events: Rc<RefCell<Vec<String>>>,
}

impl Widget for Row {
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
        if self.drop_last.replace(false) {
            self.children.pop();
        }
        for (i, child) in self.children.iter_mut().enumerate() {
            child.layout(Size::new(200.0, available.height));
            child.set_bounds(Rect::new(i as f64 * 200.0, 0.0, 200.0, available.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseMove { pos } = event {
            self.events
                .borrow_mut()
                .push(format!("move {} {}", pos.x, pos.y));
        }
        EventResult::Ignored
    }
}

/// A leaf that consumes presses (so it takes pointer capture).
struct Grabber {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Grabber {
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
        match event {
            Event::MouseDown { .. } | Event::MouseUp { .. } | Event::MouseMove { .. } => {
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

struct Fixture {
    app: App,
    drop_last: Rc<Cell<bool>>,
    events: Rc<RefCell<Vec<String>>>,
}

fn fixture(children: Vec<Box<dyn Widget>>) -> Fixture {
    let drop_last = Rc::new(Cell::new(false));
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut app = App::new(Box::new(Row {
        bounds: Rect::default(),
        children,
        drop_last: Rc::clone(&drop_last),
        events: Rc::clone(&events),
    }));
    app.layout(SIZE);
    Fixture {
        app,
        drop_last,
        events,
    }
}

fn field() -> Box<dyn Widget> {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    Box::new(TextField::new(font))
}

fn grabber() -> Box<dyn Widget> {
    Box::new(Grabber {
        bounds: Rect::default(),
        children: Vec::new(),
    })
}

fn click(app: &mut App, x: f64) {
    let y = SIZE.height * 0.5;
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
}

#[test]
fn removing_focused_text_field_on_next_layout_clears_focus() {
    let mut f = fixture(vec![field(), field()]);
    click(&mut f.app, 250.0);
    assert_eq!(f.app.focused_path(), Some(&[1usize][..]));
    assert!(f.app.focused_is_text_input());

    f.drop_last.set(true);
    f.app.layout(SIZE);
    assert!(!f.app.has_focus());
    assert!(!f.app.focused_is_text_input());
    assert_eq!(f.app.focused_widget_type_name(), None);
    // Typing and tabbing afterwards go nowhere stale.
    f.app.on_key_down(Key::Char('a'), Modifiers::default());
    f.app.on_key_down(Key::Tab, Modifiers::default());
    assert_eq!(f.app.focused_path(), Some(&[0usize][..]));
}

/// The host may query focus between an event that removed the field and the
/// next layout (web shells ask every frame whether to show the text input).
#[test]
fn querying_focus_after_removing_focused_field_does_not_panic() {
    let mut f = fixture(vec![field()]);
    click(&mut f.app, 50.0);
    assert!(f.app.has_focus());

    f.app.root_mut().children_mut().clear();
    assert!(!f.app.focused_is_text_input());
    assert_eq!(f.app.focused_widget_type_name(), None);
    f.app.layout(SIZE);
    assert!(!f.app.has_focus());
}

#[test]
fn removing_captured_widget_mid_drag_drops_capture() {
    let mut f = fixture(vec![grabber(), grabber()]);
    let y = SIZE.height * 0.5;
    f.app
        .on_mouse_down(250.0, y, MouseButton::Left, Modifiers::default());
    assert!(f.app.has_captured_pointer());

    f.drop_last.set(true);
    f.app.layout(SIZE);
    assert!(!f.app.has_captured_pointer());
    f.events.borrow_mut().clear();
    f.app.on_mouse_move(260.0, y);
    f.app
        .on_mouse_up(260.0, y, MouseButton::Left, Modifiers::default());
    assert!(!f.app.has_captured_pointer());
}

/// A dropped hovered widget gets no hover-clear, and its parent is not sent
/// the clear in its place (the stale path used to stop at the parent).
#[test]
fn removing_hovered_widget_sends_its_parent_no_hover_clear() {
    let mut f = fixture(vec![grabber(), grabber()]);
    f.app.on_mouse_move(250.0, 20.0);

    f.drop_last.set(true);
    f.app.layout(SIZE);
    f.app.update_tooltips_for_test();
    f.events.borrow_mut().clear();
    f.app.on_mouse_move(260.0, 20.0);
    let events = f.events.borrow();
    assert!(
        !events.iter().any(|e| e == "move -1 -1"),
        "the parent received a hover-clear meant for its dropped child: {events:?}"
    );
}
