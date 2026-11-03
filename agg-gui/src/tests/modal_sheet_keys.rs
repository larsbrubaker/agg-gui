//! Keys nothing focused took, under a showing [`ModalSheet`]: the sheet
//! offers them to its own content's `on_unconsumed_key` (a dialog page's
//! F5) before its default / cancel actions, and nothing behind the sheet
//! hears them; a passthrough sheet leaves the offer to the app's tree-wide
//! pass, so its content hears a key once.

use super::*;
use crate::widgets::ModalSheet;
use crate::{DrawCtx, Event, EventResult, Rect, Stack};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

/// A widget that records every key offered to its `on_unconsumed_key` and
/// takes the one it is told to take.
struct KeyListener {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    heard: Rc<RefCell<Vec<Key>>>,
    takes: Option<Key>,
}

impl KeyListener {
    fn new(heard: &Rc<RefCell<Vec<Key>>>, takes: Option<Key>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            heard: Rc::clone(heard),
            takes,
        }
    }

    /// The listener holding `child`, as a page holds its fields.
    fn holding(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }
}

impl Widget for KeyListener {
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
        for child in &mut self.children {
            let size = child.layout(Size::new(available.width, 30.0));
            child.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn on_unconsumed_key(&mut self, key: &Key, _modifiers: Modifiers) -> EventResult {
        self.heard.borrow_mut().push(key.clone());
        if self.takes.as_ref() == Some(key) {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

/// What the app behind the sheet heard: its own unconsumed-key listener and
/// the global key handler.
struct Behind {
    heard: Rc<RefCell<Vec<Key>>>,
    global: Rc<RefCell<Vec<Key>>>,
}

/// `Stack[ a listener behind, sheet ]`, laid out, with a global key handler.
fn app_with(sheet: ModalSheet) -> (App, Behind) {
    let behind = Behind {
        heard: Rc::new(RefCell::new(Vec::new())),
        global: Rc::new(RefCell::new(Vec::new())),
    };
    let stack = Stack::new()
        .add(Box::new(KeyListener::new(&behind.heard, None)))
        .add(Box::new(sheet));
    let mut app = App::new(Box::new(stack));
    let sink = Rc::clone(&behind.global);
    app.set_global_key_handler(move |key, _| {
        sink.borrow_mut().push(key);
        true
    });
    app.layout(Size::new(400.0, 400.0));
    (app, behind)
}

fn press(app: &mut App, key: Key) {
    app.on_key_down(key, Modifiers::default());
}

#[test]
fn a_sheets_content_hears_a_key_with_nothing_focused_and_nothing_behind_does() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let visible = Rc::new(Cell::new(true));
    let sheet = ModalSheet::new(
        Rc::clone(&visible),
        Box::new(KeyListener::new(&heard, None)),
    );
    let (mut app, behind) = app_with(sheet);
    assert!(app.focused_widget_type_name().is_none());

    press(&mut app, Key::F(5));
    press(&mut app, Key::Char('x'));
    assert_eq!(*heard.borrow(), vec![Key::F(5), Key::Char('x')]);
    assert!(behind.heard.borrow().is_empty(), "nothing behind hears it");
    assert!(behind.global.borrow().is_empty(), "nor the global handler");
    assert!(visible.get());
}

#[test]
fn a_sheets_content_hears_a_key_its_focused_field_left_alone() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let field = TextField::new(font()).with_focus_id(41);
    let page = KeyListener::new(&heard, None).holding(Box::new(field));
    let sheet = ModalSheet::new(Rc::new(Cell::new(true)), Box::new(page));
    let (mut app, behind) = app_with(sheet);
    crate::focus::request_focus(41);
    app.layout(Size::new(400.0, 400.0));
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));

    press(&mut app, Key::F(5));
    assert_eq!(*heard.borrow(), vec![Key::F(5)], "offered once");
    assert!(behind.heard.borrow().is_empty());
    assert!(behind.global.borrow().is_empty());
}

#[test]
fn a_key_the_content_takes_comes_before_the_sheets_escape() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let visible = Rc::new(Cell::new(true));
    let sheet = ModalSheet::new(
        Rc::clone(&visible),
        Box::new(KeyListener::new(&heard, Some(Key::Escape))),
    );
    let (mut app, behind) = app_with(sheet);

    press(&mut app, Key::Escape);
    assert_eq!(*heard.borrow(), vec![Key::Escape]);
    assert!(visible.get(), "the content took Escape: the sheet stays");
    assert!(behind.global.borrow().is_empty());
}

#[test]
fn escape_the_content_leaves_alone_still_closes_the_sheet() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let visible = Rc::new(Cell::new(true));
    let sheet = ModalSheet::new(
        Rc::clone(&visible),
        Box::new(KeyListener::new(&heard, None)),
    );
    let (mut app, behind) = app_with(sheet);

    press(&mut app, Key::Escape);
    assert_eq!(*heard.borrow(), vec![Key::Escape]);
    assert!(!visible.get(), "closed");
    assert!(behind.heard.borrow().is_empty());
    assert!(behind.global.borrow().is_empty());
}

#[test]
fn a_passthrough_sheets_content_hears_a_key_once_and_the_app_behind_hears_it() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let sheet = ModalSheet::new(
        Rc::new(Cell::new(true)),
        Box::new(KeyListener::new(&heard, None)),
    )
    .with_key_passthrough(true);
    let (mut app, behind) = app_with(sheet);

    press(&mut app, Key::F(5));
    assert_eq!(*heard.borrow(), vec![Key::F(5)], "offered once");
    assert_eq!(*behind.heard.borrow(), vec![Key::F(5)]);
    assert_eq!(*behind.global.borrow(), vec![Key::F(5)]);
}
