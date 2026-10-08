//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/ToolTipTests.cs`.
//!
//! C#'s `SystemWindow.ToolTipManager` is agg-gui's central tooltip
//! controller (`agg_gui::widgets::tooltip::controller`), fed by any widget's
//! `with_tooltip` text (C#'s `ToolTipText`). The C# tests count the window's
//! children to see the tip: a shown tip is one more child (C#'s
//! "ToolTipWidget"). agg-gui paints the tip in its overlay pass instead of
//! adding a widget, so "one more child" is `controller::is_visible()`.
//! `ToolTipShown`/`ToolTipPop` are `observe_tooltips` events and
//! `CurrentText` is `controller::current_text()`.
//!
//! C#'s delays are installed as the thread's tooltip timings (agg-gui's own
//! defaults differ). `Thread.Sleep(ms)` followed by
//! `UiThread.InvokePendingActions()` advances the headless window's virtual
//! clock by `ms` and pumps one frame: C# looks at its tooltip state only when
//! the test drains the UI queue, and agg-gui looks once per painted frame.
//! A mouse move is followed by the frame the shells paint after input.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use agg_gui::widgets::tooltip::{controller, observe_tooltips, TooltipEvent, TooltipObserver};
use agg_gui::widgets::{set_tooltip_timings, Button, TooltipTimings};
use agg_gui::{Rect, Size, Widget};
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationWindow, ClickOpts, FrameKind, HeadlessWindow,
    ProbeWidget, RunOptions, UiDriver,
};

const TOOL_TIP1_TEXT: &str = "toolTip1";
const TOOL_TIP2_TEXT: &str = "toolTip2";

const MIN_MS_TIME_TO_RESPOND: u64 = 60;
const MIN_MS_TO_BIAS: u64 = 80;

/// C# `ToolTipManager.InitialDelay`, `ReshowDelay` and `AutoPopDelay`, in
/// milliseconds.
const INITIAL_DELAY_MS: u64 = 600;
const RESHOW_DELAY_MS: u64 = 200;
const AUTO_POP_DELAY_MS: u64 = 5000;

/// C#'s `TempData`: what the tooltip events reported.
#[derive(Default)]
struct TempData {
    last_shown_text: String,
    show_count: u32,
    pop_count: u32,
}

/// The window, the event counts and the observer that keeps them coming.
struct TwoChildWindow {
    window: HeadlessWindow,
    temp_data: Rc<RefCell<TempData>>,
    _observer: TooltipObserver,
}

impl TwoChildWindow {
    fn show_count(&self) -> u32 {
        self.temp_data.borrow().show_count
    }

    fn pop_count(&self) -> u32 {
        self.temp_data.borrow().pop_count
    }

    /// C# `systemWindow.OnMouseMove(... x, y ...)`, followed by the frame a
    /// shell paints after input (agg-gui's tooltip pass runs in paint).
    fn mouse_move(&mut self, x: f64, y: f64) {
        self.window.on_mouse_move(x, y);
        self.window.driver_mut().pump(FrameKind::Forced);
    }

    /// C# `Thread.Sleep(ms)` then `UiThread.InvokePendingActions()`.
    fn sleep_then_invoke_pending_actions(&mut self, ms: u64) {
        agg_gui::clock::advance(Duration::from_millis(ms));
        self.invoke_pending_actions();
    }

    /// C# `UiThread.InvokePendingActions()`.
    fn invoke_pending_actions(&mut self) {
        self.window.driver_mut().pump(FrameKind::Reactive);
    }

    /// C# `systemWindow.Children.Count == 2 + extra` with a tip as one more
    /// child.
    fn tip_children(&self) -> usize {
        usize::from(controller::is_visible())
    }
}

fn tipped(text: &str, bounds: Rect) -> Box<dyn Widget> {
    Box::new(
        ProbeWidget::new(text)
            .with_bounds(bounds)
            .with_tooltip(text),
    )
}

/// C#'s `new GuiWidget { LocalBounds = bounds }`: no tooltip of its own.
fn tipped_free(bounds: Rect) -> Box<dyn Widget> {
    Box::new(ProbeWidget::new("covering").with_bounds(bounds))
}

fn create_two_child_window() -> TwoChildWindow {
    set_tooltip_timings(TooltipTimings {
        initial_delay: Duration::from_millis(INITIAL_DELAY_MS),
        reshow_delay: Duration::from_millis(RESHOW_DELAY_MS),
        autopop: Duration::from_millis(AUTO_POP_DELAY_MS),
    });
    let mut window = HeadlessWindow::new(200.0, 200.0);
    let temp_data = Rc::new(RefCell::new(TempData::default()));
    let events = Rc::clone(&temp_data);
    let observer = observe_tooltips(move |event| {
        let mut data = events.borrow_mut();
        match event {
            TooltipEvent::Shown(text) => {
                data.show_count += 1;
                data.last_shown_text = text.clone();
            }
            TooltipEvent::Popped => data.pop_count += 1,
        }
    });

    window.add_child(tipped(TOOL_TIP1_TEXT, Rect::new(10.0, 10.0, 10.0, 10.0)));
    window.add_child(tipped(TOOL_TIP2_TEXT, Rect::new(30.0, 30.0, 10.0, 10.0)));
    assert_eq!(
        window.root().children().len(),
        2,
        "Expected 2 children in system window"
    );

    let mut w = TwoChildWindow {
        window,
        temp_data,
        _observer: observer,
    };

    // make sure we start out with only the widgets (no tool tip)
    w.mouse_move(0.0, 0.0);
    w.invoke_pending_actions();
    assert_eq!(
        w.tip_children(),
        0,
        "Expected 2 children in system window after mouse move"
    );

    *w.temp_data.borrow_mut() = TempData::default();
    w
}

#[test]
fn tool_tip_initial_open_tests() {
    // test simple open then wait for pop
    let mut w = create_two_child_window();

    // move into the first widget
    w.mouse_move(11.0, 11.0);
    w.invoke_pending_actions();

    // show that initially we don't have a tooltip
    assert!(w.tip_children() == 0);

    // sleep 1/2 long enough to show the tool tip
    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS / 2 + MIN_MS_TO_BIAS);

    // make sure it is still not up
    assert!(w.tip_children() == 0);
    assert!(w.show_count() == 0);
    assert!(controller::current_text().is_empty());

    // sleep 1/2 long enough to show the tool tip
    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS / 2 + MIN_MS_TO_BIAS);

    // make sure the tool tip came up
    assert!(w.tip_children() == 1);
    assert!(w.show_count() == 1);
    assert!(controller::current_text() == TOOL_TIP1_TEXT);
    assert!(w.temp_data.borrow().last_shown_text == TOOL_TIP1_TEXT);

    // wait 1/2 long enough for the tool tip to go away
    w.sleep_then_invoke_pending_actions(AUTO_POP_DELAY_MS / 2 + MIN_MS_TO_BIAS);

    // make sure the tool did not go away
    assert!(w.tip_children() == 1);
    assert!(w.pop_count() == 0);
    assert!(controller::current_text() == TOOL_TIP1_TEXT);

    // wait 1/2 long enough for the tool tip to go away
    w.sleep_then_invoke_pending_actions(AUTO_POP_DELAY_MS / 2 + MIN_MS_TO_BIAS);

    // make sure the tool tip went away
    assert!(w.tip_children() == 0);
    assert!(w.pop_count() == 1);
    assert!(controller::current_text().is_empty());
}

#[test]
fn tool_tips_show() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let font = Arc::new(agg_gui::fonts::standard_ui_font());
            let mut button_container = AutomationWindow::new(300.0, 200.0);
            // (C#'s white BackgroundColor only paints; nothing here reads it.)
            let left_button = placed_button("left", 10.0, 40.0, Arc::clone(&font))
                .with_name("ButtonWithToolTip")
                .with_tooltip("Left Tool Tip");
            button_container.add_child(Box::new(left_button));
            let right_button = placed_button("right", 110.0, 40.0, font).with_name("right");
            button_container.add_child(Box::new(right_button));
            (button_container, ())
        },
        |test_runner, _| {
            test_runner.delay(1.0);

            test_runner.move_to_by_name("ButtonWithToolTip", &ClickOpts::default());
            test_runner.delay(1.5);
            assert!(controller::is_visible());
            test_runner.move_to_by_name("right", &ClickOpts::default());
            assert!(!controller::is_visible());

            test_runner.delay(1.0);
            test_runner.mark_test_complete();
        },
    )
    .expect("the run completes");
}

/// C# `new Button(text, x, y)`: a button at (`x`, `y`) at its own size.
fn placed_button(text: &str, x: f64, y: f64, font: Arc<agg_gui::Font>) -> Button {
    let mut button = Button::new(text, font);
    let size = button.layout(Size::new(f64::MAX, f64::MAX));
    button.set_bounds(Rect::new(x, y, size.width, size.height));
    button
}

#[test]
fn tool_tip_close_on_leave() {
    let mut w = create_two_child_window();

    // move into the first widget
    w.mouse_move(11.0, 11.0);
    w.invoke_pending_actions();

    // sleep long enough to show the tool tip
    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS + MIN_MS_TO_BIAS);

    // make sure the tool tip came up
    assert!(w.tip_children() == 1);
    assert!(w.show_count() == 1);
    assert!(controller::current_text() == TOOL_TIP1_TEXT);

    // move off the first widget
    w.mouse_move(9.0, 9.0);
    w.sleep_then_invoke_pending_actions(MIN_MS_TIME_TO_RESPOND); // sleep enough for the tool tip to want to respond

    // make sure the tool tip went away
    assert!(w.tip_children() == 0);
    assert!(w.pop_count() == 1);
    assert!(controller::current_text().is_empty());
}

#[path = "tool_tip/moves.rs"]
mod moves;
