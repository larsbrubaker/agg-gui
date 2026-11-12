//! The `(-1, -1)` hover-clear move (`widget/app/hover_chain.rs`) reaches the
//! widget the pointer left on every route: a move inside a showing
//! [`ModalSheet`] and a release that re-resolves hover there, as the ordinary
//! route always has (`App::dispatch_pointer_move`). A widget that clears its
//! hover only from a move outside itself, and ignores `MouseLeave`, rests
//! again. While a drag holds the capture, no clear goes along its branch (the
//! captured widget, an ancestor of it or a widget inside it), on either
//! route: the clear bubbles, so the drag's owner would take it for a move to
//! `(-1, -1)`.

use super::*;
use crate::widgets::ModalSheet;
use crate::{DrawCtx, Event, EventResult, Point, Rect, Stack};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const VIEW: Size = Size {
    width: 400.0,
    height: 400.0,
};

/// Hovered while the last move it heard was over it: it hears no
/// `MouseLeave`, so only a move outside it (the hover-clear among them)
/// lets go. It takes a press, so it holds the capture until the release.
struct HoverLeaf {
    name: &'static str,
    bounds: Rect,
    hovered: Rc<Cell<bool>>,
}

impl Widget for HoverLeaf {
    fn type_name(&self) -> &'static str {
        "HoverLeaf"
    }
    fn id(&self) -> Option<&str> {
        Some(self.name)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &[]
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        unreachable!("HoverLeaf has no children")
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let inside = pos.x >= 0.0
                    && pos.y >= 0.0
                    && pos.x <= self.bounds.width
                    && pos.y <= self.bounds.height;
                self.hovered.set(inside);
                EventResult::Ignored
            }
            Event::MouseDown { .. } | Event::MouseUp { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
}

fn leaf(name: &'static str, at: Rect, hovered: &Rc<Cell<bool>>) -> Box<dyn Widget> {
    Box::new(HoverLeaf {
        name,
        bounds: at,
        hovered: Rc::clone(hovered),
    })
}

type Log = Rc<RefCell<Vec<Point>>>;

/// A panel 200 x 120 holding its children where they are placed. With a
/// `log` it is a drag owner: it takes presses, so it holds the capture, and
/// logs every move it hears.
struct Panel {
    name: &'static str,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    log: Option<Log>,
}

impl Widget for Panel {
    fn type_name(&self) -> &'static str {
        "Panel"
    }
    fn id(&self) -> Option<&str> {
        Some(self.name)
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
        for child in &mut self.children {
            let at = child.bounds();
            child.layout(Size::new(at.width, at.height));
        }
        Size::new(200.0, 120.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        let Some(log) = &self.log else {
            return EventResult::Ignored;
        };
        match event {
            Event::MouseMove { pos } => {
                log.borrow_mut().push(*pos);
                EventResult::Consumed
            }
            Event::MouseDown { .. } | Event::MouseUp { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
}

/// `panel` in a showing sheet, or (`modal` false) on its own in the window.
fn app_with(panel: Panel, modal: bool) -> App {
    let root: Box<dyn Widget> = if modal {
        let sheet = ModalSheet::new(Rc::new(Cell::new(true)), Box::new(panel))
            .with_panel_size(Size::new(300.0, 200.0));
        Box::new(
            Stack::new()
                .add(Box::new(SizedBox::new()))
                .add(Box::new(sheet)),
        )
    } else {
        Box::new(Stack::new().add(Box::new(panel)))
    };
    let mut app = App::new(root);
    app.layout(VIEW);
    app
}

/// The Y-down screen point `(dx, dy)` (Y-up) from the bottom-left of the
/// widget named `id`.
fn at(app: &App, id: &str, dx: f64, dy: f64) -> (f64, f64) {
    let r = crate::widget::find_widget_screen_rect(app.root(), id)
        .unwrap_or_else(|| panic!("{id} is laid out"));
    (r.x + dx, VIEW.height - (r.y + dy))
}

/// Two leaves side by side on a plain panel in a sheet.
fn two_leaves() -> (App, Rc<Cell<bool>>, Rc<Cell<bool>>) {
    let (a, b) = (Rc::new(Cell::new(false)), Rc::new(Cell::new(false)));
    let panel = Panel {
        name: "Panel",
        bounds: Rect::default(),
        children: vec![
            leaf("A", Rect::new(10.0, 40.0, 40.0, 40.0), &a),
            leaf("B", Rect::new(100.0, 40.0, 40.0, 40.0), &b),
        ],
        log: None,
    };
    (app_with(panel, true), a, b)
}

#[test]
fn a_move_in_a_sheet_clears_the_hover_of_the_widget_it_left() {
    let (mut app, a, b) = two_leaves();
    for round in 1..=2 {
        let over_a = at(&app, "A", 20.0, 20.0);
        app.on_mouse_move(over_a.0, over_a.1);
        assert!(a.get(), "round {round}: over A");
        let over_b = at(&app, "B", 20.0, 20.0);
        app.on_mouse_move(over_b.0, over_b.1);
        assert!(!a.get(), "round {round}: the move to B cleared A");
        assert!(b.get(), "round {round}: over B");
        let empty = at(&app, "Panel", 180.0, 100.0);
        app.on_mouse_move(empty.0, empty.1);
        assert!(!b.get(), "round {round}: the move off B cleared B");
    }
}

#[test]
fn a_release_in_a_sheet_clears_the_hover_of_the_widget_it_left() {
    let (mut app, a, b) = two_leaves();
    let over_a = at(&app, "A", 20.0, 20.0);
    app.on_mouse_move(over_a.0, over_a.1);
    app.on_mouse_down(over_a.0, over_a.1, MouseButton::Left, Modifiers::default());
    assert!(app.has_captured_pointer(), "A holds the press");
    // A release reported somewhere the pointer was never seen moving to, as
    // a touch lifting off: the release re-resolves hover there.
    let over_b = at(&app, "B", 20.0, 20.0);
    app.on_mouse_up(over_b.0, over_b.1, MouseButton::Left, Modifiers::default());
    assert!(!a.get(), "the release over B cleared A");
    assert!(b.get(), "B is hovered after the release");
}

/// A drag owner holding a leaf: press on the owner, cross the leaf while the
/// drag holds, come back and let go. Every move the owner heard.
fn drag_across_the_leaf(modal: bool) -> Vec<Point> {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let hovered = Rc::new(Cell::new(false));
    let panel = Panel {
        name: "Dragger",
        bounds: Rect::default(),
        children: vec![leaf("Leaf", Rect::new(100.0, 40.0, 40.0, 40.0), &hovered)],
        log: Some(Rc::clone(&log)),
    };
    let mut app = app_with(panel, modal);
    let start = at(&app, "Dragger", 20.0, 20.0);
    let on_leaf = at(&app, "Leaf", 20.0, 20.0);
    let back = at(&app, "Dragger", 30.0, 20.0);
    app.on_mouse_move(start.0, start.1);
    log.borrow_mut().clear();
    app.on_mouse_down(start.0, start.1, MouseButton::Left, Modifiers::default());
    assert!(app.has_captured_pointer(), "the owner holds the drag");
    app.on_mouse_move(on_leaf.0, on_leaf.1);
    app.on_mouse_move(back.0, back.1);
    app.on_mouse_up(back.0, back.1, MouseButton::Left, Modifiers::default());
    let moves = log.borrow().clone();
    moves
}

fn assert_only_real_positions(moves: &[Point], route: &str) {
    assert!(!moves.is_empty(), "{route}: the owner heard the drag");
    assert!(
        moves.iter().all(|p| p.x >= 0.0 && p.y >= 0.0),
        "{route}: a hover-clear reached the drag's owner: {moves:?}"
    );
}

#[test]
fn a_drag_in_a_sheet_hears_no_hover_clear_from_the_widget_inside_it() {
    assert_only_real_positions(&drag_across_the_leaf(true), "in a sheet");
}

#[test]
fn a_drag_hears_no_hover_clear_from_the_widget_inside_it() {
    assert_only_real_positions(&drag_across_the_leaf(false), "no sheet");
}
