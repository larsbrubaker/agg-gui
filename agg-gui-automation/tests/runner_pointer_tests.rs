//! Rust-only tests of the runner's pointer gestures (`runner/pointer.rs`)
//! and `SimulatedInput` (`input.rs`): the eased step positions and their
//! pacing, the click counts a press reports, where presses land, and C#'s
//! rule that a press or release outside the window is not delivered while a
//! move always is. The C# tests that click by name are in
//! `automation_runner_tests.rs`.

use agg_gui::event::{Event, EventResult};
use agg_gui::{MouseButton, Point, Rect};
use agg_gui_automation::runner::{cubic_out, mouse_move_steps};
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationError, AutomationWindow, ClickOpts, ClickOrigin,
    HeadlessDriver, InputMethod, MouseAction, Point2D, ProbeLog, ProbeWidget, RunOptions,
    SimulatedInput,
};

/// A 300 × 200 window holding a probe named `target` at (50, 40) sized
/// 80 × 20, which takes every press (so it captures the release too).
fn window_with_target() -> (AutomationWindow, ProbeLog) {
    let mut window = AutomationWindow::new(300.0, 200.0);
    let target = ProbeWidget::new("target")
        .with_bounds(Rect::new(50.0, 40.0, 80.0, 20.0))
        .on_event_with(|event| match event {
            Event::MouseDown { .. } | Event::MouseUp { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        });
    let log = target.log();
    window.add_child(Box::new(target));
    (window, log)
}

fn presses(log: &ProbeLog) -> Vec<(Point, MouseButton)> {
    log.borrow()
        .iter()
        .filter_map(|event| match event {
            Event::MouseDown { pos, button, .. } => Some((*pos, *button)),
            _ => None,
        })
        .collect()
}

fn releases(log: &ProbeLog) -> Vec<(Point, MouseButton)> {
    log.borrow()
        .iter()
        .filter_map(|event| match event {
            Event::MouseUp { pos, button, .. } => Some((*pos, *button)),
            _ => None,
        })
        .collect()
}

#[test]
fn rust_only_cubic_out_matches_csharp_easing() {
    assert_eq!(cubic_out(0.0), 0.0);
    assert_eq!(cubic_out(1.0), 1.0);
    // 1 + (k - 1)^3
    assert!((cubic_out(0.2) - 0.488).abs() < 1e-12);
    assert!((cubic_out(0.5) - 0.875).abs() < 1e-12);
    assert!((cubic_out(0.8) - 0.992).abs() < 1e-12);
}

#[test]
fn rust_only_mouse_move_steps_are_eased_truncated_and_end_on_target() {
    let steps = mouse_move_steps(Point2D::new(0, 0), Point2D::new(100, 50), 5);
    assert_eq!(
        steps,
        vec![
            Point2D::new(0, 0),
            Point2D::new(48, 24),
            Point2D::new(78, 39),
            Point2D::new(93, 46),
            Point2D::new(99, 49),
            Point2D::new(100, 50),
        ]
    );

    // Truncation, not rounding, also moving toward smaller coordinates.
    let back = mouse_move_steps(Point2D::new(100, 50), Point2D::new(0, 0), 5);
    assert_eq!(back[1], Point2D::new(51, 25));
    assert_eq!(*back.last().unwrap(), Point2D::new(0, 0));
}

#[test]
fn rust_only_a_stepped_move_paces_one_frame_per_step() {
    show_window_and_execute_tests(RunOptions::default(), window_with_target, |runner, _log| {
        let frames_before = runner.driver().frames();
        runner.set_mouse_cursor_position(100, 50);
        assert_eq!(runner.current_mouse_position(), Point2D::new(100, 50));
        assert_eq!(runner.driver().forwarder().cursor(), (100.0, 50.0));
        // Five eased steps, each waiting one frame for the UI to take it;
        // the final position is sent without a wait.
        assert_eq!(runner.driver().frames() - frames_before, 5);

        // A move cut short by its time budget stops waiting: with no time
        // to move, every step is sent without a frame.
        runner.config.time_to_move_mouse = 0.0;
        let frames_before = runner.driver().frames();
        runner.set_mouse_cursor_position(10, 10);
        assert_eq!(runner.driver().frames(), frames_before);
        assert_eq!(runner.current_mouse_position(), Point2D::new(10, 10));
        runner.mark_test_complete();
    })
    .expect("stepped moves are paced");
}

#[test]
fn rust_only_click_by_name_presses_and_releases_at_the_center() {
    show_window_and_execute_tests(RunOptions::default(), window_with_target, |runner, log| {
        runner.click_by_name("target");

        // The probe spans x 50..130, y 40..60 (Y-up) in a 200-high
        // window: its center (90, 50) is pointer (90, 150).
        assert_eq!(runner.current_mouse_position(), Point2D::new(90, 150));
        assert_eq!(
            presses(log),
            vec![(Point::new(40.0, 10.0), MouseButton::Left)]
        );
        assert_eq!(
            releases(log),
            vec![(Point::new(40.0, 10.0), MouseButton::Left)]
        );
        assert_eq!(runner.input_method().click_count(), 1);
        assert_eq!(runner.driver().forwarder().click_count(), 1);
        assert!(!runner.input_method().left_button_down());
        assert_eq!(runner.driver().forwarder().buttons_down(), 0);

        // Lower-left origin with an offset: (50 + 5, 40 + 3) in the window.
        runner.click_by_name_with(
            "target",
            &ClickOpts {
                origin: ClickOrigin::LowerLeft,
                offset: Point2D::new(5, 3),
                ..ClickOpts::default()
            },
        );
        assert_eq!(runner.current_mouse_position(), Point2D::new(55, 157));
        assert_eq!(presses(log)[1], (Point::new(5.0, 3.0), MouseButton::Left));
        runner.mark_test_complete();
    })
    .expect("a click lands where it is aimed");
}

#[test]
fn rust_only_a_double_click_states_its_second_press_count() {
    show_window_and_execute_tests(RunOptions::default(), window_with_target, |runner, log| {
        runner.click_by_name_with(
            "target",
            &ClickOpts {
                is_double_click: true,
                ..ClickOpts::default()
            },
        );
        // down(1) up down(2) up: two full pairs, the second press
        // reporting 2 clicks.
        assert_eq!(presses(log).len(), 2);
        assert_eq!(releases(log).len(), 2);
        assert_eq!(runner.input_method().click_count(), 2);
        assert_eq!(runner.driver().forwarder().click_count(), 2);

        // A single click after it is a single click again, however close.
        runner.click_by_name("target");
        assert_eq!(runner.driver().forwarder().click_count(), 1);
        runner.mark_test_complete();
    })
    .expect("double clicks report their count");
}

#[test]
fn rust_only_right_click_by_name_uses_the_right_button() {
    show_window_and_execute_tests(RunOptions::default(), window_with_target, |runner, log| {
        runner.right_click_by_name("target");
        assert_eq!(
            presses(log),
            vec![(Point::new(40.0, 10.0), MouseButton::Right)]
        );
        assert_eq!(
            releases(log),
            vec![(Point::new(40.0, 10.0), MouseButton::Right)]
        );

        let handle = runner
            .get_widget_by_name("target", &Default::default())
            .expect("target is there");
        runner.right_click_widget(&handle);
        runner.click_widget(&handle, false);
        assert_eq!(presses(log).len(), 3);
        assert_eq!(presses(log)[2].1, MouseButton::Left);
        runner.mark_test_complete();
    })
    .expect("right clicks press the right button");
}

#[test]
fn rust_only_move_to_by_name_moves_without_pressing() {
    show_window_and_execute_tests(RunOptions::default(), window_with_target, |runner, log| {
        assert!(runner.move_to_by_name("target", &ClickOpts::default()));
        assert_eq!(runner.current_mouse_position(), Point2D::new(90, 150));
        assert!(presses(log).is_empty());
        assert!(log
            .borrow()
            .iter()
            .any(|event| matches!(event, Event::MouseMove { .. })));

        assert!(!runner.move_to_by_name(
            "absent",
            &ClickOpts {
                secs_to_wait: 0.0,
                ..ClickOpts::default()
            }
        ));

        // A window point (Y-up, logical) is the same place as a pointer.
        runner.set_mouse_cursor_position_in_window(20, 30);
        assert_eq!(runner.current_mouse_position(), Point2D::new(20, 170));
        assert_eq!(
            runner.pointer_to_window(Point2D::new(20, 170)),
            Point2D::new(20, 30)
        );
        runner.mark_test_complete();
    })
    .expect("moving onto a widget by name");
}

#[test]
fn rust_only_click_by_name_fails_with_the_not_found_message() {
    let result =
        show_window_and_execute_tests(RunOptions::default(), window_with_target, |runner, _log| {
            runner.click_by_name_with(
                "absent",
                &ClickOpts {
                    secs_to_wait: 0.0,
                    ..ClickOpts::default()
                },
            );
        });
    assert_eq!(
        result.err(),
        Some(AutomationError::BodyPanicked(
            "ClickByName Failed: Named GuiWidget not found [absent]".to_string()
        ))
    );
}

#[test]
fn rust_only_presses_outside_the_window_are_not_delivered_but_moves_are() {
    let mut root = ProbeWidget::new("root").on_event_with(|_| EventResult::Consumed);
    let root_log = root.log();
    root.add_child(Box::new(
        ProbeWidget::new("child").with_bounds(Rect::new(0.0, 0.0, 10.0, 10.0)),
    ));
    let mut driver = HeadlessDriver::new(Box::new(root), 100, 80);
    driver.layout_if_needed();
    let mut input = SimulatedInput::new();

    // Outside: the move is delivered, the press is not, yet the input
    // method still records that the left button went down (as C# does).
    input.set_cursor_position(&mut driver, -10, -5);
    assert_eq!(driver.forwarder().cursor(), (-10.0, -5.0));
    input.mouse_event(&mut driver, MouseAction::LeftDown, -10, -5, 0);
    assert_eq!(driver.forwarder().buttons_down(), 0);
    assert!(input.left_button_down());
    assert_eq!(input.click_count(), 0);
    input.mouse_event(&mut driver, MouseAction::LeftUp, -10, -5, 0);
    assert!(!input.left_button_down());

    // The far corner counts as inside (edges included).
    input.set_cursor_position(&mut driver, 100, 80);
    input.mouse_event(&mut driver, MouseAction::RightDown, 0, 0, 7);
    assert_eq!(driver.forwarder().buttons_down(), 1);
    assert!(driver.forwarder().is_button_down(MouseButton::Right));
    assert!(input.right_button_down());
    assert!(!input.left_button_down());
    // Any count other than 2 is a single click.
    assert_eq!(input.click_count(), 1);
    assert_eq!(driver.forwarder().click_count(), 1);
    input.mouse_event(&mut driver, MouseAction::RightUp, 0, 0, 0);
    assert_eq!(driver.forwarder().buttons_down(), 0);
    assert!(root_log
        .borrow()
        .iter()
        .any(|event| matches!(event, Event::MouseDown { .. })));
}
