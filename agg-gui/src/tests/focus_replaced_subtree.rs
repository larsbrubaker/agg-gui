//! Focus when an app replaces the focused widget with a same-shaped one.
//!
//! `App` stores focus as a child-index path, anchored to the focused widgets'
//! identities (`widget/app/path_anchor.rs`). When the app swaps the focused
//! subtree for a freshly built one of the same shape, the new widget takes
//! focus and is told so on the next resolve (`widget/app/focus_change.rs`).
//! Clicking it keeps it focused and typing reaches it. A swap made while an
//! event is being dispatched leaves, until that resolve, a focus path naming
//! a widget that never had focus: moving focus away then must not send it
//! `FocusLost` (a `TextField` would commit an edit it never started).

use crate::{
    App, DrawCtx, Event, EventResult, Font, Key, Modifiers, MouseButton, Rect, Size, TextField,
    Widget,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const SIZE: Size = Size {
    width: 400.0,
    height: 40.0,
};

type Log = Rc<RefCell<Vec<String>>>;

/// A container laying its children out side by side, 200 px each.
struct Row {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Swapped in for the first child when a key arrives, as an app
    /// rebuilding that child in a key handler would.
    swap_on_press: SwapSlot,
}

type SwapSlot = Rc<RefCell<Option<Box<dyn Widget>>>>;

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
        for (i, child) in self.children.iter_mut().enumerate() {
            child.layout(Size::new(200.0, available.height));
            child.set_bounds(Rect::new(i as f64 * 200.0, 0.0, 200.0, available.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn preview_event(&mut self, event: &Event) -> EventResult {
        if matches!(event, Event::KeyDown { .. }) {
            if let Some(new) = self.swap_on_press.borrow_mut().take() {
                self.children[0] = new;
            }
        }
        EventResult::Ignored
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A focusable leaf logging `"<name> gained"` / `"<name> lost"`.
struct FocusLog {
    name: &'static str,
    bounds: Rect,
    log: Log,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for FocusLog {
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
    fn is_focusable(&self) -> bool {
        true
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        let kind = match event {
            Event::FocusGained => "gained",
            Event::FocusLost => "lost",
            Event::MouseDown { .. } | Event::MouseUp { .. } => return EventResult::Consumed,
            _ => return EventResult::Ignored,
        };
        self.log.borrow_mut().push(format!("{} {kind}", self.name));
        EventResult::Consumed
    }
}

fn focus_log(name: &'static str, log: &Log) -> Box<dyn Widget> {
    Box::new(FocusLog {
        name,
        bounds: Rect::default(),
        log: Rc::clone(log),
        children: Vec::new(),
    })
}

fn field(cell: &Rc<RefCell<String>>) -> Box<dyn Widget> {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    Box::new(TextField::new(font).with_text_cell(Rc::clone(cell)))
}

fn click(app: &mut App, x: f64) {
    let y = SIZE.height * 0.5;
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
}

/// The reported bug: after the focused field is rebuilt, clicking the new
/// field did nothing (same path, so `set_focus` returned early) and typing
/// went nowhere (the new field never got `FocusGained`).
#[test]
fn clicking_replaced_focused_text_field_focuses_it() {
    let old_cell = Rc::new(RefCell::new(String::new()));
    let mut app = App::new(Box::new(Row {
        bounds: Rect::default(),
        children: vec![field(&old_cell)],
        swap_on_press: SwapSlot::default(),
    }));
    app.layout(SIZE);
    click(&mut app, 50.0);
    app.on_key_down(Key::Char('a'), Modifiers::default());
    assert_eq!(old_cell.borrow().as_str(), "a");

    // The app rebuilds the subtree: a new field at the same index path.
    let new_cell = Rc::new(RefCell::new(String::new()));
    app.root_mut().children_mut()[0] = field(&new_cell);
    app.layout(SIZE);

    click(&mut app, 50.0);
    for c in "xy".chars() {
        app.on_key_down(Key::Char(c), Modifiers::default());
    }
    assert_eq!(new_cell.borrow().as_str(), "xy");
    assert_eq!(old_cell.borrow().as_str(), "a");
}

/// The key that moves focus away (Tab) also rebuilds the focused widget
/// (the parent swaps it while the key is dispatched): `FocusLost` goes to
/// nobody, because the widget that had focus is gone and its replacement
/// never gained it.
#[test]
fn focus_moving_off_widget_replaced_by_same_key_sends_no_focus_lost() {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let swap = SwapSlot::default();
    let mut app = App::new(Box::new(Row {
        bounds: Rect::default(),
        children: vec![focus_log("old", &log), focus_log("other", &log)],
        swap_on_press: Rc::clone(&swap),
    }));
    app.layout(SIZE);
    click(&mut app, 50.0);
    assert_eq!(*log.borrow(), ["old gained"]);
    log.borrow_mut().clear();

    *swap.borrow_mut() = Some(focus_log("new", &log));
    app.on_key_down(Key::Tab, Modifiers::default());
    assert_eq!(app.focused_path(), Some(&[1usize][..]));
    assert_eq!(*log.borrow(), ["other gained"]);
}

/// A widget rebuilt between events is told it has focus at the next resolve,
/// so focus moving away from it later sends it `FocusLost`.
#[test]
fn focus_moving_off_told_replacement_sends_it_focus_lost() {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let mut app = App::new(Box::new(Row {
        bounds: Rect::default(),
        children: vec![focus_log("old", &log), focus_log("other", &log)],
        swap_on_press: SwapSlot::default(),
    }));
    app.layout(SIZE);
    click(&mut app, 50.0);

    app.root_mut().children_mut()[0] = focus_log("new", &log);
    app.layout(SIZE);
    click(&mut app, 250.0);
    assert_eq!(
        *log.borrow(),
        ["old gained", "new gained", "new lost", "other gained"]
    );
}
