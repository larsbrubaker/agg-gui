//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/WindowDeactivationTests.cs`,
//! plus Rust-only checks of what agg-gui's deactivation does to a pointer
//! capture.
//!
//! C#'s `window.OnDeactivated(EventArgs.Empty)` is
//! [`HeadlessWindow::on_deactivated`], which sends the same
//! `ForwarderEvent::WindowDeactivated` a shell sends when the OS reports the
//! window losing activation. `DeactivatingTheWindowClosesAnOpenMenu` and
//! `DeactivatingTheWindowClosesAnOpenPopupWidget` are not ported here:
//! closing popups on deactivation is the popup widgets' behaviour, built on
//! the event this file tests, and agg-gui's popups do not yet subscribe.
//! `AnAutomationRunIgnoresTheDesktopDeactivatingItsWindow` becomes the
//! real-input gate check: an automation run turns platform input off
//! (`InputForwarder::set_platform_input_enabled`), and a platform
//! deactivation is platform input.

use agg_gui::event::{Event, EventResult, MouseButton};
use agg_gui::shell_input::{ForwarderEvent, InputForwarder};
use agg_gui::{App, Rect, Size};
use agg_gui_automation::{HeadlessWindow, ProbeWidget, UiDriver};

fn count(log: &[Event], pred: impl Fn(&Event) -> bool) -> usize {
    log.iter().filter(|e| pred(e)).count()
}

#[test]
fn deactivating_the_window_leaves_text_focus_where_it_was() {
    // A mac app keeps its text field focused across an app switch; deactivation is not a focus change.
    let mut window = HeadlessWindow::new(600.0, 400.0);
    let field = ProbeWidget::new("textField")
        .with_bounds(Rect::new(10.0, 10.0, 200.0, 20.0))
        .with_focusable(true);
    let log = field.log();
    window.add_child(Box::new(field)).expect("added");
    window.on_mouse_down(20.0, 15.0, MouseButton::Left, 1);
    window.on_mouse_up(20.0, 15.0, MouseButton::Left);
    let focused = window.driver().app().focused_path().map(<[usize]>::to_vec);
    assert!(focused.is_some(), "the click focused the field");

    window.on_deactivated();

    assert_eq!(
        window.driver().app().focused_path().map(<[usize]>::to_vec),
        focused
    );
    assert_eq!(
        count(&log.borrow(), |e| matches!(e, Event::FocusLost)),
        0,
        "no FocusLost on deactivation"
    );
    window.on_activated();
}

#[test]
fn an_automation_run_ignores_the_desktop_deactivating_its_window() {
    // A run turns platform input off; the desktop handing key status around mid-run is not a user who
    // switched away. Simulated deactivation still arrives.
    let probe = ProbeWidget::new("root");
    let log = probe.log();
    let mut app = App::new(Box::new(probe));
    app.layout(Size::new(100.0, 100.0));
    app.on_window_activated();
    let mut input = InputForwarder::new();
    input.set_platform_input_enabled(false);

    assert!(!input.platform(&mut app, ForwarderEvent::WindowDeactivated));
    assert!(app.window_is_active());
    assert_eq!(
        count(&log.borrow(), |e| matches!(e, Event::WindowDeactivated)),
        0
    );

    input.simulated(&mut app, ForwarderEvent::WindowDeactivated);
    assert!(!app.window_is_active());
    assert_eq!(
        count(&log.borrow(), |e| matches!(e, Event::WindowDeactivated)),
        1
    );
    input.simulated(&mut app, ForwarderEvent::WindowActivated);
}

#[test]
fn rust_only_deactivation_reaches_every_widget_once_per_change() {
    let mut window = HeadlessWindow::new(200.0, 100.0);
    let a = ProbeWidget::new("a").with_bounds(Rect::new(0.0, 0.0, 50.0, 50.0));
    let b = ProbeWidget::new("b").with_bounds(Rect::new(100.0, 0.0, 50.0, 50.0));
    let (log_a, log_b) = (a.log(), b.log());
    window.add_child(Box::new(a)).expect("added");
    window.add_child(Box::new(b)).expect("added");
    window.on_activated();
    assert!(agg_gui::window_is_active());

    window.on_deactivated();
    window.on_deactivated(); // already inactive: nothing more is sent
    assert!(!agg_gui::window_is_active());
    for log in [&log_a, &log_b] {
        assert_eq!(
            count(&log.borrow(), |e| matches!(e, Event::WindowDeactivated)),
            1
        );
    }

    window.on_activated();
    window.on_activated();
    assert!(agg_gui::window_is_active());
    for log in [&log_a, &log_b] {
        assert_eq!(
            count(&log.borrow(), |e| matches!(e, Event::WindowActivated)),
            1
        );
    }
}

#[test]
fn rust_only_deactivating_mid_drag_ends_the_capture_without_a_release() {
    let mut window = HeadlessWindow::new(200.0, 100.0);
    let dragged = ProbeWidget::new("dragged")
        .with_bounds(Rect::new(0.0, 0.0, 50.0, 50.0))
        .capturing_presses();
    let log = dragged.log();
    window.add_child(Box::new(dragged)).expect("added");
    window.on_activated();
    window.on_mouse_down(10.0, 10.0, MouseButton::Left, 1);
    // A move far outside still reaches the capture holder.
    window.on_mouse_move(150.0, 80.0);
    assert!(window.driver().app().has_captured_pointer());
    assert!(window
        .driver()
        .forwarder()
        .is_button_down(MouseButton::Left));
    let moves_while_captured = count(&log.borrow(), |e| matches!(e, Event::MouseMove { .. }));

    window.on_deactivated();

    {
        let log = log.borrow();
        let lost = log
            .iter()
            .position(|e| matches!(e, Event::MouseCaptureLost));
        let deactivated = log
            .iter()
            .position(|e| matches!(e, Event::WindowDeactivated));
        assert!(lost.is_some(), "the capture holder is told");
        assert!(
            lost < deactivated,
            "capture loss comes before the broadcast"
        );
        assert_eq!(
            count(&log, |e| matches!(e, Event::MouseUp { .. })),
            0,
            "no release is made up"
        );
    }
    assert!(!window.driver().app().has_captured_pointer());
    assert_eq!(window.driver().forwarder().buttons_down(), 0);

    // The drag is over: a move elsewhere no longer goes to the old holder.
    window.on_mouse_move(160.0, 80.0);
    let moves_after = count(&log.borrow(), |e| matches!(e, Event::MouseMove { .. }));
    assert!(
        moves_after <= moves_while_captured + 1,
        "at most the hover-clearing move reaches the old holder"
    );
    window.on_mouse_move(170.0, 80.0);
    assert_eq!(
        count(&log.borrow(), |e| matches!(e, Event::MouseMove { .. })),
        moves_after,
        "later moves elsewhere do not reach it"
    );
    window.on_activated();
}

#[test]
fn rust_only_deactivating_with_no_press_sends_no_capture_loss() {
    let mut window = HeadlessWindow::new(200.0, 100.0);
    let probe = ProbeWidget::new("p")
        .with_bounds(Rect::new(0.0, 0.0, 50.0, 50.0))
        .on_event_with(|_| EventResult::Ignored);
    let log = probe.log();
    window.add_child(Box::new(probe)).expect("added");
    window.on_activated();
    window.on_deactivated();
    assert_eq!(
        count(&log.borrow(), |e| matches!(e, Event::MouseCaptureLost)),
        0
    );
    assert_eq!(
        count(&log.borrow(), |e| matches!(e, Event::WindowDeactivated)),
        1
    );
    window.on_activated();
}
