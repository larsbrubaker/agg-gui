//! A widget that takes mouse capture on `MouseDown` receives every
//! `MouseMove` until `MouseUp`, wherever the pointer is, in its own local
//! coordinates (agg-sharp's `GuiWidget` mouse capture). A slider thumb
//! dragged past either end must keep tracking so it pins to min/max, also
//! when the slider sits in a `ScrollView` inside a showing `ModalSheet`.

use super::*;
use crate::widgets::ModalSheet;
use crate::{DrawCtx, Event, EventResult, Point, Rect, ScrollView, Stack, Widget};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type Log = Rc<RefCell<Vec<(&'static str, Point)>>>;

const HOST: f64 = 400.0;
const PROBE_HEIGHT: f64 = 40.0;

/// A leaf as wide as offered and `PROBE_HEIGHT` tall that consumes presses,
/// moves and releases, logging each with its local position.
struct Probe {
    bounds: Rect,
    log: Log,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Probe {
    fn type_name(&self) -> &'static str {
        "Probe"
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
        Size::new(available.width, PROBE_HEIGHT)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        let entry = match event {
            Event::MouseDown { pos, .. } => ("down", *pos),
            Event::MouseUp { pos, .. } => ("up", *pos),
            Event::MouseMove { pos } => ("move", *pos),
            _ => return EventResult::Ignored,
        };
        self.log.borrow_mut().push(entry);
        EventResult::Consumed
    }
}

fn probe(log: &Log) -> Box<dyn Widget> {
    Box::new(Probe {
        bounds: Rect::default(),
        log: Rc::clone(log),
        children: Vec::new(),
    })
}

/// Press at `down`, move through every point of `moves`, release at the
/// last one; returns the log from the press on. Screen coordinates are
/// Y-down, so a local Y moves opposite to the screen Y.
fn drag(app: &mut App, log: &Log, down: (f64, f64), moves: &[(f64, f64)]) {
    app.on_mouse_move(down.0, down.1);
    log.borrow_mut().clear();
    app.on_mouse_down(down.0, down.1, MouseButton::Left, Modifiers::default());
    assert!(app.has_captured_pointer(), "the probe takes the capture");
    for &(x, y) in moves {
        app.on_mouse_move(x, y);
    }
    let last = *moves.last().expect("at least one move");
    app.on_mouse_up(last.0, last.1, MouseButton::Left, Modifiers::default());
}

/// Every move after the press reached the probe, at the press's local
/// position shifted by the pointer's screen travel, and some of them lay
/// outside the probe's own bounds.
fn assert_every_move_reached(log: &Log, down: (f64, f64), moves: &[(f64, f64)]) {
    let log = log.borrow();
    assert_eq!(log.first().map(|e| e.0), Some("down"), "log: {log:?}");
    let origin = log[0].1;
    let local = |(x, y): (f64, f64)| Point::new(origin.x + (x - down.0), origin.y - (y - down.1));
    let mut expected = vec![("down", origin)];
    expected.extend(moves.iter().map(|&m| ("move", local(m))));
    let last = *moves.last().expect("at least one move");
    expected.push(("up", local(last)));
    assert_eq!(*log, expected);
    let outside = expected
        .iter()
        .any(|(_, p)| p.x < 0.0 || p.y < 0.0 || p.y > PROBE_HEIGHT);
    assert!(outside, "the drag leaves the probe's bounds");
}

/// Moves that leave the probe sideways, above and below, and the window.
const MOVES: [(f64, f64); 5] = [
    (210.0, 162.0),
    (5.0, 162.0),
    (395.0, 162.0),
    (200.0, 5.0),
    (200.0, 395.0),
];

#[test]
fn captured_widget_gets_out_of_bounds_moves_without_wrappers() {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let stack = Stack::new().add(probe(&log));
    let mut app = App::new(Box::new(stack));
    app.layout(Size::new(HOST, HOST));
    // The probe fills the width; in a Y-up stack it may sit at either end,
    // so press inside it wherever it was placed.
    let b = app.root().children()[0].bounds();
    let down = (200.0, HOST - (b.y + b.height / 2.0));
    let moves = MOVES.map(|(x, y)| (x, y - 162.0 + down.1));
    drag(&mut app, &log, down, &moves);
    assert_every_move_reached(&log, down, &moves);
}

#[test]
fn captured_widget_in_scroll_view_in_modal_sheet_gets_out_of_bounds_moves() {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let visible = Rc::new(Cell::new(true));
    let sheet = ModalSheet::new(visible, Box::new(ScrollView::new(probe(&log))))
        .with_panel_size(Size::new(200.0, 100.0));
    let stack = Stack::new()
        .add(Box::new(SizedBox::new()))
        .add(Box::new(sheet));
    let mut app = App::new(Box::new(stack));
    app.layout(Size::new(HOST, HOST));
    // The panel spans screen x 100..300 and y 150..250; the probe sits at
    // the top of the scroll view, so (200, 160) presses it.
    let down = (200.0, 160.0);
    drag(&mut app, &log, down, &MOVES);
    assert_every_move_reached(&log, down, &MOVES);
}

#[test]
fn captured_widget_in_scroll_view_gets_out_of_bounds_moves() {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let stack = Stack::new().add(Box::new(ScrollView::new(probe(&log))));
    let mut app = App::new(Box::new(stack));
    app.layout(Size::new(HOST, HOST));
    // The probe sits at the top of the full-window scroll view.
    let down = (200.0, 20.0);
    let moves = MOVES.map(|(x, y)| (x, y - 162.0 + down.1));
    drag(&mut app, &log, down, &moves);
    assert_every_move_reached(&log, down, &moves);
}
