//! agg-sharp `HoverCursorTests`, 1:1: a widget whose cursor depends on
//! where the mouse is inside it (a resize bar along one edge, a tab's close
//! button), or that changes its cursor by code, must get that cursor to the
//! window, not only when the mouse first enters it.
//!
//! "The window" is [`crate::WindowCursor`], the dedupe the platform shells
//! put in front of the OS: `Harness::sync` does what a shell does after each
//! event and paint (`App::refresh_cursor`, which `App::paint` runs, then
//! `WindowCursor::update` with [`crate::current_cursor_icon`]) and records
//! each cursor that would reach the platform. C#'s per-widget `Cursor`
//! property maps to a widget that claims a cursor from its own `MouseMove`
//! and calls [`crate::invalidate_cursor`] when code changes it. C#'s
//! `Cursors` map as `VSplit` -> `ResizeHorizontal`, `Cross` -> `Crosshair`,
//! `Hand` -> `PointingHand`, `No` -> `NotAllowed`. C# coordinates are Y-up
//! in a 200x200 root; the App's screen is Y-down, so `screen_y = 200 - y`.
//!
//! `ReleasingACapturedDragOverAnotherWidgetShowsThatWidgetsCursor` is
//! covered by `widget_cursors.rs::splitter_release_off_bar_restores_default_cursor_without_move`.

use super::*;
use crate::{
    current_cursor_icon, invalidate_cursor, set_cursor_icon, CursorIcon, DrawCtx, Event,
    EventResult, Rect, Widget, WindowCursor,
};
use std::cell::Cell;
use std::rc::Rc;

const ROOT: f64 = 200.0;

/// How a [`CursorWidget`] picks its cursor on each move it receives.
enum Mode {
    /// The split cursor over its leftmost 10 units (C#'s `EdgeBarWidget`,
    /// the shape of VerticalResizeContainer's bar; also `SetAfterMoveWidget`,
    /// which decides after its move handling).
    EdgeBar,
    /// Whatever code last stored (C#'s `GuiWidget.Cursor` set by code).
    Fixed(Rc<Cell<CursorIcon>>),
}

/// A leaf at a fixed rectangle (Y-up, in its parent) that takes the mouse
/// capture on press, as every C# `GuiWidget` does.
struct CursorWidget {
    bounds: Rect,
    mode: Mode,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for CursorWidget {
    fn type_name(&self) -> &'static str {
        "CursorWidget"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, _b: Rect) {}
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let icon = match &self.mode {
                    Mode::EdgeBar if pos.x < 10.0 => CursorIcon::ResizeHorizontal,
                    Mode::EdgeBar => CursorIcon::Default,
                    Mode::Fixed(cell) => cell.get(),
                };
                set_cursor_icon(icon);
                EventResult::Consumed
            }
            Event::MouseDown { .. } | Event::MouseUp { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
}

/// The 200x200 root; it places its children at their own rectangles and
/// claims no cursor itself.
struct Root {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
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
        for c in &mut self.children {
            let b = c.bounds();
            c.layout(Size::new(b.width, b.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn widget(x: f64, y: f64, w: f64, h: f64, mode: Mode) -> Box<dyn Widget> {
    Box::new(CursorWidget {
        bounds: Rect::new(x, y, w, h),
        mode,
        children: Vec::new(),
    })
}

/// The app plus "the window": every cursor that reached the platform.
struct Harness {
    app: App,
    window: WindowCursor,
    applied: Vec<CursorIcon>,
}

impl Harness {
    fn new(children: Vec<Box<dyn Widget>>) -> Self {
        crate::cursor::reset_cursor_icon();
        crate::cursor::take_cursor_invalidation();
        let root = Root {
            bounds: Rect::new(0.0, 0.0, ROOT, ROOT),
            children,
        };
        let mut app = App::new(Box::new(root));
        app.layout(Size::new(ROOT, ROOT));
        Self {
            app,
            window: WindowCursor::new(),
            applied: Vec::new(),
        }
    }

    /// What a shell does after each event and each paint.
    fn sync(&mut self) {
        self.app.refresh_cursor();
        if let Some(icon) = self.window.update(current_cursor_icon()) {
            self.applied.push(icon);
        }
    }

    fn mouse_move(&mut self, x: f64, y: f64) {
        self.app.on_mouse_move(x, ROOT - y);
        self.sync();
    }

    fn press(&mut self, x: f64, y: f64) {
        self.app
            .on_mouse_down(x, ROOT - y, MouseButton::Left, Modifiers::default());
        self.sync();
    }

    fn release(&mut self, x: f64, y: f64) {
        self.app
            .on_mouse_up(x, ROOT - y, MouseButton::Left, Modifiers::default());
        self.sync();
    }

    fn last(&self) -> CursorIcon {
        *self.applied.last().expect("a cursor was applied")
    }
}

/// Code changes a widget's cursor (C#'s `widget.Cursor = ...`).
fn set_by_code(h: &mut Harness, cell: &Rc<Cell<CursorIcon>>, icon: CursorIcon) {
    cell.set(icon);
    invalidate_cursor();
    h.sync();
}

#[test]
fn cursor_change_while_hovering_one_widget_reaches_the_window() {
    let mut h = Harness::new(vec![widget(0.0, 0.0, 100.0, 100.0, Mode::EdgeBar)]);

    // enter the panel away from its bar
    h.mouse_move(50.0, 50.0);
    assert_eq!(h.last(), CursorIcon::Default);
    let count_after_enter = h.applied.len();

    // moving without a cursor change must not call the platform again
    h.mouse_move(60.0, 50.0);
    assert_eq!(h.applied.len(), count_after_enter);

    // slide onto the bar without leaving the panel
    h.mouse_move(5.0, 50.0);
    assert_eq!(h.applied.len(), count_after_enter + 1);
    assert_eq!(h.last(), CursorIcon::ResizeHorizontal);

    // and back off it
    h.mouse_move(50.0, 50.0);
    assert_eq!(h.last(), CursorIcon::Default);
}

#[test]
fn cursor_change_during_a_captured_drag_reaches_the_window() {
    let mut h = Harness::new(vec![widget(0.0, 0.0, 100.0, 100.0, Mode::EdgeBar)]);

    h.mouse_move(50.0, 50.0);
    h.press(50.0, 50.0);
    let count_after_press = h.applied.len();

    // the press captured the mouse on the panel; a move onto its bar is still a cursor change
    h.mouse_move(5.0, 50.0);
    assert_eq!(h.applied.len(), count_after_press + 1);
    assert_eq!(h.last(), CursorIcon::ResizeHorizontal);
}

#[test]
fn cursor_set_by_code_while_the_mouse_rests_over_the_widget_reaches_the_window() {
    let cursor = Rc::new(Cell::new(CursorIcon::Default));
    let mut h = Harness::new(vec![widget(
        0.0,
        0.0,
        100.0,
        100.0,
        Mode::Fixed(Rc::clone(&cursor)),
    )]);

    h.mouse_move(50.0, 50.0);
    let count_after_enter = h.applied.len();

    // a picking mode starts while the mouse sits still over the panel
    set_by_code(&mut h, &cursor, CursorIcon::Crosshair);
    assert_eq!(h.applied.len(), count_after_enter + 1);
    assert_eq!(h.last(), CursorIcon::Crosshair);

    // setting the same cursor again does not call the platform
    set_by_code(&mut h, &cursor, CursorIcon::Crosshair);
    assert_eq!(h.applied.len(), count_after_enter + 1);
}

#[test]
fn cursor_set_after_base_mouse_move_reaches_the_window_on_that_move() {
    let mut h = Harness::new(vec![widget(0.0, 0.0, 100.0, 100.0, Mode::EdgeBar)]);

    h.mouse_move(50.0, 50.0);
    h.mouse_move(5.0, 50.0);
    assert_eq!(h.last(), CursorIcon::ResizeHorizontal);

    h.mouse_move(50.0, 50.0);
    assert_eq!(h.last(), CursorIcon::Default);
}

#[test]
fn cursor_set_on_a_widget_the_mouse_is_not_over_does_not_reach_the_window() {
    let hovered = Rc::new(Cell::new(CursorIcon::Default));
    let other = Rc::new(Cell::new(CursorIcon::Default));
    let mut h = Harness::new(vec![
        widget(0.0, 0.0, 50.0, 50.0, Mode::Fixed(Rc::clone(&hovered))),
        widget(100.0, 100.0, 50.0, 50.0, Mode::Fixed(Rc::clone(&other))),
    ]);

    h.mouse_move(10.0, 10.0);
    let count_after_enter = h.applied.len();

    set_by_code(&mut h, &other, CursorIcon::PointingHand);
    assert_eq!(h.applied.len(), count_after_enter);
}

#[test]
fn cursor_set_by_a_captured_widget_while_the_mouse_is_outside_it_reaches_the_window() {
    let dragged = Rc::new(Cell::new(CursorIcon::Default));
    // C# adds the edge-bar widget after the first drag; it sits clear of
    // every point that drag touches, so it is placed up front here.
    let mut h = Harness::new(vec![
        widget(0.0, 0.0, 50.0, 50.0, Mode::Fixed(Rc::clone(&dragged))),
        widget(0.0, 100.0, 50.0, 50.0, Mode::EdgeBar),
    ]);

    h.mouse_move(10.0, 10.0);
    h.press(10.0, 10.0);

    // a drag out of the widget that holds the capture (the node editor refusing a dropped item)
    h.mouse_move(150.0, 150.0);
    set_by_code(&mut h, &dragged, CursorIcon::NotAllowed);
    assert_eq!(h.last(), CursorIcon::NotAllowed);

    // a captured cursor that depends on the mouse position follows moves outside the widget too
    set_by_code(&mut h, &dragged, CursorIcon::Default);
    h.release(150.0, 150.0);
    h.mouse_move(20.0, 120.0);
    h.press(20.0, 120.0);
    h.mouse_move(5.0, 190.0);
    assert_eq!(h.last(), CursorIcon::ResizeHorizontal);
}

/// A drag holding the capture is never told the pointer left the window:
/// the shell forwards the platform's "cursor left" (winit's `CursorLeft`,
/// which macOS posts when a drag crosses the window edge) and the old
/// `(-1, -1)` sentinel reached the captured widget as a real move (agg-sharp's
/// `OutOfViewMouseCaptureTests.ADragInFlightIsNeverToldThePointerLeft`).
#[test]
fn rust_only_a_mouse_leave_during_a_captured_drag_does_not_reach_the_dragged_widget() {
    let mut h = Harness::new(vec![widget(0.0, 0.0, 100.0, 100.0, Mode::EdgeBar)]);
    h.mouse_move(50.0, 50.0);
    h.press(50.0, 50.0);
    h.mouse_move(150.0, 50.0);
    // Off the bar while captured; a leave would move it to local x = -1,
    // onto the bar, and switch the cursor.
    h.app.on_mouse_leave();
    h.sync();
    assert_eq!(h.last(), CursorIcon::Default);
    assert!(h.app.has_captured_pointer());
}
