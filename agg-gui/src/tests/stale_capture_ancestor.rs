//! A press whose widget is rebuilt away mid-drag keeps its pointer capture on
//! the nearest ancestor that survived.
//!
//! A list rebuilt during a press (an app opening another folder mid-drag)
//! drops the pressed tile; the capture path then indexes past the shorter
//! list. `widget/app/path_anchor.rs` keeps the capture on the list, the
//! deepest widget along the path that is still the one the press went
//! through, so the remaining moves, the release and a later
//! `MouseCaptureLost` reach code that saw the press start, wherever the
//! pointer is. A tile that survives the rebuild at another index still gets
//! them itself. A two-finger gesture captured the same way follows the same
//! rule.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Modifiers, MouseButton, Rect, Size,
    TouchDeviceId, TouchId, Widget,
};

type Log = Rc<RefCell<Vec<String>>>;

const TILE_WIDTH: f64 = 50.0;
const SIZE: Size = Size {
    width: 400.0,
    height: 100.0,
};
/// A point over the root but outside the list (the list spans x 0..200).
const OUTSIDE_X: f64 = 300.0;
const Y: f64 = 50.0;

/// Log `"<name> <kind>"` for the pointer events this file follows.
fn log_event(log: &Log, name: &str, event: &Event) {
    let kind = match event {
        Event::MouseDown { .. } => "down",
        Event::MouseUp { .. } => "up",
        Event::MouseMove { pos } if pos.x < 0.0 => "clear",
        Event::MouseMove { .. } => "move",
        Event::MouseCaptureLost => "lost",
        Event::MultiTouch { .. } => "touch",
        _ => return,
    };
    log.borrow_mut().push(format!("{name} {kind}"));
}

/// A tile that consumes the pointer events it is sent (so a press on it takes
/// the capture), logging each one.
struct Tile {
    name: String,
    bounds: Rect,
    log: Log,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Tile {
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
        log_event(&self.log, &self.name, event);
        EventResult::Consumed
    }
}

/// A row of tiles, `TILE_WIDTH` each from x 0, inside a 200 px wide list.
/// It logs the events that reach it (by bubbling or by capture) and handles
/// none of them.
struct List {
    bounds: Rect,
    log: Log,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for List {
    fn type_name(&self) -> &'static str {
        "List"
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
        for (i, child) in self.children.iter_mut().enumerate() {
            child.set_bounds(Rect::new(
                i as f64 * TILE_WIDTH,
                0.0,
                TILE_WIDTH,
                available.height,
            ));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        log_event(&self.log, "list", event);
        EventResult::Ignored
    }
}

/// The root: the list on the left half, empty space on the right.
struct Root {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Root {
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
        let list_size = Size::new(200.0, available.height);
        for child in &mut self.children {
            child.layout(list_size);
            child.set_bounds(Rect::new(0.0, 0.0, list_size.width, list_size.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn tile(name: &str, log: &Log) -> Box<dyn Widget> {
    Box::new(Tile {
        name: name.to_string(),
        bounds: Rect::default(),
        log: Rc::clone(log),
        children: Vec::new(),
    })
}

/// An app whose list holds tiles "t0".."t3".
fn list_app() -> (App, Log) {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let list = List {
        bounds: Rect::default(),
        log: Rc::clone(&log),
        children: (0..4).map(|i| tile(&format!("t{i}"), &log)).collect(),
    };
    let mut app = App::new(Box::new(Root {
        bounds: Rect::default(),
        children: vec![Box::new(list)],
    }));
    app.layout(SIZE);
    (app, log)
}

fn list_children(app: &mut App) -> &mut Vec<Box<dyn Widget>> {
    app.root_mut().children_mut()[0].children_mut()
}

fn none() -> Modifiers {
    Modifiers::default()
}

/// Press tile "t3" (index 3), the list then rebuilt with two new tiles.
fn press_last_tile_then_rebuild(app: &mut App, log: &Log) {
    app.on_mouse_down(3.5 * TILE_WIDTH, Y, MouseButton::Left, none());
    assert_eq!(app.captured_path(), Some(&[0usize, 3][..]));
    let fresh = vec![tile("n0", log), tile("n1", log)];
    *list_children(app) = fresh;
    app.layout(SIZE);
    log.borrow_mut().clear();
}

#[test]
fn list_keeps_capture_when_pressed_tile_is_rebuilt_away() {
    let (mut app, log) = list_app();
    press_last_tile_then_rebuild(&mut app, &log);
    assert_eq!(app.captured_path(), Some(&[0usize][..]));

    app.on_mouse_move(OUTSIDE_X, Y);
    app.on_mouse_move(OUTSIDE_X + 10.0, Y);
    app.on_mouse_up(OUTSIDE_X + 10.0, Y, MouseButton::Left, none());
    assert_eq!(*log.borrow(), ["list move", "list move", "list up"]);
    assert!(!app.has_captured_pointer());
}

/// The list rebuilt between two events with no layout in between: the move
/// itself re-resolves the capture.
#[test]
fn list_keeps_capture_when_rebuilt_between_events() {
    let (mut app, log) = list_app();
    app.on_mouse_down(2.5 * TILE_WIDTH, Y, MouseButton::Left, none());
    list_children(&mut app).truncate(1);
    log.borrow_mut().clear();

    app.on_mouse_move(OUTSIDE_X, Y);
    app.on_mouse_up(OUTSIDE_X, Y, MouseButton::Left, none());
    assert_eq!(*log.borrow(), ["list move", "list up"]);
}

/// A capture cancelled after the tile was rebuilt away (the window losing
/// activation) tells the list, which saw the press start.
#[test]
fn list_hears_capture_lost_after_pressed_tile_is_rebuilt_away() {
    let (mut app, log) = list_app();
    app.on_window_activated();
    press_last_tile_then_rebuild(&mut app, &log);

    app.on_window_deactivated();
    assert_eq!(*log.borrow(), ["list lost"]);
    assert!(!app.has_captured_pointer());
}

/// The pressed tile kept by the rebuild at a lower index still gets the
/// rest of its press itself, not the list.
#[test]
fn pressed_tile_kept_at_another_index_keeps_capture() {
    let (mut app, log) = list_app();
    app.on_mouse_down(3.5 * TILE_WIDTH, Y, MouseButton::Left, none());
    let pressed = list_children(&mut app).remove(3);
    let fresh = tile("n0", &log);
    *list_children(&mut app) = vec![fresh, pressed];
    app.layout(SIZE);
    log.borrow_mut().clear();
    assert_eq!(app.captured_path(), Some(&[0usize, 1][..]));

    app.on_mouse_move(OUTSIDE_X, Y);
    app.on_mouse_up(OUTSIDE_X, Y, MouseButton::Left, none());
    // A tile consumes what it is sent, so nothing bubbles to the list.
    assert_eq!(*log.borrow(), ["t3 move", "t3 up"]);
}

/// One headless paint: where the multi-touch aggregate is routed.
fn paint(app: &mut App) {
    let mut fb = Framebuffer::new(SIZE.width as u32, SIZE.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// Two fingers 40 px apart, centred on `x`.
fn two_fingers(app: &mut App, x: f64, start: bool) {
    let dev = TouchDeviceId(0);
    for (id, dx) in [(0, -20.0), (1, 20.0)] {
        if start {
            app.on_touch_start(dev, TouchId(id), x + dx, Y, None);
        } else {
            app.on_touch_move(dev, TouchId(id), x + dx, Y, None);
        }
    }
}

/// A gesture started on tile "t1", whose tile is then rebuilt away, keeps
/// arriving at the list as the fingers move off it.
#[test]
fn list_keeps_gesture_when_its_tile_is_rebuilt_away() {
    let (mut app, log) = list_app();
    two_fingers(&mut app, 1.5 * TILE_WIDTH, true);
    paint(&mut app);
    assert!(log.borrow().iter().any(|e| e == "t1 touch"));
    list_children(&mut app).truncate(1);
    app.layout(SIZE);
    log.borrow_mut().clear();

    two_fingers(&mut app, OUTSIDE_X, false);
    paint(&mut app);
    let touches: Vec<String> = log
        .borrow()
        .iter()
        .filter(|e| e.ends_with(" touch"))
        .cloned()
        .collect();
    assert_eq!(touches, ["list touch"]);
}
