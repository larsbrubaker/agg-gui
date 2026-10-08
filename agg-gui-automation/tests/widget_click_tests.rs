//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/WidgetClickTests.cs`.
//!
//! C#'s `ClickTestsWindow` is an [`AutomationWindow`] holding three
//! [`ProbeWidget`]s with `Click` handlers (C#'s `ClickableWidget`s). C#
//! places them with anchors and padding; the probes keep the bounds that
//! layout produces, computed in [`ClickTestsWindow::new`]. C#'s
//! `testWindow.OnMouseDown`/`OnMouseUp` take window points (logical, Y-up);
//! here they are the raw `App` entry points, which take physical pixels,
//! Y-down, so [`window_mouse_down`]/[`window_mouse_up`] convert.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::{Color, Modifiers, MouseButton, Rect};
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationRunner, AutomationWindow, ProbeWidget, RunOptions,
};

/// C# `ClickableWidget`: a 35 × 25 widget counting its clicks.
struct ClickableWidget {
    click_count: Rc<Cell<u32>>,
    /// C# `BoundsRelativeToParent`.
    bounds_relative_to_parent: Rect,
}

impl ClickableWidget {
    fn new(name: &str, bounds: Rect, color: Color) -> (Self, ProbeWidget) {
        let click_count = Rc::new(Cell::new(0));
        let counter = Rc::clone(&click_count);
        let probe = ProbeWidget::new(name)
            .with_bounds(bounds)
            .with_background(color)
            // C#'s Widget_Click: widget.ClickCount += 1
            .on_click(move |_| counter.set(counter.get() + 1));
        (
            Self {
                click_count,
                bounds_relative_to_parent: bounds,
            },
            probe,
        )
    }

    fn click_count(&self) -> u32 {
        self.click_count.get()
    }
}

/// C# `ClickTestsWindow`: a window with three clickable controls.
struct ClickTestsWindow {
    blue_widget: ClickableWidget,
    orange_widget: ClickableWidget,
    purple_widget: ClickableWidget,
}

impl ClickTestsWindow {
    fn new(width: f64, height: f64) -> (AutomationWindow, Self) {
        // C#: the window's Padding is 50; BlueWidget (padding 8) stretches
        // across it and fits its 25-tall children vertically, centered.
        let window_padding = 50.0;
        let blue_padding = 8.0;
        let (child_width, child_height) = (35.0, 25.0);
        let blue_width = width - 2.0 * window_padding;
        let blue_height = child_height + 2.0 * blue_padding;
        let content_height = height - 2.0 * window_padding;
        let blue_bounds = Rect::new(
            window_padding,
            window_padding + (content_height - blue_height) / 2.0,
            blue_width,
            blue_height,
        );
        // OrangeWidget is anchored left and PurpleWidget right, inside
        // BlueWidget's padding.
        let orange_bounds = Rect::new(blue_padding, blue_padding, child_width, child_height);
        let purple_bounds = Rect::new(
            blue_width - blue_padding - child_width,
            blue_padding,
            child_width,
            child_height,
        );

        // A clickable widget containing child controls
        let (blue_widget, blue) =
            ClickableWidget::new("blueWidget", blue_bounds, Color::from_rgb8(0, 0, 255));
        // A clickable child control
        let (orange_widget, orange) =
            ClickableWidget::new("orangeWidget", orange_bounds, Color::from_rgb8(255, 165, 0));
        // A clickable child control
        let (purple_widget, purple) =
            ClickableWidget::new("purpleWidget", purple_bounds, Color::from_rgb8(141, 0, 206));

        let blue = blue
            .with_child(Box::new(orange))
            .with_child(Box::new(purple));
        let mut window = AutomationWindow::new(width, height);
        window.add_child(Box::new(blue));
        (
            window,
            Self {
                blue_widget,
                orange_widget,
                purple_widget,
            },
        )
    }
}

/// A window point (logical, Y-up, unrounded as C#'s `MouseEventArgs`) as
/// the `App`'s physical, Y-down pixels.
fn window_to_app(test_runner: &AutomationRunner, x: f64, y: f64) -> (f64, f64) {
    let scale = agg_gui::ux_scale::effective_scale();
    let (_, height_px) = test_runner.driver().size_px();
    (x * scale, f64::from(height_px) - y * scale)
}

/// C# `testWindow.OnMouseDown(new MouseEventArgs(MouseButtons.Left, 1, x, y, 0))`.
fn window_mouse_down(test_runner: &mut AutomationRunner, x: f64, y: f64) {
    let (x, y) = window_to_app(test_runner, x, y);
    test_runner
        .app_mut()
        .on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
}

/// C# `testWindow.OnMouseUp(new MouseEventArgs(MouseButtons.Left, 1, x, y, 0))`.
fn window_mouse_up(test_runner: &mut AutomationRunner, x: f64, y: f64) {
    let (x, y) = window_to_app(test_runner, x, y);
    test_runner
        .app_mut()
        .on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
}

/// C# `testRunner.SetMouseCursorPosition(testWindow, (int)x, (int)y)`.
fn set_mouse_cursor_position(test_runner: &mut AutomationRunner, x: f64, y: f64) {
    // `(int)` truncates.
    test_runner.set_mouse_cursor_position_in_window(x as i32, y as i32);
}

fn run(body: impl FnOnce(&mut AutomationRunner, &ClickTestsWindow) + Send + 'static) {
    show_window_and_execute_tests(
        RunOptions {
            secs_to_test_failure: 30.0,
            ..RunOptions::default()
        },
        || ClickTestsWindow::new(300.0, 200.0),
        body,
    )
    .expect("the test body passes");
}

#[test]
fn click_fires_on_correct_widgets() {
    run(|test_runner, test_window| {
        let counts = || {
            (
                test_window.blue_widget.click_count(),
                test_window.orange_widget.click_count(),
                test_window.purple_widget.click_count(),
            )
        };

        test_runner.click_by_name("blueWidget");
        test_runner.delay(0.1);
        assert_eq!(counts(), (1, 0, 0));

        test_runner.click_by_name("orangeWidget");
        test_runner.delay(0.1);
        assert_eq!(counts(), (1, 1, 0));

        test_runner.click_by_name("blueWidget");
        test_runner.delay(0.1);
        assert_eq!(counts(), (2, 1, 0));

        test_runner.click_by_name("orangeWidget");
        test_runner.delay(0.1);
        assert_eq!(counts(), (2, 2, 0));

        test_runner.click_by_name("purpleWidget");
        test_runner.delay(0.1);
        assert_eq!(counts(), (2, 2, 1));
        test_runner.mark_test_complete();
    });
}

#[test]
fn click_suppressed_on_external_mouse_up() {
    run(|test_runner, test_window| {
        let bounds = test_window.blue_widget.bounds_relative_to_parent;
        let mouse_down_position = (bounds.left() + 25.0, bounds.bottom() + 4.0);
        let center = (
            bounds.left() + bounds.width / 2.0,
            bounds.bottom() + bounds.height / 2.0,
        );

        // ** Click should occur on mouse[down/up] within the controls bounds **
        //
        // Move to a position within the blueWidget for mousedown
        set_mouse_cursor_position(test_runner, mouse_down_position.0, mouse_down_position.1);
        window_mouse_down(test_runner, mouse_down_position.0, mouse_down_position.1);

        // Move to a position within blueWidget for mouseup
        set_mouse_cursor_position(test_runner, center.0, center.1);
        window_mouse_up(test_runner, center.0, center.1);

        assert_eq!(test_window.blue_widget.click_count(), 1);

        // ** Click should not occur when mouse up is outside of the control bounds **
        //
        // Move to a position within BlueWidget for mousedown
        set_mouse_cursor_position(test_runner, mouse_down_position.0, mouse_down_position.1);
        window_mouse_down(test_runner, mouse_down_position.0, mouse_down_position.1);

        // Move to a position **outside** of BlueWidget for mouseup
        set_mouse_cursor_position(test_runner, 50.0, 50.0);
        window_mouse_up(test_runner, 50.0, 50.0);

        // There should be no increment in the click count
        assert_eq!(test_window.blue_widget.click_count(), 1);
        test_runner.mark_test_complete();
    });
}

#[test]
fn click_suppressed_on_mouse_up_within_child2() {
    // Agg currently fires mouse up events in child controls when the parent
    // has the mouse captured and is performing drag like operations. If the
    // mouse goes down in the parent and comes up on the child neither
    // control should get a click event
    run(|test_runner, test_window| {
        let bounds = test_window.blue_widget.bounds_relative_to_parent;
        let mouse_down_position = (bounds.left() + 25.0, bounds.bottom() + 4.0);
        let center = (
            bounds.left() + bounds.width / 2.0,
            bounds.bottom() + bounds.height / 2.0,
        );

        let child_bounds = test_window.orange_widget.bounds_relative_to_parent;
        let child_x = bounds.left() + child_bounds.left() + child_bounds.width / 2.0;
        let child_y = bounds.bottom() + child_bounds.bottom() + child_bounds.height / 2.0;

        // ** Click should occur on mouse[down/up] within the controls bounds **
        //
        // Move to a position within BlueWidget for mousedown
        set_mouse_cursor_position(test_runner, mouse_down_position.0, mouse_down_position.1);
        window_mouse_down(test_runner, mouse_down_position.0, mouse_down_position.1);

        // Move to a position within BlueWidget for mouseup
        set_mouse_cursor_position(test_runner, center.0, center.1);
        window_mouse_up(test_runner, center.0, center.1);

        assert_eq!(test_window.blue_widget.click_count(), 1);

        // ** Click should not occur when mouse up occurs on child controls **
        //
        // Move to a position within BlueWidget for mousedown
        set_mouse_cursor_position(test_runner, mouse_down_position.0, mouse_down_position.1);
        window_mouse_down(test_runner, mouse_down_position.0, mouse_down_position.1);

        // Move to a position with the OrangeWidget for mouseup
        set_mouse_cursor_position(test_runner, child_x, child_y);
        window_mouse_up(test_runner, child_x, child_y);

        // There should be no increment in the click count
        assert_eq!(test_window.blue_widget.click_count(), 1);
        test_runner.mark_test_complete();
    });
}
