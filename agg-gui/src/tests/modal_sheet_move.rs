//! Moving a [`ModalSheet`] by its top strip (`ModalSheet::with_drag_strip`),
//! driven by real mouse events through the [`App`]: the panel follows the
//! pointer, a control in the strip still gets its click, presses below the
//! strip do nothing, and the panel stays inside the host as it moves and as
//! the host shrinks, returning to its place when the host grows back.

use super::*;
use crate::widgets::ModalSheet;
use crate::{current_cursor_icon, CursorIcon, DrawCtx, Event, EventResult, Point, Rect, Stack};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Host is 400×400; the 200×100 panel first shows centred at
/// x 100..300, screen y 150..250 (Y-up local y 150..250 as well).
const HOST: f64 = 400.0;
const STRIP: f64 = 30.0;
const CENTRED: Rect = Rect {
    x: 100.0,
    y: 150.0,
    width: 200.0,
    height: 100.0,
};

/// A 20×20 control at the content's top-right, inside the drag strip, that
/// counts its clicks (a press and a release on it), like a dialog's close
/// button.
struct CloseProbe {
    bounds: Rect,
    clicks: Rc<Cell<u32>>,
    pressed: bool,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for CloseProbe {
    fn type_name(&self) -> &'static str {
        "CloseProbe"
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
        Size::new(20.0, 20.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { .. } => self.pressed = true,
            Event::MouseUp { .. } if self.pressed => {
                self.pressed = false;
                self.clicks.set(self.clicks.get() + 1);
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }
}

/// The sheet's content: fills the panel, holds the close probe, and takes
/// every pointer move over it (as a hover-tracking view does) while leaving
/// presses alone, so a strip drag has to claim the moves past it.
struct Content {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Content {
    fn type_name(&self) -> &'static str {
        "Content"
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
    fn layout(&mut self, available: Size) -> Size {
        for child in &mut self.children {
            child.layout(Size::new(20.0, 20.0));
            // Y-up: 5 units in from the top and 10 from the right.
            child.set_bounds(Rect::new(
                available.width - 30.0,
                available.height - 25.0,
                20.0,
                20.0,
            ));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
}

struct Harness {
    app: App,
    clicks: Rc<Cell<u32>>,
    sizes: Rc<RefCell<Vec<Size>>>,
}

fn harness(min: Size) -> Harness {
    let clicks = Rc::new(Cell::new(0));
    let sizes = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&sizes);
    let content = Content {
        bounds: Rect::default(),
        children: vec![Box::new(CloseProbe {
            bounds: Rect::default(),
            clicks: Rc::clone(&clicks),
            pressed: false,
            children: Vec::new(),
        })],
    };
    let sheet = ModalSheet::new(Rc::new(Cell::new(true)), Box::new(content))
        .with_panel_size(Size::new(200.0, 100.0))
        .with_min_panel_size(min)
        .with_resizable(true)
        .with_drag_strip(STRIP)
        .with_on_size_changed(move |s| log.borrow_mut().push(s));
    let stack = Stack::new()
        .add(Box::new(SizedBox::new()))
        .add(Box::new(sheet));
    let mut app = App::new(Box::new(stack));
    app.layout(Size::new(HOST, HOST));
    Harness { app, clicks, sizes }
}

fn sheet(app: &App) -> &ModalSheet {
    app.root().children()[1]
        .as_any()
        .and_then(|a| a.downcast_ref::<ModalSheet>())
        .expect("sheet is the stack's second child")
}

fn sheet_mut(app: &mut App) -> &mut ModalSheet {
    app.root_mut().children_mut()[1]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<ModalSheet>())
        .expect("sheet is the stack's second child")
}

/// Press at `from`, move halfway and then to `to`, release, lay out.
fn drag(app: &mut App, from: (f64, f64), to: (f64, f64), host: f64) {
    app.on_mouse_move(from.0, from.1);
    app.on_mouse_down(from.0, from.1, MouseButton::Left, Modifiers::default());
    app.on_mouse_move((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    app.on_mouse_move(to.0, to.1);
    app.on_mouse_up(to.0, to.1, MouseButton::Left, Modifiers::default());
    app.layout(Size::new(host, host));
}

fn click(app: &mut App, at: (f64, f64)) {
    app.on_mouse_move(at.0, at.1);
    app.on_mouse_down(at.0, at.1, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(at.0, at.1, MouseButton::Left, Modifiers::default());
}

/// Dragging the strip moves the panel and its content with the pointer,
/// shows the closed hand while moving, and is not reported as a resize.
#[test]
fn dragging_the_strip_moves_the_panel() {
    let mut h = harness(Size::ZERO);
    assert_eq!(sheet(&h.app).panel_rect(), CENTRED);
    assert_eq!(sheet(&h.app).panel_position(), None);

    h.app.on_mouse_move(150.0, 165.0);
    h.app
        .on_mouse_down(150.0, 165.0, MouseButton::Left, Modifiers::default());
    h.app.on_mouse_move(130.0, 145.0);
    assert_eq!(current_cursor_icon(), CursorIcon::Grabbing);
    h.app.on_mouse_move(110.0, 125.0);
    h.app
        .on_mouse_up(110.0, 125.0, MouseButton::Left, Modifiers::default());
    h.app.layout(Size::new(HOST, HOST));

    // 40 left and 40 up on screen (Y-down) is 40 up in the Y-up panel rect.
    let moved = Rect::new(60.0, 190.0, 200.0, 100.0);
    assert_eq!(sheet(&h.app).panel_rect(), moved);
    assert_eq!(sheet(&h.app).children()[0].bounds(), moved);
    assert_eq!(
        sheet(&h.app).panel_position(),
        Some(Point::new(60.0, 110.0))
    );
    assert!(h.sizes.borrow().is_empty(), "a move is not a resize");
}

/// A control in the strip keeps its click and the panel stays put; after
/// a move the control is found where the panel took it.
#[test]
fn a_control_in_the_strip_keeps_its_click() {
    let mut h = harness(Size::ZERO);
    click(&mut h.app, (280.0, 165.0));
    assert_eq!(h.clicks.get(), 1);
    assert_eq!(sheet(&h.app).panel_rect(), CENTRED);
    assert_eq!(sheet(&h.app).panel_position(), None);

    drag(&mut h.app, (150.0, 165.0), (100.0, 115.0), HOST);
    click(&mut h.app, (230.0, 115.0));
    assert_eq!(h.clicks.get(), 2);
}

/// Only the strip moves the panel: a drag lower in the panel does nothing.
#[test]
fn a_drag_below_the_strip_does_not_move_the_panel() {
    let mut h = harness(Size::ZERO);
    drag(&mut h.app, (200.0, 220.0), (100.0, 120.0), HOST);
    assert_eq!(sheet(&h.app).panel_rect(), CENTRED);
    assert_eq!(sheet(&h.app).panel_position(), None);
}

/// A sheet that does not opt in to a drag strip does not move.
#[test]
fn sheets_do_not_move_by_default() {
    let plain = ModalSheet::new(Rc::new(Cell::new(true)), Box::new(SizedBox::new()));
    assert_eq!(plain.drag_strip(), 0.0);

    let mut h = harness(Size::ZERO);
    sheet_mut(&mut h.app).set_drag_strip(0.0);
    drag(&mut h.app, (150.0, 165.0), (110.0, 125.0), HOST);
    assert_eq!(sheet(&h.app).panel_rect(), CENTRED);
}

/// The panel cannot be dragged out of the host.
#[test]
fn a_move_stops_at_the_host_edges() {
    let mut h = harness(Size::ZERO);
    drag(&mut h.app, (150.0, 165.0), (-500.0, -500.0), HOST);
    assert_eq!(
        sheet(&h.app).panel_rect(),
        Rect::new(0.0, 300.0, 200.0, 100.0)
    );
    drag(&mut h.app, (50.0, 15.0), (900.0, 900.0), HOST);
    assert_eq!(
        sheet(&h.app).panel_rect(),
        Rect::new(200.0, 0.0, 200.0, 100.0)
    );
}

/// When the host shrinks a moved panel is pulled inside it; when the host
/// grows back the panel returns to where the user put it.
#[test]
fn a_moved_panel_stays_inside_a_shrinking_host() {
    let mut h = harness(Size::ZERO);
    drag(&mut h.app, (150.0, 165.0), (300.0, 300.0), HOST);
    let placed = Rect::new(200.0, 15.0, 200.0, 100.0);
    assert_eq!(sheet(&h.app).panel_rect(), placed);

    h.app.layout(Size::new(300.0, 300.0));
    assert_eq!(
        sheet(&h.app).panel_rect(),
        Rect::new(100.0, 0.0, 200.0, 100.0)
    );
    h.app.layout(Size::new(HOST, HOST));
    assert_eq!(sheet(&h.app).panel_rect(), placed);
}

/// A panel held larger than the host by its minimum size overhangs it
/// centred on that axis, as an unmoved one does.
#[test]
fn a_moved_panel_larger_than_the_host_is_centred() {
    let mut h = harness(Size::new(180.0, 80.0));
    drag(&mut h.app, (150.0, 165.0), (300.0, 300.0), HOST);
    h.app.layout(Size::new(150.0, 300.0));
    assert_eq!(
        sheet(&h.app).panel_rect(),
        Rect::new(-15.0, 0.0, 180.0, 100.0)
    );
}

/// `set_panel_position(None)` centres a moved panel again.
#[test]
fn clearing_the_position_centres_the_panel() {
    let mut h = harness(Size::ZERO);
    drag(&mut h.app, (150.0, 165.0), (110.0, 125.0), HOST);
    assert_ne!(sheet(&h.app).panel_rect(), CENTRED);
    sheet_mut(&mut h.app).set_panel_position(None);
    h.app.layout(Size::new(HOST, HOST));
    assert_eq!(sheet(&h.app).panel_rect(), CENTRED);
}
