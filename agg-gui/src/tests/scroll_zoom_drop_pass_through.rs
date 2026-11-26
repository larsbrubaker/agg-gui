//! A layer that takes presses but lets scrolling, zooming and file drops
//! through: with `Widget::passes_scroll_zoom_and_drops`, a wheel turn, a
//! trackpad pinch (a marked wheel), a two-finger touch gesture, a file drag
//! and a file drop over the top layer reach the sibling layer beneath it,
//! inside an active modal too, while a press, a hover or a one-finger touch
//! over it still lands on it. Without the opt-in the top layer takes them
//! all, as every widget that hits does.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::draw_ctx::DrawCtx;
use crate::event::{DroppedFileData, Event, EventResult, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::touch_state::{TouchDeviceId, TouchId};
use crate::widget::{App, Widget};

/// Which layer took which event, in order.
type Log = Rc<RefCell<Vec<(&'static str, &'static str)>>>;

/// A full-window layer that consumes and logs presses, wheels, two-finger
/// gestures, file drags and drops.
struct Layer {
    name: &'static str,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    passes: bool,
    log: Log,
}

impl Widget for Layer {
    fn type_name(&self) -> &'static str {
        "Layer"
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
        available
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        let kind = match event {
            Event::MouseDown { .. } => "press",
            // A move outside (the hover-clear one sent to the widget the
            // pointer left) is not a hover.
            Event::MouseMove { pos } if pos.x >= 0.0 && pos.y >= 0.0 => "move",
            Event::MouseWheel { .. } => "wheel",
            Event::MultiTouch { .. } => "gesture",
            Event::FileDragHover { .. } => "drag hover",
            Event::FileDropped { .. } => "drop",
            Event::FileDataDropped { .. } => "data drop",
            _ => return EventResult::Ignored,
        };
        self.log.borrow_mut().push((self.name, kind));
        EventResult::Consumed
    }
    fn passes_scroll_zoom_and_drops(&self) -> bool {
        self.passes
    }
}

/// Lays every child over its whole area; the root of the window, or with
/// `modal` an active modal layer that takes the pointer from all beneath it.
struct Root {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    modal: bool,
}

impl Widget for Root {
    fn type_name(&self) -> &'static str {
        "Root"
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
            child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        }
        available
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn has_active_modal(&self) -> bool {
        self.modal
    }
}

/// A full-window layer named `name` logging into `log`.
fn layer(log: &Log, name: &'static str, passes: bool) -> Box<dyn Widget> {
    Box::new(Layer {
        name,
        bounds: Rect::default(),
        children: Vec::new(),
        passes,
        log: log.clone(),
    })
}

/// A 200 × 200 window with a `bottom` layer under a `top` one, the top one
/// passing scrolling, zooming and drops when `top_passes`.
fn two_layers(top_passes: bool) -> (App, Log) {
    let log: Log = Rc::default();
    let root = Root {
        bounds: Rect::default(),
        children: vec![layer(&log, "bottom", false), layer(&log, "top", top_passes)],
        modal: false,
    };
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(200.0, 200.0));
    (app, log)
}

fn taken(log: &Log) -> Vec<(&'static str, &'static str)> {
    std::mem::take(&mut *log.borrow_mut())
}

#[test]
fn a_wheel_over_a_passing_layer_reaches_the_layer_beneath() {
    let (mut app, log) = two_layers(true);
    app.on_mouse_wheel(100.0, 100.0, 1.0);
    assert_eq!(taken(&log), vec![("bottom", "wheel")]);

    // A trackpad pinch arrives as a marked wheel and passes the same way.
    app.on_trackpad_pinch(100.0, 100.0, 0.1, Modifiers::default());
    assert_eq!(taken(&log), vec![("bottom", "wheel")]);
}

#[test]
fn a_press_over_a_passing_layer_still_lands_on_it() {
    let (mut app, log) = two_layers(true);
    app.on_mouse_down(100.0, 100.0, MouseButton::Left, Modifiers::default());
    assert_eq!(taken(&log), vec![("top", "press")]);
}

#[test]
fn a_file_drop_over_a_passing_layer_reaches_the_layer_beneath() {
    let (mut app, log) = two_layers(true);
    app.on_file_dropped(100.0, 100.0, vec![PathBuf::from("part.stl")]);
    assert_eq!(taken(&log), vec![("bottom", "drop")]);

    let file = DroppedFileData {
        name: "part.stl".into(),
        bytes: std::sync::Arc::from(&b"solid"[..]),
    };
    app.on_file_data_dropped(100.0, 100.0, vec![file]);
    assert_eq!(taken(&log), vec![("bottom", "data drop")]);
}

#[test]
fn without_the_opt_in_the_top_layer_takes_the_wheel_and_drops() {
    let (mut app, log) = two_layers(false);
    app.on_mouse_wheel(100.0, 100.0, 1.0);
    app.on_file_dropped(100.0, 100.0, vec![PathBuf::from("part.stl")]);
    app.on_mouse_down(100.0, 100.0, MouseButton::Left, Modifiers::default());
    assert_eq!(
        taken(&log),
        vec![("top", "wheel"), ("top", "drop"), ("top", "press")]
    );
}

#[test]
fn a_file_drag_over_a_passing_layer_reaches_the_layer_beneath() {
    let (mut app, log) = two_layers(true);
    app.on_file_drag_hover(100.0, 100.0, vec![PathBuf::from("part.stl")]);
    assert_eq!(taken(&log), vec![("bottom", "drag hover")]);

    let (mut app, log) = two_layers(false);
    app.on_file_drag_hover(100.0, 100.0, vec![PathBuf::from("part.stl")]);
    assert_eq!(taken(&log), vec![("top", "drag hover")]);
}

/// One headless frame: the touch recogniser publishes its two-finger
/// aggregate and the App routes it as `Event::MultiTouch`.
fn paint(app: &mut App) {
    let mut fb = Framebuffer::new(200, 200);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// The layers that took a two-finger gesture (the touches' emulated mouse
/// events left out).
fn gestures(log: &Log) -> Vec<&'static str> {
    taken(log)
        .into_iter()
        .filter(|(_, kind)| *kind == "gesture")
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn a_two_finger_gesture_starting_over_a_passing_layer_reaches_the_layer_beneath() {
    for (top_passes, taker) in [(true, "bottom"), (false, "top")] {
        let (mut app, log) = two_layers(top_passes);
        app.on_touch_start(TouchDeviceId(0), TouchId(0), 80.0, 100.0, None);
        app.on_touch_start(TouchDeviceId(0), TouchId(1), 120.0, 100.0, None);
        paint(&mut app);
        assert_eq!(gestures(&log), vec![taker], "top passes: {top_passes}");
    }
}

#[test]
fn a_one_finger_touch_on_a_passing_layer_is_still_a_press_on_it() {
    let (mut app, log) = two_layers(true);
    app.on_touch_start(TouchDeviceId(0), TouchId(0), 100.0, 100.0, None);
    app.on_touch_end(TouchDeviceId(0), TouchId(0));
    let presses: Vec<_> = taken(&log)
        .into_iter()
        .filter(|(_, kind)| *kind == "press")
        .collect();
    assert_eq!(presses, vec![("top", "press")]);
}

/// A 200 × 200 window with an `outside` layer under an active modal that
/// holds `bottom` under an opted-in `top`.
fn layers_in_a_modal() -> (App, Log) {
    let log: Log = Rc::default();
    let modal = Root {
        bounds: Rect::default(),
        children: vec![layer(&log, "bottom", false), layer(&log, "top", true)],
        modal: true,
    };
    let root = Root {
        bounds: Rect::default(),
        children: vec![layer(&log, "outside", false), Box::new(modal)],
        modal: false,
    };
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(200.0, 200.0));
    (app, log)
}

#[test]
fn inside_an_active_modal_the_wheel_passes_to_the_layer_beneath() {
    let (mut app, log) = layers_in_a_modal();
    app.on_mouse_wheel(100.0, 100.0, 1.0);
    assert_eq!(taken(&log), vec![("bottom", "wheel")]);
    app.on_mouse_down(100.0, 100.0, MouseButton::Left, Modifiers::default());
    assert_eq!(taken(&log), vec![("top", "press")]);
}

#[test]
fn inside_an_active_modal_a_move_over_a_passing_layer_hovers_it() {
    let (mut app, log) = layers_in_a_modal();
    app.on_mouse_move(100.0, 100.0);
    assert_eq!(taken(&log), vec![("top", "move")]);
}
