//! A focused list-like widget rebuilt in place keeps the keyboard.
//!
//! Apps rebuild a focusable list (or the area wrapping it) after a click,
//! Ctrl+A or a drag, and rely on Escape and shortcuts still reaching the
//! list that now stands where the focused one stood. Keys must reach the
//! rebuilt list; a list with a `WidgetBase` is also told it has focus even
//! when it reuses the old list's heap address. One without a `WidgetBase`
//! is told when its address differs (the case `App` can tell apart).

use crate::{
    App, DrawCtx, Event, EventResult, Key, Modifiers, MouseButton, Rect, Size, Widget, WidgetBase,
};
use std::cell::RefCell;
use std::rc::Rc;

const SIZE: Size = Size {
    width: 400.0,
    height: 40.0,
};

type Log = Rc<RefCell<Vec<String>>>;

/// A focusable list area logging `"<name> gained"`, `"<name> lost"` and
/// `"<name> escape"`; with `base` it embeds a `WidgetBase`.
struct ListArea {
    name: &'static str,
    bounds: Rect,
    log: Log,
    base: Option<WidgetBase>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for ListArea {
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        self.base.as_ref()
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        self.base.as_mut()
    }
    fn layout(&mut self, available: Size) -> Size {
        for child in &mut self.children {
            child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn is_focusable(&self) -> bool {
        true
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        let kind = match event {
            Event::FocusGained => "gained",
            Event::FocusLost => "lost",
            Event::KeyDown {
                key: Key::Escape, ..
            } => "escape",
            Event::MouseDown { .. } | Event::MouseUp { .. } => return EventResult::Consumed,
            _ => return EventResult::Ignored,
        };
        self.log.borrow_mut().push(format!("{} {kind}", self.name));
        EventResult::Consumed
    }
}

fn list(name: &'static str, log: &Log, with_base: bool) -> Box<dyn Widget> {
    Box::new(ListArea {
        name,
        bounds: Rect::default(),
        log: Rc::clone(log),
        base: with_base.then(WidgetBase::new),
        children: Vec::new(),
    })
}

/// The list as the only child of a plain container, focused by a click.
fn focused_list(log: &Log, with_base: bool) -> App {
    let root = list("root", &Rc::default(), false);
    let mut app = App::new(root);
    app.root_mut()
        .children_mut()
        .push(list("old", log, with_base));
    app.layout(SIZE);
    app.on_mouse_down(50.0, 20.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(50.0, 20.0, MouseButton::Left, Modifiers::default());
    assert_eq!(app.focused_path(), Some(&[0usize][..]));
    assert_eq!(*log.borrow(), ["old gained"]);
    log.borrow_mut().clear();
    app
}

fn escape(app: &mut App) {
    app.on_key_down(Key::Escape, Modifiers::default());
}

/// Rebuilt with the new list built while the old one still lives (so the
/// addresses differ): the new list is told, then takes Escape.
#[test]
fn rebuilt_list_is_told_and_takes_escape() {
    for with_base in [false, true] {
        let log: Log = Rc::default();
        let mut app = focused_list(&log, with_base);

        app.root_mut().children_mut()[0] = list("new", &log, with_base);
        app.layout(SIZE);
        escape(&mut app);

        assert_eq!(
            *log.borrow(),
            ["new gained", "new escape"],
            "with_base = {with_base}"
        );
    }
}

/// Rebuilt with the old list dropped first, so the new one may reuse its
/// address: Escape still reaches the new list, and one with a `WidgetBase`
/// is told it has focus first.
#[test]
fn rebuilt_list_at_reused_address_takes_escape() {
    for with_base in [false, true] {
        let log: Log = Rc::default();
        let mut app = focused_list(&log, with_base);

        drop(app.root_mut().children_mut().pop());
        let rebuilt = list("new", &log, with_base);
        app.root_mut().children_mut().push(rebuilt);
        app.layout(SIZE);
        escape(&mut app);

        let log = log.borrow();
        assert_eq!(log.last().map(String::as_str), Some("new escape"));
        assert!(!log.iter().any(|e| e.starts_with("old")), "{log:?}");
        if with_base {
            assert_eq!(*log, ["new gained", "new escape"]);
        }
    }
}

/// Once told, the rebuilt list is the focused widget: focus moving away
/// sends it `FocusLost`.
#[test]
fn focus_leaving_rebuilt_list_sends_it_focus_lost() {
    let log: Log = Rc::default();
    let mut app = focused_list(&log, true);

    app.root_mut().children_mut()[0] = list("new", &log, true);
    app.layout(SIZE);
    crate::focus::request_blur();
    app.layout(SIZE);

    assert_eq!(*log.borrow(), ["new gained", "new lost"]);
    assert!(!app.has_focus());
}
