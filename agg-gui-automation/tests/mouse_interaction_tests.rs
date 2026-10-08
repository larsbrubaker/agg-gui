//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/MouseInteractionTests.cs`.
//!
//! Only the tests whose subjects have landed are here; the rest of the class
//! (covered widgets, overlap and capture) arrives with the runner slices
//! listed in `docs/design/gui-automation.md`, and
//! `RadioButtonSiblingsAreChildren` waits for a standalone agg-gui radio
//! button (agg-gui's `RadioGroup` keeps its options inside one widget).
//!
//! C#'s bare `GuiWidget`s are [`ProbeWidget`]s that capture presses (as
//! every C# `GuiWidget` does), and C# widget references are
//! [`WidgetHandle`]s, whose equality is widget identity. Tests that call
//! `container.OnMouseDown(...)` directly drive a [`HeadlessWindow`] the size
//! of the container, whose root holds the container at the origin. C#'s
//! `UnderMouseState`, `MouseCaptured`, `ChildHasMouseCaptured` and `Focused`
//! are [`pointer_state`]'s queries; C#'s `MouseEnter`/`MouseLeave` are
//! `Event::MouseOver`/`MouseOut` and `MouseEnterBounds`/`MouseLeaveBounds`
//! are `Event::MouseEnter`/`MouseLeave`. C#'s `MouseDownCaptured` (the press
//! made this widget capture the pointer), `MouseUpCaptured` (the release went
//! to this widget because it held capture) and `MouseDown` (the press landed
//! within this widget's bounds, which agg-gui does not bubble past the
//! widget that takes it) are read from the same state around each press and
//! release ([`Counters`]).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::event::{Event, EventResult};
use agg_gui::widgets::Button;
use agg_gui::{
    under_mouse_state_of, Font, MouseButton, Rect, Size, UnderMouseState, Widget, WidgetId,
};
use agg_gui_automation::pointer_state::{
    child_has_mouse_captured, focused, mouse_captured, under_mouse_state,
};
use agg_gui_automation::tree_query::{children, parents};
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationWindow, DragDropOpts, HeadlessWindow, Point2D,
    ProbeWidget, RunOptions, UiDriver, WidgetHandle,
};

#[path = "mouse_interaction/enter_leave.rs"]
mod enter_leave;

#[test]
fn extension_methods_tests() {
    let level3 = ProbeWidget::new("level3");
    let level2 = ProbeWidget::new("level2").with_child(Box::new(level3));
    let level1 = ProbeWidget::new("level1").with_child(Box::new(level2));
    let level0 = ProbeWidget::new("level0").with_child(Box::new(level1));
    let root: &dyn Widget = &level0;
    let handle = |path: &[usize]| WidgetHandle::new(root, path).expect("widget at path");
    let all_widgets = [
        handle(&[]),
        handle(&[0]),
        handle(&[0, 0]),
        handle(&[0, 0, 0]),
    ];

    for child in children(root, &all_widgets[0]) {
        assert!(child == all_widgets[1]);
    }

    for child in children(root, &all_widgets[1]) {
        assert!(child == all_widgets[2]);
    }

    for child in children(root, &all_widgets[2]) {
        assert!(child == all_widgets[3]);
    }

    // C# loops over the children and throws inside the loop body. A loop
    // whose body always panics trips clippy's `never_loop` deny, so the same
    // check looks at the first child instead: any child at all fails.
    if let Some(_child) = children(root, &all_widgets[3]).first() {
        panic!("there are no children we should not get here");
    }

    let mut index = all_widgets.len() - 1;
    let mut parent_count = 0;
    for parent in parents(root, &all_widgets[3]) {
        parent_count += 1;
        index -= 1;
        assert!(parent == all_widgets[index]);
    }

    assert!(parent_count == 3);
}

/// C# `new Button(text, x, y)`: a button labelled `text` at (`x`, `y`), at
/// its own size.
fn placed_button(text: &str, x: f64, y: f64, font: Arc<Font>) -> Button {
    let mut button = Button::new(text, font);
    let size = button.layout(Size::new(f64::MAX, f64::MAX));
    button.set_bounds(Rect::new(x, y, size.width, size.height));
    button
}

fn ui_font() -> Arc<Font> {
    Arc::new(agg_gui::fonts::standard_ui_font())
}

/// C#'s `container`: a window the container's size whose root holds it at
/// the origin, so window points are container points. Returns the window
/// and the container's handle.
fn container_window(container: Box<dyn Widget>) -> (HeadlessWindow, WidgetHandle) {
    let bounds = container.bounds();
    let mut window = HeadlessWindow::new(bounds.width, bounds.height);
    let handle = window
        .add_child(container)
        .expect("the container is in the window");
    (window, handle)
}

/// The handle of the child at `index` of the container (the window root's
/// only child).
fn child(window: &HeadlessWindow, path: &[usize]) -> WidgetHandle {
    let mut full = vec![0];
    full.extend_from_slice(path);
    WidgetHandle::new(window.root(), &full).expect("child at path")
}

/// C#'s `MouseDownCaptured`, `MouseUpCaptured` and `MouseDown` counts for one
/// widget, kept by [`press`] and [`release`].
struct Counters {
    handle: WidgetHandle,
    mouse_down_captured: u32,
    mouse_up_captured: u32,
    mouse_down_in_bounds: u32,
}

impl Counters {
    fn new(handle: WidgetHandle) -> Self {
        Self {
            handle,
            mouse_down_captured: 0,
            mouse_up_captured: 0,
            mouse_down_in_bounds: 0,
        }
    }

    fn reset(&mut self) {
        self.mouse_down_captured = 0;
        self.mouse_up_captured = 0;
        self.mouse_down_in_bounds = 0;
    }
}

/// `container.OnMouseDown(new MouseEventArgs(MouseButtons.Left, 1, x, y, 0))`,
/// counting C#'s `MouseDownCaptured` (the press made the widget capture the
/// pointer) and `MouseDown` (the press landed within the widget's bounds).
fn press(window: &mut HeadlessWindow, x: f64, y: f64, counters: &mut [&mut Counters]) {
    window.on_mouse_down(x, y, MouseButton::Left, 1);
    let app = window.driver().app();
    for c in counters.iter_mut() {
        if mouse_captured(app, &c.handle) {
            c.mouse_down_captured += 1;
        }
        if under_mouse_state(app, &c.handle) != UnderMouseState::NotUnderMouse {
            c.mouse_down_in_bounds += 1;
        }
    }
}

/// `container.OnMouseUp(new MouseEventArgs(MouseButtons.Left, 1, x, y, 0))`,
/// counting C#'s `MouseUpCaptured` (the release went to the widget because
/// it held the capture).
fn release(window: &mut HeadlessWindow, x: f64, y: f64, counters: &mut [&mut Counters]) {
    let held: Vec<bool> = counters
        .iter()
        .map(|c| mouse_captured(window.driver().app(), &c.handle))
        .collect();
    window.on_mouse_up(x, y, MouseButton::Left);
    for (c, held) in counters.iter_mut().zip(held) {
        if held {
            c.mouse_up_captured += 1;
        }
    }
}

#[test]
fn do_click_button_in_window() {
    show_window_and_execute_tests(
        RunOptions {
            secs_to_test_failure: 30.0,
            ..RunOptions::default()
        },
        || {
            let left_click_count = Rc::new(Cell::new(0));
            let right_click_count = Rc::new(Cell::new(0));

            let mut button_container = AutomationWindow::new(300.0, 200.0);

            let font = ui_font();
            let clicks = Rc::clone(&left_click_count);
            let left_button = placed_button("left", 10.0, 40.0, Arc::clone(&font))
                .with_name("left")
                .on_click(move || clicks.set(clicks.get() + 1));
            button_container.add_child(Box::new(left_button));
            let clicks = Rc::clone(&right_click_count);
            let right_button = placed_button("right", 110.0, 40.0, font)
                .on_click(move || clicks.set(clicks.get() + 1))
                .with_name("right");
            button_container.add_child(Box::new(right_button));
            (button_container, (left_click_count, right_click_count))
        },
        |test_runner, (left_click_count, right_click_count)| {
            // Now do the actions specific to this test. (replace this for new tests)
            test_runner.click_by_name("left");
            test_runner.delay(0.5);

            assert!(left_click_count.get() == 1);

            test_runner.click_by_name("right");
            test_runner.delay(0.5);

            assert!(right_click_count.get() == 1);

            test_runner.drag_drop_by_name(
                "left",
                "right",
                &DragDropOpts {
                    offset_drag: Point2D::new(1, 0),
                    ..DragDropOpts::default()
                },
            );
            test_runner.delay(0.5);

            assert!(left_click_count.get() == 1);

            test_runner.mark_test_complete();
        },
    )
    .expect("a press dragged off a button does not click it");
}

#[test]
fn validate_simple_left_click() {
    let container = ProbeWidget::new("Container")
        .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
        .capturing_presses();
    let got_click = Rc::new(Cell::new(false));
    let clicked = Rc::clone(&got_click);
    let button = placed_button("Test", 100.0, 100.0, ui_font())
        .with_name("button")
        .on_click(move || clicked.set(true));
    let container = container.with_child(Box::new(button));
    let (mut container, _) = container_window(Box::new(container));
    let button = child(&container, &[0]);
    let button_focused = |w: &HeadlessWindow| focused(w.driver().app(), &button);

    assert!(!got_click.get());
    assert!(!button_focused(&container));

    container.on_mouse_down(10.0, 10.0, MouseButton::Left, 1);
    container.on_mouse_up(10.0, 10.0, MouseButton::Left);
    assert!(!got_click.get());
    assert!(!button_focused(&container));

    container.on_mouse_down(110.0, 110.0, MouseButton::Left, 1);
    container.on_mouse_up(10.0, 10.0, MouseButton::Left);
    assert!(!got_click.get());
    assert!(button_focused(&container));

    assert!(!got_click.get());
    container.on_mouse_down(110.0, 110.0, MouseButton::Left, 1);
    container.on_mouse_up(110.0, 110.0, MouseButton::Left);
    assert!(got_click.get());
    assert!(button_focused(&container));

    got_click.set(false);
    container.on_mouse_down(10.0, 10.0, MouseButton::Left, 1);
    container.on_mouse_up(10.0, 10.0, MouseButton::Left);
    assert!(!got_click.get());
    assert!(!button_focused(&container));
}

#[test]
fn validate_only_top_widget_gets_left_click() {
    let got_click = Rc::new(Cell::new(false));
    let clicked = Rc::clone(&got_click);
    let button = placed_button("Test", 100.0, 100.0, ui_font())
        .with_name("button")
        .on_click(move || clicked.set(true));
    let blocking_widegt = ProbeWidget::new("blockingWidegt")
        .with_bounds(Rect::new(105.0, 105.0, 20.0, 20.0))
        .capturing_presses();
    let container = ProbeWidget::new("container")
        .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
        .capturing_presses()
        .with_child(Box::new(button))
        .with_child(Box::new(blocking_widegt));
    let (mut window, container) = container_window(Box::new(container));
    let button = child(&window, &[0]);
    let blocking_widegt = child(&window, &[1]);
    let captured = |w: &HeadlessWindow, h: &WidgetHandle| mouse_captured(w.driver().app(), h);
    let child_captured =
        |w: &HeadlessWindow, h: &WidgetHandle| child_has_mouse_captured(w.driver().app(), h);

    // the widget is not in the way
    assert!(!got_click.get());
    window.on_mouse_down(101.0, 101.0, MouseButton::Left, 1);
    assert!(!captured(&window, &container));
    assert!(!captured(&window, &blocking_widegt));
    assert!(child_captured(&window, &container));
    assert!(!child_captured(&window, &blocking_widegt));
    assert!(captured(&window, &button));
    window.on_mouse_up(101.0, 101.0, MouseButton::Left);
    assert!(!captured(&window, &container));
    assert!(!captured(&window, &blocking_widegt));
    assert!(!captured(&window, &button));
    assert!(got_click.get());

    got_click.set(false);

    // the widget is in the way
    assert!(!got_click.get());
    window.on_mouse_down(110.0, 110.0, MouseButton::Left, 1);
    assert!(!captured(&window, &container));
    assert!(captured(&window, &blocking_widegt));
    assert!(!captured(&window, &button));
    window.on_mouse_up(110.0, 110.0, MouseButton::Left);
    assert!(!captured(&window, &container));
    assert!(!captured(&window, &blocking_widegt));
    assert!(!captured(&window, &button));
    assert!(!got_click.get());
}

#[test]
fn validate_simple_mouse_up_down() {
    let top_widget = ProbeWidget::new("topWidget")
        .with_bounds(Rect::new(100.0, 100.0, 50.0, 50.0))
        .capturing_presses();
    let container = ProbeWidget::new("container")
        .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
        .capturing_presses()
        .with_child(Box::new(top_widget));
    let (mut window, container) = container_window(Box::new(container));
    let mut container = Counters::new(container);
    let mut top_widget = Counters::new(child(&window, &[0]));

    assert!(container.mouse_up_captured == 0);
    assert!(top_widget.mouse_up_captured == 0);
    // down outside everything
    press(
        &mut window,
        -10.0,
        -10.0,
        &mut [&mut container, &mut top_widget],
    );
    assert!(container.mouse_up_captured == 0);
    assert!(top_widget.mouse_up_captured == 0);
    assert!(container.mouse_down_captured == 0);
    assert!(top_widget.mouse_down_captured == 0);
    assert!(container.mouse_down_in_bounds == 0);
    assert!(top_widget.mouse_down_in_bounds == 0);
    top_widget.reset();
    container.reset();
    // up outside everything
    release(
        &mut window,
        -10.0,
        -10.0,
        &mut [&mut container, &mut top_widget],
    );
    assert!(container.mouse_up_captured == 0);
    assert!(top_widget.mouse_up_captured == 0);
    assert!(container.mouse_down_captured == 0);
    assert!(top_widget.mouse_down_captured == 0);
    assert!(container.mouse_down_in_bounds == 0);
    assert!(top_widget.mouse_down_in_bounds == 0);
    top_widget.reset();
    container.reset();
    // down on container
    press(
        &mut window,
        10.0,
        10.0,
        &mut [&mut container, &mut top_widget],
    );
    assert!(container.mouse_up_captured == 0);
    assert!(top_widget.mouse_up_captured == 0);
    assert!(container.mouse_down_captured == 1);
    assert!(top_widget.mouse_down_captured == 0);
    assert!(container.mouse_down_in_bounds == 1);
    assert!(top_widget.mouse_down_in_bounds == 0);
    top_widget.reset();
    container.reset();
    release(
        &mut window,
        10.0,
        10.0,
        &mut [&mut container, &mut top_widget],
    );
    assert!(container.mouse_up_captured == 1);
    assert!(top_widget.mouse_up_captured == 0);
    assert!(container.mouse_down_captured == 0);
    assert!(top_widget.mouse_down_captured == 0);
    assert!(container.mouse_down_in_bounds == 0);
    assert!(top_widget.mouse_down_in_bounds == 0);
    top_widget.reset();
    container.reset();
    press(
        &mut window,
        110.0,
        110.0,
        &mut [&mut container, &mut top_widget],
    );
    assert!(container.mouse_up_captured == 0);
    assert!(top_widget.mouse_up_captured == 0);
    assert!(container.mouse_down_captured == 0);
    assert!(top_widget.mouse_down_captured == 1);
    assert!(container.mouse_down_in_bounds == 1);
    assert!(top_widget.mouse_down_in_bounds == 1);
    top_widget.reset();
    container.reset();
    release(
        &mut window,
        10.0,
        10.0,
        &mut [&mut container, &mut top_widget],
    );
    assert!(container.mouse_up_captured == 0);
    assert!(top_widget.mouse_up_captured == 1);
    assert!(container.mouse_down_captured == 0);
    assert!(top_widget.mouse_down_captured == 0);
    assert!(container.mouse_down_in_bounds == 0);
    assert!(top_widget.mouse_down_in_bounds == 0);

    top_widget.reset();
    container.reset();
    press(
        &mut window,
        110.0,
        110.0,
        &mut [&mut container, &mut top_widget],
    );
    release(
        &mut window,
        110.0,
        110.0,
        &mut [&mut container, &mut top_widget],
    );
    assert!(container.mouse_up_captured == 0);
    assert!(top_widget.mouse_up_captured == 1);
    assert!(container.mouse_down_captured == 0);
    assert!(top_widget.mouse_down_captured == 1);
    assert!(container.mouse_down_in_bounds == 1);
    assert!(top_widget.mouse_down_in_bounds == 1);
}

#[test]
fn validate_only_top_widget_gets_mouse_up() {
    let top_widget = ProbeWidget::new("topWidget")
        .with_bounds(Rect::new(100.0, 100.0, 50.0, 50.0))
        .capturing_presses();
    let blocking_widegt = ProbeWidget::new("blockingWidegt")
        .with_bounds(Rect::new(105.0, 105.0, 20.0, 20.0))
        .capturing_presses();
    let container = ProbeWidget::new("container")
        .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
        .capturing_presses()
        .with_child(Box::new(top_widget))
        .with_child(Box::new(blocking_widegt));
    let (mut window, _) = container_window(Box::new(container));
    let mut top = Counters::new(child(&window, &[0]));
    let mut blocking = Counters::new(child(&window, &[1]));
    let top_got_mouse_up = |top: &Counters| top.mouse_up_captured > 0;
    let blocking_got_mouse_up = |blocking: &Counters| blocking.mouse_up_captured > 0;

    // the widget is not in the way
    assert!(!top_got_mouse_up(&top));
    press(&mut window, 101.0, 101.0, &mut [&mut top, &mut blocking]);
    release(&mut window, 101.0, 101.0, &mut [&mut top, &mut blocking]);
    assert!(!blocking_got_mouse_up(&blocking));
    assert!(top_got_mouse_up(&top));

    top.reset();

    // the widget is in the way
    assert!(!top_got_mouse_up(&top));
    press(&mut window, 110.0, 110.0, &mut [&mut top, &mut blocking]);
    release(&mut window, 110.0, 110.0, &mut [&mut top, &mut blocking]);
    assert!(blocking_got_mouse_up(&blocking));
    assert!(!top_got_mouse_up(&top));
}
