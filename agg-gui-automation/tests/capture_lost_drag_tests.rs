//! Rust-only checks that agg-gui's drag widgets end their drag on
//! `Event::MouseCaptureLost`: the window lost activation mid-drag, so the
//! release went to another application. Each drag ends where it was, as if
//! released there, and a later pointer move no longer drags.
//!
//! Driven through [`HeadlessWindow`] — the real `App` capture path that
//! sends `MouseCaptureLost` on deactivation — one test per widget; the
//! shared `drag_then_deactivate` press-move-deactivate-move sequence is in
//! this file. Complements `window_deactivation_tests.rs`, which checks the
//! event itself.

use std::sync::Arc;

use agg_gui::event::{Event, MouseButton};
use agg_gui::{DragValue, Point, Rect, Resize, Splitter, SplitterRatio, TextField, Widget, Window};
use agg_gui_automation::{HeadlessWindow, ProbeWidget, WidgetHandle};

fn font() -> Arc<agg_gui::Font> {
    Arc::new(agg_gui::fonts::standard_ui_font())
}

/// Press at `from`, drag to `to`, deactivate the window, then move on to
/// `after` and release there: returns `state` read after the drag to `to`
/// and after the final move. The final move also goes straight to the
/// widget at `handle` (as a hover over it would, whichever child is under
/// the pointer), so a drag left running would follow it.
fn drag_then_deactivate<T: PartialEq + std::fmt::Debug>(
    window: &mut HeadlessWindow,
    handle: &WidgetHandle,
    from: (f64, f64),
    to: (f64, f64),
    after: (f64, f64),
    state: impl Fn(&HeadlessWindow) -> T,
) -> (T, T) {
    window.on_activated();
    window.on_mouse_down(from.0, from.1, MouseButton::Left, 1);
    window.on_mouse_move((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    window.on_mouse_move(to.0, to.1);
    let dragged = state(window);
    window.on_deactivated();
    window.on_activated();
    window.on_mouse_move(after.0, after.1);
    let target = handle
        .widget_mut(window.driver_mut().root_mut())
        .expect("attached");
    let b = target.bounds();
    let pos = Point::new(after.0 - b.x, after.1 - b.y);
    target.on_event(&Event::MouseMove { pos });
    let later = state(window);
    window.on_mouse_up(after.0, after.1, MouseButton::Left);
    (dragged, later)
}

fn widget<'a>(window: &'a HeadlessWindow, handle: &WidgetHandle) -> &'a dyn Widget {
    handle.widget(window.root()).expect("attached")
}

#[test]
fn rust_only_a_splitter_drag_ends_on_capture_loss() {
    let mut window = HeadlessWindow::new(400.0, 200.0);
    let ratio = SplitterRatio::new(0.5);
    let mut splitter = Splitter::new(
        Box::new(ProbeWidget::new("left")),
        Box::new(ProbeWidget::new("right")),
    )
    .with_ratio_handle(ratio.clone());
    splitter.set_bounds(Rect::new(0.0, 0.0, 400.0, 200.0));
    let handle = window.add_child(Box::new(splitter)).expect("added");
    window.draw();
    let mid = 200.0;
    let (dragged, later) = drag_then_deactivate(
        &mut window,
        &handle,
        (mid, 100.0),
        (100.0, 100.0),
        (300.0, 100.0),
        |_| ratio.get(),
    );
    assert!(dragged < 0.4, "the drag moved the divider: {dragged}");
    assert_eq!(later, dragged, "the divider stays where the drag ended");
}

#[test]
fn rust_only_a_drag_value_drag_ends_on_capture_loss() {
    let mut window = HeadlessWindow::new(400.0, 200.0);
    let mut dv = DragValue::new(50.0, 0.0, 1000.0, font());
    dv.set_bounds(Rect::new(10.0, 10.0, 100.0, 24.0));
    let handle = window.add_child(Box::new(dv)).expect("added");
    window.draw();
    let value = |w: &HeadlessWindow| {
        handle
            .downcast::<DragValue>(w.root())
            .expect("a drag value")
            .value()
    };
    let (dragged, later) = drag_then_deactivate(
        &mut window,
        &handle,
        (50.0, 20.0),
        (90.0, 20.0),
        (200.0, 20.0),
        value,
    );
    assert!(dragged > 50.0, "the drag changed the value: {dragged}");
    assert_eq!(later, dragged, "the value stays where the drag ended");
}

#[test]
fn rust_only_a_window_move_ends_on_capture_loss() {
    let mut window = HeadlessWindow::new(600.0, 400.0);
    let mut win = Window::new("Moved", font(), Box::new(ProbeWidget::new("content")));
    win.set_bounds(Rect::new(100.0, 100.0, 200.0, 150.0));
    let handle = window.add_child(Box::new(win)).expect("added");
    window.draw();
    let origin = |w: &HeadlessWindow| {
        let b = widget(w, &handle).bounds();
        (b.x, b.y)
    };
    // Layout places the window; grab its title bar along the top edge,
    // clear of the resize band and the chrome buttons.
    let placed = widget(&window, &handle).bounds();
    let grab = (
        placed.x + placed.width / 3.0,
        placed.y + placed.height - 12.0,
    );
    let to = (grab.0 + 40.0, grab.1 - 30.0);
    let after = (to.0 + 20.0, to.1 - 40.0);
    let (dragged, later) = drag_then_deactivate(&mut window, &handle, grab, to, after, origin);
    assert_ne!(dragged, (placed.x, placed.y), "the drag moved the window");
    assert_eq!(later, dragged, "the window stays where the drag ended");
}

#[test]
fn rust_only_a_resize_drag_ends_on_capture_loss() {
    let mut window = HeadlessWindow::new(600.0, 400.0);
    let mut resize = Resize::new(Box::new(ProbeWidget::new("content")));
    resize.set_bounds(Rect::new(100.0, 100.0, 200.0, 150.0));
    let handle = window.add_child(Box::new(resize)).expect("added");
    window.draw();
    let size = |w: &HeadlessWindow| {
        widget(w, &handle)
            .properties()
            .into_iter()
            .filter(|(k, _)| *k == "current_w" || *k == "current_h")
            .map(|(_, v)| v)
            .collect::<Vec<_>>()
    };
    let before = size(&window);
    // The grip is the lower-right corner.
    let (dragged, later) = drag_then_deactivate(
        &mut window,
        &handle,
        (297.0, 103.0),
        (340.0, 80.0),
        (320.0, 120.0),
        size,
    );
    assert_ne!(dragged, before, "the drag resized");
    assert_eq!(later, dragged, "the size stays where the drag ended");
}

#[test]
fn rust_only_a_text_selection_drag_ends_on_capture_loss() {
    let mut window = HeadlessWindow::new(400.0, 200.0);
    let mut field = TextField::new(font()).with_text("the quick brown fox jumps");
    field.set_bounds(Rect::new(10.0, 10.0, 300.0, 24.0));
    let handle = window.add_child(Box::new(field)).expect("added");
    window.draw();
    let selection = |w: &HeadlessWindow| {
        handle
            .downcast::<TextField>(w.root())
            .expect("a field")
            .selection()
    };
    let (dragged, later) = drag_then_deactivate(
        &mut window,
        &handle,
        (14.0, 20.0),
        (80.0, 20.0),
        (300.0, 20.0),
        selection,
    );
    assert!(!dragged.is_empty(), "the drag selected text");
    assert_eq!(later, dragged, "the selection stays where the drag ended");
}
